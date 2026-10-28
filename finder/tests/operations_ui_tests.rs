#![allow(clippy::unwrap_used)]
//! Integration and widget tests for file operations progress dialog, queue popover, and conflict resolution sheet.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use finder::operations::conflict::{
    ConflictAction, ConflictContext, ConflictItemMeta, RelativeTimeStatus,
};
use finder::operations::queue::OperationsQueue;
use finder::ui::operations::conflict_dialog::{
    ConflictButtonStyle, determine_action_buttons, determine_age_badges, format_modified_time,
    guess_icon_for_path,
};
use finder::ui::operations::progress_dialog::{
    LARGE_OPERATION_BYTES_THRESHOLD, LARGE_OPERATION_ITEMS_THRESHOLD, format_operation_title,
    format_transfer_metrics, is_large_operation, operation_kind_icon,
};
use finder::ui::operations::theme::OPERATIONS_CSS;
use finder::ui::operations::{
    ConflictResolutionDialog, OperationsQueuePopover, QueueBadgeController, TransferProgressDialog,
};
use gtk4::prelude::*;
use shared::models::operation::{BatchOperation, OperationKind, ProgressReport};
use tokio::sync::oneshot;

static GTK_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn create_sample_conflict_context(
    is_dir: bool,
    relative_time: RelativeTimeStatus,
) -> ConflictContext {
    let src = ConflictItemMeta::new(
        PathBuf::from("/source/project.rs"),
        is_dir,
        4096,
        Some(SystemTime::now()),
        None,
    );
    let dst = ConflictItemMeta::new(
        PathBuf::from("/dest/project.rs"),
        is_dir,
        2048,
        Some(SystemTime::UNIX_EPOCH),
        None,
    );

    ConflictContext::new(
        "op-integration-conflict".to_string(),
        src,
        dst,
        is_dir,
        relative_time,
    )
}

#[test]
fn test_operations_theme_css_contains_all_classes() {
    assert!(OPERATIONS_CSS.contains(".operations-progress-dialog"));
    assert!(OPERATIONS_CSS.contains(".operations-progress-title"));
    assert!(OPERATIONS_CSS.contains(".operations-progress-subtitle"));
    assert!(OPERATIONS_CSS.contains(".operations-progress-metrics"));
    assert!(OPERATIONS_CSS.contains(".conflict-dialog-card"));
    assert!(OPERATIONS_CSS.contains(".conflict-age-badge-newer"));
    assert!(OPERATIONS_CSS.contains(".conflict-age-badge-older"));
    assert!(OPERATIONS_CSS.contains(".operations-queue-popover"));
    assert!(OPERATIONS_CSS.contains(".operations-queue-row"));
    assert!(OPERATIONS_CSS.contains(".operations-queue-empty"));
    assert!(OPERATIONS_CSS.contains(".queue-btn-active"));
}

#[test]
fn test_format_operation_title_comprehensive() {
    // Copy
    assert_eq!(
        format_operation_title(OperationKind::Copy, 1, Some("Music")),
        "Copying 1 item to 'Music'"
    );
    assert_eq!(
        format_operation_title(OperationKind::Copy, 42, Some("Backup")),
        "Copying 42 items to 'Backup'"
    );
    assert_eq!(
        format_operation_title(OperationKind::Copy, 10, None),
        "Copying 10 items"
    );

    // Move
    assert_eq!(
        format_operation_title(OperationKind::Move, 1, Some("Trash")),
        "Moving 1 item to 'Trash'"
    );
    assert_eq!(
        format_operation_title(OperationKind::Move, 3, None),
        "Moving 3 items"
    );

    // Other kinds
    assert_eq!(
        format_operation_title(OperationKind::Duplicate, 1, None),
        "Duplicating 1 item"
    );
    assert_eq!(
        format_operation_title(OperationKind::Trash, 7, None),
        "Moving 7 items to Trash"
    );
    assert_eq!(
        format_operation_title(OperationKind::Restore, 2, Some("Desktop")),
        "Restoring 2 items to 'Desktop'"
    );
    assert_eq!(
        format_operation_title(OperationKind::Delete, 1, None),
        "Deleting 1 item"
    );
    assert_eq!(
        format_operation_title(OperationKind::Compress, 12, None),
        "Compressing 12 items"
    );
    assert_eq!(
        format_operation_title(OperationKind::Compress, 1, Some("Archive.zip")),
        "Compressing 'Archive.zip'"
    );
}

