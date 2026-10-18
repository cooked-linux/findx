use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::coordinator::NavigationCoordinator;
use crate::views::columns::models::{
    ColumnEntry, ColumnKind, ColumnViewModel, DEFAULT_COLUMN_WIDTH, MAX_COLUMN_WIDTH,
    MIN_COLUMN_WIDTH,
};

/// Type alias for item selection callback `(index, entry)`.
pub type ItemSelectedCallback = Box<dyn Fn(usize, &ColumnEntry)>;
/// Type alias for item activation callback `(index, entry)`.
pub type ItemActivatedCallback = Box<dyn Fn(usize, &ColumnEntry)>;

struct ColumnListInner {
    model: RefCell<ColumnViewModel>,
    width_px: Cell<i32>,
    is_ancestor: Cell<bool>,
    on_select: RefCell<Option<ItemSelectedCallback>>,
    on_activate: RefCell<Option<ItemActivatedCallback>>,
    coordinator: RefCell<Option<Rc<NavigationCoordinator>>>,
}

impl fmt::Debug for ColumnListInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ColumnListInner")
            .field("model", &self.model)
            .field("width_px", &self.width_px)
            .field("is_ancestor", &self.is_ancestor)
            .finish_non_exhaustive()
    }
}

/// GTK4 widget displaying a single scrollable directory column with row glyphs and chevrons.
#[derive(Debug, Clone)]
pub struct ColumnListWidget {
    /// Root scrolled window wrapping the vertical list.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Vertical container list box.
    pub list_box: gtk4::ListBox,
    inner: Rc<ColumnListInner>,
}

impl ColumnListWidget {
    /// Constructs a new [`ColumnListWidget`] from a [`ColumnViewModel`].
    #[must_use]
    pub fn new(model: ColumnViewModel) -> Self {
        let width = model.width_px;

        let list_box = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .css_classes(["cl"])
            .hexpand(false)
            .vexpand(true)
            .build();

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .width_request(width)
            .css_classes(["column-scrolled-window"])
            .child(&list_box)
            .build();

        let inner = Rc::new(ColumnListInner {
            model: RefCell::new(model),
            width_px: Cell::new(width),
            is_ancestor: Cell::new(false),
            on_select: RefCell::new(None),
            on_activate: RefCell::new(None),
            coordinator: RefCell::new(None),
        });

        let widget = Self {
            scrolled_window,
            list_box,
            inner,
        };

        widget.populate_rows();
        widget.wire_signals();

        widget
    }

    /// Attaches a [`NavigationCoordinator`] to handle context menus and drag-and-drop on rows.
    pub fn set_coordinator(&self, coordinator: &Rc<NavigationCoordinator>) {
        *self.inner.coordinator.borrow_mut() = Some(Rc::clone(coordinator));
        self.populate_rows();
    }

    /// Returns the zero-based index of this column in the horizontal stack.
    #[must_use]
    pub fn column_index(&self) -> usize {
        self.inner.model.borrow().index
    }

    /// Returns the current width of the column in pixels.
    #[must_use]
    pub fn width(&self) -> i32 {
        self.inner.width_px.get()
    }

    /// Sets the width of the column, bounded by `[MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH]`.
    pub fn set_width(&self, width: i32) {
        let clamped = width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
        self.inner.width_px.set(clamped);
        self.scrolled_window.set_width_request(clamped);
    }

