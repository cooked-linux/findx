//! Custom icon replacement, clipboard paste, deletion, and badge styling.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gdk4::prelude::*;
use gtk4::prelude::*;

use crate::ui::operations::guess_icon_for_path;

/// Canonical folder custom icon filename.
pub const FOLDER_CUSTOM_ICON_NAME: &str = ".icon.png";
/// Canonical KDE / Freedesktop folder metadata file.
pub const FOLDER_DIRECTORY_ENTRY: &str = ".directory";

/// Writes a custom icon file for a target directory and registers it in `.directory`.
///
/// # Errors
///
/// Returns an error string if `dir_path` is not a directory or saving the PNG fails.
pub fn write_custom_folder_icon(
    dir_path: &Path,
    texture: &gdk4::Texture,
) -> Result<PathBuf, String> {
    if !dir_path.is_dir() {
        return Err("Target is not a directory".to_string());
    }

    let icon_path = dir_path.join(FOLDER_CUSTOM_ICON_NAME);
    texture
        .save_to_png(&icon_path)
        .map_err(|e| format!("Failed to save texture to PNG: {e}"))?;

    let dir_entry_path = dir_path.join(FOLDER_DIRECTORY_ENTRY);
    let entry_content = format!("[Desktop Entry]\nIcon=./{FOLDER_CUSTOM_ICON_NAME}\n");
    let _ = std::fs::write(&dir_entry_path, entry_content);

    Ok(icon_path)
}

/// Removes custom folder icon and configuration.
#[must_use]
pub fn remove_custom_folder_icon(dir_path: &Path) -> bool {
    let mut removed = false;
    let icon_path = dir_path.join(FOLDER_CUSTOM_ICON_NAME);
    if icon_path.exists() && std::fs::remove_file(&icon_path).is_ok() {
        removed = true;
    }
    let dir_entry_path = dir_path.join(FOLDER_DIRECTORY_ENTRY);
    if dir_entry_path.exists() && std::fs::remove_file(&dir_entry_path).is_ok() {
        removed = true;
    }
    removed
}

/// Resolves whether a directory has a custom icon, returning its path if present.
#[must_use]
pub fn resolve_custom_folder_icon(dir_path: &Path) -> Option<PathBuf> {
    if !dir_path.is_dir() {
        return None;
    }
    let icon_path = dir_path.join(FOLDER_CUSTOM_ICON_NAME);
    if icon_path.is_file() {
        Some(icon_path)
    } else {
        None
    }
}

/// Custom icon badge controller wrapping the header icon button and image.
#[derive(Clone)]
pub struct CustomIconBadge {
    /// Focusable button container with `.inspector-icon-button`.
    pub button: gtk4::Button,
    /// Image displayed inside button.
    pub image: gtk4::Image,
    current_path: Rc<RefCell<Option<PathBuf>>>,
    default_icon: Rc<RefCell<String>>,
    is_multi: Rc<RefCell<bool>>,
}

impl CustomIconBadge {
    /// Constructs a new [`CustomIconBadge`].
    #[must_use]
    pub fn new() -> Self {
        let image = gtk4::Image::builder()
            .pixel_size(64)
            .icon_name("text-x-generic")
            .build();

        let button = gtk4::Button::builder()
            .css_classes(["inspector-icon-button"])
            .focusable(true)
            .can_focus(true)
            .width_request(64)
            .height_request(64)
            .child(&image)
            .build();

        let current_path = Rc::new(RefCell::new(None));
        let default_icon = Rc::new(RefCell::new("text-x-generic".to_string()));
        let is_multi = Rc::new(RefCell::new(false));

        let badge = Self {
            button,
            image,
            current_path,
            default_icon,
            is_multi,
        };

        badge.wire_keyboard();
        badge
    }

