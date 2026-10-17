//! Complete keyboard navigation controller for Miller Columns.

use gtk4::prelude::*;

/// Actions triggered by Miller Columns keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKeyAction {
    /// Select previous row in active column (`Up`).
    Up,
    /// Select next row in active column (`Down`).
    Down,
    /// Focus parent column to the left (`Left`).
    Left,
    /// Focus child column to the right, selecting first entry if unselected (`Right`).
    Right,
    /// Activate / open selected item (`Enter` / `Return`).
    Enter,
    /// Trigger Quick Look preview (`Spacebar`).
    Spacebar,
}

/// Maps key values to [`ColumnKeyAction`] variants.
#[must_use]
pub const fn map_key_to_column_action(
    keyval: gdk4::Key,
    _state: gdk4::ModifierType,
) -> Option<ColumnKeyAction> {
    match keyval {
        gdk4::Key::Up => Some(ColumnKeyAction::Up),
        gdk4::Key::Down => Some(ColumnKeyAction::Down),
        gdk4::Key::Left => Some(ColumnKeyAction::Left),
        gdk4::Key::Right => Some(ColumnKeyAction::Right),
        gdk4::Key::Return | gdk4::Key::KP_Enter => Some(ColumnKeyAction::Enter),
        gdk4::Key::space => Some(ColumnKeyAction::Spacebar),
        _ => None,
    }
}

/// Attaches keyboard navigation event controller to `target_widget`.
pub fn attach_column_keyboard_controller<F>(target_widget: &impl IsA<gtk4::Widget>, on_action: F)
where
    F: Fn(ColumnKeyAction) -> glib::Propagation + 'static,
{
    let controller = gtk4::EventControllerKey::new();
    controller.connect_key_pressed(move |_ctrl, keyval, _code, state| {
        map_key_to_column_action(keyval, state).map_or(glib::Propagation::Proceed, &on_action)
    });
    target_widget.add_controller(controller);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_key_to_column_action() {
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
            map_key_to_column_action(gdk4::Key::Escape, gdk4::ModifierType::empty()),
            None
        );
    }
}
