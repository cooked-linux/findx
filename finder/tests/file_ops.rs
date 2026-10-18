#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::uninlined_format_args,
    clippy::significant_drop_tightening,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
//! Milestone M4 Comprehensive Integration Test Suite for Finder Operations.
//!
//! Validates:
//! - High-throughput batch copy with streaming/reflink integrity across 100+ files.
//! - Interactive conflict resolution modes (KeepBoth, Replace, Merge, Skip).
//! - Multi-level transactional undo/redo across Move, Copy, and Trash restorations.
//! - Asynchronous folder size computation with physical block deduplication for hardlinks.
//! - Batch operation pause/resume and sub-100ms cancellation cleanup responsiveness.

use std::fs::{self, File};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use finder::operations::folder_size::{FolderSizeCache, FolderSizeManager, Priority};
use finder::operations::{
    BatchOperation, ConflictAction, ConflictArbiter, OperationEvent, OperationsQueue, TaskControl,
    copy_file_with_control, execute_directory_merge,
};
use finder::undo::{
    FileOperationRecord, UndoManager, UndoOperationType, UndoTransaction, connect_to_queue,
};
use shared::fs::stat::stat_path;
use shared::models::operation::OperationKind;
use tempfile::tempdir;

#[tokio::test]
async fn test_batch_copy_reflink_and_data_integrity() {
    let dir = tempdir().expect("tempdir");
    let src_dir = dir.path().join("src_hierarchy");
    let dst_dir = dir.path().join("dst_hierarchy");
    fs::create_dir_all(&src_dir).expect("create src");
    fs::create_dir_all(&dst_dir).expect("create dst");

    // Creates temporary directory structure with nested files and subfolders (100+ files, 10MB total data)
    let folder_count = 10;
    let files_per_folder = 10;
    let mut created_files = Vec::new();
    // 10 folders * 10 files = 100 files + 2 root files = 102 files.
    // 100_000 bytes each * 102 ~ 10.2 MB data.
    let payload_chunk = vec![0x5Au8; 100_000];

    for folder_idx in 0..folder_count {
        let folder = src_dir.join(format!("sub_{folder_idx}/nested"));
        fs::create_dir_all(&folder).expect("create sub folder");
        for file_idx in 0..files_per_folder {
            let fpath = folder.join(format!("item_{file_idx}.dat"));
            let mut data = payload_chunk.clone();
            data[0] = folder_idx as u8;
            data[1] = file_idx as u8;
            fs::write(&fpath, &data).expect("write file");
            created_files.push(fpath);
        }
    }

    let root_file_1 = src_dir.join("root_file_1.txt");
    fs::write(&root_file_1, &payload_chunk).expect("write root 1");
    created_files.push(root_file_1);
    let root_file_2 = src_dir.join("root_file_2.txt");
    fs::write(&root_file_2, &payload_chunk).expect("write root 2");
    created_files.push(root_file_2);

    assert!(created_files.len() > 100);

    let op = BatchOperation {
        id: "batch-integrity-copy".into(),
        kind: OperationKind::Copy,
        sources: vec![src_dir.clone()],
        destination: Some(dst_dir.clone()),
    };

    let queue = OperationsQueue::new(2);
    let rx = queue.subscribe_events();
    queue.enqueue_operation(op);

    while let Ok(evt) = rx.recv().await {
        if let OperationEvent::Completed { operation_id } = evt {
            if operation_id == "batch-integrity-copy" {
                break;
            }
        }
    }

    let copied_root = dst_dir.join("src_hierarchy");
    assert!(
        copied_root.exists(),
        "copied_root must exist in destination"
    );

    // Verifies all files exist at destination with identical bytes and permissions
    for orig_path in &created_files {
        let rel_path = orig_path.strip_prefix(&src_dir).expect("strip prefix");
        let dest_path = copied_root.join(rel_path);

        assert!(dest_path.exists(), "file must exist: {:?}", dest_path);

        let orig_meta = fs::metadata(orig_path).expect("orig meta");
        let dest_meta = fs::metadata(&dest_path).expect("dest meta");
        assert_eq!(
            orig_meta.len(),
            dest_meta.len(),
            "length mismatch on {:?}",
            dest_path
        );

        let orig_bytes = fs::read(orig_path).expect("read orig");
        let dest_bytes = fs::read(&dest_path).expect("read dest");
        assert_eq!(
            orig_bytes, dest_bytes,
            "content mismatch on {:?}",
            dest_path
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                orig_meta.permissions().mode(),
                dest_meta.permissions().mode(),
                "permissions mismatch"
            );
        }
    }
}

