//! Keyboard navigation controller for Hierarchical List View.
//!
//! Maps navigation keys:
//! - `Right`: If selected row is collapsed folder, expand it; if already expanded, move selection to first child.
//! - `Left`: If selected row is expanded folder, collapse it; if collapsed or a file, select parent directory row.
//! - `Up` / `Down`: Move selection up/down flattened visible rows.
//! - `Enter`: Open file or ascend into directory.
//! - `Spacebar`: Trigger Quick Look preview on selected item.

use gtk4::prelude::*;

/// Actions triggered by Hierarchical List View keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKeyAction {
    /// Move selection to previous row (`Up`).
    Up,
    /// Move selection to next row (`Down`).
    Down,
    /// Expand directory or step into first child (`Right`).
    Right,
    /// Collapse directory or step out to parent (`Left`).
    Left,
    /// Open file or ascend into directory (`Enter` / `Return`).
    Enter,
    /// Trigger Quick Look preview on selected item (`Spacebar`).
    Spacebar,
}

/// Maps key values to [`ListKeyAction`] variants.
#[must_use]
pub const fn map_key_to_list_action(
    keyval: gdk4::Key,
    _state: gdk4::ModifierType,
) -> Option<ListKeyAction> {
    match keyval {
        gdk4::Key::Up => Some(ListKeyAction::Up),
        gdk4::Key::Down => Some(ListKeyAction::Down),
        gdk4::Key::Right => Some(ListKeyAction::Right),
        gdk4::Key::Left => Some(ListKeyAction::Left),
        gdk4::Key::Return | gdk4::Key::KP_Enter => Some(ListKeyAction::Enter),
        gdk4::Key::space => Some(ListKeyAction::Spacebar),
        _ => None,
    }
}

/// Attaches keyboard navigation event controller to `target_widget`.
pub fn attach_list_keyboard_controller<F>(target_widget: &impl IsA<gtk4::Widget>, on_action: F)
where
    F: Fn(ListKeyAction) -> glib::Propagation + 'static,
{
    let controller = gtk4::EventControllerKey::new();
    controller.connect_key_pressed(move |_ctrl, keyval, _code, state| {
        map_key_to_list_action(keyval, state).map_or(glib::Propagation::Proceed, &on_action)
    });
    target_widget.add_controller(controller);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_key_to_list_action() {
        assert_eq!(
            map_key_to_list_action(gdk4::Key::Up, gdk4::ModifierType::empty()),
            Some(ListKeyAction::Up)
        );
        assert_eq!(
            map_key_to_list_action(gdk4::Key::Down, gdk4::ModifierType::empty()),
            Some(ListKeyAction::Down)
        );
        assert_eq!(
            map_key_to_list_action(gdk4::Key::Right, gdk4::ModifierType::empty()),
            Some(ListKeyAction::Right)
        );
        assert_eq!(
            map_key_to_list_action(gdk4::Key::Left, gdk4::ModifierType::empty()),
            Some(ListKeyAction::Left)
        );
        assert_eq!(
            map_key_to_list_action(gdk4::Key::Return, gdk4::ModifierType::empty()),
            Some(ListKeyAction::Enter)
        );
        assert_eq!(
            map_key_to_list_action(gdk4::Key::KP_Enter, gdk4::ModifierType::empty()),
            Some(ListKeyAction::Enter)
        );
        assert_eq!(
            map_key_to_list_action(gdk4::Key::space, gdk4::ModifierType::empty()),
            Some(ListKeyAction::Spacebar)
        );
        assert_eq!(
            map_key_to_list_action(gdk4::Key::Escape, gdk4::ModifierType::empty()),
            None
        );
    }
}
