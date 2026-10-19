//! Cascading Miller columns container and push/pop navigation controller.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use shared::models::FileItem;

use super::column_list::ColumnListWidget;
use super::divider::ColumnDividerWidget;
use super::keyboard::{ColumnKeyAction, attach_column_keyboard_controller};
use super::preview_column::ColumnPreviewWidget;
use crate::coordinator::NavigationCoordinator;
use crate::views::columns::loader::{
    ColumnWatcherEvent, ColumnWatcherHandle, read_directory, start_column_watcher,
};
use crate::views::columns::models::{ColumnEntry, ColumnViewModel, DEFAULT_COLUMN_WIDTH};

/// Maximum active rendered column depth before horizontal virtualization engages.
pub const VIRTUALIZATION_THRESHOLD: usize = 30;

/// Callback receiving path when active navigation selection changes.
pub type PathChangedCallback = Box<dyn Fn(&Path)>;
/// Callback receiving file item for Quick Look or Open actions.
pub type FileActionCallback = Box<dyn Fn(&FileItem)>;

/// Represents an active column in the cascading stack (directory list or file preview).
#[derive(Debug, Clone)]
pub enum ActiveColumn {
    /// Scrollable directory listing column.
    Directory {
        /// Associated GTK4 list widget.
        widget: ColumnListWidget,
        /// Inotify background watcher handle.
        watcher_handle: Option<ColumnWatcherHandle>,
        /// Absolute directory path.
        path: PathBuf,
    },
    /// Terminal leaf file preview card column.
    Preview {
        /// Associated GTK4 preview card widget.
        widget: ColumnPreviewWidget,
        /// Inspected file item.
        item: FileItem,
    },
}

impl ActiveColumn {
    /// Returns a reference to the column's root scrolled window.
    #[must_use]
    pub const fn scrolled_window(&self) -> &gtk4::ScrolledWindow {
        match self {
            Self::Directory { widget, .. } => &widget.scrolled_window,
            Self::Preview { widget, .. } => &widget.scrolled_window,
        }
    }

    /// Sets the width of the column.
    pub fn set_width(&self, width: i32) {
        match self {
            Self::Directory { widget, .. } => widget.set_width(width),
            Self::Preview { widget, .. } => widget.set_width(width),
        }
    }

    /// Returns the current width of the column.
    #[must_use]
    pub fn width(&self) -> i32 {
        match self {
            Self::Directory { widget, .. } => widget.width(),
            Self::Preview { widget, .. } => widget.scrolled_window.width_request(),
        }
    }
}

struct ContainerInner {
    columns: RefCell<Vec<ActiveColumn>>,
    dividers: RefCell<Vec<ColumnDividerWidget>>,
    active_column_index: Cell<usize>,
    root_path: RefCell<PathBuf>,
    default_column_width: Cell<i32>,
    on_path_changed: RefCell<Option<PathChangedCallback>>,
    on_quick_look: RefCell<Option<FileActionCallback>>,
    on_open: RefCell<Option<FileActionCallback>>,
    coordinator: RefCell<Option<Rc<NavigationCoordinator>>>,
}

impl fmt::Debug for ContainerInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerInner")
            .field("columns", &self.columns)
            .field("active_column_index", &self.active_column_index)
            .field("root_path", &self.root_path)
            .field("default_column_width", &self.default_column_width)
            .finish_non_exhaustive()
    }
}

/// Horizontal scrollable container hosting the cascading stack of Miller columns.
#[derive(Debug, Clone)]
pub struct ColumnContainer {
    /// Root horizontal scrolled window with kinetic panning.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Inner horizontal box holding dynamic columns and divider handles.
    pub inner_box: gtk4::Box,
    inner: Rc<ContainerInner>,
}

