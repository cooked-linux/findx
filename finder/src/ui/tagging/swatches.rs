//! 7-color circular swatch palette widget for file tagging and color labels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use shared::models::TagColor;

/// Canonical visual state for an individual color swatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwatchState {
    /// Color is not applied to any selected item.
    Inactive,
    /// Color is applied to all selected items (solid ring).
    Active,
    /// Color is applied to a subset of selected items (dashed ring).
    Partial,
}

/// Type alias for color toggled callback: `(color, is_now_active)`.
pub type ColorToggledCallback = Box<dyn Fn(TagColor, bool) + 'static>;

/// Circular 7-color swatch bar widget embedded in context menus and metadata inspectors.
#[derive(Clone)]
pub struct ColorSwatchBar {
    /// Root horizontal box containing the 7 circular buttons.
    pub container: gtk4::Box,
    buttons: Rc<HashMap<TagColor, gtk4::Button>>,
    states: Rc<RefCell<HashMap<TagColor, SwatchState>>>,
    callbacks: Rc<RefCell<Vec<ColorToggledCallback>>>,
}

impl Default for ColorSwatchBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorSwatchBar {
    /// The standard 7 colors presented in the swatch bar in canonical order.
    pub const COLORS: [TagColor; 7] = TagColor::ALL;

    /// Constructs a new [`ColorSwatchBar`].
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(4)
            .css_classes(["color-swatch-bar"])
            .build();

        let mut buttons_map = HashMap::new();
        let mut initial_states = HashMap::new();

        let states_rc: Rc<RefCell<HashMap<TagColor, SwatchState>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let callbacks_rc: Rc<RefCell<Vec<ColorToggledCallback>>> =
            Rc::new(RefCell::new(Vec::new()));

        for color in Self::COLORS {
            let css_class = Self::color_css_class(color);
            let btn = gtk4::Button::builder()
                .has_frame(false)
                .tooltip_text(color.display_name())
                .css_classes(["color-dot-button", css_class])
                .build();

            container.append(&btn);
            buttons_map.insert(color, btn.clone());
            initial_states.insert(color, SwatchState::Inactive);

            let states_weak = Rc::downgrade(&states_rc);
            let callbacks_weak = Rc::downgrade(&callbacks_rc);
            let btn_clone = btn.clone();

            btn.connect_clicked(move |_| {
                let Some(states) = states_weak.upgrade() else {
                    return;
                };
                let Some(callbacks) = callbacks_weak.upgrade() else {
                    return;
                };

                let current = states
                    .borrow()
                    .get(&color)
                    .copied()
                    .unwrap_or(SwatchState::Inactive);
                let will_be_active = current == SwatchState::Inactive;
                let next_state = if will_be_active {
                    SwatchState::Active
                } else {
                    SwatchState::Inactive
                };

                states.borrow_mut().insert(color, next_state);
                Self::apply_button_css(&btn_clone, next_state);

                let cbs = callbacks.borrow();
                for cb in cbs.iter() {
                    cb(color, will_be_active);
                }
            });
        }

        *states_rc.borrow_mut() = initial_states;

