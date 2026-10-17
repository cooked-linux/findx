//! Split-pane preview inspector container component.
//!
//! Integrates the visual preview viewport (images, PDFs, or syntax-highlighted code),
//! metadata summary table, and quick actions ("Open in File Manager", "Copy Path",
//! "Open Terminal").

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use super::code_view::CodePreviewWidget;
use super::image_view::ImagePreviewWidget;
use super::meta_table::MetaTableWidget;
use crate::preview::loader::{FileMetadataSummary, PreviewData, PreviewKind};

type PathCallback = Box<dyn Fn(&Path)>;

/// Right-hand inspector pane container widget.
#[derive(Clone)]
pub struct PreviewPaneWidget {
    /// Root vertical box container with `.pv` and `.preview-pane` styling.
    pub container: gtk4::Box,
    /// Stack switcher between image viewport and code editor.
    pub preview_stack: gtk4::Stack,
    /// Image and PDF thumbnail viewer widget.
    pub image_view: ImagePreviewWidget,
    /// Syntax-highlighted code and text viewer widget.
    pub code_view: CodePreviewWidget,
    /// Metadata card widget displaying filename, kind, and attribute rows.
    pub meta_table: MetaTableWidget,
    /// Bottom quick action buttons container.
    pub actions_bar: gtk4::Box,
    /// Quick action button: "Open in File Manager".
    pub reveal_btn: gtk4::Button,
    /// Quick action button: "Copy Path".
    pub copy_btn: gtk4::Button,
    /// Quick action button: "Open Terminal".
    pub term_btn: gtk4::Button,
    current_file_path: Rc<RefCell<Option<PathBuf>>>,
    reveal_cbs: Rc<RefCell<Vec<PathCallback>>>,
    copy_cbs: Rc<RefCell<Vec<PathCallback>>>,
    term_cbs: Rc<RefCell<Vec<PathCallback>>>,
}

impl fmt::Debug for PreviewPaneWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewPaneWidget")
            .field("container", &self.container)
            .field("current_file_path", &self.current_file_path.borrow())
            .finish_non_exhaustive()
    }
}

impl Default for PreviewPaneWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewPaneWidget {
    /// Constructs a new [`PreviewPaneWidget`].
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("pv");
        container.add_css_class("preview-pane");
        container.set_hexpand(true);
        container.set_vexpand(true);
        container.set_valign(gtk4::Align::Fill);

        let preview_stack = gtk4::Stack::new();
        preview_stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        preview_stack.set_transition_duration(100);
        preview_stack.set_hexpand(true);

        let image_view = ImagePreviewWidget::new();
        let code_view = CodePreviewWidget::new();

        preview_stack.add_named(&image_view.container, Some("image"));
        preview_stack.add_named(&code_view.container, Some("code"));
        preview_stack.set_visible_child_name("image");

        let meta_table = MetaTableWidget::new();

        // Quick action buttons bar
        let actions_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        actions_bar.add_css_class("preview-actions-bar");
        actions_bar.set_halign(gtk4::Align::Center);
        actions_bar.set_margin_top(16);

        let reveal_btn = gtk4::Button::builder()
            .label("Open in Files")
            .icon_name("system-file-manager")
            .css_classes(["preview-action-btn"])
            .build();

        let copy_btn = gtk4::Button::builder()
            .label("Copy Path")
            .icon_name("edit-copy")
            .css_classes(["preview-action-btn"])
            .build();

        let term_btn = gtk4::Button::builder()
            .label("Terminal")
            .icon_name("utilities-terminal")
            .css_classes(["preview-action-btn"])
            .build();

        actions_bar.append(&reveal_btn);
        actions_bar.append(&copy_btn);
        actions_bar.append(&term_btn);

        container.append(&preview_stack);
        container.append(&meta_table.container);
        container.append(&actions_bar);

        let current_file_path = Rc::new(RefCell::new(None));
        let reveal_cbs = Rc::new(RefCell::new(Vec::new()));
        let copy_cbs = Rc::new(RefCell::new(Vec::new()));
        let term_cbs = Rc::new(RefCell::new(Vec::new()));

