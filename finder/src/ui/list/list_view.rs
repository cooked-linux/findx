//! Hierarchical tabular list view widget with in-place nested tree expansion.
//!
//! Implements multi-column tabular layout (Name, Date Modified, Size, Kind, Tags)
//! with disclosure chevrons, asynchronous subtree loading with spinner indicators,
//! Option+Click recursive expansion with cycle loop prevention, sticky header sorting,
//! expansion caching, and keyboard traversal.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gdk4::prelude::*;
use gtk4::prelude::*;
use shared::models::FileItem;
use tokio::sync::mpsc;

use super::keyboard::{ListKeyAction, attach_list_keyboard_controller};
use crate::breadcrumbs::FinderPreferences;
use crate::coordinator::NavigationCoordinator;
use crate::grouping::kind::classify_kind;
use crate::operations::Priority;
use crate::sorting::comparator::{SortColumn, SortCriteria, SortDirection};
use crate::ui::columns::{format_byte_size, format_system_time};
use crate::views::list::{
    ExpandedNodesCache, ListWatcherEvent, LruWatchPool, MAX_WATCH_BUDGET, TreeListViewModel,
    TreeNode, batch_process_rows, read_dir_async, read_subtree_recursive_async,
};

/// Width constants for tabular columns.
pub const COL_WIDTH_NAME: i32 = 320;
pub const COL_MIN_WIDTH_NAME: i32 = 200;
pub const COL_WIDTH_DATE: i32 = 150;
pub const COL_WIDTH_SIZE: i32 = 140;
pub const COL_WIDTH_KIND: i32 = 120;
pub const COL_WIDTH_TAGS: i32 = 80;

/// Type alias for path change callback.
pub type PathChangedCallback = Box<dyn Fn(&Path)>;
/// Type alias for item activation callback.
pub type ItemActivatedCallback = Box<dyn Fn(&TreeNode)>;
/// Type alias for Quick Look callback.
pub type QuickLookCallback = Box<dyn Fn(&TreeNode)>;

struct ListViewInner {
    model: RefCell<TreeListViewModel>,
    criteria: RefCell<SortCriteria>,
    watch_pool: RefCell<LruWatchPool>,
    cache: RefCell<ExpandedNodesCache>,
    selected_index: Cell<Option<usize>>,
    on_path_changed: RefCell<Option<PathChangedCallback>>,
    on_item_activated: RefCell<Option<ItemActivatedCallback>>,
    on_quick_look: RefCell<Option<QuickLookCallback>>,
    name_header_btn: gtk4::Button,
    date_header_btn: gtk4::Button,
    size_header_btn: gtk4::Button,
    kind_header_btn: gtk4::Button,
    tags_header_btn: gtk4::Button,
    coordinator: RefCell<Option<Rc<NavigationCoordinator>>>,
    size_labels: RefCell<HashMap<PathBuf, gtk4::Label>>,
}

impl fmt::Debug for ListViewInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListViewInner")
            .field("model", &self.model)
            .field("criteria", &self.criteria)
            .field("selected_index", &self.selected_index)
            .finish_non_exhaustive()
    }
}

/// Primary widget providing hierarchical tabular list view with nested tree expansion.
#[derive(Debug, Clone)]
pub struct HierarchicalListViewWidget {
    /// Root vertical box container enclosing header and scrollable table.
    pub container: gtk4::Box,
    /// Sticky column header bar pinned at the top.
    pub header_box: gtk4::Box,
    /// Scrolled window wrapping the vertical list.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Vertical container list box hosting flattened rows.
    pub list_box: gtk4::ListBox,
    inner: Rc<ListViewInner>,
}

