//! Finder 210px persistent sidebar widget with categorized sections and live volume monitoring.

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use shared::models::TagColor;

use crate::bookmarks::{BookmarkEntry, load_user_bookmarks, save_user_bookmarks};
use crate::sidebar::config::SidebarConfig;
use crate::sidebar::tags::{MACOS_TAG_PALETTE, TagFilterEvent, load_user_tags};
use crate::sidebar::volumes::{DiscoveredVolume, VolumeService};

/// Callback signature for path navigation events.
pub type NavigateCallback = Box<dyn Fn(PathBuf) + 'static>;

/// Callback signature for tag filtering events.
pub type TagFilterCallback = Box<dyn Fn(TagFilterEvent) + 'static>;

/// Primary Finder sidebar navigation panel widget (210px default width).
#[derive(Clone)]
pub struct SidebarWidget {
    /// Top-level scrolled window container.
    pub container: gtk4::ScrolledWindow,
    /// Vertical container box housing all sections.
    pub root_box: gtk4::Box,
    /// Items container for the Favorites section.
    pub favorites_box: gtk4::Box,
    /// Items container for the Locations section.
    pub locations_box: gtk4::Box,
    /// Items container for the Tags section.
    pub tags_box: gtk4::Box,
    /// Toggle button for Favorites collapse state.
    pub favorites_toggle_btn: gtk4::Button,
    /// Toggle button for Locations collapse state.
    pub locations_toggle_btn: gtk4::Button,
    /// Toggle button for Tags collapse state.
    pub tags_toggle_btn: gtk4::Button,
    config: Rc<RefCell<SidebarConfig>>,
    active_path: Rc<RefCell<Option<PathBuf>>>,
    active_tag: Rc<RefCell<Option<String>>>,
    navigate_callback: Rc<RefCell<Option<NavigateCallback>>>,
    tag_filter_callback: Rc<RefCell<Option<TagFilterCallback>>>,
    favorite_rows: Rc<RefCell<Vec<(PathBuf, gtk4::Box)>>>,
    location_rows: Rc<RefCell<Vec<(PathBuf, gtk4::Box)>>>,
    tag_rows: Rc<RefCell<Vec<(String, gtk4::Widget)>>>,
}

impl fmt::Debug for SidebarWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SidebarWidget")
            .field("container", &self.container)
            .finish_non_exhaustive()
    }
}

impl Default for SidebarWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl SidebarWidget {
    /// Constructs a new [`SidebarWidget`] loaded with persistent configuration and bookmarks.
    #[must_use]
    pub fn new() -> Self {
        let config = Rc::new(RefCell::new(SidebarConfig::load()));
        let initial_width = config.borrow().width;

        let container = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .width_request(initial_width)
            .css_classes(["sd", "finder-sidebar"])
            .hexpand(false)
            .vexpand(true)
            .build();

        let root_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(0)
            .hexpand(false)
            .vexpand(true)
            .build();
        container.set_child(Some(&root_box));

        let favorites_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(1)
            .build();
        let locations_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(1)
            .build();
        let tags_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(1)
            .build();

        let (fav_hdr, favorites_toggle_btn) = Self::build_section_header("Favorites");
        let (loc_hdr, locations_toggle_btn) = Self::build_section_header("Locations");
        let (tags_hdr, tags_toggle_btn) = Self::build_section_header("Tags");

        root_box.append(&fav_hdr);
        root_box.append(&favorites_box);
        root_box.append(&loc_hdr);
        root_box.append(&locations_box);
        root_box.append(&tags_hdr);
        root_box.append(&tags_box);

        let sidebar = Self {
            container,
            root_box,
            favorites_box,
            locations_box,
            tags_box,
            favorites_toggle_btn,
            locations_toggle_btn,
            tags_toggle_btn,
            config,
            active_path: Rc::new(RefCell::new(None)),
            active_tag: Rc::new(RefCell::new(None)),
            navigate_callback: Rc::new(RefCell::new(None)),
            tag_filter_callback: Rc::new(RefCell::new(None)),
            favorite_rows: Rc::new(RefCell::new(Vec::new())),
            location_rows: Rc::new(RefCell::new(Vec::new())),
            tag_rows: Rc::new(RefCell::new(Vec::new())),
        };

        sidebar.wire_collapse_toggles();
        sidebar.apply_initial_collapse_states();
        sidebar.refresh_favorites();
        sidebar.refresh_locations();
        sidebar.refresh_tags();
        sidebar.start_udisks2_listener();

        sidebar
    }

