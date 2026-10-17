#![allow(clippy::unwrap_used, clippy::similar_names)]
//! Integration tests for Miller Columns cascading navigation view.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use finder::ui::columns::{
    ColumnContainer, ColumnKeyAction, ColumnListWidget, VIRTUALIZATION_THRESHOLD, format_byte_size,
    format_exact_bytes, format_system_time, map_key_to_column_action,
};
use finder::views::columns::{
    ColumnEntry, ColumnViewModel, DEFAULT_COLUMN_WIDTH, MAX_COLUMN_WIDTH, MIN_COLUMN_WIDTH,
    stat_file_item,
};
use shared::models::{FileItem, FileMetadata};

static GTK_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn create_dummy_item(path: impl AsRef<Path>, is_dir: bool, size: u64) -> FileItem {
    let p = path.as_ref().to_path_buf();
    let name = p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    FileItem::new(
        p,
        name,
        is_dir,
        false,
        None,
        FileMetadata {
            size_bytes: size,
            modified: Some(SystemTime::now()),
            accessed: None,
            created: None,
            permissions: if is_dir { 0o755 } else { 0o644 },
            inode: 100,
            device_id: 1,
            mime_type: if is_dir {
                "inode/directory".to_string()
            } else {
                "text/plain".to_string()
            },
            is_hidden: false,
            is_executable: is_dir,
        },
        vec![],
    )
}

#[test]
fn test_column_view_model_stack_truncation_logic() {
    let mut stack: Vec<ColumnViewModel> = Vec::new();

    // 1. Initial Root Column (index 0)
    let root_entries = vec![
        ColumnEntry::new(create_dummy_item("/root/folder_a", true, 4096), false),
        ColumnEntry::new(create_dummy_item("/root/folder_b", true, 4096), false),
        ColumnEntry::new(create_dummy_item("/root/file.txt", false, 120), false),
    ];
    stack.push(ColumnViewModel::new_directory(
        0,
        DEFAULT_COLUMN_WIDTH,
        PathBuf::from("/root"),
        root_entries,
    ));
    assert_eq!(stack.len(), 1);

    // 2. Select folder_a -> Push child column 1
    stack[0].set_selected_index(Some(0));
    assert!(
        stack[0]
            .selected_entry()
            .is_some_and(|e| e.file_name() == "folder_a")
    );
    assert!(
        stack[0]
            .selected_entry()
            .is_some_and(|e| e.is_expanded_directory)
    );

    let folder_a_entries = vec![ColumnEntry::new(
        create_dummy_item("/root/folder_a/sub_1", true, 4096),
        false,
    )];
    stack.push(ColumnViewModel::new_directory(
        1,
        DEFAULT_COLUMN_WIDTH,
        PathBuf::from("/root/folder_a"),
        folder_a_entries,
    ));
    assert_eq!(stack.len(), 2);

    // 3. Select sub_1 in column 1 -> Push child column 2
    stack[1].set_selected_index(Some(0));
    let sub_entries = vec![ColumnEntry::new(
        create_dummy_item("/root/folder_a/sub_1/doc.pdf", false, 5000),
        false,
    )];
    stack.push(ColumnViewModel::new_directory(
        2,
        DEFAULT_COLUMN_WIDTH,
        PathBuf::from("/root/folder_a/sub_1"),
        sub_entries,
    ));
    assert_eq!(stack.len(), 3);

    // 4. Sibling re-selection in Column 0: user clicks folder_b (index 1)
    // All columns > 0 must be truncated!
    stack.truncate(1);
    assert_eq!(stack.len(), 1);

    stack[0].set_selected_index(Some(1));
    assert!(
        stack[0]
            .selected_entry()
            .is_some_and(|e| e.file_name() == "folder_b")
    );
    assert!(
        stack[0]
            .selected_entry()
            .is_some_and(|e| e.is_expanded_directory)
    );

    // Push new child column 1 for folder_b
    let folder_b_entries = vec![ColumnEntry::new(
        create_dummy_item("/root/folder_b/item.rs", false, 800),
        false,
    )];
    stack.push(ColumnViewModel::new_directory(
        1,
        DEFAULT_COLUMN_WIDTH,
        PathBuf::from("/root/folder_b"),
        folder_b_entries,
    ));
    assert_eq!(stack.len(), 2);
    assert_eq!(stack[1].kind.path(), Path::new("/root/folder_b"));

    // 5. User selects file in Column 0: click file.txt (index 2)
    stack.truncate(1);
    stack[0].set_selected_index(Some(2));
    assert!(
        stack[0]
            .selected_entry()
            .is_some_and(|e| e.file_name() == "file.txt")
    );
    // Since file.txt is not a directory, is_expanded_directory must be false
    assert!(
        !stack[0]
            .selected_entry()
            .is_some_and(|e| e.is_expanded_directory)
    );

    // Push terminal leaf preview column
    let file_item = create_dummy_item("/root/file.txt", false, 120);
    stack.push(ColumnViewModel::new_preview(
        1,
        DEFAULT_COLUMN_WIDTH,
        file_item,
        None,
    ));
    assert_eq!(stack.len(), 2);
    assert!(stack[1].kind.is_file_preview());
}