#[tokio::test]
async fn test_conflict_resolution_modes() {
    let dir = tempdir().expect("tempdir");
    let src_dir = dir.path().join("src");
    let dst_dir = dir.path().join("dst");
    fs::create_dir_all(&src_dir).expect("create src");
    fs::create_dir_all(&dst_dir).expect("create dst");

    let (prompt_tx, _prompt_rx) = async_channel::unbounded();
    let mut arbiter = ConflictArbiter::new(prompt_tx);
    let control = TaskControl::new();

    // 1. ConflictAction::KeepBoth: creates unique numbered name generation ("file (1).txt")
    {
        let src_file = src_dir.join("report.txt");
        let dst_file = dst_dir.join("report.txt");
        fs::write(&src_file, b"incoming report data").expect("write src");
        fs::write(&dst_file, b"existing destination report").expect("write dst");

        let res = arbiter
            .execute_resolution(
                ConflictAction::KeepBoth,
                &src_file,
                &dst_file,
                OperationKind::Copy,
                &control,
                |_, _| {},
            )
            .await;
        assert!(res.is_ok(), "KeepBoth resolution failed");

        let expected_keep_both = dst_dir.join("report (1).txt");
        assert!(dst_file.exists(), "original dst file should still exist");
        assert!(
            expected_keep_both.exists(),
            "report (1).txt should be created"
        );
        assert_eq!(
            fs::read(&dst_file).expect("read orig"),
            b"existing destination report"
        );
        assert_eq!(
            fs::read(&expected_keep_both).expect("read copy"),
            b"incoming report data"
        );
    }

    // 2. ConflictAction::Replace: destination file is atomically replaced and staging cleanup works
    {
        let src_file = src_dir.join("replace_me.txt");
        let dst_file = dst_dir.join("replace_me.txt");
        fs::write(&src_file, b"new updated content").expect("write src");
        fs::write(&dst_file, b"old stale content").expect("write dst");

        let res = arbiter
            .execute_resolution(
                ConflictAction::Replace,
                &src_file,
                &dst_file,
                OperationKind::Copy,
                &control,
                |_, _| {},
            )
            .await;
        assert!(res.is_ok(), "Replace resolution failed");

        assert!(dst_file.exists(), "dst_file should exist");
        assert_eq!(
            fs::read(&dst_file).expect("read dst"),
            b"new updated content"
        );
    }

    // 3. ConflictAction::Skip: existing destination file is untouched
    {
        let src_file = src_dir.join("skip_me.txt");
        let dst_file = dst_dir.join("skip_me.txt");
        fs::write(&src_file, b"incoming content that should be skipped").expect("write src");
        fs::write(&dst_file, b"precious destination content").expect("write dst");

        let res = arbiter
            .execute_resolution(
                ConflictAction::Skip,
                &src_file,
                &dst_file,
                OperationKind::Copy,
                &control,
                |_, _| {},
            )
            .await;
        assert!(res.is_ok(), "Skip resolution failed");

        assert_eq!(
            fs::read(&dst_file).expect("read dst"),
            b"precious destination content"
        );
    }

    // 4. ConflictAction::Merge: directory trees merge recursively preserving newer files
    {
        let merge_src = src_dir.join("merge_folder");
        let merge_dst = dst_dir.join("merge_folder");
        fs::create_dir_all(&merge_src).expect("create merge src");
        fs::create_dir_all(&merge_dst).expect("create merge dst");

        let f_newer_src = merge_src.join("newer.txt");
        let f_newer_dst = merge_dst.join("newer.txt");
        fs::write(&f_newer_src, b"new version from src").expect("write src");
        fs::write(&f_newer_dst, b"old version in dst").expect("write dst");

        let now = SystemTime::now();
        filetime::set_file_mtime(&f_newer_src, filetime::FileTime::from_system_time(now))
            .expect("mtime src");
        filetime::set_file_mtime(
            &f_newer_dst,
            filetime::FileTime::from_system_time(now - Duration::from_secs(60)),
        )
        .expect("mtime dst");

        let f_older_src = merge_src.join("older.txt");
        let f_older_dst = merge_dst.join("older.txt");
        fs::write(&f_older_src, b"stale version in src").expect("write src");
        fs::write(&f_older_dst, b"newer version in dst").expect("write dst");

        filetime::set_file_mtime(
            &f_older_src,
            filetime::FileTime::from_system_time(now - Duration::from_secs(60)),
        )
        .expect("mtime src");
        filetime::set_file_mtime(&f_older_dst, filetime::FileTime::from_system_time(now))
            .expect("mtime dst");

        let summary = execute_directory_merge(&merge_src, &merge_dst, &control, |_| {})
            .await
            .expect("execute directory merge");

        assert_eq!(summary.files_replaced, 1);
        assert_eq!(summary.files_skipped, 1);
        assert_eq!(
            fs::read(&f_newer_dst).expect("read newer"),
            b"new version from src"
        );
        assert_eq!(
            fs::read(&f_older_dst).expect("read older"),
            b"newer version in dst"
        );
    }
}

