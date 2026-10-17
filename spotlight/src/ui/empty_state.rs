//! Empty-state view presenting suggested applications and navigation hints.
//!
//! When the search input is blank, Spotlight renders the top frecent applications
//! in a horizontal dock row with icon, title, keyboard navigation (Left/Right/Enter),
//! and shortcuts hint bar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk4::prelude::*;
use shared::desktop::DesktopApp;
use shared::frecency::FrecencyTracker;

/// Retrieves the top suggested applications based on historical engagement frecency.
#[must_use]
pub fn fetch_suggested_apps(
    frecency: &FrecencyTracker,
    apps: &[DesktopApp],
    limit: usize,
) -> Vec<DesktopApp> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let mut scored: Vec<(f64, &DesktopApp)> = apps
        .iter()
        .filter(|app| !app.hidden && !app.no_display)
        .map(|app| {
            let score = frecency
                .calculate_score(&app.desktop_id, now)
                .unwrap_or(0.0);
            (score, app)
        })
        .collect();

    // Sort descending by frecency score
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));

    scored
        .into_iter()
        .take(limit)
        .map(|(_, app)| app.clone())
        .collect()
}

/// Headless selection state manager for empty-state suggested applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyStateSelection {
    selected_index: Option<usize>,
    item_count: usize,
}

impl EmptyStateSelection {
    /// Constructs a new selection manager with `count` available items.
    #[must_use]
    pub const fn new(count: usize) -> Self {
        Self {
            selected_index: if count > 0 { Some(0) } else { None },
            item_count: count,
        }
    }

    /// Returns the currently selected item index.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Selects the next item to the right (wrapping around to the first).
    pub const fn select_next(&mut self) {
        if self.item_count == 0 {
            self.selected_index = None;
            return;
        }

        self.selected_index = Some(match self.selected_index {
            Some(idx) => (idx + 1) % self.item_count,
            None => 0,
        });
    }

    /// Selects the previous item to the left (wrapping around to the last).
    pub const fn select_prev(&mut self) {
        if self.item_count == 0 {
            self.selected_index = None;
            return;
        }

        self.selected_index = Some(match self.selected_index {
            Some(idx) => {
                if idx == 0 {
                    self.item_count.saturating_sub(1)
                } else {
                    idx.saturating_sub(1)
                }
            }
            None => self.item_count.saturating_sub(1),
        });
    }

    /// Updates the total number of items, clamping or resetting selection.
    pub const fn update_count(&mut self, new_count: usize) {
        self.item_count = new_count;
        if new_count == 0 {
            self.selected_index = None;
        } else if let Some(idx) = self.selected_index {
            if idx >= new_count {
                self.selected_index = Some(new_count - 1);
            }
        } else {
            self.selected_index = Some(0);
        }
    }
}

type ActivateCallback = Box<dyn Fn(DesktopApp)>;

/// Empty-state container widget displaying suggested applications dock.
#[derive(Clone)]
pub struct EmptyStateWidget {
    /// Root vertical box container.
    pub container: gtk4::Box,
    /// Horizontal dock row holding suggested application tiles.
    pub dock_row: gtk4::Box,
    apps: Rc<RefCell<Vec<DesktopApp>>>,
    item_widgets: Rc<RefCell<Vec<gtk4::Box>>>,
    selection: Rc<Cell<EmptyStateSelection>>,
    activate_cbs: Rc<RefCell<Vec<ActivateCallback>>>,
}

impl std::fmt::Debug for EmptyStateWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmptyStateWidget")
            .field("container", &self.container)
            .field("dock_row", &self.dock_row)
            .field("selection", &self.selection.get())
            .finish_non_exhaustive()
    }
}

impl EmptyStateWidget {
    /// Constructs an `EmptyStateWidget` populated with the given suggested applications.
    #[must_use]
    pub fn new(suggested_apps: Vec<DesktopApp>) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("empty-state");

        let dock_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        dock_row.add_css_class("suggested-dock");
        dock_row.set_halign(gtk4::Align::Center);
        dock_row.set_valign(gtk4::Align::Center);

        container.append(&dock_row);

        let count = suggested_apps.len();
        let selection = Rc::new(Cell::new(EmptyStateSelection::new(count)));
        let apps = Rc::new(RefCell::new(suggested_apps));
        let item_widgets = Rc::new(RefCell::new(Vec::new()));
        let activate_cbs = Rc::new(RefCell::new(Vec::new()));

        let widget = Self {
            container,
            dock_row,
            apps,
            item_widgets,
            selection,
            activate_cbs,
        };

