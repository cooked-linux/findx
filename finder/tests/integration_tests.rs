#![allow(clippy::unwrap_used, clippy::expect_used, clippy::similar_names)]
//! End-to-end integration tests for Finder application lifecycle, view switching,
//! navigation coordination, and D-Bus IPC routing.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use finder::app::{FinderCliArgs, resolve_target_path};
use finder::coordinator::NavigationCoordinator;
use finder::service::{FileManagerAction, FileManagerService};
use finder::ui::view_mode::ViewMode;
use finder::ui::window::{FinderWindow, WindowKeyAction, map_key_to_window_action};
use shared::ipc::path_to_file_uri;

static GTK_TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Helper to set up a temporary directory hierarchy for integration testing.
fn setup_temp_hierarchy() -> PathBuf {
    let base = std::env::temp_dir().join(format!("findx_integ_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("Documents/Work")).unwrap();
    fs::create_dir_all(base.join("Downloads")).unwrap();
    fs::create_dir_all(base.join("Music")).unwrap();
    fs::write(base.join("Documents/report.pdf"), b"sample pdf content").unwrap();
    fs::write(base.join("Documents/Work/notes.txt"), b"sample notes").unwrap();
    fs::write(base.join("Downloads/archive.tar.gz"), b"sample archive").unwrap();
    base
}

#[test]
fn test_coordinator_lifecycle_and_history_transitions() {
    let base = setup_temp_hierarchy();

    let coordinator = NavigationCoordinator::new(&base);
    assert_eq!(coordinator.current_path(), base);
    assert!(!coordinator.can_go_back());
    assert!(!coordinator.can_go_forward());
    assert_eq!(coordinator.view_mode(), ViewMode::Columns);

    // 1. Navigate to Documents
    let docs = base.join("Documents");
    coordinator.navigate_to(&docs);
    assert_eq!(coordinator.current_path(), docs);
    assert!(coordinator.can_go_back());
    assert!(!coordinator.can_go_forward());

    // 2. Navigate to Documents/Work
    let work = docs.join("Work");
    coordinator.navigate_to(&work);
    assert_eq!(coordinator.current_path(), work);
    assert!(coordinator.can_go_back());
    assert!(!coordinator.can_go_forward());

    // 3. Navigate back to Documents
    let back1 = coordinator.go_back();
    assert_eq!(back1, Some(docs.clone()));
    assert_eq!(coordinator.current_path(), docs);
    assert!(coordinator.can_go_back());
    assert!(coordinator.can_go_forward());

    // 4. Navigate back to base
    let back2 = coordinator.go_back();
    assert_eq!(back2, Some(base.clone()));
    assert_eq!(coordinator.current_path(), base);
    assert!(!coordinator.can_go_back());
    assert!(coordinator.can_go_forward());

    // 5. Navigate forward to Documents
    let fwd1 = coordinator.go_forward();
    assert_eq!(fwd1, Some(docs.clone()));
    assert_eq!(coordinator.current_path(), docs);
    assert!(coordinator.can_go_back());
    assert!(coordinator.can_go_forward());

    // 6. Navigate forward to Work
    let fwd2 = coordinator.go_forward();
    assert_eq!(fwd2, Some(work.clone()));
    assert_eq!(coordinator.current_path(), work);
    assert!(coordinator.can_go_back());
    assert!(!coordinator.can_go_forward());

    let _ = fs::remove_dir_all(&base);
}

#[tokio::test]
async fn test_dbus_show_folders_and_items_uri_routing() {
    let base = setup_temp_hierarchy();
    let (action_tx, mut action_rx) = tokio::sync::mpsc::unbounded_channel::<FileManagerAction>();
    let service = FileManagerService::new(action_tx);

    // 1. Test show_folders URI routing
    let downloads_dir = base.join("Downloads");
    let downloads_uri = path_to_file_uri(&downloads_dir).unwrap();
    let res = service
        .show_folders(vec![downloads_uri], "startup_id_folders".to_string())
        .await;
    assert!(res.is_ok());

    let action = action_rx.recv().await.expect("show_folders action sent");
    assert_eq!(
        action,
        FileManagerAction::ShowFolders {
            paths: vec![downloads_dir.clone()],
            startup_id: "startup_id_folders".to_string(),
        }
    );

    // 2. Test show_items URI routing with file
    let notes_file = base.join("Documents/Work/notes.txt");
    let notes_uri = path_to_file_uri(&notes_file).unwrap();
    let res = service
        .show_items(vec![notes_uri], "startup_id_items".to_string())
        .await;
    assert!(res.is_ok());

    let action = action_rx.recv().await.expect("show_items action sent");
    assert_eq!(
        action,
        FileManagerAction::ShowItems {
            paths: vec![notes_file.clone()],
            startup_id: "startup_id_items".to_string(),
        }
    );

    // 3. Test show_item_properties URI routing
    let report_file = base.join("Documents/report.pdf");
    let report_uri = path_to_file_uri(&report_file).unwrap();
    let res = service
        .show_item_properties(vec![report_uri], "startup_id_props".to_string())
        .await;
    assert!(res.is_ok());

    let action = action_rx
        .recv()
        .await
        .expect("show_item_properties action sent");
    assert_eq!(
        action,
        FileManagerAction::ShowItemProperties {
            paths: vec![report_file],
            startup_id: "startup_id_props".to_string(),
        }
    );

    // 4. Test invalid URI rejection
    let invalid_res = service
        .show_folders(
            vec!["https://example.com/not_file".to_string()],
            String::new(),
        )
        .await;
    assert!(invalid_res.is_err());

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn test_cli_argument_parsing_and_resolution() {
    // 1. Version flag
    let args = vec!["findx", "--version"];
    let parsed = FinderCliArgs::parse(args);
    assert!(parsed.show_version);
    assert!(!parsed.new_window);
    assert_eq!(parsed.target_path, None);

    // 2. New window flag with target directory
    let args = vec!["findx", "--new-window", "/usr/share"];
    let parsed = FinderCliArgs::parse(args);
    assert!(!parsed.show_version);
    assert!(parsed.new_window);
    assert_eq!(parsed.target_path, Some(PathBuf::from("/usr/share")));

    // 3. Positional path resolution
    let target = Path::new("subfolder");
    let cwd = Path::new("/home/user");
    let resolved = resolve_target_path(Some(target), Some(cwd));
    assert_eq!(resolved, PathBuf::from("/home/user/subfolder"));
}

#[test]
fn test_window_keyboard_action_mapping() {
    use gdk4::ModifierType;

    // View Switching
    assert_eq!(
        map_key_to_window_action(gdk4::Key::_1, ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::SwitchView(ViewMode::Icons))
    );
    assert_eq!(
        map_key_to_window_action(gdk4::Key::_2, ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::SwitchView(ViewMode::List))
    );
    assert_eq!(
        map_key_to_window_action(gdk4::Key::_3, ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::SwitchView(ViewMode::Columns))
    );

    // History traversal
    assert_eq!(
        map_key_to_window_action(gdk4::Key::Left, ModifierType::ALT_MASK),
        Some(WindowKeyAction::HistoryBack)
    );
    assert_eq!(
        map_key_to_window_action(gdk4::Key::Right, ModifierType::ALT_MASK),
        Some(WindowKeyAction::HistoryForward)
    );

    // Global panels
    assert_eq!(
        map_key_to_window_action(
            gdk4::Key::s,
            ModifierType::CONTROL_MASK | ModifierType::ALT_MASK
        ),
        Some(WindowKeyAction::ToggleSidebar)
    );
    assert_eq!(
        map_key_to_window_action(
            gdk4::Key::p,
            ModifierType::CONTROL_MASK | ModifierType::ALT_MASK
        ),
        Some(WindowKeyAction::TogglePathBar)
    );
}

#[test]
fn test_gtk_window_view_switching_and_navigation() {
    let _lock = GTK_TEST_MUTEX.lock().unwrap();
    if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err()) {
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let base = setup_temp_hierarchy();

    // 1. Initialize window with temporary directory hierarchy
    let win = FinderWindow::with_initial_path(None, &base);
    assert_eq!(win.coordinator.current_path(), base);
    assert_eq!(win.view_mode(), ViewMode::Columns);
    assert_eq!(
        win.stack.visible_child_name().as_deref(),
        Some(ViewMode::Columns.as_str())
    );

    // 2. View mode switcher transitions: Columns -> List
    win.set_view_mode(ViewMode::List);
    assert_eq!(win.view_mode(), ViewMode::List);
    assert_eq!(
        win.stack.visible_child_name().as_deref(),
        Some(ViewMode::List.as_str())
    );

    // 3. View mode switcher transitions: List -> Icons
    win.set_view_mode(ViewMode::Icons);
    assert_eq!(win.view_mode(), ViewMode::Icons);
    assert_eq!(
        win.stack.visible_child_name().as_deref(),
        Some(ViewMode::Icons.as_str())
    );

    // 4. View mode switcher transitions: Icons -> Columns
    win.set_view_mode(ViewMode::Columns);
    assert_eq!(win.view_mode(), ViewMode::Columns);
    assert_eq!(
        win.stack.visible_child_name().as_deref(),
        Some(ViewMode::Columns.as_str())
    );

    // 5. History traversal on window
    let docs = base.join("Documents");
    win.navigate_to(&docs);
    assert_eq!(win.coordinator.current_path(), docs);
    assert!(win.coordinator.can_go_back());

    win.go_back();
    assert_eq!(win.coordinator.current_path(), base);
    assert!(win.coordinator.can_go_forward());

    win.go_forward();
    assert_eq!(win.coordinator.current_path(), docs);

    let _ = fs::remove_dir_all(&base);
}
