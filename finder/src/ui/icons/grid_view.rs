//! macOS Finder-style Icon Grid View widget.
//!
//! Provides a responsive, multi-column grid of filesystem items with large icons/thumbnails,
//! two-line centered filename labels, tag badges, asynchronous directory loading,
//! multi-selection, and double-click navigation.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use shared::models::FileItem;

use crate::coordinator::NavigationCoordinator;
use crate::views::columns::loader::read_directory;
use crate::views::columns::models::ColumnEntry;

/// Default icon size in pixels.
pub const DEFAULT_ICON_SIZE: i32 = 64;

/// Minimum allowed icon size in pixels.
pub const MIN_ICON_SIZE: i32 = 32;

/// Maximum allowed icon size in pixels.
pub const MAX_ICON_SIZE: i32 = 192;

/// Callback invoked when selected path or directory changes.
pub type PathChangedCallback = Box<dyn Fn(&Path)>;

struct IconGridInner {
    entries: RefCell<Vec<ColumnEntry>>,
    current_path: RefCell<PathBuf>,
    icon_size: Cell<i32>,
    coordinator: RefCell<Option<Rc<NavigationCoordinator>>>,
    on_path_changed: RefCell<Option<PathChangedCallback>>,
}

impl fmt::Debug for IconGridInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IconGridInner")
            .field("current_path", &self.current_path)
            .field("icon_size", &self.icon_size.get())
            .field("entries_count", &self.entries.borrow().len())
            .finish_non_exhaustive()
    }
}

/// Primary widget displaying directory contents as a wrapped, multi-selection icon grid.
#[derive(Debug, Clone)]
pub struct IconGridViewWidget {
    /// Top-level vertical box container.
    pub container: gtk4::Box,
    /// Scrolled window enclosing the responsive flow box.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Wrapped grid container hosting icon cards.
    pub flow_box: gtk4::FlowBox,
    inner: Rc<IconGridInner>,
}

