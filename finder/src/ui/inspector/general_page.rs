//! General and Name & Extension accordion groups for the Get Info dialog.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use gtk4::prelude::*;
use libadwaita::prelude::*;

use crate::metadata::aggregate::MultiSelectionSummary;
use crate::metadata::types::DeepMetadata;
use crate::metadata::xattr::ExtendedAttributesService;
use crate::ui::columns::preview_column::{format_byte_size, format_exact_bytes};

/// Formats dual size metric string (e.g. `"14.2 MB on disk (14,188,544 bytes)"`).
#[must_use]
pub fn format_dual_size(logical_bytes: u64, physical_bytes: u64) -> String {
    let disk_bytes = if physical_bytes > 0 {
        physical_bytes
    } else {
        logical_bytes
    };
    let human_disk = format_byte_size(disk_bytes);
    let exact_bytes = format_exact_bytes(logical_bytes);
    format!("{human_disk} on disk ({exact_bytes})")
}

/// Formats creation or modification timestamp into user-friendly inspector string.
#[must_use]
pub fn format_inspector_time(time: Option<SystemTime>) -> String {
    let Some(t) = time else {
        return "—".to_string();
    };
    let dt: DateTime<Local> = DateTime::from(t);
    dt.format("%B %-d, %Y at %H:%M").to_string()
}

/// Callback invoked when a target item is renamed.
pub type RenameCallback = Box<dyn Fn(PathBuf) + 'static>;

/// Sets or removes write permissions to lock or unlock a file.
///
/// # Errors
///
/// Returns [`std::io::Error`] if reading or writing permissions fails.
pub fn set_file_locked(path: &Path, locked: bool) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    let mut perms = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = perms.mode();
        if locked {
            perms.set_mode(mode & !0o222);
        } else {
            perms.set_mode(mode | 0o200);
        }
    }
    #[cfg(not(unix))]
    {
        perms.set_readonly(locked);
    }
    std::fs::set_permissions(path, perms)
}

/// Renames a target file or folder atomically.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the filename is empty, contains slashes, or rename fails.
pub fn rename_target(path: &Path, new_name: &str) -> std::io::Result<PathBuf> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() || trimmed.contains('/') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid filename: cannot be empty or contain slashes",
        ));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("/"));
    let new_path = parent.join(trimmed);
    if new_path == path {
        return Ok(new_path);
    }
    std::fs::rename(path, &new_path)?;
    Ok(new_path)
}

/// "General" property group in the Get Info dialog.
#[derive(Clone)]
pub struct GeneralInspectorGroup {
    /// Root libadwaita preferences group.
    pub group: libadwaita::PreferencesGroup,
    kind_row: libadwaita::ActionRow,
    size_row: libadwaita::ActionRow,
    where_row: libadwaita::ActionRow,
    created_row: libadwaita::ActionRow,
    modified_row: libadwaita::ActionRow,
    locked_row: libadwaita::ActionRow,
    locked_check: gtk4::CheckButton,
    current_paths: Rc<RefCell<Vec<PathBuf>>>,
    updating_lock: Rc<RefCell<bool>>,
}

