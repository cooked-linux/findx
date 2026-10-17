//! Search bar header and input component.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use gtk4::prelude::*;

pub use crate::providers::compute_ghost_completion;

/// Actions emitted on special navigation keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Up,
    Down,
    Enter,
    Tab,
    Escape,
    Toggle,
}

/// Maps GDK keys and modifier states to high-level navigation `KeyAction`.
#[must_use]
pub const fn map_key_to_action(key: gdk4::Key, modifiers: gdk4::ModifierType) -> Option<KeyAction> {
    let ctrl = modifiers.contains(gdk4::ModifierType::CONTROL_MASK);
    let super_mod = modifiers.contains(gdk4::ModifierType::SUPER_MASK)
        || modifiers.contains(gdk4::ModifierType::META_MASK);

    match key {
        gdk4::Key::space if super_mod => Some(KeyAction::Toggle),
        gdk4::Key::Up => Some(KeyAction::Up),
        gdk4::Key::Down => Some(KeyAction::Down),
        gdk4::Key::k | gdk4::Key::K if ctrl => Some(KeyAction::Up),
        gdk4::Key::j | gdk4::Key::J if ctrl => Some(KeyAction::Down),
        gdk4::Key::Return | gdk4::Key::KP_Enter => Some(KeyAction::Enter),
        gdk4::Key::Tab | gdk4::Key::ISO_Left_Tab => Some(KeyAction::Tab),
        gdk4::Key::Escape => Some(KeyAction::Escape),
        _ => None,
    }
}

type QueryCallback = Box<dyn Fn(&str)>;
type KeyActionCallback = Box<dyn Fn(KeyAction)>;

/// Search bar widget containing search icon, text entry, and ghost text overlay.
#[derive(Clone)]
pub struct SearchBarWidget {
    /// Container box encompassing icon and text input.
    pub container: gtk4::Box,
    /// 22px magnifying glass icon.
    pub icon: gtk4::Image,
    /// Borderless text input field.
    pub entry: gtk4::Text,
    /// Overlaid dimmed completion label.
    pub ghost_label: gtk4::Label,
    ghost_suffix: Rc<RefCell<String>>,
    query_cbs: Rc<RefCell<Vec<QueryCallback>>>,
    key_cbs: Rc<RefCell<Vec<KeyActionCallback>>>,
}

impl fmt::Debug for SearchBarWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchBarWidget")
            .field("container", &self.container)
            .field("icon", &self.icon)
            .field("entry", &self.entry)
            .field("ghost_label", &self.ghost_label)
            .finish_non_exhaustive()
    }
}

impl Default for SearchBarWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchBarWidget {
    /// Constructs a new `SearchBarWidget`.
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        container.add_css_class("sb");

        let icon = gtk4::Image::from_icon_name("system-search-symbolic");
        icon.set_pixel_size(22);
        icon.add_css_class("sb-icon");
        icon.set_valign(gtk4::Align::Center);

        let entry = gtk4::Text::new();
        entry.set_placeholder_text(Some("Spotlight Search"));
        entry.add_css_class("sb-text");
        entry.set_hexpand(true);
        entry.set_valign(gtk4::Align::Center);

        let ghost_label = gtk4::Label::new(None);
        ghost_label.add_css_class("ghost-text");
        ghost_label.set_can_target(false);
        ghost_label.set_xalign(0.0);
        ghost_label.set_yalign(0.5);
        ghost_label.set_hexpand(true);
        ghost_label.set_valign(gtk4::Align::Center);

        let overlay = gtk4::Overlay::new();
        overlay.set_hexpand(true);
        overlay.set_valign(gtk4::Align::Center);
        overlay.set_child(Some(&entry));
        overlay.add_overlay(&ghost_label);

        container.append(&icon);
        container.append(&overlay);

        let ghost_suffix = Rc::new(RefCell::new(String::new()));
        let query_cbs: Rc<RefCell<Vec<QueryCallback>>> = Rc::new(RefCell::new(Vec::new()));
        let key_cbs: Rc<RefCell<Vec<KeyActionCallback>>> = Rc::new(RefCell::new(Vec::new()));

        let query_cbs_clone = Rc::clone(&query_cbs);
        entry.connect_changed(move |entry| {
            let text = entry.text();
            for cb in &*query_cbs_clone.borrow() {
                cb(text.as_str());
            }
        });