#[tokio::test]
async fn test_transactional_undo_redo() {
    let dir = tempdir().expect("tempdir");
    let src_dir = dir.path().join("undo_src");
    let dst_dir = dir.path().join("undo_dst");
    fs::create_dir_all(&src_dir).expect("create src");
    fs::create_dir_all(&dst_dir).expect("create dst");

    let queue = OperationsQueue::new(1);
    let undo_mgr = Arc::new(Mutex::new(UndoManager::new()));
    connect_to_queue(&queue, Arc::clone(&undo_mgr));

    // 1. Move A -> B, Undo (returns to A), Redo (returns to B)
    {
        let file_a = src_dir.join("file_move.txt");
        let file_b = dst_dir.join("file_move.txt");
        fs::write(&file_a, b"move payload").expect("write a");

        let op = BatchOperation::new(
            "op-move-test",
            OperationKind::Move,
            vec![file_a.clone()],
            Some(dst_dir.clone()),
        );
        let rx = queue.subscribe_events();
        queue.enqueue_operation(op);
        while let Ok(evt) = rx.recv().await {
            if matches!(evt, OperationEvent::Completed { .. }) {
                break;
            }
        }

        assert!(!file_a.exists());
        assert!(file_b.exists());

        // Undo move
        {
            let mut mgr = undo_mgr.lock().unwrap();
            assert!(mgr.can_undo());
            let undone = mgr.undo().expect("undo move").expect("some transaction");
            assert_eq!(undone.op_type, UndoOperationType::Move);
        }
        assert!(file_a.exists(), "file should be restored to A after undo");
        assert!(!file_b.exists(), "file B should no longer exist after undo");

        // Redo move
        {
            let mut mgr = undo_mgr.lock().unwrap();
            assert!(mgr.can_redo());
            let redone = mgr.redo().expect("redo move").expect("some transaction");
            assert_eq!(redone.op_type, UndoOperationType::Move);
        }
        assert!(!file_a.exists(), "file A should no longer exist after redo");
        assert!(file_b.exists(), "file B should exist after redo");
    }

    // 2. Copy A -> C, Undo (copy C is sent to Trash / removed)
    {
        let file_a = src_dir.join("file_copy_src.txt");
        let file_c = dst_dir.join("file_copy_src.txt");
        fs::write(&file_a, b"copy payload").expect("write a");

        let op = BatchOperation::new(
            "op-copy-test",
            OperationKind::Copy,
            vec![file_a.clone()],
            Some(dst_dir.clone()),
        );
        let rx = queue.subscribe_events();
        queue.enqueue_operation(op);
        while let Ok(evt) = rx.recv().await {
            if matches!(evt, OperationEvent::Completed { .. }) {
                break;
            }
        }

        assert!(file_a.exists());
        assert!(file_c.exists());

        // Undo copy
        {
            let mut mgr = undo_mgr.lock().unwrap();
            assert!(mgr.can_undo());
            let undone = mgr.undo().expect("undo copy").expect("some transaction");
            assert_eq!(undone.op_type, UndoOperationType::Copy);
        }
        assert!(file_a.exists(), "source A must still exist after copy undo");
        assert!(
            !file_c.exists(),
            "destination C must be trashed/removed after copy undo"
        );
    }

    // 3. Trash A -> Trash, Undo (restored from Trash back to A)
    {
        let file_to_trash = src_dir.join("trash_me.txt");
        let trash_dir = dir.path().join("mock_trash");
        let trash_files = trash_dir.join("files");
        let trash_info = trash_dir.join("info");
        fs::create_dir_all(&trash_files).expect("create trash files");
        fs::create_dir_all(&trash_info).expect("create trash info");

        let trashed_file = trash_files.join("trash_me.txt");
        fs::write(&trashed_file, b"trashed payload").expect("write trashed");
        let stat = stat_path(&trashed_file).expect("stat");

        let info_file = trash_info.join("trash_me.txt.trashinfo");
        let info_content = format!(
            "[Trash Info]\nPath={}\nDeletionDate=2026-10-18T12:00:00\n",
            file_to_trash.display()
        );
        fs::write(&info_file, info_content).expect("write info");

        let record = FileOperationRecord::new(
            file_to_trash.clone(),
            trashed_file.clone(),
            stat.ino,
            stat.dev,
            SystemTime::now(),
            Some(info_file.clone()),
        );

        let tx = UndoTransaction::new(
            "tx_trash_restore",
            UndoOperationType::Trash,
            "Trash trash_me.txt",
            vec![record],
            SystemTime::now(),
        );

        {
            let mut mgr = undo_mgr.lock().unwrap();
            mgr.record_transaction(tx);
            assert!(mgr.can_undo());
            let undone = mgr.undo().expect("undo trash").expect("some transaction");
            assert_eq!(undone.op_type, UndoOperationType::Trash);
        }

        assert!(
            file_to_trash.exists(),
            "file should be restored back from Trash to A"
        );
        assert_eq!(fs::read(&file_to_trash).expect("read"), b"trashed payload");
        assert!(
            !trashed_file.exists(),
            "trashed file in Trash/files should be gone"
        );
        assert!(
            !info_file.exists(),
            "trashinfo in Trash/info should be gone"
        );
    }
}