impl GeneralInspectorGroup {
    /// Constructs a new [`GeneralInspectorGroup`].
    #[must_use]
    pub fn new() -> Self {
        let group = libadwaita::PreferencesGroup::builder()
            .title("General")
            .build();

        let kind_row = libadwaita::ActionRow::builder().title("Kind").build();

        let size_row = libadwaita::ActionRow::builder().title("Size").build();

        let where_row = libadwaita::ActionRow::builder().title("Where").build();
        let copy_button = gtk4::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy path to clipboard")
            .valign(gtk4::Align::Center)
            .css_classes(["flat"])
            .build();
        where_row.add_suffix(&copy_button);

        let created_row = libadwaita::ActionRow::builder().title("Created").build();

        let modified_row = libadwaita::ActionRow::builder().title("Modified").build();

        let locked_row = libadwaita::ActionRow::builder().title("Locked").build();
        let locked_check = gtk4::CheckButton::builder()
            .valign(gtk4::Align::Center)
            .build();
        locked_row.add_suffix(&locked_check);
        locked_row.set_activatable_widget(Some(&locked_check));

        group.add(&kind_row);
        group.add(&size_row);
        group.add(&where_row);
        group.add(&created_row);
        group.add(&modified_row);
        group.add(&locked_row);

        let current_paths: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let updating_lock = Rc::new(RefCell::new(false));

        // Wire copy button
        let paths_clone = Rc::clone(&current_paths);
        copy_button.connect_clicked(move |btn| {
            let paths = paths_clone.borrow();
            if let Some(first) = paths.first()
                && let Some(parent) = first.parent()
            {
                let path_str = parent.to_string_lossy();
                btn.clipboard().set_text(&path_str);
            }
        });

        // Wire locked toggle
        let paths_lock = Rc::clone(&current_paths);
        let updating_clone = Rc::clone(&updating_lock);
        locked_check.connect_toggled(move |chk| {
            if *updating_clone.borrow() {
                return;
            }
            let is_locked = chk.is_active();
            let paths = paths_lock.borrow();
            for path in paths.iter() {
                let _ = set_file_locked(path, is_locked);
            }
        });

        Self {
            group,
            kind_row,
            size_row,
            where_row,
            created_row,
            modified_row,
            locked_row,
            locked_check,
            current_paths,
            updating_lock,
        }
    }

    /// Binds single file metadata to the group.
    pub fn bind_single(&self, meta: &DeepMetadata) {
        *self.current_paths.borrow_mut() = vec![meta.path.clone()];

        self.kind_row.set_subtitle(&meta.kind_display);
        self.size_row
            .set_subtitle(&format_dual_size(meta.size_logical, meta.size_physical));

        let parent_str = meta
            .path
            .parent()
            .map_or_else(|| "/".to_string(), |p| p.to_string_lossy().to_string());
        self.where_row.set_subtitle(&parent_str);

        self.created_row
            .set_subtitle(&format_inspector_time(meta.created_time));
        self.modified_row
            .set_subtitle(&format_inspector_time(Some(meta.modified_time)));

        *self.updating_lock.borrow_mut() = true;
        self.locked_check.set_active(meta.permissions.is_locked);
        *self.updating_lock.borrow_mut() = false;

        self.locked_row.set_visible(true);
        self.created_row.set_visible(true);
        self.modified_row.set_visible(true);
    }

    /// Binds aggregated multi-item summary to the group.
    pub fn bind_multi(&self, summary: &MultiSelectionSummary, paths: &[PathBuf]) {
        *self.current_paths.borrow_mut() = paths.to_vec();

        let count_str = format!("{} items", summary.total_items);
        self.kind_row.set_subtitle(&count_str);

        let size_str = format!(
            "Total: {} across {} items ({})",
            format_byte_size(summary.total_logical_bytes),
            summary.total_items,
            format_exact_bytes(summary.total_physical_bytes)
        );
        self.size_row.set_subtitle(&size_str);

        if let Some(first) = paths.first()
            && let Some(parent) = first.parent()
        {
            self.where_row
                .set_subtitle(parent.to_string_lossy().as_ref());
        } else {
            self.where_row.set_subtitle("—");
        }

        self.created_row.set_visible(false);
        self.modified_row.set_visible(false);

        *self.updating_lock.borrow_mut() = true;
        self.locked_check.set_active(summary.has_locked_items);
        *self.updating_lock.borrow_mut() = false;
        self.locked_row.set_visible(true);
    }
}

impl Default for GeneralInspectorGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// "Name & Extension" property group in the Get Info dialog.
#[derive(Clone)]
pub struct NameExtensionGroup {
    /// Root libadwaita preferences group.
    pub group: libadwaita::PreferencesGroup,
    /// Row for filename editing.
    pub name_entry: libadwaita::EntryRow,
    /// Row for extension hiding toggle.
    pub hide_extension_row: libadwaita::ActionRow,
    hide_extension_check: gtk4::CheckButton,
    current_path: Rc<RefCell<Option<PathBuf>>>,
    updating_ext: Rc<RefCell<bool>>,
    on_renamed: Rc<RefCell<Option<RenameCallback>>>,
}