        Self {
            container,
            buttons: Rc::new(buttons_map),
            states: states_rc,
            callbacks: callbacks_rc,
        }
    }

    /// Registers a callback handler invoked when a color swatch button is toggled.
    pub fn connect_color_toggled<F: Fn(TagColor, bool) + 'static>(&self, callback: F) {
        self.callbacks.borrow_mut().push(Box::new(callback));
    }

    /// Sets the state for a single color dot, updating CSS classes immediately.
    pub fn set_color_state(&self, color: TagColor, state: SwatchState) {
        self.states.borrow_mut().insert(color, state);
        if let Some(btn) = self.buttons.get(&color) {
            Self::apply_button_css(btn, state);
        }
    }

    /// Returns the current state for a given color.
    #[must_use]
    pub fn get_color_state(&self, color: TagColor) -> SwatchState {
        self.states
            .borrow()
            .get(&color)
            .copied()
            .unwrap_or(SwatchState::Inactive)
    }

    /// Returns `true` if the color is currently Active or Partial.
    #[must_use]
    pub fn is_color_active(&self, color: TagColor) -> bool {
        self.get_color_state(color) != SwatchState::Inactive
    }

    /// Resets all swatch buttons to Inactive.
    pub fn clear_states(&self) {
        for color in Self::COLORS {
            self.set_color_state(color, SwatchState::Inactive);
        }
    }

    /// Bulk updates active and partial colors across multiple selected items.
    pub fn set_active_colors(&self, full_colors: &[TagColor], partial_colors: &[TagColor]) {
        for color in Self::COLORS {
            let state = if full_colors.contains(&color) {
                SwatchState::Active
            } else if partial_colors.contains(&color) {
                SwatchState::Partial
            } else {
                SwatchState::Inactive
            };
            self.set_color_state(color, state);
        }
    }

    /// Resolves the specific CSS class name for a given [`TagColor`].
    #[must_use]
    pub const fn color_css_class(color: TagColor) -> &'static str {
        match color {
            TagColor::Gray => "tag-color-gray",
            TagColor::Green => "tag-color-green",
            TagColor::Purple => "tag-color-purple",
            TagColor::Blue => "tag-color-blue",
            TagColor::Yellow => "tag-color-yellow",
            TagColor::Red => "tag-color-red",
            TagColor::Orange => "tag-color-orange",
        }
    }

    fn apply_button_css(button: &gtk4::Button, state: SwatchState) {
        match state {
            SwatchState::Active => {
                button.remove_css_class("partial");
                button.add_css_class("active");
            }
            SwatchState::Partial => {
                button.remove_css_class("active");
                button.add_css_class("partial");
            }
            SwatchState::Inactive => {
                button.remove_css_class("active");
                button.remove_css_class("partial");
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ui::tagging::GTK_TEST_MUTEX;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn ensure_gtk() -> bool {
        if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err())
        {
            return false;
        }
        true
    }

    #[test]
    fn test_swatch_colors_count_and_mapping() {
        assert_eq!(ColorSwatchBar::COLORS.len(), 7);
        assert_eq!(
            ColorSwatchBar::color_css_class(TagColor::Red),
            "tag-color-red"
        );
        assert_eq!(
            ColorSwatchBar::color_css_class(TagColor::Blue),
            "tag-color-blue"
        );
        assert_eq!(
            ColorSwatchBar::color_css_class(TagColor::Gray),
            "tag-color-gray"
        );
    }

    #[test]
    fn test_swatch_state_transitions() {
        let _lock = GTK_TEST_MUTEX.lock().unwrap();
        if !ensure_gtk() {
            return;
        }
        let bar = ColorSwatchBar::new();

        assert_eq!(bar.get_color_state(TagColor::Red), SwatchState::Inactive);
        assert!(!bar.is_color_active(TagColor::Red));

        bar.set_color_state(TagColor::Red, SwatchState::Active);
        assert_eq!(bar.get_color_state(TagColor::Red), SwatchState::Active);
        assert!(bar.is_color_active(TagColor::Red));

        bar.set_color_state(TagColor::Red, SwatchState::Partial);
        assert_eq!(bar.get_color_state(TagColor::Red), SwatchState::Partial);
        assert!(bar.is_color_active(TagColor::Red));

        bar.clear_states();
        assert_eq!(bar.get_color_state(TagColor::Red), SwatchState::Inactive);
    }

    #[test]
    fn test_bulk_active_and_partial_colors() {
        let _lock = GTK_TEST_MUTEX.lock().unwrap();
        if !ensure_gtk() {
            return;
        }
        let bar = ColorSwatchBar::new();

        let full = [TagColor::Green, TagColor::Blue];
        let partial = [TagColor::Yellow];

        bar.set_active_colors(&full, &partial);

        assert_eq!(bar.get_color_state(TagColor::Green), SwatchState::Active);
        assert_eq!(bar.get_color_state(TagColor::Blue), SwatchState::Active);
        assert_eq!(bar.get_color_state(TagColor::Yellow), SwatchState::Partial);
        assert_eq!(bar.get_color_state(TagColor::Red), SwatchState::Inactive);
    }

    #[test]
    fn test_callback_invocations() {
        let _lock = GTK_TEST_MUTEX.lock().unwrap();
        if !ensure_gtk() {
            return;
        }
        let bar = ColorSwatchBar::new();

        let triggered = Arc::new(AtomicBool::new(false));
        let triggered_clone = Arc::clone(&triggered);

        bar.connect_color_toggled(move |color, active| {
            if color == TagColor::Red && active {
                triggered_clone.store(true, Ordering::SeqCst);
            }
        });

        // Simulate button click
        if let Some(btn) = bar.buttons.get(&TagColor::Red) {
            btn.emit_clicked();
        }

        assert!(triggered.load(Ordering::SeqCst));
        assert_eq!(bar.get_color_state(TagColor::Red), SwatchState::Active);
    }
}
