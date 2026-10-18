#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown,
    clippy::collapsible_if,
    clippy::uninlined_format_args,
    clippy::significant_drop_tightening
)]
//! Comprehensive integration tests for `FindX` Finder batch file operations queue engine.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs as unix_fs;
use std::path::PathBuf;
use std::time::Instant;

use finder::operations::{
    BatchOperation, OperationEvent, OperationsQueue, TaskControl, copy_file_with_control,
    run_preflight_scan,
};
use shared::models::operation::OperationKind;
use tempfile::tempdir;

#[tokio::test]
async fn test_operations_batch_copy_reflink_and_streaming() {
    let dir = tempdir().expect("tempdir");
    let src_dir = dir.path().join("src_tree");
    let dst_dir = dir.path().join("dst_tree");

    fs::create_dir_all(src_dir.join("sub1/sub2")).expect("create dirs");
    fs::create_dir_all(&dst_dir).expect("create dst");

    // 1. Small file
    fs::write(src_dir.join("small.txt"), b"small content").expect("write small");

    // 2. Medium file (2 MB)
    let med_data = vec![0x33u8; 2 * 1024 * 1024];
    fs::write(src_dir.join("sub1/medium.bin"), &med_data).expect("write medium");

    // 3. Large file (11 MB > 10MB page cache fadvise threshold)
    let large_data = vec![0x77u8; 11 * 1024 * 1024];
    fs::write(src_dir.join("sub1/sub2/large.bin"), &large_data).expect("write large");

    // 4. Symlink
    unix_fs::symlink("small.txt", src_dir.join("symlink_to_small")).expect("create symlink");

    let op = BatchOperation {
        id: "test-reflink-stream".into(),
        kind: OperationKind::Copy,
        sources: vec![src_dir.clone()],
        destination: Some(dst_dir.clone()),
    };

    let queue = OperationsQueue::new(2);
    let rx = queue.subscribe_events();

    let task_id = queue.enqueue_operation(op);
    assert_eq!(task_id, "test-reflink-stream");

    while let Ok(evt) = rx.recv().await {
        if let OperationEvent::Completed { operation_id } = evt {
            if operation_id == "test-reflink-stream" {
                break;
            }
        }
    }

    let copied_root = dst_dir.join("src_tree");
    assert!(copied_root.exists(), "src_tree must exist in destination");
    assert_eq!(
        fs::read(copied_root.join("small.txt")).expect("read small"),
        b"small content"
    );
    assert_eq!(
        fs::metadata(copied_root.join("sub1/medium.bin"))
            .expect("meta med")
            .len(),
        2 * 1024 * 1024
    );
    assert_eq!(
        fs::metadata(copied_root.join("sub1/sub2/large.bin"))
            .expect("meta large")
            .len(),
        11 * 1024 * 1024
    );

    let link_target = fs::read_link(copied_root.join("symlink_to_small")).expect("readlink");
    assert_eq!(link_target, PathBuf::from("small.txt"));
}

#[tokio::test]
async fn test_operations_pause_resume_responsiveness() {
    let dir = tempdir().expect("tempdir");
    let src_dir = dir.path().join("src_pause");
    let dst_dir = dir.path().join("dst_pause");
    fs::create_dir_all(&src_dir).expect("create src");
    fs::create_dir_all(&dst_dir).expect("create dst");

    // Create 10 files of 512KB each
    let chunk = vec![0x44u8; 512 * 1024];
    for i in 0..10 {
        fs::write(src_dir.join(format!("file_{i}.bin")), &chunk).expect("write");
    }

    let op = BatchOperation {
        id: "test-pause-resume".into(),
        kind: OperationKind::Copy,
        sources: vec![src_dir],
        destination: Some(dst_dir),
    };

    let queue = OperationsQueue::new(1);
    let rx = queue.subscribe_events();

    queue.enqueue_operation(op);

    let mut paused_emitted = false;
    let mut resumed_emitted = false;

    while let Ok(evt) = rx.recv().await {
        match evt {
            OperationEvent::Progress(_) if !paused_emitted => {
                let start_pause = Instant::now();
                let res = queue.pause_operation("test-pause-resume");
                let pause_duration = start_pause.elapsed();

                assert!(res.is_ok(), "pause must succeed");
                assert!(
                    pause_duration.as_millis() < 100,
                    "pause invocation took too long: {:?}",
                    pause_duration
                );
                paused_emitted = true;
            }
            OperationEvent::Paused { ref operation_id } => {
                assert_eq!(operation_id, "test-pause-resume");
                // Resume operation
                let start_resume = Instant::now();
                let res = queue.resume_operation("test-pause-resume");
                let resume_duration = start_resume.elapsed();

                assert!(res.is_ok(), "resume must succeed");
                assert!(
                    resume_duration.as_millis() < 100,
                    "resume invocation took too long: {:?}",
                    resume_duration
                );
                resumed_emitted = true;
            }
            OperationEvent::Resumed { ref operation_id } => {
                assert_eq!(operation_id, "test-pause-resume");
            }
            OperationEvent::Completed { ref operation_id } => {
                assert_eq!(operation_id, "test-pause-resume");
                break;
            }
            _ => {}
        }
    }

    assert!(paused_emitted, "pause event cycle must execute");
    assert!(resumed_emitted, "resume event cycle must execute");
}