impl IconGridViewWidget {
    /// Instantiates a new [`IconGridViewWidget`] targeting `root_path`.
    #[must_use]
    pub fn new(root_path: &Path) -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["icon-grid-view"])
            .build();

        let flow_box = gtk4::FlowBox::builder()
            .homogeneous(true)
            .selection_mode(gtk4::SelectionMode::Multiple)
            .min_children_per_line(2)
            .max_children_per_line(30)
            .column_spacing(16)
            .row_spacing(16)
            .margin_start(16)
            .margin_end(16)
            .margin_top(16)
            .margin_bottom(16)
            .valign(gtk4::Align::Start)
            .css_classes(["icon-grid-flowbox"])
            .build();

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&flow_box)
            .build();

        container.append(&scrolled_window);

        let inner = Rc::new(IconGridInner {
            entries: RefCell::new(Vec::new()),
            current_path: RefCell::new(root_path.to_path_buf()),
            icon_size: Cell::new(DEFAULT_ICON_SIZE),
            coordinator: RefCell::new(None),
            on_path_changed: RefCell::new(None),
        });

        let widget = Self {
            container,
            scrolled_window,
            flow_box,
            inner,
        };

        widget.wire_signals();
        widget.navigate_root(root_path);

        widget
    }

    /// Sets the central [`NavigationCoordinator`] for context menus, drag/drop, and selection.
    pub fn set_coordinator(&self, coordinator: &Rc<NavigationCoordinator>) {
        *self.inner.coordinator.borrow_mut() = Some(Rc::clone(coordinator));
        self.populate_cards();
    }

    /// Connects a callback invoked when selection or navigation target path changes.
    pub fn connect_path_changed<F: Fn(&Path) + 'static>(&self, callback: F) {
        *self.inner.on_path_changed.borrow_mut() = Some(Box::new(callback));
    }

    /// Sets the icon pixel dimensions, clamped to `[MIN_ICON_SIZE, MAX_ICON_SIZE]`.
    pub fn set_icon_size(&self, size: i32) {
        let clamped = size.clamp(MIN_ICON_SIZE, MAX_ICON_SIZE);
        self.inner.icon_size.set(clamped);
        self.populate_cards();
    }

    /// Returns the active icon size in pixels.
    #[must_use]
    pub fn icon_size(&self) -> i32 {
        self.inner.icon_size.get()
    }

    /// Returns the currently active directory path.
    #[must_use]
    pub fn current_path(&self) -> PathBuf {
        self.inner.current_path.borrow().clone()
    }

    /// Asynchronously reloads and navigates to `new_root`.
    pub fn navigate_root(&self, new_root: &Path) {
        *self.inner.current_path.borrow_mut() = new_root.to_path_buf();
        let path_clone = new_root.to_path_buf();
        let this = self.clone();

        glib::MainContext::default().spawn_local(async move {
            let res = read_directory(&path_clone, None).await;
            match res {
                Ok(entries) => {
                    *this.inner.entries.borrow_mut() = entries;
                    this.populate_cards();
                }
                Err(err) => {
                    tracing::warn!("Failed to read directory {}: {err}", path_clone.display());
                    this.render_empty_state("dialog-error-symbolic", &err.to_string());
                }
            }
        });
    }

    /// Displays custom virtual items (e.g. tag query or saved search results).
    pub fn set_custom_items(&self, virtual_path: &Path, items: Vec<FileItem>) {
        *self.inner.current_path.borrow_mut() = virtual_path.to_path_buf();
        let entries = items
            .into_iter()
            .map(|item| ColumnEntry::new(item, false))
            .collect();
        *self.inner.entries.borrow_mut() = entries;
        self.populate_cards();
    }

    /// Clears and repopulates all flow box child cards from current entries.
    fn populate_cards(&self) {
        while let Some(child) = self.flow_box.first_child() {
            self.flow_box.remove(&child);
        }

        let entries = self.inner.entries.borrow().clone();
        if entries.is_empty() {
            self.render_empty_state("folder-symbolic", "Folder is empty");
            return;
        }

        let icon_size = self.inner.icon_size.get();
        let coord = self.inner.coordinator.borrow().clone();

        for entry in &entries {
            let child = Self::create_card(entry, icon_size, coord.as_ref());
            self.flow_box.append(&child);
        }
    }

    fn create_card(
        entry: &ColumnEntry,
        icon_size: i32,
        coordinator: Option<&Rc<NavigationCoordinator>>,
    ) -> gtk4::FlowBoxChild {
        let child = gtk4::FlowBoxChild::builder()
            .focusable(true)
            .css_classes(["icon-grid-item"])
            .build();

        let card_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Start)
            .width_request(icon_size + 36)
            .css_classes(["icon-card"])
            .build();

        // 1. Icon Container with tag badges
        let icon_container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .build();

        let (icon_name, icon_class) = Self::resolve_icon_info(entry);
        let icon_img = gtk4::Image::builder()
            .icon_name(icon_name)
            .pixel_size(icon_size)
            .css_classes(["icon-card-image", icon_class])
            .build();
        icon_container.append(&icon_img);

        let tags = if entry.item.tags.is_empty() {
            crate::ui::tagging::badges::TagBadgeCache::global().get_or_load(entry.path())
        } else {
            entry.item.tags.clone()
        };
        if !tags.is_empty() {
            let badges = crate::ui::tagging::badges::render_column_badge_cluster(&tags);
            badges.set_halign(gtk4::Align::Center);
            icon_container.append(&badges);
        }

        card_box.append(&icon_container);

        // 2. Filename Label (two-line wrapped/ellipsized)
        let label = gtk4::Label::builder()
            .label(entry.file_name())
            .halign(gtk4::Align::Center)
            .justify(gtk4::Justification::Center)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
            .max_width_chars(14)
            .lines(2)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .css_classes(["icon-card-label"])
            .build();
        card_box.append(&label);

        if let Some(coord) = coordinator {
            crate::ui::context_menu::attach_row_context_menu(
                &child,
                entry.path(),
                entry.is_dir(),
                coord,
            );
            if entry.is_dir() {
                crate::ui::context_menu::attach_directory_drop_target(&child, entry.path(), coord);
            }
        }

        child.set_child(Some(&card_box));
        child
    }

    fn resolve_icon_info(entry: &ColumnEntry) -> (&'static str, &'static str) {
        if entry.is_dir() {
            ("folder-symbolic", "fi")
        } else if entry.is_symlink() && entry.item.symlink_target.is_none() {
            ("emblem-unreadable", "fl")
        } else {
            let mime = entry.item.metadata.mime_type.as_str();
            if mime.starts_with("image/") {
                ("image-x-generic-symbolic", "fl")
            } else if mime.starts_with("video/") {
                ("video-x-generic-symbolic", "fl")
            } else if mime.starts_with("audio/") {
                ("audio-x-generic-symbolic", "fl")
            } else if mime == "application/pdf" {
                ("x-office-document-symbolic", "fl")
            } else if mime.contains("zip")
                || mime.contains("tar")
                || mime.contains("compressed")
                || mime.contains("archive")
            {
                ("package-x-generic-symbolic", "fl")
            } else {
                ("text-x-generic-symbolic", "fl")
            }
        }
    }

    fn render_empty_state(&self, icon_name: &str, message: &str) {
        let empty_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::Center)
            .spacing(12)
            .margin_top(48)
            .margin_bottom(48)
            .build();

        let icon = gtk4::Image::builder()
            .icon_name(icon_name)
            .pixel_size(48)
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
        self.flow_box.append(&empty_box);
    }

    fn wire_signals(&self) {
        let this = self.clone();
        self.flow_box.connect_child_activated(move |_fb, child| {
            let idx = child.index();
            if idx < 0 {
                return;
            }
            let Ok(idx_usize) = usize::try_from(idx) else {
                return;
            };

            let entry = {
                let entries = this.inner.entries.borrow();
                entries.get(idx_usize).cloned()
            };

            if let Some(entry) = entry {
                if entry.is_dir() {
                    if let Some(ref coord) = *this.inner.coordinator.borrow() {
                        coord.navigate_to(entry.path());
                    } else if let Some(ref cb) = *this.inner.on_path_changed.borrow() {
                        cb(entry.path());
                    }
                } else if let Some(ref cb) = *this.inner.on_path_changed.borrow() {
                    cb(entry.path());
                }
            }
        });

        let this = self.clone();
        self.flow_box
            .connect_selected_children_changed(move |flow_box| {
                let selected = flow_box.selected_children();
                let entries = this.inner.entries.borrow();

                let paths: Vec<PathBuf> = selected
                    .iter()
                    .filter_map(|c| {
                        let idx = c.index();
                        if idx >= 0
                            && let Ok(u) = usize::try_from(idx)
                        {
                            entries.get(u).map(|e| e.path().to_path_buf())
                        } else {
                            None
                        }
                    })
                    .collect();

                if let Some(ref coord) = *this.inner.coordinator.borrow() {
                    if paths.len() == 1 {
                        coord.set_selection(paths.first().map(PathBuf::as_path));
                    } else {
                        coord.set_selected_paths(paths.clone());
                    }
                }

                if let Some(first) = paths.first()
                    && let Some(ref cb) = *this.inner.on_path_changed.borrow()
                {
                    cb(first);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_icon_grid_defaults() {
        let dummy_path = PathBuf::from("/tmp");
        assert_eq!(DEFAULT_ICON_SIZE, 64);
        assert_eq!(MIN_ICON_SIZE, 32);
        assert_eq!(MAX_ICON_SIZE, 192);
        assert_eq!(dummy_path.to_str(), Some("/tmp"));
    }
}