#[test]
fn test_keyboard_navigation_mapping() {
    assert_eq!(
        map_key_to_column_action(gdk4::Key::Up, gdk4::ModifierType::empty()),
        Some(ColumnKeyAction::Up)
    );
    assert_eq!(
        map_key_to_column_action(gdk4::Key::Down, gdk4::ModifierType::empty()),
        Some(ColumnKeyAction::Down)
    );
    assert_eq!(
        map_key_to_column_action(gdk4::Key::Left, gdk4::ModifierType::empty()),
        Some(ColumnKeyAction::Left)
    );
    assert_eq!(
        map_key_to_column_action(gdk4::Key::Right, gdk4::ModifierType::empty()),
        Some(ColumnKeyAction::Right)
    );
    assert_eq!(
        map_key_to_column_action(gdk4::Key::Return, gdk4::ModifierType::empty()),
        Some(ColumnKeyAction::Enter)
    );
    assert_eq!(
        map_key_to_column_action(gdk4::Key::KP_Enter, gdk4::ModifierType::empty()),
        Some(ColumnKeyAction::Enter)
    );
    assert_eq!(
        map_key_to_column_action(gdk4::Key::space, gdk4::ModifierType::empty()),
        Some(ColumnKeyAction::Spacebar)
    );
    assert_eq!(
        map_key_to_column_action(gdk4::Key::Tab, gdk4::ModifierType::empty()),
        None
    );
}

#[test]
fn test_format_byte_size_and_exact_bytes() {
    assert_eq!(format_byte_size(0), "0 B");
    assert_eq!(format_byte_size(512), "512 B");
    assert_eq!(format_byte_size(1024), "1.0 KB");
    assert_eq!(format_byte_size(4312), "4.2 KB");
    assert_eq!(format_byte_size(1024 * 1024), "1.0 MB");
    assert_eq!(format_byte_size(1024 * 1024 * 1024), "1.0 GB");

    assert_eq!(format_exact_bytes(0), "0 bytes");
    assert_eq!(format_exact_bytes(42), "42 bytes");
    assert_eq!(format_exact_bytes(1000), "1,000 bytes");
    assert_eq!(format_exact_bytes(4312), "4,312 bytes");
    assert_eq!(format_exact_bytes(1_234_567), "1,234,567 bytes");
}

#[test]
fn test_format_system_time() {
    assert_eq!(format_system_time(None), "—");

    let now = SystemTime::now();
    let formatted_now = format_system_time(Some(now));
    assert!(formatted_now.starts_with("Today, "));

    // Past date (UNIX_EPOCH)
    let past = SystemTime::UNIX_EPOCH;
    let formatted_past = format_system_time(Some(past));
    assert!(formatted_past.contains("1969") || formatted_past.contains("1970"));
}

#[tokio::test]
async fn test_stat_file_item_and_broken_symlink() {
    let temp_dir = std::env::temp_dir().join(format!("findx_test_stat_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let regular_file = temp_dir.join("normal.txt");
    fs::write(&regular_file, b"content").unwrap();

    let item = stat_file_item(&temp_dir, "normal.txt").await.unwrap();
    assert_eq!(item.file_name(), "normal.txt");
    assert!(!item.is_dir());
    assert!(!item.is_symlink());
    assert_eq!(item.metadata.size_bytes, 7);

    // Broken symlink pointing to non-existent target
    let broken_link = temp_dir.join("broken.lnk");
    #[cfg(unix)]
    std::os::unix::fs::symlink(temp_dir.join("ghost_file.txt"), &broken_link).unwrap();

    let broken_item = stat_file_item(&temp_dir, "broken.lnk").await.unwrap();
    assert_eq!(broken_item.file_name(), "broken.lnk");
    assert!(broken_item.is_symlink());
    assert_eq!(broken_item.metadata.mime_type, "application/x-symlink");
    assert!(broken_item.symlink_target.is_some());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_virtualization_threshold_constant() {
    assert_eq!(VIRTUALIZATION_THRESHOLD, 30);
}

#[test]
fn test_gtk_widget_auto_fit_and_container() {
    let _lock = GTK_TEST_MUTEX.lock().unwrap();
    let _ = gtk4::init();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    // 1. Auto-fit width calculation
    let model =
        ColumnViewModel::new_directory(0, DEFAULT_COLUMN_WIDTH, PathBuf::from("/empty"), vec![]);
    let widget = ColumnListWidget::new(model);
    assert_eq!(widget.calculate_auto_fit_width(), DEFAULT_COLUMN_WIDTH);

    // Width clamping
    widget.set_width(100);
    assert_eq!(widget.width(), MIN_COLUMN_WIDTH);

    widget.set_width(1200);
    assert_eq!(widget.width(), MAX_COLUMN_WIDTH);

    widget.set_width(320);
    assert_eq!(widget.width(), 320);

    // 2. Container initialization and keyboard actions
    let temp_dir =
        std::env::temp_dir().join(format!("findx_test_container_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let container = ColumnContainer::new(&temp_dir);
    assert_eq!(container.column_count(), 1);

    container.resize_all_columns(260);
    container.handle_key_action(ColumnKeyAction::Down);
    container.handle_key_action(ColumnKeyAction::Up);

    let _ = fs::remove_dir_all(&temp_dir);
}
