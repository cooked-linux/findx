//! Interactive bottom breadcrumbs path bar component (28px height).
//!
//! Provides spatial orientation from filesystem root/volume to current directory or leaf item,
//! sibling popover menus, responsive overflow collapse, and spring-loaded hover navigation.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use shared::models::{BatchOperation, OperationKind};

use crate::breadcrumbs::{
    BreadcrumbDisplayItem, BreadcrumbLineage, PathSegment, layout_segments, parse_uri_list,
};

/// Navigation callback invoked when a breadcrumb segment or sibling is activated.
pub type BreadcrumbNavCallback = Box<dyn Fn(&Path)>;
/// Callback invoked when a folder should be opened in a new window or tab.
pub type BreadcrumbOpenCallback = Box<dyn Fn(&Path)>;
/// Callback invoked on drag-and-drop file ingestion.
pub type BreadcrumbDropCallback = Box<dyn Fn(&Path, Vec<PathBuf>)>;
/// Callback invoked when a batch operation is generated from a drop event.
pub type BreadcrumbBatchOpCallback = Box<dyn Fn(BatchOperation)>;

#[derive(Default)]
struct PathBarCallbacks {
    navigate: Option<BreadcrumbNavCallback>,
    open_new_window: Option<BreadcrumbOpenCallback>,
    open_new_tab: Option<BreadcrumbOpenCallback>,
    drop_files: Option<BreadcrumbDropCallback>,
    batch_op: Option<BreadcrumbBatchOpCallback>,
}

struct PathBarInner {
    lineage: RefCell<BreadcrumbLineage>,
    callbacks: RefCell<PathBarCallbacks>,
    spring_timer: RefCell<Option<glib::SourceId>>,
}

impl std::fmt::Debug for PathBarInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathBarInner")
            .field("lineage", &self.lineage)
            .finish_non_exhaustive()
    }
}

impl PathBarInner {
    fn emit_navigate(&self, path: &Path) {
        if let Some(ref cb) = self.callbacks.borrow().navigate {
            cb(path);
        }
    }

    fn emit_open_new_window(&self, path: &Path) {
        if let Some(ref cb) = self.callbacks.borrow().open_new_window {
            cb(path);
        }
    }

    fn emit_open_new_tab(&self, path: &Path) {
        if let Some(ref cb) = self.callbacks.borrow().open_new_tab {
            cb(path);
        }
    }

    fn emit_drop_files(&self, target_dir: &Path, files: Vec<PathBuf>) {
        if let Some(ref cb) = self.callbacks.borrow().drop_files {
            cb(target_dir, files.clone());
        }

        let op_id = format!("drop-{}", chrono::Utc::now().timestamp_millis());
        let batch_op = BatchOperation::new(
            op_id,
            OperationKind::Copy,
            files,
            Some(target_dir.to_path_buf()),
        );

        if let Some(ref cb) = self.callbacks.borrow().batch_op {
            cb(batch_op);
        }
    }
}

/// Interactive 28px bottom path bar widget.
#[derive(Debug, Clone)]
pub struct PathBarWidget {
    /// Outer horizontal container box with height 28px and class `.pb`.
    pub container: gtk4::Box,
    /// Inner horizontal container hosting rendered segments, ellipsis, and separators.
    pub segments_box: gtk4::Box,
    inner: Rc<PathBarInner>,
}

impl Default for PathBarWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl PathBarWidget {
    /// Constructs a new [`PathBarWidget`] initialized to root.
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(0)
            .valign(gtk4::Align::Center)
            .hexpand(true)
            .css_classes(["pb", "finder-breadcrumbs"])
            .build();

        let segments_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(7)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();

        container.append(&segments_box);

        let initial_lineage = BreadcrumbLineage::from_path(Path::new("/"));
        let inner = Rc::new(PathBarInner {
            lineage: RefCell::new(initial_lineage),
            callbacks: RefCell::new(PathBarCallbacks::default()),
            spring_timer: RefCell::new(None),
        });

        let widget = Self {
            container,
            segments_box,
            inner,
        };

