//! File and folder search result row component.
//!
//! Replicates the 44px horizontal result row from the Spotlight design specification,
//! featuring a 28px gradient-backed icon chip categorized by MIME/extension (PDF, Office,
//! Document, Folder), an ellipsized item filename label (`.n`), a category tag (`.t`),
//! keyboard activation (`Enter`), file manager reveal (`Ctrl+Enter`), and path clipboard copying.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;

use shared::models::{SearchCategory, SearchRecord};

use crate::providers::file_provider::{
    copy_file_path_to_clipboard, open_file, show_in_file_manager,
};

/// Visual category variant for the 28px icon chip gradient and symbolic icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileChipKind {
    /// Portable Document Format (.pdf).
    Pdf,
    /// Office documents, presentations, and spreadsheets (.docx, .xlsx, .pptx, etc.).
    Office,
    /// Standard documents, source code, and text files (.txt, .md, .rs, .py, etc.).
    #[default]
    Document,
    /// Filesystem folder/directory.
    Folder,
}

impl FileChipKind {
    /// Resolves the [`FileChipKind`] variant from a path and optional MIME type.
    #[must_use]
    pub fn from_path_and_mime(path: &Path, mime_type: Option<&str>) -> Self {
        if path.is_dir() || mime_type == Some("inode/directory") {
            return Self::Folder;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if ext == "pdf" || mime_type == Some("application/pdf") {
            return Self::Pdf;
        }

        if matches!(
            ext.as_str(),
            "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
                | "odt"
                | "ods"
                | "odp"
                | "csv"
                | "tsv"
        ) || mime_type.is_some_and(|m| {
            m.contains("spreadsheet")
                || m.contains("wordprocessing")
                || m.contains("presentation")
                || m.contains("msword")
                || m.contains("ms-excel")
                || m.contains("ms-powerpoint")
        }) {
            return Self::Office;
        }

        Self::Document
    }

    /// Returns the CSS class corresponding to this icon chip variant.
    #[must_use]
    pub const fn css_class(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Office => "office",
            Self::Document => "document",
            Self::Folder => "folder",
        }
    }

    /// Returns the CSS gradient value matching the design specifications.
    #[must_use]
    pub const fn gradient_css(self) -> &'static str {
        match self {
            Self::Pdf => "linear-gradient(160deg, #ff6a5f, #e63b3b)",
            Self::Office => "linear-gradient(160deg, #3ecf72, #1e9c4d)",
            Self::Document => "linear-gradient(160deg, #4f8dff, #2f62e0)",
            Self::Folder => "linear-gradient(160deg, #5ac8fa, #2f8fe0)",
        }
    }

    /// Returns the themed symbolic icon name.
    #[must_use]
    pub const fn icon_name(self) -> &'static str {
        match self {
            Self::Pdf => "x-office-document",
            Self::Office => "x-office-spreadsheet",
            Self::Document => "text-x-generic",
            Self::Folder => "folder",
        }
    }
}

type FileActionCallback = Box<dyn Fn(&SearchRecord)>;

/// File search result row component widget.
#[derive(Clone)]
pub struct FileRowWidget {
    /// Root horizontal container box with `.row` class.
    pub container: gtk4::Box,
    /// Gradient-backed 28px icon chip box with `.ic` class.
    pub icon_chip: gtk4::Box,
    /// Symbolic icon image widget inside the chip.
    pub icon_image: gtk4::Image,
    /// Primary filename label (`.n`).
    pub name_label: gtk4::Label,
    /// Secondary category label (`.t`).
    pub category_label: gtk4::Label,
    record: Rc<RefCell<SearchRecord>>,
    chip_kind: Rc<Cell<FileChipKind>>,
    is_selected: Rc<Cell<bool>>,
    activate_cbs: Rc<RefCell<Vec<FileActionCallback>>>,
    reveal_cbs: Rc<RefCell<Vec<FileActionCallback>>>,
}