    fn wire_keyboard(&self) {
        let key_controller = gtk4::EventControllerKey::new();
        let path_clone = Rc::clone(&self.current_path);
        let image_clone = self.image.clone();
        let default_clone = Rc::clone(&self.default_icon);
        let is_multi_clone = Rc::clone(&self.is_multi);

        key_controller.connect_key_pressed(move |ctrl, keyval, _code, state| {
            if *is_multi_clone.borrow() {
                return glib::Propagation::Proceed;
            }

            let is_ctrl = state.contains(gdk4::ModifierType::CONTROL_MASK);

            // Ctrl+V: Paste image from clipboard as custom icon
            if is_ctrl
                && (keyval == gdk4::Key::v || keyval == gdk4::Key::V)
                && let Some(target_path) = path_clone.borrow().clone()
                && let Some(widget) = ctrl.widget()
            {
                let clipboard = widget.clipboard();
                let img = image_clone.clone();

                clipboard.read_texture_async(None as Option<&gio::Cancellable>, move |res| {
                    if let Ok(Some(texture)) = res {
                        if target_path.is_dir() {
                            let _ = write_custom_folder_icon(&target_path, &texture);
                        }
                        img.set_paintable(Some(&texture));
                    }
                });
                return glib::Propagation::Stop;
            }

            // Delete / BackSpace: Reset custom icon
            if !is_ctrl
                && (keyval == gdk4::Key::Delete
                    || keyval == gdk4::Key::KP_Delete
                    || keyval == gdk4::Key::BackSpace)
            {
                if let Some(target_path) = path_clone.borrow().clone() {
                    if target_path.is_dir() {
                        let _ = remove_custom_folder_icon(&target_path);
                    }
                    image_clone.set_icon_name(Some(&*default_clone.borrow()));
                }
                return glib::Propagation::Stop;
            }

            glib::Propagation::Proceed
        });

        self.button.add_controller(key_controller);
    }

    /// Sets badge display for a single path.
    pub fn set_single(&self, path: &Path, is_dir: bool) {
        *self.is_multi.borrow_mut() = false;
        *self.current_path.borrow_mut() = Some(path.to_path_buf());

        if let Some(custom_icon_path) = resolve_custom_folder_icon(path) {
            let file = gio::File::for_path(&custom_icon_path);
            let icon = gio::FileIcon::new(&file);
            self.image.set_from_gicon(&icon);
            *self.default_icon.borrow_mut() = "folder".to_string();
            return;
        }

        let icon_name = guess_icon_for_path(path, is_dir);
        *self.default_icon.borrow_mut() = icon_name.to_string();
        self.image.set_icon_name(Some(icon_name));
    }

    /// Sets badge display for multi-item selection.
    pub fn set_multi(&self, _count: usize) {
        *self.is_multi.borrow_mut() = true;
        *self.current_path.borrow_mut() = None;
        *self.default_icon.borrow_mut() = "document-multiple-symbolic".to_string();
        self.image.set_icon_name(Some("document-multiple-symbolic"));
    }
}

impl Default for CustomIconBadge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_inspector_icon_folder_lifecycle() {
        let dir = tempdir().unwrap();
        let folder_path = dir.path().to_path_buf();

        assert!(resolve_custom_folder_icon(&folder_path).is_none());

        // Simulate custom icon write
        let icon_path = folder_path.join(FOLDER_CUSTOM_ICON_NAME);
        std::fs::write(&icon_path, b"fake png bytes").unwrap();
        let dir_entry = folder_path.join(FOLDER_DIRECTORY_ENTRY);
        std::fs::write(&dir_entry, b"[Desktop Entry]\nIcon=./.icon.png\n").unwrap();

        assert_eq!(
            resolve_custom_folder_icon(&folder_path),
            Some(icon_path.clone())
        );

        // Remove custom icon
        assert!(remove_custom_folder_icon(&folder_path));
        assert!(!icon_path.exists());
        assert!(!dir_entry.exists());
        assert!(resolve_custom_folder_icon(&folder_path).is_none());
    }

    #[test]
    fn test_inspector_icon_multi_selection_aggregation() {
        let dir = tempdir().unwrap();
        let path1 = dir.path().join("a.txt");
        let path2 = dir.path().join("b.txt");
        std::fs::write(&path1, b"hello").unwrap();
        std::fs::write(&path2, b"world!").unwrap();

        let summary = crate::metadata::aggregate::aggregate_metadata(&[path1, path2]).unwrap();
        assert_eq!(summary.total_items, 2);
        assert_eq!(summary.total_files, 2);
        assert_eq!(summary.total_directories, 0);
        assert_eq!(summary.total_logical_bytes, 11);
    }
}