        let key_controller = gtk4::EventControllerKey::new();
        let key_cbs_clone = Rc::clone(&key_cbs);
        let ghost_suffix_key = Rc::clone(&ghost_suffix);
        let ghost_label_key = ghost_label.clone();
        let entry_key = entry.clone();

        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            // Check for inline completion acceptance on Tab or Right Arrow
            if key == gdk4::Key::Tab || key == gdk4::Key::ISO_Left_Tab {
                let suffix = ghost_suffix_key.borrow().clone();
                if !suffix.is_empty() {
                    let current = entry_key.text();
                    entry_key.set_text(&format!("{current}{suffix}"));
                    ghost_suffix_key.borrow_mut().clear();
                    ghost_label_key.set_text("");
                    entry_key.set_position(-1);
                    return glib::Propagation::Stop;
                }
            } else if key == gdk4::Key::Right {
                let suffix = ghost_suffix_key.borrow().clone();
                let pos = entry_key.position();
                let len = i32::try_from(entry_key.text().chars().count()).unwrap_or(0);
                if !suffix.is_empty() && pos >= len {
                    let current = entry_key.text();
                    entry_key.set_text(&format!("{current}{suffix}"));
                    ghost_suffix_key.borrow_mut().clear();
                    ghost_label_key.set_text("");
                    entry_key.set_position(-1);
                    return glib::Propagation::Stop;
                }
            }

            map_key_to_action(key, modifiers).map_or(glib::Propagation::Proceed, |action| {
                for cb in &*key_cbs_clone.borrow() {
                    cb(action);
                }
                glib::Propagation::Stop
            })
        });
        entry.add_controller(key_controller);

        Self {
            container,
            icon,
            entry,
            ghost_label,
            ghost_suffix,
            query_cbs,
            key_cbs,
        }
    }

    /// Registers a listener for text change events.
    pub fn connect_query_changed<F: Fn(&str) + 'static>(&self, callback: F) {
        self.query_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Registers a listener for keyboard navigation action events.
    pub fn connect_key_action<F: Fn(KeyAction) + 'static>(&self, callback: F) {
        self.key_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Gets current text from the input field.
    #[must_use]
    pub fn text(&self) -> String {
        self.entry.text().to_string()
    }

    /// Sets the text inside the input field.
    pub fn set_text(&self, text: &str) {
        self.entry.set_text(text);
    }

    /// Clears the input field and ghost completion text.
    pub fn clear(&self) {
        self.entry.set_text("");
        self.clear_ghost();
    }

    /// Clears ghost completion text and buffer.
    pub fn clear_ghost(&self) {
        self.ghost_suffix.borrow_mut().clear();
        self.ghost_label.set_text("");
    }

    /// Sets ghost completion suffix formatted with transparent prefix markup.
    pub fn set_ghost_completion(&self, prefix: &str, suffix: &str) {
        *self.ghost_suffix.borrow_mut() = suffix.to_string();
        if suffix.is_empty() {
            self.ghost_label.set_text("");
        } else {
            let escaped_prefix = glib::markup_escape_text(prefix);
            let escaped_suffix = glib::markup_escape_text(suffix);
            self.ghost_label.set_markup(&format!(
                "<span foreground=\"transparent\">{escaped_prefix}</span>{escaped_suffix}"
            ));
        }
    }

    /// Sets ghost text for inline autocompletion hints.
    pub fn set_ghost_text(&self, text: &str) {
        *self.ghost_suffix.borrow_mut() = text.to_string();
        self.ghost_label.set_text(text);
    }

    /// Gets current ghost text.
    #[must_use]
    pub fn ghost_text(&self) -> String {
        self.ghost_suffix.borrow().clone()
    }

    /// Explicitly accepts ghost completion, appending the suffix to the text entry.
    #[must_use]
    pub fn accept_ghost_completion(&self) -> bool {
        let suffix = self.ghost_suffix.borrow().clone();
        if suffix.is_empty() {
            return false;
        }
        let current = self.text();
        self.set_text(&format!("{current}{suffix}"));
        self.clear_ghost();
        self.entry.set_position(-1);
        true
    }

    /// Selects all characters inside the input entry.
    pub fn select_all(&self) {
        self.entry.select_region(0, -1);
    }

    /// Requests keyboard focus on the input entry.
    #[must_use]
    pub fn grab_focus(&self) -> bool {
        self.entry.grab_focus()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_key_navigation_actions() {
        assert_eq!(
            map_key_to_action(gdk4::Key::Up, gdk4::ModifierType::empty()),
            Some(KeyAction::Up)
        );
        assert_eq!(
            map_key_to_action(gdk4::Key::Down, gdk4::ModifierType::empty()),
            Some(KeyAction::Down)
        );
        assert_eq!(
            map_key_to_action(gdk4::Key::k, gdk4::ModifierType::CONTROL_MASK),
            Some(KeyAction::Up)
        );
        assert_eq!(
            map_key_to_action(gdk4::Key::j, gdk4::ModifierType::CONTROL_MASK),
            Some(KeyAction::Down)
        );
        assert_eq!(
            map_key_to_action(gdk4::Key::Return, gdk4::ModifierType::empty()),
            Some(KeyAction::Enter)
        );
        assert_eq!(
            map_key_to_action(gdk4::Key::Tab, gdk4::ModifierType::empty()),
            Some(KeyAction::Tab)
        );
        assert_eq!(
            map_key_to_action(gdk4::Key::Escape, gdk4::ModifierType::empty()),
            Some(KeyAction::Escape)
        );
        assert_eq!(
            map_key_to_action(gdk4::Key::a, gdk4::ModifierType::empty()),
            None
        );
    }
}