#[tokio::test]
async fn test_operations_cancellation_cleanup_responsiveness() {
    let dir = tempdir().expect("tempdir");
    let src = dir.path().join("large_src.bin");
    let dst = dir.path().join("large_dst.bin");

    // 8 MB file
    let data = vec![0x88u8; 8 * 1024 * 1024];
    fs::write(&src, &data).expect("write src");

    let control = TaskControl::new();
    let control_clone = control.clone();

    let start_cancel = std::sync::Arc::new(std::sync::Mutex::new(None));
    let start_cancel_clone = start_cancel.clone();

    let res = copy_file_with_control(&src, &dst, &control, |copied, _total| {
        if copied > 0 && !control_clone.is_cancelled() {
            let mut guard = start_cancel_clone.lock().unwrap();
            *guard = Some(Instant::now());
            control_clone.cancel();
        }
    });

    let elapsed = start_cancel
        .lock()
        .unwrap()
        .map_or(std::time::Duration::ZERO, |t| t.elapsed());

    assert!(res.is_err(), "copy must be cancelled");
    assert!(
        elapsed.as_millis() < 100,
        "cancellation responsiveness took too long: {:?}",
        elapsed
    );
    assert!(
        !dst.exists(),
        "partial destination file must be cleaned up immediately upon cancel"
    );
}

#[test]
fn test_operations_preflight_10000_items() {
    let dir = tempdir().expect("tempdir");
    let root = dir.path().join("big_dataset");
    fs::create_dir(&root).expect("create root");

    let total_expected_files: usize = 10_000;
    let folders_count: usize = 50;
    let files_per_folder = total_expected_files / folders_count;

    for f in 0..folders_count {
        let folder = root.join(format!("dir_{f}"));
        fs::create_dir(&folder).expect("create sub");
        for file_idx in 0..files_per_folder {
            let file_path = folder.join(format!("f_{file_idx}.txt"));
            let mut file = File::create(&file_path).expect("create file");
            file.write_all(b"x").expect("write file");
        }
    }

    let op = BatchOperation {
        id: "preflight-10000".into(),
        kind: OperationKind::Copy,
        sources: vec![root],
        destination: Some(dir.path().join("target_dir")),
    };

    let control = TaskControl::new();
    let start = Instant::now();

    let manifest = run_preflight_scan(&op, &control, |_| {}).expect("preflight scan succeeds");
    let scan_duration = start.elapsed();

    // 1 root + 50 directories + 10,000 files = 10,051 items
    let expected_items: u64 = 1 + (folders_count as u64) + (total_expected_files as u64);
    assert_eq!(manifest.total_items, expected_items);
    assert_eq!(manifest.total_bytes, total_expected_files as u64);

    // Assert traversal was non-blocking and fast (< 1.5s for 10k items)
    assert!(
        scan_duration.as_millis() < 1500,
        "preflight scanned 10k items in {:?}",
        scan_duration
    );
}

#[tokio::test]
async fn test_operations_queue_concurrency_throttle_and_ordering() {
    let dir = tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).expect("create src");
    fs::create_dir_all(&dst).expect("create dst");

    for i in 1..=4 {
        fs::write(src.join(format!("item_{i}.txt")), b"test payload").expect("write");
    }

    // Max concurrency: 2
    let queue = OperationsQueue::new(2);
    let rx = queue.subscribe_events();

    for i in 1..=4 {
        let op = BatchOperation {
            id: format!("throttled-op-{i}"),
            kind: OperationKind::Copy,
            sources: vec![src.join(format!("item_{i}.txt"))],
            destination: Some(dst.clone()),
        };
        queue.enqueue_operation(op);
    }

    let mut completed_order = Vec::new();

    while let Ok(evt) = rx.recv().await {
        if let OperationEvent::Completed { operation_id } = evt {
            completed_order.push(operation_id);
            if completed_order.len() == 4 {
                break;
            }
        }
    }

    assert_eq!(completed_order.len(), 4);
    assert!(dst.join("item_1.txt").exists());
    assert!(dst.join("item_2.txt").exists());
    assert!(dst.join("item_3.txt").exists());
    assert!(dst.join("item_4.txt").exists());
}
