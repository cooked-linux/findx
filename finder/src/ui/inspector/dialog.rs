//! Get Info property inspector window implementation using libadwaita.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::prelude::*;

use crate::coordinator::NavigationCoordinator;
use crate::metadata::aggregate::aggregate_metadata;
use crate::metadata::extract::extract_deep_metadata;
use crate::ui::columns::preview_column::format_byte_size;

use super::general_page::{GeneralInspectorGroup, NameExtensionGroup};
use super::icon_custom::CustomIconBadge;
use super::more_info_page::{CommentsGroup, MoreInfoGroup, OriginGroup};
use super::permissions_page::{OpenWithGroup, SharingPermissionsGroup};
use super::theme::install_inspector_theme;

/// Execution and presentation mode for the Get Info property inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorMode {
    /// Opens bound to a specific snapshot of paths at invocation time.
    Modal,
    /// Floating persistent inspector dynamically re-binding as selection changes.
    Persistent,
}

/// Primary Get Info property inspector window shell wrapping `adw::PreferencesWindow`.
#[derive(Clone)]
pub struct GetInfoDialog {
    /// Top-level libadwaita preferences window.
    pub window: libadwaita::PreferencesWindow,
    /// Operating mode (Modal snapshot or Persistent live floating).
    pub mode: InspectorMode,
    current_paths: Rc<RefCell<Vec<PathBuf>>>,
    icon_badge: CustomIconBadge,
    title_label: gtk4::Label,
    subtitle_label: gtk4::Label,
    general_group: GeneralInspectorGroup,
    more_info_group: MoreInfoGroup,
    name_group: NameExtensionGroup,
    comments_group: CommentsGroup,
    origin_group: OriginGroup,
    open_with_group: OpenWithGroup,
    permissions_group: SharingPermissionsGroup,
}

