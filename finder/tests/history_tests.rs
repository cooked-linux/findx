//! Integration tests for `NavigationHistory` and Window primitives.

use std::path::{Path, PathBuf};

use finder::history::NavigationHistory;
use finder::ui::view_mode::ViewMode;
use finder::ui::window::{WindowKeyAction, map_key_to_window_action};

#[test]
fn test_history_transitions() {
    let mut history = NavigationHistory::new("/home/user");
    assert_eq!(history.current_path(), Path::new("/home/user"));
    assert!(!history.can_go_back());
    assert!(!history.can_go_forward());

    // Navigate deeper
    history.navigate_to("/home/user/Documents");
    assert_eq!(history.current_path(), Path::new("/home/user/Documents"));
    assert!(history.can_go_back());
    assert!(!history.can_go_forward());

    history.navigate_to("/home/user/Documents/Projects");
    assert_eq!(
        history.current_path(),
        Path::new("/home/user/Documents/Projects")
    );
    assert_eq!(history.back_stack().len(), 2);

    // Go back
    let prev = history.go_back();
    assert_eq!(prev, Some(PathBuf::from("/home/user/Documents")));
    assert_eq!(history.current_path(), Path::new("/home/user/Documents"));
    assert!(history.can_go_back());
    assert!(history.can_go_forward());
    assert_eq!(history.forward_stack().len(), 1);

    // Go forward
    let next = history.go_forward();
    assert_eq!(next, Some(PathBuf::from("/home/user/Documents/Projects")));
    assert_eq!(
        history.current_path(),
        Path::new("/home/user/Documents/Projects")
    );
    assert!(history.can_go_back());
    assert!(!history.can_go_forward());
}

#[test]
fn test_history_clears_forward_on_new_path() {
    let mut history = NavigationHistory::new("/root");
    history.navigate_to("/root/folder1");
    history.navigate_to("/root/folder2");

    assert_eq!(history.go_back(), Some(PathBuf::from("/root/folder1")));
    assert!(history.can_go_forward());
    assert_eq!(history.forward_stack().len(), 1);

    // Branching navigation
    history.navigate_to("/root/folder3");
    assert_eq!(history.current_path(), Path::new("/root/folder3"));
    assert!(!history.can_go_forward());
    assert_eq!(history.forward_stack().len(), 0);
    assert_eq!(history.back_stack().len(), 2);
}

#[test]
fn test_history_deep_cap_at_100() {
    let mut history = NavigationHistory::new("/dir/0");
    for i in 1..=150 {
        history.navigate_to(format!("/dir/{i}"));
    }

    assert_eq!(history.back_stack().len(), 100);
    assert_eq!(history.current_path(), Path::new("/dir/150"));
    // Oldest entry should be /dir/50 because 150 items were pushed and cap is 100
    assert_eq!(history.back_stack()[0], PathBuf::from("/dir/50"));
}

#[test]
fn test_history_noop_on_current_path() {
    let mut history = NavigationHistory::new("/home/user");
    history.navigate_to("/home/user");
    assert_eq!(history.current_path(), Path::new("/home/user"));
    assert!(!history.can_go_back());
    assert_eq!(history.back_stack().len(), 0);
    assert_eq!(history.forward_stack().len(), 0);
}

#[test]
fn test_window_shortcut_key_mapping() {
    // History back (Alt+Left)
    assert_eq!(
        map_key_to_window_action(gdk4::Key::Left, gdk4::ModifierType::ALT_MASK),
        Some(WindowKeyAction::HistoryBack)
    );

    // History forward (Alt+Right)
    assert_eq!(
        map_key_to_window_action(gdk4::Key::Right, gdk4::ModifierType::ALT_MASK),
        Some(WindowKeyAction::HistoryForward)
    );

    // View modes (Ctrl+1, Ctrl+2, Ctrl+3)
    assert_eq!(
        map_key_to_window_action(gdk4::Key::_1, gdk4::ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::SwitchView(ViewMode::Icons))
    );
    assert_eq!(
        map_key_to_window_action(gdk4::Key::_2, gdk4::ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::SwitchView(ViewMode::List))
    );
    assert_eq!(
        map_key_to_window_action(gdk4::Key::_3, gdk4::ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::SwitchView(ViewMode::Columns))
    );

    // Focus Search (Ctrl+F, Ctrl+f)
    assert_eq!(
        map_key_to_window_action(gdk4::Key::f, gdk4::ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::FocusSearch)
    );
    assert_eq!(
        map_key_to_window_action(gdk4::Key::F, gdk4::ModifierType::CONTROL_MASK),
        Some(WindowKeyAction::FocusSearch)
    );

    // Non-matching keys
    assert_eq!(
        map_key_to_window_action(gdk4::Key::Return, gdk4::ModifierType::empty()),
        None
    );
}