#[tokio::test]
async fn test_async_folder_size_with_hardlinks() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("hardlink_root");
    let sub = root.join("nested");
    fs::create_dir_all(&sub).expect("create nested");

    // 1. Regular file of 8KB
    let file1 = root.join("file1.bin");
    {
        let mut f = File::create(&file1).expect("create file1");
        f.write_all(&vec![0xAAu8; 8192]).expect("write file1");
        f.sync_all().expect("sync");
    }

    // 2. Hardlink to file1 inside nested directory
    let hardlink = sub.join("hardlink_to_file1.bin");
    fs::hard_link(&file1, &hardlink).expect("create hardlink");

    // 3. Regular file of 4KB
    let file2 = sub.join("file2.bin");
    {
        let mut f = File::create(&file2).expect("create file2");
        f.write_all(&vec![0xBBu8; 4096]).expect("write file2");
        f.sync_all().expect("sync");
    }

    let cache = Arc::new(FolderSizeCache::new_in_memory().expect("in-memory cache"));
    let manager = FolderSizeManager::new(Arc::clone(&cache), 2);
    let rx = manager.results_receiver().clone();

    // Query FolderSizeManager
    let initial = manager.request_folder_size(&root, Priority::High);
    if initial.is_none() {
        let (recv_path, _res) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timeout waiting for size")
            .expect("channel active");
        assert_eq!(recv_path, root);
    }

    let result = manager
        .request_folder_size(&root, Priority::High)
        .expect("cache hit");

    // Total items: file1 + hardlink + file2 = 3 files, nested = 1 folder -> 4 total items
    assert_eq!(result.file_count, 3);
    assert_eq!(result.folder_count, 1);
    assert_eq!(result.total_items(), 4);

    // Physical block deduplication check:
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta1 = fs::symlink_metadata(&file1).expect("meta1");
        let meta2 = fs::symlink_metadata(&file2).expect("meta2");
        let expected_bytes = (meta1.blocks() + meta2.blocks()) * 512;
        assert_eq!(
            result.total_bytes, expected_bytes,
            "hardlink blocks must not be double counted"
        );
    }
}