#[test]
fn test_is_large_operation_thresholds() {
    // Normal small operation
    assert!(!is_large_operation(10 * 1024 * 1024, 10));

    // Exceeds bytes threshold
    assert!(is_large_operation(LARGE_OPERATION_BYTES_THRESHOLD, 1));
    assert!(is_large_operation(LARGE_OPERATION_BYTES_THRESHOLD + 1, 1));

    // Exceeds items threshold
    assert!(is_large_operation(1024, LARGE_OPERATION_ITEMS_THRESHOLD));
    assert!(is_large_operation(
        1024,
        LARGE_OPERATION_ITEMS_THRESHOLD + 10
    ));
}

#[test]
fn test_format_transfer_metrics_scenarios() {
    // Full metrics with remaining ETA
    let m1 = format_transfer_metrics(1_450_000_000, 12_800_000_000, 145_000_000.0, Some(45.0));
    assert!(m1.contains("About 45 seconds remaining"));
    assert!(m1.contains("of"));

    // ETA < 5s
    let m2 = format_transfer_metrics(500, 1000, 100.0, Some(2.0));
    assert!(m2.contains("Less than 5 seconds remaining"));

    // ETA minutes
    let m3 = format_transfer_metrics(500, 1000, 100.0, Some(180.0));
    assert!(m3.contains("About 3 minutes remaining"));

    // Zero total bytes (e.g. calculating)
    let m4 = format_transfer_metrics(500, 0, 0.0, None);
    assert!(!m4.contains("of"));
    assert!(m4.contains("Estimating…"));
}

#[test]
fn test_operation_kind_icons() {
    assert_eq!(
        operation_kind_icon(OperationKind::Copy),
        "folder-copy-symbolic"
    );
    assert_eq!(
        operation_kind_icon(OperationKind::Move),
        "edit-cut-symbolic"
    );
    assert_eq!(
        operation_kind_icon(OperationKind::Trash),
        "user-trash-symbolic"
    );
    assert_eq!(
        operation_kind_icon(OperationKind::Delete),
        "user-trash-symbolic"
    );
    assert_eq!(
        operation_kind_icon(OperationKind::Duplicate),
        "folder-copy-symbolic"
    );
    assert_eq!(
        operation_kind_icon(OperationKind::Restore),
        "edit-undo-symbolic"
    );
    assert_eq!(
        operation_kind_icon(OperationKind::Compress),
        "package-x-generic-symbolic"
    );
}

#[test]
fn test_determine_age_badges_all_variants() {
    let (d1, s1) = determine_age_badges(RelativeTimeStatus::IncomingNewer);
    assert_eq!(d1.label(), Some("Older"));
    assert_eq!(d1.css_class(), Some("conflict-age-badge-older"));
    assert_eq!(s1.label(), Some("Newer"));
    assert_eq!(s1.css_class(), Some("conflict-age-badge-newer"));

    let (d2, s2) = determine_age_badges(RelativeTimeStatus::IncomingOlder);
    assert_eq!(d2.label(), Some("Newer"));
    assert_eq!(s2.label(), Some("Older"));

    let (d3, s3) = determine_age_badges(RelativeTimeStatus::IdenticalTime);
    assert_eq!(d3.label(), None);
    assert_eq!(s3.label(), None);
}

