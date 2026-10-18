#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown,
    clippy::collapsible_if,
    clippy::uninlined_format_args,
    clippy::significant_drop_tightening,
    clippy::similar_names
)]
//! Integration tests for the interactive conflict resolution and merge engine.

use std::fs;
use std::time::{Duration, SystemTime};

use finder::operations::{
    BatchOperation, ConflictAction, ConflictArbiter, ConflictResolution, OperationEvent,
    OperationKind, OperationWorker, RelativeTimeStatus, TaskControl, check_collision,
    execute_directory_merge, generate_unique_name, generate_unique_name_with_case_sensitivity,
    generate_unique_path,
};
use tempfile::tempdir;

#[test]
fn test_conflict_detection_and_relative_mtime() {
    let dir = tempdir().expect("tempdir");
    let src = dir.path().join("source.txt");
    let dst = dir.path().join("dest.txt");

    fs::write(&src, b"source payload").expect("write src");
    fs::write(&dst, b"destination payload").expect("write dst");

    let now = SystemTime::now();

    // Source newer by 5 seconds
    filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(now))
        .expect("set mtime src");
    filetime::set_file_mtime(
        &dst,
        filetime::FileTime::from_system_time(now - Duration::from_secs(5)),
    )
    .expect("set mtime dst");

    let ctx = check_collision("test-op", &src, &dst)
        .expect("check_collision")
        .expect("collision found");
    assert_eq!(ctx.relative_time, RelativeTimeStatus::IncomingNewer);
    assert!(!ctx.is_directory_collision);
    assert_eq!(ctx.source.size_bytes, 14);
    assert_eq!(ctx.destination.size_bytes, 19);

    // Swap: destination newer
    filetime::set_file_mtime(
        &src,
        filetime::FileTime::from_system_time(now - Duration::from_secs(5)),
    )
    .expect("set mtime src");
    filetime::set_file_mtime(&dst, filetime::FileTime::from_system_time(now))
        .expect("set mtime dst");

    let ctx2 = check_collision("test-op", &src, &dst)
        .expect("check_collision")
        .expect("collision found");
    assert_eq!(ctx2.relative_time, RelativeTimeStatus::IncomingOlder);
}

#[test]
fn test_naming_auto_increment_and_case_sensitivity() {
    let dir = tempdir().expect("tempdir");

    // Standard increment
    fs::write(dir.path().join("report.docx"), b"content").expect("write");
    let u1 = generate_unique_name(dir.path(), "report.docx");
    assert_eq!(u1, "report (1).docx");

    // Increment existing counter
    fs::write(dir.path().join("report (1).docx"), b"content").expect("write");
    let u2 = generate_unique_name(dir.path(), "report (1).docx");
    assert_eq!(u2, "report (2).docx");

    // Compound extension
    fs::write(dir.path().join("backup.tar.gz"), b"content").expect("write");
    let u3 = generate_unique_name(dir.path(), "backup.tar.gz");
    assert_eq!(u3, "backup (1).tar.gz");

    // Case-insensitive check
    fs::write(dir.path().join("PHOTO (1).JPG"), b"content").expect("write");
    let u4 = generate_unique_name_with_case_sensitivity(dir.path(), "photo.jpg", true);
    assert_eq!(u4, "photo (2).jpg");

    // Unique path
    let p = generate_unique_path(&dir.path().join("report.docx"));
    assert_eq!(p, dir.path().join("report (2).docx"));
}