        let widget = Self {
            container,
            preview_stack,
            image_view,
            code_view,
            meta_table,
            actions_bar,
            reveal_btn,
            copy_btn,
            term_btn,
            current_file_path,
            reveal_cbs,
            copy_cbs,
            term_cbs,
        };

        widget.setup_actions();
        widget
    }

    fn setup_actions(&self) {
        let current_path = Rc::clone(&self.current_file_path);
        let reveal_cbs = Rc::clone(&self.reveal_cbs);
        self.reveal_btn.connect_clicked(move |_| {
            if let Some(path) = current_path.borrow().as_ref() {
                for cb in &*reveal_cbs.borrow() {
                    cb(path);
                }
            }
        });

        let current_path = Rc::clone(&self.current_file_path);
        let copy_cbs = Rc::clone(&self.copy_cbs);
        self.copy_btn.connect_clicked(move |_| {
            if let Some(path) = current_path.borrow().as_ref() {
                for cb in &*copy_cbs.borrow() {
                    cb(path);
                }
            }
        });

        let current_path = Rc::clone(&self.current_file_path);
        let term_cbs = Rc::clone(&self.term_cbs);
        self.term_btn.connect_clicked(move |_| {
            if let Some(path) = current_path.borrow().as_ref() {
                for cb in &*term_cbs.borrow() {
                    cb(path);
                }
            }
        });
    }

    /// Sets provisional fast-path preview state.
    pub fn set_provisional(&self, meta: &FileMetadataSummary, thumb_path: Option<&Path>) {
        *self.current_file_path.borrow_mut() = Some(meta.raw_path.clone());
        self.meta_table.set_metadata(meta);

        if let Some(thumb) = thumb_path {
            self.image_view.set_image_path(thumb);
        } else {
            let icon_name = if meta.kind.contains("PDF") {
                "x-office-document"
            } else if meta.kind.contains("Image") {
                "image-x-generic"
            } else {
                "text-x-generic"
            };
            self.image_view.set_fallback_icon(icon_name);
        }
        self.preview_stack.set_visible_child_name("image");
    }

    /// Updates preview with complete rich [`PreviewData`].
    pub fn set_preview_data(&self, data: &PreviewData) {
        *self.current_file_path.borrow_mut() = Some(data.metadata.raw_path.clone());
        self.meta_table.set_metadata(&data.metadata);

        match &data.kind {
            PreviewKind::Image { thumbnail_path } | PreviewKind::Pdf { thumbnail_path } => {
                self.image_view.set_image_path(thumbnail_path);
                self.preview_stack.set_visible_child_name("image");
            }
            PreviewKind::Code {
                highlighted_markup,
                is_truncated,
                ..
            } => {
                self.code_view.code_label.set_markup(highlighted_markup);
                if *is_truncated {
                    self.code_view
                        .notice_label
                        .set_text("... truncated for preview");
                    self.code_view.notice_label.set_visible(true);
                } else {
                    self.code_view.notice_label.set_text("");
                    self.code_view.notice_label.set_visible(false);
                }
                self.preview_stack.set_visible_child_name("code");
            }
            PreviewKind::Generic { thumbnail_path } => {
                if let Some(thumb) = thumbnail_path {
                    self.image_view.set_image_path(thumb);
                } else {
                    self.image_view.set_fallback_icon("text-x-generic");
                }
                self.preview_stack.set_visible_child_name("image");
            }
        }
    }

    /// Clears the preview inspector pane.
    pub fn clear(&self) {
        *self.current_file_path.borrow_mut() = None;
        self.image_view.clear();
        self.code_view.clear();
        self.meta_table.clear();
    }

    /// Connects callback for the "Open in Files" action.
    pub fn connect_reveal<F: Fn(&Path) + 'static>(&self, callback: F) {
        self.reveal_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Connects callback for the "Copy Path" action.
    pub fn connect_copy_path<F: Fn(&Path) + 'static>(&self, callback: F) {
        self.copy_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Connects callback for the "Open Terminal" action.
    pub fn connect_open_terminal<F: Fn(&Path) + 'static>(&self, callback: F) {
        self.term_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Returns the active file path if any.
    #[must_use]
    pub fn current_file_path(&self) -> Option<PathBuf> {
        self.current_file_path.borrow().clone()
    }
}