impl fmt::Debug for FileRowWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileRowWidget")
            .field("container", &self.container)
            .field("name_label", &self.name_label)
            .field("category_label", &self.category_label)
            .field("chip_kind", &self.chip_kind.get())
            .field("is_selected", &self.is_selected.get())
            .finish_non_exhaustive()
    }
}

impl FileRowWidget {
    /// Constructs a new [`FileRowWidget`] initialized with the given [`SearchRecord`].
    #[must_use]
    pub fn new(record: &SearchRecord) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        container.add_css_class("row");
        container.set_height_request(44);
        container.set_focusable(true);

        let path = record
            .payload
            .file_path()
            .unwrap_or_else(|| Path::new(&record.title));
        let chip_kind = FileChipKind::from_path_and_mime(path, None);

        let icon_chip = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        icon_chip.add_css_class("ic");
        icon_chip.add_css_class("file-icon-chip");
        icon_chip.add_css_class(chip_kind.css_class());
        icon_chip.set_size_request(28, 28);
        icon_chip.set_halign(gtk4::Align::Center);
        icon_chip.set_valign(gtk4::Align::Center);

        let icon_image = gtk4::Image::from_icon_name(chip_kind.icon_name());
        icon_image.set_pixel_size(18);
        icon_chip.append(&icon_image);
        container.append(&icon_chip);

        let name_label = gtk4::Label::new(Some(&record.title));
        name_label.add_css_class("n");
        name_label.set_hexpand(true);
        name_label.set_xalign(0.0);
        name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        container.append(&name_label);

        let cat_text = if record.category == SearchCategory::Folders {
            "Folders"
        } else {
            "Documents"
        };
        let category_label = gtk4::Label::new(Some(cat_text));
        category_label.add_css_class("t");
        category_label.set_xalign(1.0);
        container.append(&category_label);

        let record_rc = Rc::new(RefCell::new(record.clone()));
        let chip_kind_rc = Rc::new(Cell::new(chip_kind));
        let is_selected_rc = Rc::new(Cell::new(false));
        let activate_cbs = Rc::new(RefCell::new(Vec::new()));
        let reveal_cbs = Rc::new(RefCell::new(Vec::new()));

        let widget = Self {
            container,
            icon_chip,
            icon_image,
            name_label,
            category_label,
            record: record_rc,
            chip_kind: chip_kind_rc,
            is_selected: is_selected_rc,
            activate_cbs,
            reveal_cbs,
        };