    /// Registers a callback handler for directory navigation events.
    pub fn connect_navigate<F: Fn(PathBuf) + 'static>(&self, callback: F) {
        *self.navigate_callback.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback handler for tag filter selection events.
    pub fn connect_tag_filter<F: Fn(TagFilterEvent) + 'static>(&self, callback: F) {
        *self.tag_filter_callback.borrow_mut() = Some(Box::new(callback));
    }

    /// Synchronizes active selection state across all sidebar items to match `path`.
    pub fn set_active_path(&self, path: &Path) {
        *self.active_path.borrow_mut() = Some(path.to_path_buf());
        *self.active_tag.borrow_mut() = None;

        let target_canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        for (item_path, row) in self.favorite_rows.borrow().iter() {
            let canon = item_path
                .canonicalize()
                .unwrap_or_else(|_| item_path.clone());
            if canon == target_canon {
                row.add_css_class("on");
            } else {
                row.remove_css_class("on");
            }
        }

        for (vol_path, row) in self.location_rows.borrow().iter() {
            let canon = vol_path.canonicalize().unwrap_or_else(|_| vol_path.clone());
            if canon == target_canon {
                row.add_css_class("on");
            } else {
                row.remove_css_class("on");
            }
        }

        for (_tag, widget) in self.tag_rows.borrow().iter() {
            widget.remove_css_class("on");
        }
    }

    /// Synchronizes active selection state for tag filtering.
    pub fn set_active_tag(&self, tag_name: Option<&str>) {
        *self.active_tag.borrow_mut() = tag_name.map(ToString::to_string);
        *self.active_path.borrow_mut() = None;

        // Clear path selections
        for (_, row) in self.favorite_rows.borrow().iter() {
            row.remove_css_class("on");
        }
        for (_, row) in self.location_rows.borrow().iter() {
            row.remove_css_class("on");
        }

        // Highlight matching tag
        for (name, widget) in self.tag_rows.borrow().iter() {
            if let Some(target) = tag_name
                && name.eq_ignore_ascii_case(target)
            {
                widget.add_css_class("on");
                continue;
            }
            widget.remove_css_class("on");
        }
    }

    /// Reloads the Favorites section from Freedesktop bookmarks.
    pub fn refresh_favorites(&self) {
        clear_box_children(&self.favorites_box);
        self.favorite_rows.borrow_mut().clear();

        let bookmarks = load_user_bookmarks();
        for (index, b) in bookmarks.iter().enumerate() {
            let row = self.create_favorite_row(index, b);
            self.favorites_box.append(&row);
            self.favorite_rows.borrow_mut().push((b.path.clone(), row));
        }

        if let Some(path) = self.active_path.borrow().clone() {
            self.set_active_path(&path);
        }
    }

    /// Reloads mounted volumes and block devices in the Locations section.
    pub fn refresh_locations(&self) {
        clear_box_children(&self.locations_box);
        self.location_rows.borrow_mut().clear();

        let vol_service = VolumeService::new();
        let sidebar_weak = self.clone();

        glib::MainContext::default().spawn_local(async move {
            let volumes = vol_service.discover_volumes().await;
            for vol in volumes {
                let row = sidebar_weak.create_volume_row(&vol);
                sidebar_weak.locations_box.append(&row);
                sidebar_weak
                    .location_rows
                    .borrow_mut()
                    .push((vol.mount_point.clone(), row));
            }

            if let Some(path) = sidebar_weak.active_path.borrow().clone() {
                sidebar_weak.set_active_path(&path);
            }
        });
    }

    /// Reloads the Tags section with 7 canonical macOS color dots and database tags.
    pub fn refresh_tags(&self) {
        clear_box_children(&self.tags_box);
        self.tag_rows.borrow_mut().clear();

        // 1. Horizontal color dots row matching macOS design
        let dots_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(7)
            .css_classes(["tags-dots-row"])
            .build();

        for &(color, name, _hex) in &MACOS_TAG_PALETTE {
            let color_class = name.to_ascii_lowercase();
            let dot_btn = gtk4::Button::builder()
                .css_classes(["tg", &color_class])
                .tooltip_text(name)
                .build();

            let sidebar_weak = self.clone();
            let tag_name = name.to_string();
            dot_btn.connect_clicked(move |_| {
                sidebar_weak.emit_tag_filter(TagFilterEvent::Selected {
                    tag_name: tag_name.clone(),
                    color: Some(color),
                });
            });

            dots_row.append(&dot_btn);
            self.tag_rows
                .borrow_mut()
                .push((name.to_string(), dot_btn.upcast()));
        }
        self.tags_box.append(&dots_row);

        // 2. Custom user tags from database
        let user_tags = load_user_tags();
        for tag in user_tags {
            // Skip canonical tags from redundant listing
            if MACOS_TAG_PALETTE
                .iter()
                .any(|&(_, name, _)| name.eq_ignore_ascii_case(&tag.label))
            {
                continue;
            }

            let row = self.create_tag_row(&tag.label, tag.color);
            self.tags_box.append(&row);
            self.tag_rows
                .borrow_mut()
                .push((tag.label.clone(), row.upcast()));
        }
    }

    /// Toggles the collapsed state of a named section and persists the change.
    pub fn toggle_section(&self, section: &str) {
        let (box_widget, btn) = match section.to_ascii_lowercase().as_str() {
            "favorites" => (&self.favorites_box, &self.favorites_toggle_btn),
            "locations" => (&self.locations_box, &self.locations_toggle_btn),
            "tags" => (&self.tags_box, &self.tags_toggle_btn),
            _ => return,
        };

        let currently_visible = box_widget.is_visible();
        let new_collapsed = currently_visible;

        box_widget.set_visible(!new_collapsed);
        btn.set_label(if new_collapsed { "Show" } else { "Hide" });

        self.config
            .borrow_mut()
            .set_section_collapsed(section, new_collapsed);
        let _ = self.config.borrow().save();
    }

    /// Checks if a section is currently collapsed.
    #[must_use]
    pub fn is_section_collapsed(&self, section: &str) -> bool {
        self.config.borrow().is_section_collapsed(section)
    }

    fn build_section_header(title: &'static str) -> (gtk4::Box, gtk4::Button) {
        let header_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .css_classes(["sh-box", "sh"])
            .build();

        let title_label = gtk4::Label::builder()
            .label(title)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .css_classes(["sh-title"])
            .build();

        let toggle_btn = gtk4::Button::builder()
            .label("Hide")
            .css_classes(["sh-toggle"])
            .halign(gtk4::Align::End)
            .build();

        header_box.append(&title_label);
        header_box.append(&toggle_btn);

        (header_box, toggle_btn)
    }

    fn wire_collapse_toggles(&self) {
        let sb = self.clone();
        self.favorites_toggle_btn.connect_clicked(move |_| {
            sb.toggle_section("Favorites");
        });

        let sb = self.clone();
        self.locations_toggle_btn.connect_clicked(move |_| {
            sb.toggle_section("Locations");
        });

        let sb = self.clone();
        self.tags_toggle_btn.connect_clicked(move |_| {
            sb.toggle_section("Tags");
        });
    }

    fn apply_initial_collapse_states(&self) {
        let cfg = self.config.borrow();

        if cfg.is_section_collapsed("Favorites") {
            self.favorites_box.set_visible(false);
            self.favorites_toggle_btn.set_label("Show");
        }
        if cfg.is_section_collapsed("Locations") {
            self.locations_box.set_visible(false);
            self.locations_toggle_btn.set_label("Show");
        }
        if cfg.is_section_collapsed("Tags") {
            self.tags_box.set_visible(false);
            self.tags_toggle_btn.set_label("Show");
        }
    }

    fn create_favorite_row(&self, index: usize, b: &BookmarkEntry) -> gtk4::Box {
        let row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .css_classes(["si"])
            .build();

        let glyph = gtk4::Image::builder()
            .icon_name(b.icon_name())
            .pixel_size(13)
            .css_classes(["gl"])
            .build();

        let label = gtk4::Label::builder()
            .label(b.display_name())
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .single_line_mode(true)
            .build();

        row.append(&glyph);
        row.append(&label);

        // Click gesture for navigation
        let gesture = gtk4::GestureClick::new();
        let sb = self.clone();
        let target_path = b.path.clone();
        gesture.connect_released(move |_gest, _n, _x, _y| {
            sb.emit_navigate(target_path.clone());
        });
        row.add_controller(gesture);

        // Drag Source (reordering)
        let drag_source = gtk4::DragSource::builder()
            .actions(gdk4::DragAction::MOVE | gdk4::DragAction::COPY)
            .build();

        let uri = b.uri.clone();
        drag_source.connect_prepare(move |_src, _x, _y| {
            let val = glib::Value::from(uri.as_str());
            Some(gdk4::ContentProvider::for_value(&val))
        });
        row.add_controller(drag_source);

        // Drop Target (reordering & pinning)
        let drop_target = gtk4::DropTarget::new(
            glib::Type::STRING,
            gdk4::DragAction::MOVE | gdk4::DragAction::COPY,
        );

        let row_weak = row.downgrade();
        drop_target.connect_enter(move |_dt, _x, _y| {
            if let Some(r) = row_weak.upgrade() {
                r.add_css_class("drop-target");
            }
            gdk4::DragAction::MOVE
        });

        let row_weak = row.downgrade();
        drop_target.connect_leave(move |_dt| {
            if let Some(r) = row_weak.upgrade() {
                r.remove_css_class("drop-target");
            }
        });

        let sb = self.clone();
        let row_weak = row.downgrade();
        drop_target.connect_drop(move |_dt, value, _x, _y| {
            if let Some(r) = row_weak.upgrade() {
                r.remove_css_class("drop-target");
            }
            if let Ok(dropped_str) = value.get::<String>() {
                sb.handle_favorite_drop(&dropped_str, index);
                return true;
            }
            false
        });
        row.add_controller(drop_target);

        row
    }

    fn handle_favorite_drop(&self, dropped_payload: &str, target_index: usize) {
        let mut bookmarks = load_user_bookmarks();

        // 1. Check if dropped item is an existing bookmark
        if let Some(existing_idx) = bookmarks.iter().position(|b| b.uri == dropped_payload) {
            let item = bookmarks.remove(existing_idx);
            let insert_at = target_index.min(bookmarks.len());
            bookmarks.insert(insert_at, item);
        } else {
            // 2. Folder dropped from main view or external URI
            let target_path = if dropped_payload.starts_with("file://") {
                shared::ipc::file_uri_to_path(dropped_payload)
                    .unwrap_or_else(|_| PathBuf::from(dropped_payload))
            } else {
                PathBuf::from(dropped_payload)
            };

            let label = target_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Folder")
                .to_string();

            let new_entry = BookmarkEntry::from_path(target_path, Some(label));
            let insert_at = target_index.min(bookmarks.len());
            bookmarks.insert(insert_at, new_entry);
        }

        let _ = save_user_bookmarks(&bookmarks);
        self.refresh_favorites();
    }

    fn create_volume_row(&self, vol: &DiscoveredVolume) -> gtk4::Box {
        let row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .css_classes(["si"])
            .build();

        let glyph = gtk4::Image::builder()
            .icon_name(&vol.icon_name)
            .pixel_size(13)
            .css_classes(["gl"])
            .build();

        let label = gtk4::Label::builder()
            .label(&vol.label)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .single_line_mode(true)
            .build();

        row.append(&glyph);
        row.append(&label);

        let spinner = gtk4::Spinner::builder()
            .spinning(false)
            .visible(false)
            .build();
        row.append(&spinner);

        if vol.is_ejectable {
            let eject_btn = gtk4::Button::builder()
                .label("⏏")
                .css_classes(["si-eject-btn"])
                .tooltip_text("Eject or Unmount volume")
                .build();

            let dev_path = vol.device_path.clone();
            let mount_point = vol.mount_point.clone();
            let sidebar_weak = self.clone();
            let spinner_clone = spinner;
            let eject_btn_clone = eject_btn.clone();

            eject_btn.connect_clicked(move |_| {
                eject_btn_clone.set_visible(false);
                spinner_clone.set_visible(true);
                spinner_clone.start();

                let dev = dev_path.clone();
                let mp = mount_point.clone();
                let sb = sidebar_weak.clone();
                let spin = spinner_clone.clone();
                let btn = eject_btn_clone.clone();

                glib::MainContext::default().spawn_local(async move {
                    let res = VolumeService::new().unmount_volume(&dev, Some(&mp)).await;
                    spin.stop();
                    spin.set_visible(false);
                    btn.set_visible(true);

                    match res {
                        Ok(()) => {
                            sb.refresh_locations();
                        }
                        Err(err) => {
                            tracing::warn!("Failed to unmount volume {dev}: {err}");
                        }
                    }
                });
            });

            row.append(&eject_btn);
        }

        let gesture = gtk4::GestureClick::new();
        let sb = self.clone();
        let mount_point = vol.mount_point.clone();
        gesture.connect_released(move |_gest, _n, _x, _y| {
            sb.emit_navigate(mount_point.clone());
        });
        row.add_controller(gesture);

        row
    }

    fn create_tag_row(&self, label: &str, color: Option<shared::models::TagColor>) -> gtk4::Box {
        let row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .css_classes(["si"])
            .build();

        let color_class = match color {
            Some(TagColor::Red) => "red",
            Some(TagColor::Orange) => "orange",
            Some(TagColor::Yellow) => "yellow",
            Some(TagColor::Green) => "green",
            Some(TagColor::Blue) => "blue",
            Some(TagColor::Purple) => "purple",
            Some(TagColor::Gray) | None => "default",
        };
        let dot = gtk4::Box::builder()
            .css_classes(["tg", color_class])
            .build();

        let lbl = gtk4::Label::builder()
            .label(label)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .single_line_mode(true)
            .build();

        row.append(&dot);
        row.append(&lbl);

        let gesture = gtk4::GestureClick::new();
        let sb = self.clone();
        let tag_name = label.to_string();
        gesture.connect_released(move |_gest, _n, _x, _y| {
            sb.emit_tag_filter(TagFilterEvent::Selected {
                tag_name: tag_name.clone(),
                color,
            });
        });
        row.add_controller(gesture);

        row
    }

    fn emit_navigate(&self, path: PathBuf) {
        self.set_active_path(&path);
        if let Some(cb) = self.navigate_callback.borrow().as_ref() {
            cb(path);
        }
    }

    fn emit_tag_filter(&self, event: TagFilterEvent) {
        if let TagFilterEvent::Selected { ref tag_name, .. } = event {
            self.set_active_tag(Some(tag_name));
        } else {
            self.set_active_tag(None);
        }
        if let Some(cb) = self.tag_filter_callback.borrow().as_ref() {
            cb(event);
        }
    }

    fn start_udisks2_listener(&self) {
        let sidebar_weak = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let monitor = shared::ipc::UDisks2Monitor::new();
            if let Ok(mut rx) = monitor.listen_device_changes().await {
                while rx.recv().await.is_some() {
                    sidebar_weak.refresh_locations();
                }
            }
        });
    }
}

fn clear_box_children(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
