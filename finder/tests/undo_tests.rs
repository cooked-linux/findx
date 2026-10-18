//! Integration tests for the multi-level transactional undo/redo engine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone,
    clippy::significant_drop_tightening
)]

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use finder::operations::{BatchOperation, OperationEvent, OperationsQueue};
use finder::undo::{
    ConflictStrategy, DEFAULT_MAX_DEPTH, FileOperationRecord, UndoManager, UndoOperationType,
    UndoTransaction, connect_to_queue, execute_inverse, load_journal, save_journal,
};
use shared::fs::stat::stat_path;
use shared::models::operation::OperationKind;
use tempfile::tempdir;

#[tokio::test]
async fn test_operations_queue_auto_records_to_attached_undo_manager() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    let dst_dir = dir.path().join("dst");

    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&dst_dir).unwrap();

    let file1 = src_dir.join("alpha.txt");
    let file2 = src_dir.join("beta.txt");
    fs::write(&file1, "Alpha payload").unwrap();
    fs::write(&file2, "Beta payload").unwrap();

    let queue = OperationsQueue::new(1);
    let undo_mgr = Arc::new(Mutex::new(UndoManager::new()));
    connect_to_queue(&queue, Arc::clone(&undo_mgr));

    let rx = queue.subscribe_events();

    let op = BatchOperation::new(
        "batch_copy_test",
        OperationKind::Copy,
        vec![file1.clone(), file2.clone()],
        Some(dst_dir.clone()),
    );

    let _task_id = queue.enqueue_operation(op);

    // Wait for Completed event
    while let Ok(event) = rx.recv().await {
        if matches!(event, OperationEvent::Completed { .. }) {
            break;
        }
    }

    // Verify UndoManager recorded the transaction
    let mgr = undo_mgr.lock().unwrap();
    assert_eq!(mgr.undo_stack().len(), 1);
    assert!(mgr.can_undo());
    assert!(!mgr.can_redo());
    let label = mgr.current_undo_label().unwrap();
    assert!(label.contains("Undo"));
    assert!(label.contains("Copy"));

    drop(mgr);

    // Verify files exist in destination
    let dst1 = dst_dir.join("alpha.txt");
    let dst2 = dst_dir.join("beta.txt");
    assert!(dst1.exists());
    assert!(dst2.exists());

    // Execute Undo
    let mut mgr = undo_mgr.lock().unwrap();
    let undone = mgr.undo().unwrap().unwrap();
    assert_eq!(undone.op_type, UndoOperationType::Copy);
    assert!(!mgr.can_undo());
    assert!(mgr.can_redo());
    assert_eq!(mgr.redo_stack().len(), 1);

    // Target files should now be trashed
    assert!(!dst1.exists());
    assert!(!dst2.exists());
    assert!(file1.exists());
    assert!(file2.exists());

    // Execute Redo
    let redone = mgr.redo().unwrap().unwrap();
    assert_eq!(redone.op_type, UndoOperationType::Copy);
    assert!(mgr.can_undo());
    assert!(!mgr.can_redo());

    // Target files should exist again
    assert!(dst1.exists());
    assert!(dst2.exists());
}

#[test]
fn test_bounded_50_depth_ring_buffer() {
    let mut manager = UndoManager::new();
    assert_eq!(manager.max_depth(), DEFAULT_MAX_DEPTH);

    for i in 1..=60 {
        let tx = UndoTransaction::new(
            format!("tx_{i}"),
            UndoOperationType::Move,
            format!("Move {i}"),
            Vec::new(),
            SystemTime::now(),
        );
        manager.record_transaction(tx);
    }

    assert_eq!(manager.undo_stack().len(), 50);
    // Oldest 10 dropped; first item should be tx_11
    assert_eq!(manager.undo_stack()[0].id, "tx_11");
    // Latest item should be tx_60
    assert_eq!(manager.undo_stack()[49].id, "tx_60");
}

#[test]
fn test_inode_safety_verification_detects_swapped_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("orig.txt");
    let dst = dir.path().join("renamed.txt");

    fs::write(&dst, "initial content").unwrap();
    let original_stat = stat_path(&dst).unwrap();

    let record = FileOperationRecord::new(
        src.clone(),
        dst.clone(),
        original_stat.ino,
        original_stat.dev,
        SystemTime::now(),
        None,
    );

    let tx = UndoTransaction::new(
        "tx_swap",
        UndoOperationType::Rename,
        "Rename orig",
        vec![record],
        SystemTime::now(),
    );

    // Simulate external swap: delete dst and recreate it with a new file (different inode)
    fs::remove_file(&dst).unwrap();
    fs::write(&dst, "external application replaced this file").unwrap();

    let mut manager = UndoManager::new();
    manager.record_transaction(tx);

    let err = manager.undo();
    assert!(err.is_err());
    let err_str = err.unwrap_err().to_string();
    assert!(err_str.contains("inode verification failed"));
}