        widget.setup_events();
        widget
    }

    fn setup_events(&self) {
        // Click gesture for row activation
        let click = gtk4::GestureClick::new();
        let widget_clone = self.clone();
        click.connect_pressed(move |_, _, _, _| {
            widget_clone.activate();
        });
        self.container.add_controller(click);

        // Key event controller for keyboard interaction:
        // - Enter -> open_file
        // - Ctrl+Enter / Cmd+Enter -> reveal in file manager
        // - Ctrl+Shift+C -> copy path to clipboard
        let key_controller = gtk4::EventControllerKey::new();
        let widget_clone = self.clone();
        key_controller.connect_key_pressed(move |_, keyval, _keycode, state| {
            if widget_clone.handle_key(keyval, state) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.container.add_controller(key_controller);
    }

    /// Handles keyboard input events according to Spotlight shortcuts.
    ///
    /// Returns `true` if the event was handled.
    #[must_use]
    pub fn handle_key(&self, keyval: gdk4::Key, state: gdk4::ModifierType) -> bool {
        let is_ctrl = state.contains(gdk4::ModifierType::CONTROL_MASK);
        let is_shift = state.contains(gdk4::ModifierType::SHIFT_MASK);

        // Ctrl+Shift+C: Copy path to clipboard
        if is_ctrl && is_shift && (keyval == gdk4::Key::C || keyval == gdk4::Key::c) {
            return self.copy_path();
        }

        // Ctrl+Enter or Super/Cmd+Enter: Reveal in file manager
        if (is_ctrl || state.contains(gdk4::ModifierType::SUPER_MASK))
            && (keyval == gdk4::Key::Return || keyval == gdk4::Key::KP_Enter)
        {
            self.reveal();
            return true;
        }

        // Enter: Open file
        if keyval == gdk4::Key::Return || keyval == gdk4::Key::KP_Enter {
            self.activate();
            return true;
        }

        false
    }

    /// Returns the underlying [`SearchRecord`].
    #[must_use]
    pub fn record(&self) -> SearchRecord {
        self.record.borrow().clone()
    }

    /// Returns `true` if the row is currently visually selected.
    #[must_use]
    pub fn is_selected(&self) -> bool {
        self.is_selected.get()
    }

    /// Updates the row selection visual state.
    pub fn set_selected(&self, selected: bool) {
        self.is_selected.set(selected);
        if selected {
            self.container.add_css_class("sel");
            self.container.add_css_class("selected");
        } else {
            self.container.remove_css_class("sel");
            self.container.remove_css_class("selected");
        }
    }

    /// Triggers primary activation (open file) and invokes registered callbacks.
    pub fn activate(&self) {
        let rec = self.record.borrow().clone();
        if let Some(path) = rec.payload.file_path() {
            let _ = open_file(path);
        }
        for cb in &*self.activate_cbs.borrow() {
            cb(&rec);
        }
    }

    /// Triggers reveal action (show in file manager) and invokes registered callbacks.
    pub fn reveal(&self) {
        let rec = self.record.borrow().clone();
        if let Some(path) = rec.payload.file_path() {
            let path_buf = path.to_path_buf();
            tokio::spawn(async move {
                let _ = show_in_file_manager(&path_buf).await;
            });
        }
        for cb in &*self.reveal_cbs.borrow() {
            cb(&rec);
        }
    }

    /// Copies the file's canonical path to the clipboard.
    ///
    /// Returns `true` if successfully copied.
    #[must_use]
    pub fn copy_path(&self) -> bool {
        let rec = self.record.borrow();
        rec.payload
            .file_path()
            .is_some_and(copy_file_path_to_clipboard)
    }

    /// Registers a callback invoked upon file activation (Enter / click).
    pub fn connect_activate<F: Fn(&SearchRecord) + 'static>(&self, callback: F) {
        self.activate_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Registers a callback invoked upon file reveal (Ctrl+Enter).
    pub fn connect_reveal<F: Fn(&SearchRecord) + 'static>(&self, callback: F) {
        self.reveal_cbs.borrow_mut().push(Box::new(callback));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_chip_kind_categorization() {
        assert_eq!(
            FileChipKind::from_path_and_mime(Path::new("report.pdf"), None),
            FileChipKind::Pdf
        );
        assert_eq!(
            FileChipKind::from_path_and_mime(Path::new("spreadsheet.xlsx"), None),
            FileChipKind::Office
        );
        assert_eq!(
            FileChipKind::from_path_and_mime(Path::new("presentation.pptx"), None),
            FileChipKind::Office
        );
        assert_eq!(
            FileChipKind::from_path_and_mime(Path::new("document.docx"), None),
            FileChipKind::Office
        );
        assert_eq!(
            FileChipKind::from_path_and_mime(Path::new("main.rs"), None),
            FileChipKind::Document
        );
        assert_eq!(
            FileChipKind::from_path_and_mime(Path::new("notes.txt"), None),
            FileChipKind::Document
        );
        assert_eq!(
            FileChipKind::from_path_and_mime(Path::new("my_folder"), Some("inode/directory")),
            FileChipKind::Folder
        );
    }

    #[test]
    fn test_file_chip_kind_css_and_gradients() {
        assert_eq!(FileChipKind::Pdf.css_class(), "pdf");
        assert!(FileChipKind::Pdf.gradient_css().contains("#ff6a5f"));

        assert_eq!(FileChipKind::Office.css_class(), "office");
        assert!(FileChipKind::Office.gradient_css().contains("#3ecf72"));

        assert_eq!(FileChipKind::Document.css_class(), "document");
        assert!(FileChipKind::Document.gradient_css().contains("#4f8dff"));

        assert_eq!(FileChipKind::Folder.css_class(), "folder");
        assert!(FileChipKind::Folder.gradient_css().contains("#5ac8fa"));
    }
}