        widget.render_items();
        widget
    }

    /// Updates the active path lineage and re-renders the path bar items.
    pub fn set_path(&self, current_path: &Path, selected_leaf: Option<&Path>) {
        let lineage = BreadcrumbLineage::resolve(current_path, selected_leaf);
        *self.inner.lineage.borrow_mut() = lineage;
        self.render_items();
    }

    /// Sets an explicit lineage model.
    pub fn set_lineage(&self, lineage: BreadcrumbLineage) {
        *self.inner.lineage.borrow_mut() = lineage;
        self.render_items();
    }

    /// Sets a custom tag filter breadcrumb display (`Tags › {tag_name}`).
    pub fn set_tag_filter(&self, tag_name: &str) {
        let segments = vec![
            PathSegment::new(
                "Tags",
                PathBuf::from("/"),
                "tag-symbolic",
                false,
                false,
                false,
            ),
            PathSegment::new(
                tag_name,
                PathBuf::from("/"),
                "tag-symbolic",
                false,
                false,
                true,
            ),
        ];
        let lineage = BreadcrumbLineage::new(segments, 1);
        self.set_lineage(lineage);
    }

    /// Returns the current [`BreadcrumbLineage`].
    #[must_use]
    pub fn lineage(&self) -> BreadcrumbLineage {
        self.inner.lineage.borrow().clone()
    }

    /// Re-renders the segments inside `segments_box` using responsive overflow layout.
    pub fn render_items(&self) {
        while let Some(child) = self.segments_box.first_child() {
            self.segments_box.remove(&child);
        }

        let lineage = self.inner.lineage.borrow().clone();
        if lineage.segments.is_empty() {
            return;
        }

        let alloc_w = self.container.width();
        let available_width = if alloc_w > 50 { alloc_w - 24 } else { 800 };

        let items = layout_segments(&lineage.segments, available_width, 60);
        let items_count = items.len();

        for (idx, item) in items.iter().enumerate() {
            let is_last = idx == items_count - 1;
            match item {
                BreadcrumbDisplayItem::Segment(seg) => {
                    let is_active = idx == items_count - 1;
                    let btn = Self::create_segment_button(seg, is_active, &self.inner);
                    self.segments_box.append(&btn);
                }
                BreadcrumbDisplayItem::Overflow { hidden_segments } => {
                    let btn = Self::create_overflow_button(hidden_segments, &self.inner);
                    self.segments_box.append(&btn);
                }
            }
            if !is_last {
                let sep = Self::create_separator();
                self.segments_box.append(&sep);
            }
        }
    }

    fn create_separator() -> gtk4::Label {
        gtk4::Label::builder()
            .label("▸")
            .css_classes(["breadcrumb-sep"])
            .valign(gtk4::Align::Center)
            .build()
    }

    fn create_segment_button(
        seg: &PathSegment,
        is_active: bool,
        inner: &Rc<PathBarInner>,
    ) -> gtk4::Button {
        let btn = gtk4::Button::builder()
            .has_frame(false)
            .css_classes(["flat", "breadcrumb-btn"])
            .valign(gtk4::Align::Center)
            .tooltip_text(seg.path.to_string_lossy().as_ref())
            .build();

        if is_active {
            btn.add_css_class("active");
        }

        let box_content = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(5)
            .valign(gtk4::Align::Center)
            .build();

        let icon = gtk4::Image::builder()
            .icon_name(&seg.icon_name)
            .pixel_size(14)
            .valign(gtk4::Align::Center)
            .build();
        box_content.append(&icon);

        let label = gtk4::Label::builder().valign(gtk4::Align::Center).build();

        if is_active {
            label.set_markup(&format!("<u>{}</u>", glib::markup_escape_text(&seg.name)));
        } else {
            label.set_text(&seg.name);
        }
        box_content.append(&label);

        btn.set_child(Some(&box_content));

        // Click Gesture handling:
        // Single-click: navigate to directory
        // Double-click: open in new window
        // Middle-click: open in new tab
        // Secondary/Right-click: sibling popover menu
        let click_gesture = gtk4::GestureClick::new();
        click_gesture.set_button(0);

        let seg_path = seg.path.clone();
        let inner_for_click = Rc::clone(inner);
        let seg_clone = seg.clone();
        let btn_weak = btn.downgrade();

        click_gesture.connect_pressed(move |gesture, n_press, _x, _y| {
            let button = gesture.current_button();
            match button {
                gdk4::BUTTON_PRIMARY => {
                    if n_press == 1 {
                        inner_for_click.emit_navigate(&seg_path);
                    } else if n_press == 2 {
                        inner_for_click.emit_open_new_window(&seg_path);
                    }
                }
                gdk4::BUTTON_MIDDLE => {
                    inner_for_click.emit_open_new_tab(&seg_path);
                }
                gdk4::BUTTON_SECONDARY => {
                    if let Some(b) = btn_weak.upgrade() {
                        let nav_inner = Rc::clone(&inner_for_click);
                        Self::show_sibling_popover(&b, &seg_clone, move |p| {
                            nav_inner.emit_navigate(p);
                        });
                    }
                }
                _ => {}
            }
        });
        btn.add_controller(click_gesture);

        if seg.is_directory {
            Self::attach_drop_target(&btn, seg, inner);
        }

        btn
    }

    fn attach_drop_target(btn: &gtk4::Button, seg: &PathSegment, inner: &Rc<PathBarInner>) {
        let drop_target = gtk4::DropTarget::new(
            glib::Type::STRING,
            gdk4::DragAction::COPY | gdk4::DragAction::MOVE,
        );

        let btn_weak_dnd = btn.downgrade();
        let inner_dnd = Rc::clone(inner);
        let path_dnd = seg.path.clone();

        drop_target.connect_enter(move |_dt, _x, _y| {
            if let Some(b) = btn_weak_dnd.upgrade() {
                b.add_css_class("drop-target");
            }

            // Step 6.7: 750ms spring-loaded hover navigation timer
            if let Some(src) = inner_dnd.spring_timer.borrow_mut().take() {
                src.remove();
            }
            let inner_timer = Rc::clone(&inner_dnd);
            let nav_target = path_dnd.clone();
            let timer_id = glib::timeout_add_local_once(Duration::from_millis(750), move || {
                inner_timer.emit_navigate(&nav_target);
                *inner_timer.spring_timer.borrow_mut() = None;
            });
            *inner_dnd.spring_timer.borrow_mut() = Some(timer_id);

            gdk4::DragAction::COPY
        });

        let btn_weak_leave = btn.downgrade();
        let inner_leave = Rc::clone(inner);
        drop_target.connect_leave(move |_dt| {
            if let Some(b) = btn_weak_leave.upgrade() {
                b.remove_css_class("drop-target");
            }
            if let Some(src) = inner_leave.spring_timer.borrow_mut().take() {
                src.remove();
            }
        });

        let btn_weak_drop = btn.downgrade();
        let inner_drop = Rc::clone(inner);
        let drop_dest = seg.path.clone();
        drop_target.connect_drop(move |_dt, value, _x, _y| {
            if let Some(b) = btn_weak_drop.upgrade() {
                b.remove_css_class("drop-target");
            }
            if let Some(src) = inner_drop.spring_timer.borrow_mut().take() {
                src.remove();
            }

            if let Ok(raw_str) = value.get::<String>() {
                let paths = parse_uri_list(&raw_str);
                if !paths.is_empty() {
                    inner_drop.emit_drop_files(&drop_dest, paths);
                    return true;
                }
            }
            false
        });

        btn.add_controller(drop_target);
    }

    fn create_overflow_button(
        hidden_segments: &[PathSegment],
        inner: &Rc<PathBarInner>,
    ) -> gtk4::Button {
        let btn = gtk4::Button::builder()
            .has_frame(false)
            .css_classes(["flat", "breadcrumb-btn"])
            .valign(gtk4::Align::Center)
            .tooltip_text("Hidden intermediate locations")
            .build();

        let label = gtk4::Label::builder()
            .label("···")
            .valign(gtk4::Align::Center)
            .build();
        btn.set_child(Some(&label));

        let hidden = hidden_segments.to_vec();
        let inner_for_pop = Rc::clone(inner);
        let btn_weak = btn.downgrade();

        btn.connect_clicked(move |_| {
            if let Some(b) = btn_weak.upgrade() {
                let inner_nav = Rc::clone(&inner_for_pop);
                Self::show_overflow_popover(&b, &hidden, move |p| {
                    inner_nav.emit_navigate(p);
                });
            }
        });

        btn
    }

    fn show_sibling_popover<F: Fn(&Path) + 'static>(
        anchor: &impl IsA<gtk4::Widget>,
        segment: &PathSegment,
        nav_cb: F,
    ) {
        let parent_dir = if segment.is_root {
            PathBuf::from("/")
        } else {
            segment
                .path
                .parent()
                .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
        };

        let popover = gtk4::Popover::builder()
            .has_arrow(true)
            .css_classes(["breadcrumb-popover"])
            .build();
        popover.set_parent(anchor);

        let scrolled = gtk4::ScrolledWindow::builder()
            .max_content_height(260)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .build();

        let list_box = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .build();

        let current_seg_path = segment.path.clone();
        let nav_cb = Rc::new(nav_cb);

        let mut siblings = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&parent_dir) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type()
                    && ft.is_dir()
                {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();
                    siblings.push((name, path));
                }
            }
        }

        siblings.sort_by(|a, b| shared::fs::natural_cmp(&a.0, &b.0));

        if siblings.is_empty() {
            let empty_row = gtk4::Label::builder()
                .label("No sibling directories")
                .css_classes(["breadcrumb-popover-item"])
                .build();
            list_box.append(&empty_row);
        } else {
            for (name, path) in siblings {
                let row = gtk4::Box::builder()
                    .orientation(gtk4::Orientation::Horizontal)
                    .spacing(8)
                    .css_classes(["breadcrumb-popover-item"])
                    .build();

                let icon = gtk4::Image::builder()
                    .icon_name("folder-symbolic")
                    .pixel_size(14)
                    .build();
                row.append(&icon);

                let label = gtk4::Label::builder()
                    .label(&name)
                    .halign(gtk4::Align::Start)
                    .build();
                if path == current_seg_path {
                    label.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&name)));
                }
                row.append(&label);

                let click = gtk4::GestureClick::new();
                let pop_weak = popover.downgrade();
                let nav_clone = Rc::clone(&nav_cb);
                let target_path = path.clone();
                click.connect_pressed(move |_g, _n, _x, _y| {
                    if let Some(p) = pop_weak.upgrade() {
                        p.popdown();
                    }
                    nav_clone(&target_path);
                });
                row.add_controller(click);

                list_box.append(&row);
            }
        }

        scrolled.set_child(Some(&list_box));
        popover.set_child(Some(&scrolled));

        popover.connect_closed(|p| {
            p.unparent();
        });

        popover.popup();
    }

    fn show_overflow_popover<F: Fn(&Path) + 'static>(
        anchor: &impl IsA<gtk4::Widget>,
        hidden_segments: &[PathSegment],
        nav_cb: F,
    ) {
        let popover = gtk4::Popover::builder()
            .has_arrow(true)
            .css_classes(["breadcrumb-popover"])
            .build();
        popover.set_parent(anchor);

        let scrolled = gtk4::ScrolledWindow::builder()
            .max_content_height(260)
            .propagate_natural_height(true)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .build();

        let list_box = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .build();

        let nav_cb = Rc::new(nav_cb);

        for seg in hidden_segments {
            let row = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(8)
                .css_classes(["breadcrumb-popover-item"])
                .tooltip_text(seg.path.to_string_lossy().as_ref())
                .build();

            let icon = gtk4::Image::builder()
                .icon_name(&seg.icon_name)
                .pixel_size(14)
                .build();
            row.append(&icon);

            let label = gtk4::Label::builder()
                .label(&seg.name)
                .halign(gtk4::Align::Start)
                .build();
            row.append(&label);

            let click = gtk4::GestureClick::new();
            let pop_weak = popover.downgrade();
            let nav_clone = Rc::clone(&nav_cb);
            let target_path = seg.path.clone();
            click.connect_pressed(move |_g, _n, _x, _y| {
                if let Some(p) = pop_weak.upgrade() {
                    p.popdown();
                }
                nav_clone(&target_path);
            });
            row.add_controller(click);

            list_box.append(&row);
        }

        scrolled.set_child(Some(&list_box));
        popover.set_child(Some(&scrolled));

        popover.connect_closed(|p| {
            p.unparent();
        });

        popover.popup();
    }

    /// Connects a callback invoked when the user clicks a segment or sibling to navigate.
    pub fn connect_navigate<F: Fn(&Path) + 'static>(&self, callback: F) {
        self.inner.callbacks.borrow_mut().navigate = Some(Box::new(callback));
    }

    /// Connects a callback invoked when the user double-clicks a segment to open in a new window.
    pub fn connect_open_new_window<F: Fn(&Path) + 'static>(&self, callback: F) {
        self.inner.callbacks.borrow_mut().open_new_window = Some(Box::new(callback));
    }

    /// Connects a callback invoked when the user middle-clicks a segment to open in a new tab.
    pub fn connect_open_new_tab<F: Fn(&Path) + 'static>(&self, callback: F) {
        self.inner.callbacks.borrow_mut().open_new_tab = Some(Box::new(callback));
    }

    /// Connects a callback invoked when files are dropped onto a segment.
    pub fn connect_drop_files<F: Fn(&Path, Vec<PathBuf>) + 'static>(&self, callback: F) {
        self.inner.callbacks.borrow_mut().drop_files = Some(Box::new(callback));
    }

    /// Connects a callback receiving a [`BatchOperation`] generated from a drop event.
    pub fn connect_batch_operation<F: Fn(BatchOperation) + 'static>(&self, callback: F) {
        self.inner.callbacks.borrow_mut().batch_op = Some(Box::new(callback));
    }
}