impl GetInfoDialog {
    /// Constructs and presents a new [`GetInfoDialog`] bound to snapshot `paths`.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn open_for_paths(parent: Option<&impl IsA<gtk4::Window>>, paths: Vec<PathBuf>) -> Self {
        let dialog = Self::new(InspectorMode::Modal, parent);
        dialog.rebind_target(&paths);
        dialog.window.present();
        dialog
    }

    /// Constructs and presents a persistent floating [`GetInfoDialog`].
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn open_persistent(
        parent: Option<&impl IsA<gtk4::Window>>,
        coordinator: Option<Rc<NavigationCoordinator>>,
    ) -> Self {
        let dialog = Self::new(InspectorMode::Persistent, parent);

        if let Some(coord) = coordinator {
            if let Some(sel) = coord.selection() {
                dialog.rebind_target(&[sel]);
            }

            let dialog_clone = dialog.clone();
            coord.connect_selection_changed(move |opt_path| {
                if let Some(path) = opt_path {
                    dialog_clone.update_selection(&[path.to_path_buf()]);
                }
            });
        }

        dialog.window.present();
        dialog
    }

    /// Constructs a new [`GetInfoDialog`] in the requested mode.
    #[must_use]
    pub fn new(mode: InspectorMode, parent: Option<&impl IsA<gtk4::Window>>) -> Self {
        install_inspector_theme();

        let window = libadwaita::PreferencesWindow::builder()
            .title("Info")
            .default_width(420)
            .default_height(640)
            .css_classes(["inspector-window"])
            .build();

        if let Some(p) = parent {
            window.set_transient_for(Some(p));
        }

        window.set_modal(mode == InspectorMode::Modal);

        let page = libadwaita::PreferencesPage::new();

        // 1. Header Card (Thumbnail badge + title + subtitle)
        let header_group = libadwaita::PreferencesGroup::new();
        let header_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(16)
            .css_classes(["inspector-header-card"])
            .halign(gtk4::Align::Fill)
            .build();

        let icon_badge = CustomIconBadge::new();

        let text_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .valign(gtk4::Align::Center)
            .hexpand(true)
            .build();

        let title_label = gtk4::Label::builder()
            .label("Info")
            .css_classes(["inspector-title"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .wrap_mode(gtk4::pango::WrapMode::WordChar)
            .build();

        let subtitle_label = gtk4::Label::builder()
            .label("")
            .css_classes(["inspector-subtitle"])
            .halign(gtk4::Align::Start)
            .wrap(true)
            .build();

        text_box.append(&title_label);
        text_box.append(&subtitle_label);

        header_box.append(&icon_badge.button);
        header_box.append(&text_box);
        header_group.add(&header_box);
        page.add(&header_group);

        // 2. Accordion Groups
        let general_group = GeneralInspectorGroup::new();
        let more_info_group = MoreInfoGroup::new();
        let name_group = NameExtensionGroup::new();
        let comments_group = CommentsGroup::new();
        let origin_group = OriginGroup::new();
        let open_with_group = OpenWithGroup::new();
        let permissions_group = SharingPermissionsGroup::new();

        page.add(&general_group.group);
        page.add(&more_info_group.group);
        page.add(&name_group.group);
        page.add(&comments_group.group);
        page.add(&origin_group.group);
        page.add(&open_with_group.group);
        page.add(&permissions_group.group);

        window.add(&page);

        let current_paths = Rc::new(RefCell::new(Vec::new()));

        let dialog = Self {
            window,
            mode,
            current_paths,
            icon_badge,
            title_label,
            subtitle_label,
            general_group,
            more_info_group,
            name_group,
            comments_group,
            origin_group,
            open_with_group,
            permissions_group,
        };

        // Wire rename notification from NameExtensionGroup
        let dialog_clone = dialog.clone();
        dialog.name_group.connect_renamed(move |new_path| {
            dialog_clone.rebind_target(&[new_path]);
        });

        dialog
    }

    /// Dynamically updates the active target selection.
    pub fn update_selection(&self, paths: &[PathBuf]) {
        self.rebind_target(paths);
    }

    /// Rebinds all cards and accordions to the specified filesystem paths.
    pub fn rebind_target(&self, paths: &[PathBuf]) {
        *self.current_paths.borrow_mut() = paths.to_vec();

        if paths.is_empty() {
            self.window.set_title(Some("Info"));
            self.title_label.set_label("No Selection");
            self.subtitle_label.set_label("—");
            return;
        }

        if paths.len() == 1 {
            let target = &paths[0];
            if let Ok(meta) = extract_deep_metadata(target) {
                let win_title = format!("{} Info", meta.file_name);
                self.window.set_title(Some(&win_title));
                self.title_label.set_label(&meta.file_name);
                self.subtitle_label.set_label(&format!(
                    "{} · {}",
                    meta.kind_display,
                    format_byte_size(meta.size_logical)
                ));

                self.icon_badge.set_single(&meta.path, meta.is_directory);
                self.general_group.bind_single(&meta);
                self.more_info_group.bind_single(&meta);
                self.name_group.bind_single(&meta);
                self.comments_group.bind_single(&meta);
                self.origin_group.bind_single(&meta);
                self.open_with_group.bind_single(&meta);
                self.permissions_group.bind_single(&meta);
            } else {
                let fallback_name = target
                    .file_name()
                    .map_or_else(|| "Item".to_string(), |n| n.to_string_lossy().to_string());
                self.window
                    .set_title(Some(&format!("{fallback_name} Info")));
                self.title_label.set_label(&fallback_name);
                self.subtitle_label.set_label("Unreadable item");
            }
        } else {
            // Multi-item selection mode
            let count = paths.len();
            let summary = aggregate_metadata(paths).unwrap_or_else(|_| {
                crate::metadata::aggregate::MultiSelectionSummary {
                    total_items: count,
                    total_directories: 0,
                    total_files: count,
                    total_logical_bytes: 0,
                    total_physical_bytes: 0,
                    shared_tags: Vec::new(),
                    partial_tags: Vec::new(),
                    uniform_permissions: None,
                    has_locked_items: false,
                }
            });

            let win_title = format!("{count} Items Info");
            self.window.set_title(Some(&win_title));
            self.title_label.set_label(&format!("{count} Items"));
            self.subtitle_label.set_label(&format!(
                "Total: {} across {count} items",
                format_byte_size(summary.total_logical_bytes)
            ));

            self.icon_badge.set_multi(count);
            self.general_group.bind_multi(&summary, paths);
            self.more_info_group.bind_multi();
            self.name_group.bind_multi();
            self.comments_group.bind_multi(paths);
            self.origin_group.bind_multi();
            self.open_with_group.bind_multi(paths);
            self.permissions_group.bind_multi(&summary, paths);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspector_mode_equality() {
        assert_eq!(InspectorMode::Modal, InspectorMode::Modal);
        assert_eq!(InspectorMode::Persistent, InspectorMode::Persistent);
        assert_ne!(InspectorMode::Modal, InspectorMode::Persistent);
    }
}