    /// Measures the maximum natural text width of visible rows and calculates the auto-fit width (+24px).
    #[must_use]
    pub fn calculate_auto_fit_width(&self) -> i32 {
        if self.entry_count() == 0 {
            return DEFAULT_COLUMN_WIDTH;
        }

        let mut max_width = 0;
        let mut idx = 0;
        while let Some(row) = self.list_box.row_at_index(idx) {
            let (_, nat_w, _, _) = row.measure(gtk4::Orientation::Horizontal, -1);
            if nat_w > max_width {
                max_width = nat_w;
            }
            idx += 1;
        }

        if max_width == 0 {
            DEFAULT_COLUMN_WIDTH
        } else {
            (max_width + 24).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH)
        }
    }

    /// Toggles ancestor mode (`.ia`) versus active focused mode (`.on`) for the selected row.
    pub fn set_ancestor_mode(&self, is_ancestor: bool) {
        self.inner.is_ancestor.set(is_ancestor);
        self.update_row_styles(self.selected_index());
    }

    /// Returns the selected row index, if any.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        let row = self.list_box.selected_row()?;
        usize::try_from(row.index()).ok()
    }

    /// Returns the entry at `row_index` if valid.
    #[must_use]
    pub fn entry_at(&self, row_index: usize) -> Option<ColumnEntry> {
        let model = self.inner.model.borrow();
        if let ColumnKind::Directory { entries, .. } = &model.kind {
            entries.get(row_index).cloned()
        } else {
            None
        }
    }

    /// Returns the currently selected entry, if any.
    #[must_use]
    pub fn selected_entry(&self) -> Option<ColumnEntry> {
        let idx = self.selected_index()?;
        self.entry_at(idx)
    }

    /// Selects row at `index`, updating row style classes.
    pub fn set_selected_index(&self, index: Option<usize>) {
        if let Some(idx) = index
            && let Ok(idx_i32) = i32::try_from(idx)
            && let Some(row) = self.list_box.row_at_index(idx_i32)
        {
            self.list_box.select_row(Some(&row));
        } else if index.is_none() {
            self.list_box.unselect_all();
        }
        self.update_row_styles(index);
    }

    /// Selects the previous item in the column.
    pub fn select_previous(&self) {
        let current = self.selected_index().unwrap_or(0);
        if current > 0 {
            self.set_selected_index(Some(current - 1));
            self.emit_selected(current - 1);
        }
    }

    /// Selects the next item in the column.
    pub fn select_next(&self) {
        let count = self.entry_count();
        if count == 0 {
            return;
        }
        let next = self
            .selected_index()
            .map_or(0, |idx| (idx + 1).min(count - 1));
        self.set_selected_index(Some(next));
        self.emit_selected(next);
    }

    /// Updates the column entries and restores selection if requested.
    pub fn set_entries(&self, entries: Vec<ColumnEntry>, preserved_selection: Option<usize>) {
        {
            let mut model = self.inner.model.borrow_mut();
            if let ColumnKind::Directory {
                entries: old_entries,
                ..
            } = &mut model.kind
            {
                *old_entries = entries;
            }
        }
        self.populate_rows();
        self.set_selected_index(preserved_selection);
    }

    /// Registers a callback triggered when an item row is selected.
    pub fn connect_item_selected<F: Fn(usize, &ColumnEntry) + 'static>(&self, callback: F) {
        *self.inner.on_select.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback triggered when an item row is activated (double-clicked / Enter).
    pub fn connect_item_activated<F: Fn(usize, &ColumnEntry) + 'static>(&self, callback: F) {
        *self.inner.on_activate.borrow_mut() = Some(Box::new(callback));
    }

    fn entry_count(&self) -> usize {
        let model = self.inner.model.borrow();
        if let ColumnKind::Directory { entries, .. } = &model.kind {
            entries.len()
        } else {
            0
        }
    }

    fn populate_rows(&self) {
        // Clear existing rows
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        let model = self.inner.model.borrow();
        if let Some(ref err) = model.error {
            self.render_empty_state("channel-insecure-symbolic", err);
            return;
        }

        let ColumnKind::Directory { entries, .. } = &model.kind else {
            return;
        };

        if entries.is_empty() {
            self.render_empty_state("folder-symbolic", "Folder is empty");
            return;
        }

        for entry in entries {
            let row = self.create_row(entry);
            self.list_box.append(&row);
        }
    }

    fn render_empty_state(&self, icon_name: &str, message: &str) {
        let empty_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::Center)
            .spacing(10)
            .margin_top(40)
            .margin_bottom(40)
            .margin_start(16)
            .margin_end(16)
            .build();

        let icon = gtk4::Image::builder()
            .icon_name(icon_name)
            .pixel_size(40)
            .css_classes(["dim-label"])
            .build();

        let label = gtk4::Label::builder()
            .label(message)
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .css_classes(["dim-label"])
            .build();

        empty_box.append(&icon);
        empty_box.append(&label);
        self.list_box.append(&empty_box);
    }

    fn create_row(&self, entry: &ColumnEntry) -> gtk4::ListBoxRow {
        let row = gtk4::ListBoxRow::builder()
            .css_classes(["rw"])
            .activatable(true)
            .selectable(true)
            .build();

        if let Some(coord) = self.inner.coordinator.borrow().clone() {
            crate::ui::context_menu::attach_row_context_menu(
                &row,
                entry.path(),
                entry.is_dir(),
                &coord,
            );
            if entry.is_dir() {
                crate::ui::context_menu::attach_directory_drop_target(&row, entry.path(), &coord);
            }
        }

        let hbox = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk4::Align::Center)
            .build();

        let (icon_name, icon_class) = if entry.is_dir() {
            ("folder-symbolic", "fi")
        } else if entry.is_symlink() && entry.item.symlink_target.is_none() {
            ("emblem-unreadable", "fl")
        } else {
            ("text-x-generic-symbolic", "fl")
        };

        let icon = gtk4::Image::builder()
            .icon_name(icon_name)
            .pixel_size(16)
            .css_classes(["column-icon", icon_class])
            .build();
        hbox.append(&icon);

        let label = gtk4::Label::builder()
            .label(entry.file_name())
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        hbox.append(&label);

        if entry.is_dir() {
            let chevron = gtk4::Label::builder()
                .label("›")
                .css_classes(["chevron"])
                .halign(gtk4::Align::End)
                .build();
            hbox.append(&chevron);
        }

        row.set_child(Some(&hbox));
        row
    }

    fn update_row_styles(&self, selected_idx: Option<usize>) {
        let is_ancestor = self.inner.is_ancestor.get();
        let mut idx = 0;
        while let Some(row) = self.list_box.row_at_index(idx) {
            let is_sel = usize::try_from(idx).is_ok_and(|u| selected_idx == Some(u));
            if is_sel {
                if is_ancestor {
                    row.remove_css_class("on");
                    row.add_css_class("ia");
                } else {
                    row.remove_css_class("ia");
                    row.add_css_class("on");
                }
            } else {
                row.remove_css_class("on");
                row.remove_css_class("ia");
            }
            idx += 1;
        }
    }

    fn emit_selected(&self, idx: usize) {
        let model = self.inner.model.borrow();
        if let ColumnKind::Directory { entries, .. } = &model.kind
            && let Some(entry) = entries.get(idx)
            && let Some(ref cb) = *self.inner.on_select.borrow()
        {
            cb(idx, entry);
        }
    }

    fn wire_signals(&self) {
        let this = self.clone();
        self.list_box.connect_row_selected(move |_lb, maybe_row| {
            if let Some(row) = maybe_row
                && let Ok(idx) = usize::try_from(row.index())
            {
                this.update_row_styles(Some(idx));
                this.emit_selected(idx);
            } else if maybe_row.is_none() {
                this.update_row_styles(None);
            }
        });

        let this = self.clone();
        self.list_box.connect_row_activated(move |_lb, row| {
            if let Ok(idx) = usize::try_from(row.index()) {
                let model = this.inner.model.borrow();
                if let ColumnKind::Directory { entries, .. } = &model.kind
                    && let Some(entry) = entries.get(idx)
                    && let Some(ref cb) = *this.inner.on_activate.borrow()
                {
                    cb(idx, entry);
                }
            }
        });
    }
}