impl NameExtensionGroup {
    /// Constructs a new [`NameExtensionGroup`].
    #[must_use]
    pub fn new() -> Self {
        let group = libadwaita::PreferencesGroup::builder()
            .title("Name &amp; Extension")
            .build();

        let name_entry = libadwaita::EntryRow::builder().title("Name").build();

        let hide_extension_row = libadwaita::ActionRow::builder()
            .title("Hide extension")
            .build();
        let hide_extension_check = gtk4::CheckButton::builder()
            .valign(gtk4::Align::Center)
            .build();
        hide_extension_row.add_suffix(&hide_extension_check);
        hide_extension_row.set_activatable_widget(Some(&hide_extension_check));

        group.add(&name_entry);
        group.add(&hide_extension_row);

        let current_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let updating_ext = Rc::new(RefCell::new(false));
        let on_renamed: Rc<RefCell<Option<RenameCallback>>> = Rc::new(RefCell::new(None));

        // Wire entry rename on activated (Enter key)
        let path_clone = Rc::clone(&current_path);
        let on_renamed_clone = Rc::clone(&on_renamed);
        name_entry.connect_entry_activated(move |entry| {
            let new_name = entry.text().to_string();
            let opt_path = path_clone.borrow().clone();
            if let Some(old_path) = opt_path
                && let Ok(new_path) = rename_target(&old_path, &new_name)
            {
                *path_clone.borrow_mut() = Some(new_path.clone());
                if let Some(ref cb) = *on_renamed_clone.borrow() {
                    cb(new_path);
                }
            }
        });

        // Wire hide extension toggle
        let path_ext = Rc::clone(&current_path);
        let updating_clone = Rc::clone(&updating_ext);
        hide_extension_check.connect_toggled(move |chk| {
            if *updating_clone.borrow() {
                return;
            }
            let is_hidden = chk.is_active();
            if let Some(ref path) = *path_ext.borrow() {
                let _ = ExtendedAttributesService::set_hide_extension(path, is_hidden);
            }
        });

        Self {
            group,
            name_entry,
            hide_extension_row,
            hide_extension_check,
            current_path,
            updating_ext,
            on_renamed,
        }
    }

    /// Sets callback invoked when target item is renamed.
    pub fn connect_renamed<F: Fn(PathBuf) + 'static>(&self, callback: F) {
        *self.on_renamed.borrow_mut() = Some(Box::new(callback));
    }

    /// Binds single file metadata to the name/extension group.
    pub fn bind_single(&self, meta: &DeepMetadata) {
        *self.current_path.borrow_mut() = Some(meta.path.clone());
        self.group.set_visible(true);

        self.name_entry.set_text(&meta.file_name);

        *self.updating_ext.borrow_mut() = true;
        let is_hidden = ExtendedAttributesService::get_hide_extension(&meta.path).unwrap_or(false);
        self.hide_extension_check.set_active(is_hidden);
        *self.updating_ext.borrow_mut() = false;
    }

    /// Hides name & extension group for multi-selection.
    pub fn bind_multi(&self) {
        *self.current_path.borrow_mut() = None;
        self.group.set_visible(false);
    }
}

impl Default for NameExtensionGroup {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_format_dual_size() {
        let text = format_dual_size(14_188_544, 14_888_960);
        assert!(text.contains("on disk"));
        assert!(text.contains("14,188,544 bytes"));
    }

    #[test]
    fn test_rename_and_lock_lifecycle() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        // Lock file
        assert!(set_file_locked(&path, true).is_ok());
        let meta = std::fs::symlink_metadata(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(meta.permissions().mode() & 0o222, 0);
        }

        // Unlock file
        assert!(set_file_locked(&path, false).is_ok());
        let meta2 = std::fs::symlink_metadata(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(meta2.permissions().mode() & 0o200, 0);
        }
    }
}
