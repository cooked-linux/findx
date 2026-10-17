//! Syntax-highlighted code and text viewer component.
//!
//! Renders source code, markup, and text files with `syntect` syntax coloring conforming to the
//! desktop color scheme (Light/Dark mode) with monospace typography, line truncation, and
//! memory protection ceilings.

use std::cell::Cell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::preview::loader::{highlight_code_lines, read_capped_lines};

/// Code and text preview widget encapsulating scrolled monospace viewport.
#[derive(Clone)]
pub struct CodePreviewWidget {
    /// Outer layout box container with `.code-preview-scroll` styling.
    pub container: gtk4::Box,
    /// Scrolled window viewport enabling bidirectional scrolling.
    pub scrolled: gtk4::ScrolledWindow,
    /// Text label displaying Pango markup colored source lines.
    pub code_label: gtk4::Label,
    /// Status notice label displayed when truncated.
    pub notice_label: gtk4::Label,
    is_truncated: Rc<Cell<bool>>,
    total_lines: Rc<Cell<usize>>,
}

impl fmt::Debug for CodePreviewWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodePreviewWidget")
            .field("container", &self.container)
            .field("is_truncated", &self.is_truncated.get())
            .field("total_lines", &self.total_lines.get())
            .finish_non_exhaustive()
    }
}

impl Default for CodePreviewWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl CodePreviewWidget {
    /// Constructs a new [`CodePreviewWidget`].
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        container.add_css_class("code-preview-scroll");
        container.set_hexpand(true);
        container.set_vexpand(true);

        let scrolled = gtk4::ScrolledWindow::new();
        scrolled.set_hexpand(true);
        scrolled.set_vexpand(true);
        scrolled.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);

        let code_label = gtk4::Label::new(None);
        code_label.set_use_markup(true);
        code_label.set_xalign(0.0);
        code_label.set_yalign(0.0);
        code_label.set_selectable(true);
        code_label.add_css_class("code-preview-text");

        scrolled.set_child(Some(&code_label));
        container.append(&scrolled);

        let notice_label = gtk4::Label::new(None);
        notice_label.add_css_class("code-preview-notice");
        notice_label.set_xalign(0.0);
        notice_label.set_visible(false);
        container.append(&notice_label);

        Self {
            container,
            scrolled,
            code_label,
            notice_label,
            is_truncated: Rc::new(Cell::new(false)),
            total_lines: Rc::new(Cell::new(0)),
        }
    }

    /// Sets the highlighted code lines content.
    pub fn set_code(&self, lines: &[String], extension: &str, is_dark: bool, force_plain: bool) {
        let (markup, truncated, total) =
            highlight_code_lines(lines, extension, is_dark, force_plain);

        self.is_truncated.set(truncated);
        self.total_lines.set(total);

        self.code_label.set_markup(&markup);

        if truncated {
            self.notice_label.set_text("... truncated for preview");
            self.notice_label.set_visible(true);
        } else {
            self.notice_label.set_text("");
            self.notice_label.set_visible(false);
        }
    }

    /// Loads and renders code preview directly from `path`.
    #[must_use]
    pub fn load_file(&self, path: &Path, is_dark: bool) -> bool {
        let Ok((lines, is_large)) = read_capped_lines(path) else {
            self.clear();
            return false;
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        self.set_code(&lines, &ext, is_dark, is_large);
        true
    }

    /// Clears the code viewport and hides notices.
    pub fn clear(&self) {
        self.code_label.set_text("");
        self.notice_label.set_text("");
        self.notice_label.set_visible(false);
        self.is_truncated.set(false);
        self.total_lines.set(0);
    }

    /// Returns `true` if content was truncated due to the 500-line limit.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        self.is_truncated.get()
    }

    /// Returns total line count from the source document.
    #[must_use]
    pub fn total_lines(&self) -> usize {
        self.total_lines.get()
    }
}