#[tokio::test]
async fn test_operation_pause_resume_cancel() {
    let dir = tempdir().expect("tempdir");
    let src_dir = dir.path().join("pause_src");
    let dst_dir = dir.path().join("pause_dst");
    fs::create_dir_all(&src_dir).expect("create src");
    fs::create_dir_all(&dst_dir).expect("create dst");

    // 1. Test Pause & Resume on batch operation
    let file_chunk = vec![0xCCu8; 512 * 1024];
    for i in 0..10 {
        fs::write(src_dir.join(format!("data_{i}.bin")), &file_chunk).expect("write");
    }

    let op = BatchOperation {
        id: "op-pause-resume".into(),
        kind: OperationKind::Copy,
        sources: vec![src_dir.clone()],
        destination: Some(dst_dir.clone()),
    };

    let queue = OperationsQueue::new(1);
    let rx = queue.subscribe_events();
    queue.enqueue_operation(op);

    let mut pause_invoked = false;
    let mut resume_invoked = false;

    while let Ok(evt) = rx.recv().await {
        match evt {
            OperationEvent::Progress(_) if !pause_invoked => {
                let start_pause = Instant::now();
                let res = queue.pause_operation("op-pause-resume");
                assert!(res.is_ok());
                assert!(start_pause.elapsed().as_millis() < 100);
                pause_invoked = true;
            }
            OperationEvent::Paused { ref operation_id } => {
                assert_eq!(operation_id, "op-pause-resume");
                let start_resume = Instant::now();
                let res = queue.resume_operation("op-pause-resume");
                assert!(res.is_ok());
                assert!(start_resume.elapsed().as_millis() < 100);
                resume_invoked = true;
            }
            OperationEvent::Completed { ref operation_id } if operation_id == "op-pause-resume" => {
                break;
            }
            _ => {}
        }
    }

    assert!(pause_invoked);
    assert!(resume_invoked);

    // 2. Test Cancellation: assert immediate stoppage and partial write cleanup
    let cancel_src = dir.path().join("cancel_src.bin");
    let cancel_dst = dir.path().join("cancel_dst.bin");
    let large_data = vec![0xEEu8; 8 * 1024 * 1024];
    fs::write(&cancel_src, &large_data).expect("write cancel src");

    let control = TaskControl::new();
    let control_clone = control.clone();
    let cancel_time = Arc::new(Mutex::new(None));
    let cancel_time_clone = Arc::clone(&cancel_time);

    let copy_res = copy_file_with_control(&cancel_src, &cancel_dst, &control, |copied, _total| {
        if copied > 0 && !control_clone.is_cancelled() {
            let mut guard = cancel_time_clone.lock().unwrap();
            *guard = Some(Instant::now());
            control_clone.cancel();
        }
    });

    let elapsed = cancel_time
        .lock()
        .unwrap()
        .map_or(Duration::ZERO, |t| t.elapsed());

    assert!(copy_res.is_err(), "copy should be cancelled");
    assert!(
        elapsed.as_millis() < 100,
        "cancellation took too long: {:?}",
        elapsed
    );
    assert!(
        !cancel_dst.exists(),
        "partial destination file must be cleaned up immediately upon cancel"
    );
}