impl HierarchicalListViewWidget {
    /// Constructs a new [`HierarchicalListViewWidget`] for `root_path`.
    #[must_use]
    pub fn new(root_path: &Path) -> Self {
        let (event_tx, mut event_rx) = mpsc::channel(64);
        let watch_pool = LruWatchPool::new(event_tx, MAX_WATCH_BUDGET);
        let cache = ExpandedNodesCache::load();
        let criteria = SortCriteria::default();
        let model = TreeListViewModel::new_empty(root_path.to_path_buf());

        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["hierarchical-list-view"])
            .build();

        let (header_box, name_btn, date_btn, size_btn, kind_btn, tags_btn) =
            Self::build_header_row(criteria);
        container.append(&header_box);

        let list_box = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .hexpand(true)
            .vexpand(true)
            .build();

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["list-scrolled-window"])
            .child(&list_box)
            .build();
        container.append(&scrolled_window);

        let inner = Rc::new(ListViewInner {
            model: RefCell::new(model),
            criteria: RefCell::new(criteria),
            watch_pool: RefCell::new(watch_pool),
            cache: RefCell::new(cache),
            selected_index: Cell::new(None),
            on_path_changed: RefCell::new(None),
            on_item_activated: RefCell::new(None),
            on_quick_look: RefCell::new(None),
            name_header_btn: name_btn,
            date_header_btn: date_btn,
            size_header_btn: size_btn,
            kind_header_btn: kind_btn,
            tags_header_btn: tags_btn,
            coordinator: RefCell::new(None),
            size_labels: RefCell::new(HashMap::new()),
        });

        let widget = Self {
            container,
            header_box,
            scrolled_window,
            list_box,
            inner,
        };

        widget.wire_header_signals();
        widget.wire_list_signals();
        widget.wire_keyboard_navigation();

        // Listen for inotify watch pool deletion events on GTK main loop
        let widget_clone = widget.clone();
        glib::spawn_future_local(async move {
            while let Some(event) = event_rx.recv().await {
                match event {
                    ListWatcherEvent::ItemDeleted(path) => {
                        widget_clone.handle_item_deleted(&path);
                    }
                    ListWatcherEvent::DirectoryDeleted(path) => {
                        widget_clone.handle_dir_deleted(&path);
                    }
                }
            }
        });

        widget.navigate_root(root_path);
        widget
    }

    fn build_header_row(
        criteria: SortCriteria,
    ) -> (
        gtk4::Box,
        gtk4::Button,
        gtk4::Button,
        gtk4::Button,
        gtk4::Button,
        gtk4::Button,
    ) {
        let hbox = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(0)
            .css_classes(["hd"])
            .hexpand(true)
            .build();

        let name_btn = Self::create_header_button(
            "Name",
            COL_WIDTH_NAME,
            COL_MIN_WIDTH_NAME,
            criteria,
            SortColumn::Name,
        );
        let date_btn = Self::create_header_button(
            "Date Modified",
            COL_WIDTH_DATE,
            COL_WIDTH_DATE,
            criteria,
            SortColumn::DateModified,
        );
        let size_btn = Self::create_header_button(
            "Size",
            COL_WIDTH_SIZE,
            COL_WIDTH_SIZE,
            criteria,
            SortColumn::Size,
        );
        let kind_btn = Self::create_header_button(
            "Kind",
            COL_WIDTH_KIND,
            COL_WIDTH_KIND,
            criteria,
            SortColumn::Kind,
        );
        let tags_btn = Self::create_header_button(
            "Tags",
            COL_WIDTH_TAGS,
            COL_WIDTH_TAGS,
            criteria,
            SortColumn::Tags,
        );

        hbox.append(&name_btn);
        hbox.append(&date_btn);
        hbox.append(&size_btn);
        hbox.append(&kind_btn);
        hbox.append(&tags_btn);

        (hbox, name_btn, date_btn, size_btn, kind_btn, tags_btn)
    }

    fn create_header_button(
        title: &str,
        width: i32,
        min_width: i32,
        criteria: SortCriteria,
        col: SortColumn,
    ) -> gtk4::Button {
        let label_text = if criteria.primary == col {
            let arrow = match criteria.direction {
                SortDirection::Ascending => "▲",
                SortDirection::Descending => "▼",
            };
            format!("{title} {arrow}")
        } else {
            title.to_string()
        };

        let label = gtk4::Label::builder()
            .label(&label_text)
            .halign(gtk4::Align::Start)
            .build();

        gtk4::Button::builder()
            .child(&label)
            .width_request(min_width.max(width))
            .css_classes(["hd-cell"])
            .halign(gtk4::Align::Start)
            .build()
    }

    fn update_header_labels(&self) {
        let criteria = *self.inner.criteria.borrow();

        let update_btn = |btn: &gtk4::Button, title: &str, col: SortColumn| {
            let text = if criteria.primary == col {
                let arrow = match criteria.direction {
                    SortDirection::Ascending => "▲",
                    SortDirection::Descending => "▼",
                };
                format!("{title} {arrow}")
            } else {
                title.to_string()
            };
            if let Some(label) = btn.child().and_downcast::<gtk4::Label>() {
                label.set_text(&text);
            }
        };

        update_btn(&self.inner.name_header_btn, "Name", SortColumn::Name);
        update_btn(
            &self.inner.date_header_btn,
            "Date Modified",
            SortColumn::DateModified,
        );
        update_btn(&self.inner.size_header_btn, "Size", SortColumn::Size);
        update_btn(&self.inner.kind_header_btn, "Kind", SortColumn::Kind);
        update_btn(&self.inner.tags_header_btn, "Tags", SortColumn::Tags);
    }

    /// Navigates the view to display the directory contents of `new_root`.
    pub fn navigate_root(&self, new_root: &Path) {
        let root_buf = new_root.to_path_buf();
        self.inner.watch_pool.borrow_mut().clear();

        // Create new model with new root
        *self.inner.model.borrow_mut() = TreeListViewModel::new_empty(root_buf.clone());
        self.inner.selected_index.set(None);
        self.clear_list_rows();

        let criteria = *self.inner.criteria.borrow();
        let cached_expanded = self.inner.cache.borrow().get_expanded(&root_buf);

        let this = self.clone();
        let load_root = root_buf;

        glib::MainContext::default().spawn_local(async move {
            if let Ok(children) = read_dir_async(load_root.clone(), 0, criteria).await {
                this.inner.model.borrow_mut().set_root_children(children);
                this.inner
                    .watch_pool
                    .borrow_mut()
                    .add_watch(load_root.clone());

                this.refresh_rows();

                // Auto-expand cached directories without blocking initial view
                if !cached_expanded.is_empty() {
                    this.restore_cached_expansions(&cached_expanded);
                }
            }
        });

        if let Some(ref cb) = *self.inner.on_path_changed.borrow() {
            cb(new_root);
        }
    }

    fn restore_cached_expansions(&self, cached: &HashSet<PathBuf>) {
        for path in cached {
            if self.inner.model.borrow().find_node(path).is_some() {
                self.expand_path_async(path, false);
            }
        }
    }

    /// Toggles the expansion state of `target_path`. If `recursive` is true, performs Option+Click expansion.
    pub fn toggle_expand_path(&self, target_path: &Path, recursive: bool) {
        let is_currently_expanded = self
            .inner
            .model
            .borrow()
            .find_node(target_path)
            .is_some_and(|n| n.is_expanded);

        if is_currently_expanded {
            self.collapse_path(target_path);
        } else {
            self.expand_path_async(target_path, recursive);
        }
    }

    /// Expands `target_path` asynchronously. If `recursive` is true, descends up to 20 levels.
    pub fn expand_path_async(&self, target_path: &Path, recursive: bool) {
        let path_buf = target_path.to_path_buf();
        let (depth, inode_key) = {
            let model = self.inner.model.borrow();
            let Some(node) = model.find_node(target_path) else {
                return;
            };
            if !node.is_directory {
                return;
            }
            (node.depth, node.inode_key)
        };

        // Mark node as loading
        self.inner.model.borrow_mut().set_loading(target_path, true);
        self.refresh_rows();

        let criteria = *self.inner.criteria.borrow();
        let this = self.clone();

        glib::MainContext::default().spawn_local(async move {
            if recursive {
                let mut seen_inodes = HashSet::new();
                seen_inodes.insert(inode_key);

                if let Ok((children, _)) =
                    read_subtree_recursive_async(path_buf.clone(), depth + 1, criteria, seen_inodes)
                        .await
                {
                    this.inner
                        .model
                        .borrow_mut()
                        .set_node_children(&path_buf, children);

                    let root_path = this.inner.model.borrow().root_path.clone();
                    let mut cache = this.inner.cache.borrow_mut();
                    cache.add_expanded(&root_path, path_buf.clone());
                    let _ = cache.save();

                    this.inner.watch_pool.borrow_mut().add_watch(path_buf);
                } else {
                    this.inner.model.borrow_mut().set_loading(&path_buf, false);
                }
            } else if let Ok(children) = read_dir_async(path_buf.clone(), depth + 1, criteria).await
            {
                this.inner
                    .model
                    .borrow_mut()
                    .set_node_children(&path_buf, children);

                let root_path = this.inner.model.borrow().root_path.clone();
                let mut cache = this.inner.cache.borrow_mut();
                cache.add_expanded(&root_path, path_buf.clone());
                let _ = cache.save();

                this.inner.watch_pool.borrow_mut().add_watch(path_buf);
            } else {
                this.inner.model.borrow_mut().set_loading(&path_buf, false);
            }
            this.refresh_rows();
        });
    }

    /// Collapses `target_path` and evicts descendant watchers immediately.
    pub fn collapse_path(&self, target_path: &Path) {
        self.inner.model.borrow_mut().collapse_node(target_path);

        let root_path = self.inner.model.borrow().root_path.clone();
        let mut cache = self.inner.cache.borrow_mut();
        cache.remove_expanded(&root_path, target_path);
        let _ = cache.save();

        self.inner
            .watch_pool
            .borrow_mut()
            .evict_subtree(target_path);
        self.refresh_rows();
    }

    fn handle_item_deleted(&self, path: &Path) {
        if self.inner.model.borrow_mut().remove_node(path) {
            self.refresh_rows();
        }
    }

    fn handle_dir_deleted(&self, path: &Path) {
        let root = self.inner.model.borrow().root_path.clone();
        if path == root {
            if let Some(parent) = root.parent() {
                self.navigate_root(parent);
            }
        } else {
            self.handle_item_deleted(path);
        }
    }

    /// Attaches a [`NavigationCoordinator`] to handle folder size calculations, context menus, and drag-and-drop.
    pub fn set_coordinator(&self, coordinator: &Rc<NavigationCoordinator>) {
        *self.inner.coordinator.borrow_mut() = Some(Rc::clone(coordinator));

        let this = self.clone();
        coordinator
            .folder_size_manager
            .attach_glib_dispatcher(move |path, result| {
                let labels = this.inner.size_labels.borrow();
                if let Some(lbl) = labels.get(&path) {
                    lbl.set_label(&result.formatted_summary());
                }
            });
    }

    fn clear_list_rows(&self) {
        self.inner.size_labels.borrow_mut().clear();
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }
    }

    /// Populates the list view with a custom collection of file items (e.g. for virtual tag queries).
    pub fn set_custom_items(&self, virtual_root: &Path, items: Vec<FileItem>) {
        self.clear_list_rows();
        let nodes: Vec<TreeNode> = items
            .into_iter()
            .map(|item| TreeNode::new(item, 0))
            .collect();
        {
            let mut model = self.inner.model.borrow_mut();
            model.root_path = virtual_root.to_path_buf();
            model.set_root_children(nodes);
        }
        self.refresh_rows();
    }

    /// Rebuilds visible rows in the `ListBox` matching the flattened model.
    pub fn refresh_rows(&self) {
        let rows = self.inner.model.borrow().flattened_rows.clone();
        let selected_idx = self.inner.selected_index.get();

        self.clear_list_rows();

        let this = self.clone();
        batch_process_rows(
            rows,
            50,
            move |chunk, is_last| {
                for node in chunk {
                    let row_widget = this.create_row_widget(node);
                    this.list_box.append(&row_widget);
                }
                if is_last {
                    this.update_row_selection_styles(selected_idx);
                }
            },
            None,
        );
    }

    fn create_row_widget(&self, node: &TreeNode) -> gtk4::ListBoxRow {
        let row = gtk4::ListBoxRow::builder().css_classes(["r"]).build();

        if let Some(coord) = self.inner.coordinator.borrow().clone() {
            crate::ui::context_menu::attach_row_context_menu(
                &row,
                &node.path,
                node.is_directory,
                &coord,
            );
            if node.is_directory {
                crate::ui::context_menu::attach_directory_drop_target(&row, &node.path, &coord);
            }
        }

        let hbox = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(0)
            .hexpand(true)
            .build();

        // 1. Name Column (320px) with indent
        let name_cell = self.create_name_cell(node);
        hbox.append(&name_cell);

        // 2. Date Modified Column (150px)
        let date_cell = Self::create_date_cell(node);
        hbox.append(&date_cell);

        // 3. Size Column (140px)
        let size_cell = self.create_size_cell(node);
        hbox.append(&size_cell);

        // 4. Kind Column (120px)
        let kind_cell = Self::create_kind_cell(node);
        hbox.append(&kind_cell);

        // 5. Tags Column (80px)
        let tags_cell = Self::create_tags_cell(node);
        hbox.append(&tags_cell);

        row.set_child(Some(&hbox));
        row
    }

    fn create_name_cell(&self, node: &TreeNode) -> gtk4::Box {
        let cell = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(6)
            .width_request(COL_WIDTH_NAME)
            .css_classes(["dc", "dc-name"])
            .halign(gtk4::Align::Start)
            .hexpand(false)
            .build();

        // Indentation padding: depth * 16px
        let indent_px = i32::try_from(node.depth * 16).unwrap_or(0);
        if indent_px > 0 {
            let spacer = gtk4::Box::builder().width_request(indent_px).build();
            cell.append(&spacer);
        }

        // Disclosure chevron or loading spinner
        if node.is_directory {
            if node.is_loading {
                let spinner = gtk4::Spinner::builder()
                    .spinning(true)
                    .css_classes(["disclosure-spinner"])
                    .build();
                cell.append(&spinner);
            } else {
                let chevron_char = if node.is_expanded { "▾" } else { "▸" };
                let chevron_btn = gtk4::Button::builder()
                    .label(chevron_char)
                    .css_classes(["disclosure-chevron"])
                    .halign(gtk4::Align::Center)
                    .valign(gtk4::Align::Center)
                    .build();

                let this = self.clone();
                let path = node.path.clone();

                let click_gesture = gtk4::GestureClick::new();
                click_gesture.connect_pressed(move |g, _n_press, _x, _y| {
                    let state = g.current_event_state();
                    let is_alt = state.contains(gdk4::ModifierType::ALT_MASK)
                        || state.contains(gdk4::ModifierType::META_MASK);
                    this.toggle_expand_path(&path, is_alt);
                });
                chevron_btn.add_controller(click_gesture);
                cell.append(&chevron_btn);
            }
        } else {
            // Placeholder spacer to align file icons with folders
            let blank = gtk4::Box::builder().width_request(16).build();
            cell.append(&blank);
        }

        // Icon
        let icon_name = Self::resolve_icon_name(&node.item);
        let icon = gtk4::Image::builder()
            .icon_name(icon_name)
            .pixel_size(16)
            .css_classes(["column-icon", if node.is_directory { "fi" } else { "fl" }])
            .build();
        cell.append(&icon);

        // Name Label
        let label = gtk4::Label::builder()
            .label(node.file_name())
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .hexpand(true)
            .build();
        cell.append(&label);

        cell
    }

    fn create_date_cell(node: &TreeNode) -> gtk4::Box {
        let cell = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .width_request(COL_WIDTH_DATE)
            .css_classes(["dc", "dc-date"])
            .halign(gtk4::Align::Start)
            .build();

        let date_str = format_system_time(node.item.metadata.modified);
        let label = gtk4::Label::builder()
            .label(&date_str)
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        cell.append(&label);
        cell
    }

    fn create_size_cell(&self, node: &TreeNode) -> gtk4::Box {
        let cell = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .width_request(COL_WIDTH_SIZE)
            .css_classes(["dc", "dc-size"])
            .halign(gtk4::Align::Start)
            .build();

        let label = gtk4::Label::builder()
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();

        if node.is_directory {
            let prefs = FinderPreferences::load();
            if prefs.calculate_all_sizes {
                let coord_opt = self.inner.coordinator.borrow().clone();
                if let Some(coord) = coord_opt {
                    if let Some(cached) = coord
                        .folder_size_manager
                        .request_folder_size(&node.path, Priority::High)
                    {
                        label.set_label(&cached.formatted_summary());
                    } else {
                        label.set_label("--");
                        self.inner
                            .size_labels
                            .borrow_mut()
                            .insert(node.path.clone(), label.clone());
                    }
                } else {
                    label.set_label("--");
                }
            } else if !node.children.is_empty() {
                let count = node.children.len();
                if count == 1 {
                    label.set_label("1 item");
                } else {
                    label.set_label(&format!("{count} items"));
                }
            } else if node.item.metadata.size_bytes > 0 {
                label.set_label(&format_byte_size(node.item.metadata.size_bytes));
            } else {
                label.set_label("—");
            }
        } else {
            label.set_label(&format_byte_size(node.item.metadata.size_bytes));
        }

        cell.append(&label);
        cell
    }

    fn create_kind_cell(node: &TreeNode) -> gtk4::Box {
        let cell = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .width_request(COL_WIDTH_KIND)
            .css_classes(["dc", "dc-kind"])
            .halign(gtk4::Align::Start)
            .build();

        let kind_str = Self::format_kind_display(&node.item);
        let label = gtk4::Label::builder()
            .label(&kind_str)
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        cell.append(&label);
        cell
    }

    fn create_tags_cell(node: &TreeNode) -> gtk4::Box {
        let tags = if node.item.tags.is_empty() {
            crate::ui::tagging::badges::TagBadgeCache::global().get_or_load(&node.path)
        } else {
            node.item.tags.clone()
        };
        crate::ui::tagging::badges::render_list_view_tags_cell(&tags)
    }

    fn format_kind_display(item: &FileItem) -> String {
        if item.is_dir() {
            return "Folder".to_string();
        }
        item.extension().map_or_else(
            || classify_kind(item).title,
            |ext| match ext.to_lowercase().as_str() {
                "rs" => "Rust Source".to_string(),
                "toml" => "TOML Document".to_string(),
                "json" => "JSON Document".to_string(),
                "md" | "markdown" => "Markdown".to_string(),
                "txt" => "Plain Text".to_string(),
                "pdf" => "PDF Document".to_string(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => "Image".to_string(),
                "mp4" | "mov" | "mkv" | "avi" => "Video".to_string(),
                "mp3" | "wav" | "flac" | "ogg" => "Audio".to_string(),
                "zip" | "tar" | "gz" | "bz2" | "xz" => "Archive".to_string(),
                _ => classify_kind(item).title,
            },
        )
    }

    const fn resolve_icon_name(item: &FileItem) -> &'static str {
        if item.is_dir() {
            "folder-symbolic"
        } else if item.is_symlink() && item.symlink_target.is_none() {
            "emblem-unreadable"
        } else {
            "text-x-generic-symbolic"
        }
    }

    fn update_row_selection_styles(&self, selected_idx: Option<usize>) {
        let mut idx = 0;
        while let Some(row) = self.list_box.row_at_index(idx) {
            let is_sel = usize::try_from(idx).is_ok_and(|u| selected_idx == Some(u));
            if is_sel {
                row.add_css_class("on");
            } else {
                row.remove_css_class("on");
            }
            idx += 1;
        }
    }

    fn wire_header_signals(&self) {
        let attach_sort = |btn: &gtk4::Button, col: SortColumn, this: Self| {
            btn.connect_clicked(move |_| {
                let mut c = *this.inner.criteria.borrow();
                if c.primary == col {
                    c.direction = c.direction.toggle();
                } else {
                    c.primary = col;
                    c.direction = SortDirection::Ascending;
                }
                *this.inner.criteria.borrow_mut() = c;
                this.inner.model.borrow_mut().sort(&c);
                this.update_header_labels();
                this.refresh_rows();
            });
        };

        attach_sort(&self.inner.name_header_btn, SortColumn::Name, self.clone());
        attach_sort(
            &self.inner.date_header_btn,
            SortColumn::DateModified,
            self.clone(),
        );
        attach_sort(&self.inner.size_header_btn, SortColumn::Size, self.clone());
        attach_sort(&self.inner.kind_header_btn, SortColumn::Kind, self.clone());
        attach_sort(&self.inner.tags_header_btn, SortColumn::Tags, self.clone());
    }

    fn wire_list_signals(&self) {
        let this = self.clone();
        self.list_box.connect_row_selected(move |_lb, maybe_row| {
            if let Some(row) = maybe_row
                && let Ok(idx) = usize::try_from(row.index())
            {
                this.inner.selected_index.set(Some(idx));
                this.update_row_selection_styles(Some(idx));
            } else if maybe_row.is_none() {
                this.inner.selected_index.set(None);
                this.update_row_selection_styles(None);
            }
        });

        let this = self.clone();
        self.list_box.connect_row_activated(move |_lb, row| {
            if let Ok(idx) = usize::try_from(row.index()) {
                let model = this.inner.model.borrow();
                if let Some(node) = model.row_at(idx) {
                    if node.is_directory {
                        let path = node.path.clone();
                        drop(model);
                        this.toggle_expand_path(&path, false);
                    } else if let Some(ref cb) = *this.inner.on_item_activated.borrow() {
                        cb(node);
                    }
                }
            }
        });
    }

    fn wire_keyboard_navigation(&self) {
        let this = self.clone();
        attach_list_keyboard_controller(&self.container, move |action| {
            this.handle_key_action(action)
        });
    }

    /// Handles keyboard action according to list traversal rules.
    #[must_use]
    pub fn handle_key_action(&self, action: ListKeyAction) -> glib::Propagation {
        let current_idx = self.inner.selected_index.get();
        let total = self.inner.model.borrow().row_count();

        match action {
            ListKeyAction::Up => {
                if total == 0 {
                    return glib::Propagation::Proceed;
                }
                let next_idx = current_idx.map_or(0, |idx| idx.saturating_sub(1));
                self.select_index(next_idx);
                glib::Propagation::Stop
            }
            ListKeyAction::Down => {
                if total == 0 {
                    return glib::Propagation::Proceed;
                }
                let next_idx = current_idx.map_or(0, |idx| (idx + 1).min(total.saturating_sub(1)));
                self.select_index(next_idx);
                glib::Propagation::Stop
            }
            ListKeyAction::Right => {
                let Some(idx) = current_idx else {
                    return glib::Propagation::Proceed;
                };
                let model = self.inner.model.borrow();
                let Some(node) = model.row_at(idx) else {
                    return glib::Propagation::Proceed;
                };
                if !node.is_directory {
                    return glib::Propagation::Proceed;
                }

                if node.is_expanded {
                    // Move selection to first child
                    if let Some(first_child) = model.first_child_of(&node.path)
                        && let Some(child_idx) = model.index_of_path(&first_child)
                    {
                        drop(model);
                        self.select_index(child_idx);
                    }
                } else {
                    let path = node.path.clone();
                    drop(model);
                    self.expand_path_async(&path, false);
                }
                glib::Propagation::Stop
            }
            ListKeyAction::Left => {
                let Some(idx) = current_idx else {
                    return glib::Propagation::Proceed;
                };
                let model = self.inner.model.borrow();
                let Some(node) = model.row_at(idx) else {
                    return glib::Propagation::Proceed;
                };

                if node.is_directory && node.is_expanded {
                    let path = node.path.clone();
                    drop(model);
                    self.collapse_path(&path);
                } else if let Some(parent_path) = model.parent_path_of(&node.path)
                    && let Some(parent_idx) = model.index_of_path(&parent_path)
                {
                    drop(model);
                    self.select_index(parent_idx);
                }
                glib::Propagation::Stop
            }
            ListKeyAction::Enter => {
                let Some(idx) = current_idx else {
                    return glib::Propagation::Proceed;
                };
                let model = self.inner.model.borrow();
                let Some(node) = model.row_at(idx) else {
                    return glib::Propagation::Proceed;
                };
                if node.is_directory {
                    let path = node.path.clone();
                    drop(model);
                    self.toggle_expand_path(&path, false);
                } else if let Some(ref cb) = *self.inner.on_item_activated.borrow() {
                    cb(node);
                }
                glib::Propagation::Stop
            }
            ListKeyAction::Spacebar => {
                let Some(idx) = current_idx else {
                    return glib::Propagation::Proceed;
                };
                let model = self.inner.model.borrow();
                if let Some(node) = model.row_at(idx)
                    && let Some(ref cb) = *self.inner.on_quick_look.borrow()
                {
                    cb(node);
                }
                glib::Propagation::Stop
            }
        }
    }

    /// Selects row at `index` and scrolls it into view.
    pub fn select_index(&self, index: usize) {
        if let Ok(i) = i32::try_from(index)
            && let Some(row) = self.list_box.row_at_index(i)
        {
            self.inner.selected_index.set(Some(index));
            self.list_box.select_row(Some(&row));
            self.update_row_selection_styles(Some(index));
            row.grab_focus();
        }
    }

    /// Connects callback invoked when path changes.
    pub fn connect_path_changed<F>(&self, cb: F)
    where
        F: Fn(&Path) + 'static,
    {
        *self.inner.on_path_changed.borrow_mut() = Some(Box::new(cb));
    }

    /// Connects callback invoked when item is activated (e.g. Enter or double click).
    pub fn connect_item_activated<F>(&self, cb: F)
    where
        F: Fn(&TreeNode) + 'static,
    {
        *self.inner.on_item_activated.borrow_mut() = Some(Box::new(cb));
    }

    /// Connects callback invoked for Quick Look preview (Spacebar).
    pub fn connect_quick_look<F>(&self, cb: F)
    where
        F: Fn(&TreeNode) + 'static,
    {
        *self.inner.on_quick_look.borrow_mut() = Some(Box::new(cb));
    }

    /// Returns the currently active sort criteria.
    #[must_use]
    pub fn sort_criteria(&self) -> SortCriteria {
        *self.inner.criteria.borrow()
    }

    /// Sets the active sort criteria and resorts rows.
    pub fn set_sort_criteria(&self, criteria: SortCriteria) {
        *self.inner.criteria.borrow_mut() = criteria;
        self.inner.model.borrow_mut().sort(&criteria);
        self.update_header_labels();
        self.refresh_rows();
    }
}