        widget.rebuild_items();
        widget
    }

    /// Rebuilds visual dock item tiles from the current applications list.
    fn rebuild_items(&self) {
        // Clear existing children
        while let Some(child) = self.dock_row.first_child() {
            self.dock_row.remove(&child);
        }
        self.item_widgets.borrow_mut().clear();

        let apps = self.apps.borrow().clone();
        let current_sel = self.selection.get().selected_index();

        for (idx, app) in apps.iter().enumerate() {
            let item_box = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
            item_box.add_css_class("suggested-app-item");
            if Some(idx) == current_sel {
                item_box.add_css_class("selected");
            }

            // Themed application icon
            let icon_widget = create_dock_icon(app.icon_name.as_deref());
            icon_widget.add_css_class("suggested-app-icon");
            icon_widget.set_halign(gtk4::Align::Center);

            // Shortened application label
            let label = gtk4::Label::new(Some(&app.name));
            label.add_css_class("suggested-app-label");
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.set_max_width_chars(12);
            label.set_halign(gtk4::Align::Center);

            item_box.append(&icon_widget);
            item_box.append(&label);

            // Click gesture to select and activate
            let click = gtk4::GestureClick::new();
            let app_clone = app.clone();
            let cbs_clone = Rc::clone(&self.activate_cbs);
            let selection_clone = Rc::clone(&self.selection);
            let widgets_clone = Rc::clone(&self.item_widgets);

            click.connect_pressed(move |_, _, _, _| {
                // Update selection classes
                let mut sel = selection_clone.get();
                if let Some(prev) = sel.selected_index()
                    && let Some(prev_box) = widgets_clone.borrow().get(prev)
                {
                    prev_box.remove_css_class("selected");
                }
                sel.selected_index = Some(idx);
                selection_clone.set(sel);
                if let Some(cur_box) = widgets_clone.borrow().get(idx) {
                    cur_box.add_css_class("selected");
                }

                // Fire activation callbacks
                for cb in &*cbs_clone.borrow() {
                    cb(app_clone.clone());
                }
            });
            item_box.add_controller(click);

            self.dock_row.append(&item_box);
            self.item_widgets.borrow_mut().push(item_box);
        }
    }

    /// Selects the next suggested app tile to the right.
    pub fn select_next(&self) {
        let mut sel = self.selection.get();
        let prev_idx = sel.selected_index();
        sel.select_next();
        let new_idx = sel.selected_index();
        self.selection.set(sel);

        self.update_selection_visuals(prev_idx, new_idx);
    }

    /// Selects the previous suggested app tile to the left.
    pub fn select_prev(&self) {
        let mut sel = self.selection.get();
        let prev_idx = sel.selected_index();
        sel.select_prev();
        let new_idx = sel.selected_index();
        self.selection.set(sel);

        self.update_selection_visuals(prev_idx, new_idx);
    }

    /// Updates CSS classes to reflect selection change.
    fn update_selection_visuals(&self, prev: Option<usize>, current: Option<usize>) {
        let widgets = self.item_widgets.borrow();
        if let Some(p) = prev
            && let Some(widget) = widgets.get(p)
        {
            widget.remove_css_class("selected");
        }
        if let Some(c) = current
            && let Some(widget) = widgets.get(c)
        {
            widget.add_css_class("selected");
        }
    }

    /// Returns the currently selected application if one exists.
    #[must_use]
    pub fn selected_app(&self) -> Option<DesktopApp> {
        let idx = self.selection.get().selected_index()?;
        self.apps.borrow().get(idx).cloned()
    }

    /// Triggers activation callback on the currently selected application.
    #[must_use]
    pub fn activate_selected(&self) -> bool {
        self.selected_app().is_some_and(|app| {
            for cb in &*self.activate_cbs.borrow() {
                cb(app.clone());
            }
            true
        })
    }

    /// Registers a callback invoked when a suggested application tile is activated.
    pub fn connect_activate<F: Fn(DesktopApp) + 'static>(&self, callback: F) {
        self.activate_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Updates the applications displayed in the suggested apps dock.
    pub fn set_apps(&self, apps: Vec<DesktopApp>) {
        let count = apps.len();
        *self.apps.borrow_mut() = apps;
        let mut sel = self.selection.get();
        sel.update_count(count);
        self.selection.set(sel);
        self.rebuild_items();
    }
}

/// Helper creating an icon widget from an optional icon name or absolute path.
fn create_dock_icon(icon_name: Option<&str>) -> gtk4::Image {
    let Some(name) = icon_name else {
        let img = gtk4::Image::from_icon_name("system-run");
        img.set_pixel_size(44);
        return img;
    };

    shared::desktop::resolve_icon_path(name).map_or_else(
        || {
            let img = gtk4::Image::from_icon_name(name);
            img.set_pixel_size(44);
            img
        },
        |path| {
            let img = gtk4::Image::from_file(&path);
            img.set_pixel_size(44);
            img
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_state_selection_navigation() {
        let mut sel = EmptyStateSelection::new(4);
        assert_eq!(sel.selected_index(), Some(0));

        sel.select_next();
        assert_eq!(sel.selected_index(), Some(1));

        sel.select_next();
        assert_eq!(sel.selected_index(), Some(2));

        sel.select_next();
        assert_eq!(sel.selected_index(), Some(3));

        // Wrap around to beginning
        sel.select_next();
        assert_eq!(sel.selected_index(), Some(0));

        // Wrap around to end
        sel.select_prev();
        assert_eq!(sel.selected_index(), Some(3));

        sel.select_prev();
        assert_eq!(sel.selected_index(), Some(2));
    }

    #[test]
    fn test_empty_state_selection_empty() {
        let mut sel = EmptyStateSelection::new(0);
        assert_eq!(sel.selected_index(), None);

        sel.select_next();
        assert_eq!(sel.selected_index(), None);

        sel.select_prev();
        assert_eq!(sel.selected_index(), None);
    }
}
