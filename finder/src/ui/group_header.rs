//! Sticky group header GTK widget displaying category title, badge count, and disclosure chevron.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::grouping::models::GroupKey;

/// Callback invoked when a section's collapse state is toggled.
pub type GroupToggleCallback = Box<dyn Fn(&GroupKey, bool) + 'static>;

/// Sticky section header widget matching macOS Finder group row styling.
#[derive(Clone)]
pub struct GroupHeaderWidget {
    /// Top-level horizontal container for the header.
    pub container: gtk4::Box,
    /// Disclosure chevron indicator ("▾" when expanded, "▸" when collapsed).
    pub chevron_label: gtk4::Label,
    /// Bold category title label.
    pub title_label: gtk4::Label,
    /// Count badge label (e.g. `"(48)"`).
    pub count_badge: gtk4::Label,
    key: GroupKey,
    is_collapsed: Rc<RefCell<bool>>,
    toggle_callback: Rc<RefCell<Option<GroupToggleCallback>>>,
}

impl fmt::Debug for GroupHeaderWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GroupHeaderWidget")
            .field("key", &self.key)
            .field("is_collapsed", &self.is_collapsed.borrow())
            .finish_non_exhaustive()
    }
}

impl GroupHeaderWidget {
    /// Creates a new [`GroupHeaderWidget`] for the specified group key and item count.
    #[must_use]
    pub fn new(key: GroupKey, count: usize, is_collapsed: bool) -> Self {
        let is_collapsed_rc = Rc::new(RefCell::new(is_collapsed));
        let toggle_callback: Rc<RefCell<Option<GroupToggleCallback>>> = Rc::new(RefCell::new(None));

        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk4::Align::Center)
            .css_classes(["group-header"])
            .build();

        let chevron_symbol = if is_collapsed { "▸" } else { "▾" };
        let chevron_label = gtk4::Label::builder()
            .label(chevron_symbol)
            .css_classes(["group-chevron"])
            .build();

        let title_label = gtk4::Label::builder()
            .label(&key.title)
            .css_classes(["group-title"])
            .build();

        let count_text = format!("({count})");
        let count_badge = gtk4::Label::builder()
            .label(&count_text)
            .css_classes(["group-count"])
            .build();

        container.append(&chevron_label);
        container.append(&title_label);
        container.append(&count_badge);

        let click_gesture = gtk4::GestureClick::new();
        let is_col = Rc::clone(&is_collapsed_rc);
        let chev = chevron_label.clone();
        let k = key.clone();
        let cb = Rc::clone(&toggle_callback);

        click_gesture.connect_released(move |_, _, _, _| {
            let next_state = !*is_col.borrow();
            *is_col.borrow_mut() = next_state;
            chev.set_text(if next_state { "▸" } else { "▾" });

            if let Some(handler) = cb.borrow().as_ref() {
                handler(&k, next_state);
            }
        });
        container.add_controller(click_gesture);

        Self {
            container,
            chevron_label,
            title_label,
            count_badge,
            key,
            is_collapsed: is_collapsed_rc,
            toggle_callback,
        }
    }

    /// Returns a borrowed reference to the top-level container widget.
    #[must_use]
    pub const fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Returns the associated [`GroupKey`].
    #[must_use]
    pub const fn key(&self) -> &GroupKey {
        &self.key
    }

    /// Returns `true` if this section is currently collapsed.
    #[must_use]
    pub fn is_collapsed(&self) -> bool {
        *self.is_collapsed.borrow()
    }

    /// Updates the collapsed state programmatically.
    pub fn set_collapsed(&self, collapsed: bool) {
        *self.is_collapsed.borrow_mut() = collapsed;
        self.chevron_label
            .set_text(if collapsed { "▸" } else { "▾" });
    }

    /// Updates the item count badge text.
    pub fn set_count(&self, count: usize) {
        self.count_badge.set_text(&format!("({count})"));
    }

    /// Connects a handler to be invoked when the header is clicked.
    pub fn connect_toggle<F>(&self, callback: F)
    where
        F: Fn(&GroupKey, bool) + 'static,
    {
        *self.toggle_callback.borrow_mut() = Some(Box::new(callback));
    }
}