#[test]
fn test_determine_action_buttons_file_and_directory() {
    // 1. File vs File, Incoming is Newer
    let file_btns_newer = determine_action_buttons(false, RelativeTimeStatus::IncomingNewer);
    assert_eq!(file_btns_newer.len(), 4);
    assert_eq!(file_btns_newer[0].action, ConflictAction::Replace);
    assert_eq!(file_btns_newer[0].style, ConflictButtonStyle::Default);
    assert_eq!(file_btns_newer[1].action, ConflictAction::KeepBoth);
    assert_eq!(file_btns_newer[1].style, ConflictButtonStyle::Suggested);
    assert_eq!(file_btns_newer[2].action, ConflictAction::Skip);
    assert_eq!(file_btns_newer[3].action, ConflictAction::Stop);

    // 2. File vs File, Incoming is Older -> Replace should be Destructive
    let file_btns_older = determine_action_buttons(false, RelativeTimeStatus::IncomingOlder);
    assert_eq!(file_btns_older[0].action, ConflictAction::Replace);
    assert_eq!(file_btns_older[0].style, ConflictButtonStyle::Destructive);

    // 3. Directory vs Directory -> Merge should be Suggested, Replace Destructive
    let dir_btns = determine_action_buttons(true, RelativeTimeStatus::IncomingNewer);
    assert_eq!(dir_btns.len(), 5);
    assert_eq!(dir_btns[0].action, ConflictAction::Merge);
    assert_eq!(dir_btns[0].style, ConflictButtonStyle::Suggested);
    assert_eq!(dir_btns[1].action, ConflictAction::Replace);
    assert_eq!(dir_btns[1].style, ConflictButtonStyle::Destructive);
    assert_eq!(dir_btns[2].action, ConflictAction::KeepBoth);
    assert_eq!(dir_btns[3].action, ConflictAction::Skip);
    assert_eq!(dir_btns[4].action, ConflictAction::Stop);
}

#[test]
fn test_guess_icon_for_path_extensions() {
    assert_eq!(guess_icon_for_path(Path::new("folder"), true), "folder");
    assert_eq!(
        guess_icon_for_path(Path::new("photo.jpg"), false),
        "image-x-generic"
    );
    assert_eq!(
        guess_icon_for_path(Path::new("movie.mkv"), false),
        "video-x-generic"
    );
    assert_eq!(
        guess_icon_for_path(Path::new("song.mp3"), false),
        "audio-x-generic"
    );
    assert_eq!(
        guess_icon_for_path(Path::new("manual.pdf"), false),
        "application-pdf"
    );
    assert_eq!(
        guess_icon_for_path(Path::new("archive.zip"), false),
        "package-x-generic"
    );
    assert_eq!(
        guess_icon_for_path(Path::new("main.rs"), false),
        "text-x-script"
    );
    assert_eq!(
        guess_icon_for_path(Path::new("unknown.xyz"), false),
        "text-x-generic"
    );
}

#[test]
fn test_format_modified_time_handling() {
    assert_eq!(format_modified_time(None), "Unknown date");
    let now = SystemTime::now();
    let formatted = format_modified_time(Some(now));
    assert_ne!(formatted, "");
    assert_ne!(formatted, "Unknown date");
}