#[tokio::test]
async fn test_recursive_directory_merge_newer_older() {
    let dir = tempdir().expect("tempdir");
    let src = dir.path().join("src_folder");
    let dst = dir.path().join("dst_folder");

    fs::create_dir_all(src.join("nested")).expect("create src nested");
    fs::create_dir_all(dst.join("nested")).expect("create dst nested");

    // File A: in src and dst; src is newer -> dst should be replaced with src content
    let file_a_src = src.join("nested/file_a.txt");
    let file_a_dst = dst.join("nested/file_a.txt");
    fs::write(&file_a_src, b"new content a").expect("write a src");
    fs::write(&file_a_dst, b"old content a").expect("write a dst");

    let now = SystemTime::now();
    filetime::set_file_mtime(&file_a_src, filetime::FileTime::from_system_time(now))
        .expect("set mtime a src");
    filetime::set_file_mtime(
        &file_a_dst,
        filetime::FileTime::from_system_time(now - Duration::from_secs(100)),
    )
    .expect("set mtime a dst");

    // File B: in src and dst; dst is newer -> dst content should be preserved
    let file_b_src = src.join("nested/file_b.txt");
    let file_b_dst = dst.join("nested/file_b.txt");
    fs::write(&file_b_src, b"old content b").expect("write b src");
    fs::write(&file_b_dst, b"new content b").expect("write b dst");

    filetime::set_file_mtime(
        &file_b_src,
        filetime::FileTime::from_system_time(now - Duration::from_secs(100)),
    )
    .expect("set mtime b src");
    filetime::set_file_mtime(&file_b_dst, filetime::FileTime::from_system_time(now))
        .expect("set mtime b dst");

    // File C: only in src -> should be copied to dst
    let file_c_src = src.join("file_c.txt");
    fs::write(&file_c_src, b"only in src").expect("write c src");

    // File D: only in dst -> should remain untouched
    let file_d_dst = dst.join("file_d.txt");
    fs::write(&file_d_dst, b"only in dst").expect("write d dst");

    let control = TaskControl::new();
    let summary = execute_directory_merge(&src, &dst, &control, |_| {})
        .await
        .expect("execute merge");

    assert_eq!(summary.files_copied, 1);
    assert_eq!(summary.files_replaced, 1);
    assert_eq!(summary.files_skipped, 1);

    // Verify final destination contents
    let content_a = fs::read_to_string(&file_a_dst).expect("read a");
    assert_eq!(content_a, "new content a");

    let content_b = fs::read_to_string(&file_b_dst).expect("read b");
    assert_eq!(content_b, "new content b");

    assert!(dst.join("file_c.txt").exists());
    assert!(dst.join("file_d.txt").exists());
}

#[tokio::test]
async fn test_resolver_staging_rollback_on_failure() {
    let dir = tempdir().expect("tempdir");
    let src = dir.path().join("source.txt");
    let dst = dir.path().join("destination.txt");

    fs::write(&src, b"source data").expect("write src");
    fs::write(&dst, b"valuable original data").expect("write dst");

    let control = TaskControl::new();
    control.cancel(); // Cancel immediately so transfer fails

    let (prompt_tx, _prompt_rx) = async_channel::unbounded();
    let mut arbiter = ConflictArbiter::new(prompt_tx);

    let res = arbiter
        .execute_resolution(
            ConflictAction::Replace,
            &src,
            &dst,
            OperationKind::Copy,
            &control,
            |_, _| {},
        )
        .await;

    assert!(res.is_err(), "transfer should be cancelled");

    // Ensure staging rollback preserved the valuable original destination data
    let original = fs::read_to_string(&dst).expect("read dst");
    assert_eq!(original, "valuable original data");
}

#[tokio::test]
async fn test_worker_interactive_conflict_keep_both_and_apply_to_all() {
    let dir = tempdir().expect("tempdir");
    let src_dir = dir.path().join("src");
    let dst_dir = dir.path().join("dst");
    fs::create_dir_all(&src_dir).expect("create src");
    fs::create_dir_all(&dst_dir).expect("create dst");

    let mut sources = Vec::new();
    for i in 1..=3 {
        let src_file = src_dir.join(format!("item_{i}.txt"));
        let dst_file = dst_dir.join(format!("item_{i}.txt"));

        fs::write(&src_file, format!("incoming item {i}")).expect("write src");
        fs::write(&dst_file, format!("existing item {i}")).expect("write dst");
        sources.push(src_file);
    }

    let op = BatchOperation {
        id: "batch-apply-all-keep".into(),
        kind: OperationKind::Copy,
        sources,
        destination: Some(dst_dir.clone()),
    };

    let control = TaskControl::new();
    let (tx, rx) = async_channel::unbounded();
    let (prompt_tx, prompt_rx) = async_channel::unbounded::<finder::operations::ConflictPrompt>();

    // Mock UI handler: responds to first prompt with KeepBoth and apply_to_all = true
    tokio::spawn(async move {
        if let Ok(prompt) = prompt_rx.recv().await {
            let _ = prompt
                .responder
                .send(ConflictResolution::new(ConflictAction::KeepBoth, true));
        }
    });

    let arbiter = ConflictArbiter::new(prompt_tx);
    let worker = OperationWorker::new(op, control, tx).with_conflict_arbiter(arbiter);

    let handle = worker.spawn();
    while let Ok(evt) = rx.recv().await {
        if matches!(
            evt,
            OperationEvent::Completed { .. }
                | OperationEvent::Cancelled { .. }
                | OperationEvent::Failed { .. }
        ) {
            break;
        }
    }

    let res = handle.await.expect("join");
    assert!(res.is_ok(), "worker should succeed");

    // All 3 items should have kept original and created (1) copy
    for i in 1..=3 {
        let orig = dst_dir.join(format!("item_{i}.txt"));
        let copy = dst_dir.join(format!("item_{i} (1).txt"));
        assert!(orig.exists(), "original item_{i}.txt must exist");
        assert!(copy.exists(), "item_{i} (1).txt must exist");
    }
}