#[test]
fn test_freedesktop_trash_restoration_via_trashinfo() {
    let dir = tempdir().unwrap();
    let original = dir.path().join("important_doc.pdf");
    let trash_dir = dir.path().join("Trash");
    let trash_files = trash_dir.join("files");
    let trash_info = trash_dir.join("info");

    fs::create_dir_all(&trash_files).unwrap();
    fs::create_dir_all(&trash_info).unwrap();

    let trashed_file = trash_files.join("important_doc.pdf");
    fs::write(&trashed_file, "%PDF-1.4 Mock document").unwrap();
    let stat = stat_path(&trashed_file).unwrap();

    let info_path = trash_info.join("important_doc.pdf.trashinfo");
    let info_body = format!(
        "[Trash Info]\nPath={}\nDeletionDate=2026-10-17T22:30:00\n",
        original.display()
    );
    fs::write(&info_path, info_body).unwrap();

    let record = FileOperationRecord::new(
        original.clone(),
        trashed_file.clone(),
        stat.ino,
        stat.dev,
        SystemTime::now(),
        Some(info_path.clone()),
    );

    let tx = UndoTransaction::new(
        "trash_tx",
        UndoOperationType::Trash,
        "Trash important_doc.pdf",
        vec![record],
        SystemTime::now(),
    );

    let mut manager = UndoManager::new();
    manager.record_transaction(tx);

    assert!(!original.exists());
    assert!(trashed_file.exists());
    assert!(info_path.exists());

    let undone = manager.undo().unwrap().unwrap();
    assert_eq!(undone.id, "trash_tx");

    assert!(original.exists());
    assert_eq!(
        fs::read_to_string(&original).unwrap(),
        "%PDF-1.4 Mock document"
    );
    assert!(!trashed_file.exists());
    assert!(!info_path.exists());
}

#[test]
fn test_journal_persistence_and_reconstruction() {
    let dir = tempdir().unwrap();
    let journal_path = dir.path().join("cache").join("undo_journal.json");

    let live_item = dir.path().join("file.txt");
    fs::write(&live_item, "persisted data").unwrap();

    let rec = FileOperationRecord::new(
        PathBuf::from("/orig/file.txt"),
        live_item.clone(),
        stat_path(&live_item).unwrap().ino,
        stat_path(&live_item).unwrap().dev,
        SystemTime::now(),
        None,
    );

    let tx = UndoTransaction::new(
        "tx_persisted",
        UndoOperationType::Move,
        "Move file.txt",
        vec![rec],
        SystemTime::now(),
    );

    let mut manager = UndoManager::new();
    manager.record_transaction(tx);

    save_journal(&journal_path, &manager).unwrap();
    assert!(journal_path.exists());

    // Simulate process restart
    let restored = load_journal(&journal_path, 50).unwrap();
    assert_eq!(restored.undo_stack().len(), 1);
    assert_eq!(restored.undo_stack()[0].id, "tx_persisted");
    assert_eq!(
        restored.current_undo_label().as_deref(),
        Some("Undo Move file.txt")
    );
}

#[test]
fn test_conflict_resolution_strategies_on_inverse() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("target.txt");
    let dst = dir.path().join("current.txt");

    fs::write(&dst, "current data").unwrap();
    let stat = stat_path(&dst).unwrap();

    let record = FileOperationRecord::new(
        src.clone(),
        dst.clone(),
        stat.ino,
        stat.dev,
        SystemTime::now(),
        None,
    );

    let tx = UndoTransaction::new(
        "tx_conflict",
        UndoOperationType::Move,
        "Move target",
        vec![record],
        SystemTime::now(),
    );

    // 1. Destination already exists and Error strategy
    fs::write(&src, "blocking data").unwrap();
    let err = execute_inverse(&tx, ConflictStrategy::Error);
    assert!(err.is_err());

    // 2. KeepBoth strategy generates auto-incremented name
    execute_inverse(&tx, ConflictStrategy::KeepBoth).unwrap();
    assert!(src.exists());
    assert_eq!(fs::read_to_string(&src).unwrap(), "blocking data");

    let auto_incremented = dir.path().join("target (1).txt");
    assert!(auto_incremented.exists());
    assert_eq!(
        fs::read_to_string(&auto_incremented).unwrap(),
        "current data"
    );
}