#[test]
#[allow(clippy::too_many_lines)]
fn test_gtk_operations_ui_components() {
    let _lock = GTK_TEST_MUTEX.lock().unwrap();
    if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err()) {
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    // 1. Progress dialog lifecycle & updates
    {
        let dialog = TransferProgressDialog::new(
            "op-test-lifecycle",
            OperationKind::Copy,
            5,
            Some("Downloads"),
        );

        assert_eq!(dialog.title_label.text(), "Copying 5 items to 'Downloads'");
        assert_eq!(dialog.progress_bar.fraction(), 0.0);

        // Update progress report
        let report = ProgressReport::new(
            "op-test-lifecycle",
            2,
            5,
            500,
            1000,
            100.0,
            Some(5.0),
            Some(PathBuf::from("file2.txt")),
        );
        dialog.update_progress(&report);

        assert_eq!(dialog.progress_bar.fraction(), 0.5);
        assert_eq!(dialog.subtitle_label.text(), "file2.txt");

        // Pause and resume
        dialog.set_paused(true);
        assert!(dialog.metrics_label.text().ends_with("(Paused)"));
        dialog.set_paused(false);
        assert!(!dialog.metrics_label.text().ends_with("(Paused)"));

        // Complete
        dialog.mark_completed();
        assert_eq!(dialog.progress_bar.fraction(), 1.0);
        assert_eq!(dialog.subtitle_label.text(), "Completed");
    }

    // 2. Queue Badge and Popover binding
    {
        let btn = gtk4::Button::with_label("Queue");
        let controller = QueueBadgeController::new(btn.clone());
        let popover = OperationsQueuePopover::new(&btn);

        assert!(popover.empty_label.property::<bool>("visible"));
        assert!(!popover.list_box.property::<bool>("visible"));
        assert_eq!(controller.current_badge_count(), 0);

        // Simulate active task
        controller.update(2);
        assert_eq!(btn.label().as_deref(), Some("Queue (2)"));
        assert!(btn.has_css_class("queue-btn-active"));
        assert_eq!(controller.current_badge_count(), 2);

        // Refresh with empty queue
        let queue = OperationsQueue::default();
        popover.refresh(&queue);
        assert!(popover.empty_label.property::<bool>("visible"));
        assert!(!popover.list_box.property::<bool>("visible"));

        // Enqueue an operation into queue with valid temp files
        let temp_dir = tempfile::tempdir().unwrap();
        let src_file = temp_dir.path().join("test.txt");
        std::fs::write(&src_file, b"sample content").unwrap();
        let dst_dir = temp_dir.path().join("dst");
        std::fs::create_dir_all(&dst_dir).unwrap();

        let op = BatchOperation::new(
            "op-popover-1",
            OperationKind::Copy,
            vec![src_file],
            Some(dst_dir),
        );
        queue.enqueue_operation(op);

        popover.refresh(&queue);
        assert!(!popover.empty_label.property::<bool>("visible"));
        assert!(popover.list_box.property::<bool>("visible"));

        // Detach popover before dropping button
        popover.unparent();
    }

    // 3. Conflict Dialog actions and Alt modifier
    {
        // 3.1. Test Replace action with Apply to All
        let (tx1, mut rx1) = oneshot::channel();
        let ctx1 = create_sample_conflict_context(false, RelativeTimeStatus::IncomingNewer);
        let dialog1 = ConflictResolutionDialog::new(ctx1, tx1);

        dialog1.apply_to_all_check.set_active(true);
        dialog1.trigger_action(ConflictAction::Replace);

        let res1 = rx1.try_recv().unwrap();
        assert_eq!(res1.action, ConflictAction::Replace);
        assert!(res1.apply_to_all);

        // 3.2. Test Alt/Option toggling Stop -> Skip
        let (tx2, _rx2) = oneshot::channel();
        let ctx2 = create_sample_conflict_context(false, RelativeTimeStatus::IncomingNewer);
        let dialog2 = ConflictResolutionDialog::new(ctx2, tx2);

        let stop_btn = dialog2.stop_button.as_ref().unwrap();
        assert_eq!(stop_btn.label().as_deref(), Some("Stop"));

        dialog2.set_alt_pressed(true);
        assert_eq!(stop_btn.label().as_deref(), Some("Skip"));

        dialog2.set_alt_pressed(false);
        assert_eq!(stop_btn.label().as_deref(), Some("Stop"));

        // 3.3. Test Directory Collision Merge Action
        let (tx3, mut rx3) = oneshot::channel();
        let ctx3 = create_sample_conflict_context(true, RelativeTimeStatus::IncomingNewer);
        let dialog3 = ConflictResolutionDialog::new(ctx3, tx3);

        dialog3.trigger_action(ConflictAction::Merge);
        let res3 = rx3.try_recv().unwrap();
        assert_eq!(res3.action, ConflictAction::Merge);
        assert!(!res3.apply_to_all);

        // 3.4. Test Skip Action
        let (tx4, mut rx4) = oneshot::channel();
        let ctx4 = create_sample_conflict_context(false, RelativeTimeStatus::IncomingNewer);
        let dialog4 = ConflictResolutionDialog::new(ctx4, tx4);

        dialog4.trigger_action(ConflictAction::Skip);
        let res4 = rx4.try_recv().unwrap();
        assert_eq!(res4.action, ConflictAction::Skip);

        // 3.5. Test Stop Action
        let (tx5, mut rx5) = oneshot::channel();
        let ctx5 = create_sample_conflict_context(false, RelativeTimeStatus::IncomingNewer);
        let dialog5 = ConflictResolutionDialog::new(ctx5, tx5);

        dialog5.trigger_action(ConflictAction::Stop);
        let res5 = rx5.try_recv().unwrap();
        assert_eq!(res5.action, ConflictAction::Stop);
    }
}