impl ColumnContainer {
    /// Constructs a new [`ColumnContainer`] initialized at `initial_root`.
    #[must_use]
    pub fn new(initial_root: impl AsRef<Path>) -> Self {
        let root_buf = initial_root.as_ref().to_path_buf();

        let inner_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(0)
            .hexpand(true)
            .vexpand(true)
            .build();

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Never)
            .kinetic_scrolling(true)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["column-scrolled-window"])
            .child(&inner_box)
            .build();

        let inner = Rc::new(ContainerInner {
            columns: RefCell::new(Vec::new()),
            dividers: RefCell::new(Vec::new()),
            active_column_index: Cell::new(0),
            root_path: RefCell::new(root_buf.clone()),
            default_column_width: Cell::new(DEFAULT_COLUMN_WIDTH),
            on_path_changed: RefCell::new(None),
            on_quick_look: RefCell::new(None),
            on_open: RefCell::new(None),
            coordinator: RefCell::new(None),
        });

        let container = Self {
            scrolled_window,
            inner_box,
            inner,
        };

        container.wire_keyboard();
        container.navigate_root(&root_buf);

        container
    }

    /// Attaches a [`NavigationCoordinator`] to handle context menus and drag-and-drop on columns.
    pub fn set_coordinator(&self, coordinator: &Rc<NavigationCoordinator>) {
        *self.inner.coordinator.borrow_mut() = Some(Rc::clone(coordinator));
        for col in self.inner.columns.borrow().iter() {
            if let ActiveColumn::Directory { widget, .. } = col {
                widget.set_coordinator(coordinator);
            }
        }
    }

    /// Navigates the container to a new root directory, clearing all existing columns.
    pub fn navigate_root(&self, root_path: &Path) {
        self.truncate_all();
        *self.inner.root_path.borrow_mut() = root_path.to_path_buf();
        self.push_directory_column(0, root_path);
    }

    /// Navigates the container to a virtual tag directory displaying custom file items.
    pub fn navigate_virtual_tag(&self, virtual_root: &Path, items: Vec<FileItem>) {
        self.truncate_all();
        *self.inner.root_path.borrow_mut() = virtual_root.to_path_buf();
        self.push_custom_items_column(0, virtual_root, items);
    }

    /// Returns the number of currently active columns in the stack.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.inner.columns.borrow().len()
    }

    /// Selects an item at row `row_index` in column `column_index`.
    pub fn select_item(&self, column_index: usize, row_index: usize) {
        // 1. Truncate all child columns to the right (column_index + 1 ..)
        self.truncate_to(column_index);

        // 2. Inspect selected item in column_index
        let Some(selected_entry) = self.get_entry_at(column_index, row_index) else {
            return;
        };

        self.inner.active_column_index.set(column_index);
        self.update_ancestor_styles();

        // 3. If selected item is directory -> push child directory column
        if selected_entry.is_dir() {
            let child_path = selected_entry.path().to_path_buf();
            self.push_directory_column(column_index + 1, &child_path);
        } else {
            // 4. If selected item is regular file -> push ColumnPreviewWidget
            self.push_preview_column(column_index + 1, selected_entry.item);
        }

        // 5. Scroll container horizontally to ensure new column is fully visible
        self.ensure_last_column_visible();
    }

    /// Truncates all columns to the right of `column_index` and detaches their file watchers.
    pub fn truncate_to(&self, column_index: usize) {
        let mut columns = self.inner.columns.borrow_mut();
        let mut dividers = self.inner.dividers.borrow_mut();

        while columns.len() > column_index + 1 {
            if let Some(popped) = columns.pop() {
                if let ActiveColumn::Directory {
                    watcher_handle: Some(ref handle),
                    ..
                } = popped
                {
                    handle.stop();
                }
                self.inner_box.remove(popped.scrolled_window());
            }

            if let Some(div) = dividers.pop() {
                self.inner_box.remove(&div.widget);
            }
        }
    }

    /// Resizes all columns uniformly to `new_width` (Alt/Option-drag modifier).
    pub fn resize_all_columns(&self, new_width: i32) {
        self.inner.default_column_width.set(new_width);
        for col in self.inner.columns.borrow().iter() {
            col.set_width(new_width);
        }
    }

    /// Resizes column at `column_index` to `new_width`.
    pub fn resize_column(&self, column_index: usize, new_width: i32) {
        let columns = self.inner.columns.borrow();
        if let Some(col) = columns.get(column_index) {
            col.set_width(new_width);
        }
    }

    /// Snaps column at `column_index` to fit content (double-click divider).
    pub fn auto_fit_column(&self, column_index: usize) {
        let columns = self.inner.columns.borrow();
        if let Some(ActiveColumn::Directory { widget, .. }) = columns.get(column_index) {
            let fit_w = widget.calculate_auto_fit_width();
            widget.set_width(fit_w);
            let dividers = self.inner.dividers.borrow();
            if let Some(div) = dividers.get(column_index) {
                div.set_current_width(fit_w);
            }
        }
    }

    /// Focuses column at `index`.
    pub fn focus_column(&self, index: usize) {
        let count = self.column_count();
        if index >= count {
            return;
        }

        self.inner.active_column_index.set(index);
        self.update_ancestor_styles();

        let columns = self.inner.columns.borrow();
        if let Some(col) = columns.get(index) {
            let _ = col.scrolled_window().grab_focus();
            if let ActiveColumn::Directory { widget, path, .. } = col {
                if let Some(entry) = widget.selected_entry() {
                    self.emit_path_changed(entry.path());
                } else {
                    self.emit_path_changed(path);
                }
            } else if let ActiveColumn::Preview { item, .. } = col {
                self.emit_path_changed(&item.path);
            }
        }
    }

    /// Dispatches keyboard navigation action.
    pub fn handle_key_action(&self, action: ColumnKeyAction) {
        let active_idx = self.inner.active_column_index.get();
        let columns = self.inner.columns.borrow();

        match action {
            ColumnKeyAction::Up => {
                if let Some(ActiveColumn::Directory { widget, .. }) = columns.get(active_idx) {
                    widget.select_previous();
                }
            }
            ColumnKeyAction::Down => {
                if let Some(ActiveColumn::Directory { widget, .. }) = columns.get(active_idx) {
                    widget.select_next();
                }
            }
            ColumnKeyAction::Left => {
                if active_idx > 0 {
                    drop(columns);
                    self.focus_column(active_idx - 1);
                }
            }
            ColumnKeyAction::Right => {
                if let Some(ActiveColumn::Directory { widget, .. }) = columns.get(active_idx)
                    && let Some(entry) = widget.selected_entry()
                    && entry.is_dir()
                    && active_idx + 1 < columns.len()
                {
                    drop(columns);
                    self.focus_column(active_idx + 1);
                    let cols = self.inner.columns.borrow();
                    if let Some(ActiveColumn::Directory {
                        widget: child_w, ..
                    }) = cols.get(active_idx + 1)
                        && child_w.selected_index().is_none()
                    {
                        child_w.set_selected_index(Some(0));
                    }
                }
            }
            ColumnKeyAction::Enter => {
                if let Some(ActiveColumn::Directory { widget, .. }) = columns.get(active_idx)
                    && let Some(entry) = widget.selected_entry()
                {
                    if entry.is_dir() {
                        let path = entry.path().to_path_buf();
                        drop(columns);
                        self.navigate_root(&path);
                    } else if let Some(ref cb) = *self.inner.on_open.borrow() {
                        cb(&entry.item);
                    }
                }
            }
            ColumnKeyAction::Spacebar => {
                if let Some(ActiveColumn::Directory { widget, .. }) = columns.get(active_idx)
                    && let Some(entry) = widget.selected_entry()
                    && let Some(ref cb) = *self.inner.on_quick_look.borrow()
                {
                    cb(&entry.item);
                }
            }
        }
    }

    /// Registers a callback triggered when active directory or file path changes.
    pub fn connect_path_changed<F: Fn(&Path) + 'static>(&self, callback: F) {
        *self.inner.on_path_changed.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback triggered when Quick Look is requested.
    pub fn connect_quick_look<F: Fn(&FileItem) + 'static>(&self, callback: F) {
        *self.inner.on_quick_look.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback triggered when an item is opened.
    pub fn connect_open<F: Fn(&FileItem) + 'static>(&self, callback: F) {
        *self.inner.on_open.borrow_mut() = Some(Box::new(callback));
    }

    fn push_directory_column(&self, index: usize, path: &Path) {
        let width = self.inner.default_column_width.get();
        let path_buf = path.to_path_buf();
        let model = ColumnViewModel::new_directory(index, width, path_buf.clone(), Vec::new());
        let widget = ColumnListWidget::new(model);
        if let Some(coord) = self.inner.coordinator.borrow().as_ref() {
            widget.set_coordinator(coord);
        }

        self.append_divider_if_needed(index, width);
        self.inner_box.append(&widget.scrolled_window);

        // Async read directory
        let widget_clone = widget.clone();
        let path_clone = path_buf.clone();
        let this = self.clone();

        glib::MainContext::default().spawn_local(async move {
            let res = read_directory(&path_clone, None).await;
            match res {
                Ok(entries) => {
                    widget_clone.set_entries(entries, None);
                }
                Err(err) => {
                    let mut error_model = ColumnViewModel::new_directory(
                        index,
                        width,
                        path_clone.clone(),
                        Vec::new(),
                    );
                    error_model.error = Some(err.to_string());
                    widget_clone.set_entries(Vec::new(), None);
                }
            }
            this.apply_virtualization();
        });

        // Start inotify watcher
        let (watcher_handle, mut rx) = start_column_watcher(path_buf.clone(), None, None);
        let widget_for_watcher = widget.clone();
        let this_for_teardown = self.clone();
        let path_for_watcher = path_buf.clone();

        glib::MainContext::default().spawn_local(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    ColumnWatcherEvent::Reloaded {
                        entries,
                        preserved_selected_index,
                    } => {
                        widget_for_watcher.set_entries(entries, preserved_selected_index);
                    }
                    ColumnWatcherEvent::DirectoryDeleted(deleted_path) => {
                        if deleted_path == path_for_watcher {
                            this_for_teardown.handle_directory_deleted(index);
                            break;
                        }
                    }
                }
            }
        });

        // Wire selection and activation signals
        let this_sel = self.clone();
        widget.connect_item_selected(move |row_idx, _entry| {
            this_sel.select_item(index, row_idx);
        });

        let this_act = self.clone();
        widget.connect_item_activated(move |_row_idx, entry| {
            if entry.is_dir() {
                let p = entry.path().to_path_buf();
                this_act.navigate_root(&p);
            } else if let Some(ref cb) = *this_act.inner.on_open.borrow() {
                cb(&entry.item);
            }
        });

        let active_col = ActiveColumn::Directory {
            widget,
            watcher_handle: Some(watcher_handle),
            path: path_buf.clone(),
        };

        self.inner.columns.borrow_mut().push(active_col);
        self.emit_path_changed(&path_buf);
        self.apply_virtualization();
    }

    fn push_custom_items_column(&self, index: usize, path: &Path, items: Vec<FileItem>) {
        let width = self.inner.default_column_width.get();
        let path_buf = path.to_path_buf();
        let entries: Vec<ColumnEntry> = items
            .into_iter()
            .map(|it| ColumnEntry::new(it, false))
            .collect();
        let model = ColumnViewModel::new_directory(index, width, path_buf.clone(), entries.clone());
        let widget = ColumnListWidget::new(model);
        if let Some(coord) = self.inner.coordinator.borrow().as_ref() {
            widget.set_coordinator(coord);
        }

        self.append_divider_if_needed(index, width);
        self.inner_box.append(&widget.scrolled_window);
        widget.set_entries(entries, None);

        // Wire selection and activation signals
        let this_sel = self.clone();
        widget.connect_item_selected(move |row_idx, _entry| {
            this_sel.select_item(index, row_idx);
        });

        let this_act = self.clone();
        widget.connect_item_activated(move |_row_idx, entry| {
            if entry.is_dir() {
                let p = entry.path().to_path_buf();
                this_act.navigate_root(&p);
            } else if let Some(ref cb) = *this_act.inner.on_open.borrow() {
                cb(&entry.item);
            }
        });

        let active_col = ActiveColumn::Directory {
            widget,
            watcher_handle: None,
            path: path_buf.clone(),
        };

        self.inner.columns.borrow_mut().push(active_col);
        self.emit_path_changed(&path_buf);
        self.apply_virtualization();
    }

    fn push_preview_column(&self, index: usize, item: FileItem) {
        let width = self.inner.default_column_width.get();
        let widget = ColumnPreviewWidget::new(item.clone(), width);

        self.append_divider_if_needed(index, width);
        self.inner_box.append(&widget.scrolled_window);

        // Wire preview action buttons
        let this_open = self.clone();
        widget.connect_open(move |it| {
            if let Some(ref cb) = *this_open.inner.on_open.borrow() {
                cb(it);
            }
        });

        let this_ql = self.clone();
        widget.connect_quick_look(move |it| {
            if let Some(ref cb) = *this_ql.inner.on_quick_look.borrow() {
                cb(it);
            }
        });

        let path = item.path.clone();
        let active_col = ActiveColumn::Preview { widget, item };
        self.inner.columns.borrow_mut().push(active_col);
        self.emit_path_changed(&path);
        self.apply_virtualization();
    }

    fn append_divider_if_needed(&self, index: usize, current_width: i32) {
        if index > 0 {
            let divider = ColumnDividerWidget::new(index - 1, current_width);
            let this_resize = self.clone();
            divider.connect_resize(move |col_idx, w| {
                this_resize.resize_column(col_idx, w);
            });

            let this_uniform = self.clone();
            divider.connect_uniform_resize(move |w| {
                this_uniform.resize_all_columns(w);
            });

            let this_autofit = self.clone();
            divider.connect_auto_fit(move |col_idx| {
                this_autofit.auto_fit_column(col_idx);
            });

            self.inner_box.append(&divider.widget);
            self.inner.dividers.borrow_mut().push(divider);
        }
    }

    fn handle_directory_deleted(&self, column_index: usize) {
        self.truncate_to(column_index.saturating_sub(1));
        if column_index > 0 {
            self.focus_column(column_index - 1);
        }
    }

    fn update_ancestor_styles(&self) {
        let active_idx = self.inner.active_column_index.get();
        let columns = self.inner.columns.borrow();

        for (i, col) in columns.iter().enumerate() {
            if let ActiveColumn::Directory { widget, .. } = col {
                widget.set_ancestor_mode(i < active_idx);
            }
        }
    }

    fn apply_virtualization(&self) {
        let columns = self.inner.columns.borrow();
        let count = columns.len();
        if count <= VIRTUALIZATION_THRESHOLD {
            for col in columns.iter() {
                col.scrolled_window().set_visible(true);
            }
            return;
        }

        let cutoff = count - VIRTUALIZATION_THRESHOLD;
        for (i, col) in columns.iter().enumerate() {
            col.scrolled_window().set_visible(i >= cutoff);
        }
    }

    fn ensure_last_column_visible(&self) {
        let hadj = self.scrolled_window.hadjustment();
        glib::idle_add_local_once(move || {
            let upper = hadj.upper();
            let page_size = hadj.page_size();
            let target = (upper - page_size).max(0.0);
            hadj.set_value(target);
        });
    }

    fn truncate_all(&self) {
        self.truncate_to(0);
        let mut columns = self.inner.columns.borrow_mut();
        if let Some(col) = columns.pop() {
            if let ActiveColumn::Directory {
                watcher_handle: Some(ref handle),
                ..
            } = col
            {
                handle.stop();
            }
            self.inner_box.remove(col.scrolled_window());
        }
    }

    fn get_entry_at(&self, column_index: usize, row_index: usize) -> Option<ColumnEntry> {
        let columns = self.inner.columns.borrow();
        if let Some(ActiveColumn::Directory { widget, .. }) = columns.get(column_index) {
            return widget.entry_at(row_index);
        }
        None
    }

    fn emit_path_changed(&self, path: &Path) {
        if let Some(ref cb) = *self.inner.on_path_changed.borrow() {
            cb(path);
        }
    }

    fn wire_keyboard(&self) {
        let this = self.clone();
        attach_column_keyboard_controller(&self.scrolled_window, move |action| {
            this.handle_key_action(action);
            glib::Propagation::Stop
        });
    }
}
