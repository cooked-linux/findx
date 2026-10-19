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
}
