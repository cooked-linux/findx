//! Metadata summary table component.
//!
//! Renders file name (`.pn`), kind subtitle (`.pk`), and attribute rows (`.meta`)
//! for Kind, Size, Modified, and Where breadcrumbs according to the Spotlight design spec.

use std::fmt;

use gtk4::prelude::*;

use crate::preview::loader::FileMetadataSummary;

/// Metadata table row containing key and value labels.
#[derive(Clone)]
pub struct MetaRow {
    /// Horizontal row container box with `.meta-row` styling.
    pub container: gtk4::Box,
    /// Muted key name label (`.meta-key`).
    pub key_label: gtk4::Label,
    /// Value label (`.meta-val`).
    pub val_label: gtk4::Label,
}

impl MetaRow {
    #[must_use]
    fn new(key: &str) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        container.add_css_class("meta-row");
        container.set_hexpand(true);

        let key_label = gtk4::Label::new(Some(key));
        key_label.add_css_class("meta-key");
        key_label.set_halign(gtk4::Align::Start);
        key_label.set_xalign(0.0);

        let val_label = gtk4::Label::new(None);
        val_label.add_css_class("meta-val");
        val_label.set_halign(gtk4::Align::End);
        val_label.set_hexpand(true);
        val_label.set_xalign(1.0);
        val_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);

        container.append(&key_label);
        container.append(&val_label);

        Self {
            container,
            key_label,
            val_label,
        }
    }

    fn set_value(&self, val: &str) {
        self.val_label.set_text(val);
    }
}

/// Metadata inspector card displaying filename, kind, and attribute rows.
#[derive(Clone)]
pub struct MetaTableWidget {
    /// Vertical container box encompassing header and table rows.
    pub container: gtk4::Box,
    /// Primary file name label (`.pn`).
    pub name_label: gtk4::Label,
    /// Secondary kind label (`.pk`).
    pub kind_label: gtk4::Label,
    /// Attributes container box (`.meta`).
    pub table_box: gtk4::Box,
    row_kind: MetaRow,
    row_size: MetaRow,
    row_modified: MetaRow,
    row_where: MetaRow,
}

impl fmt::Debug for MetaTableWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetaTableWidget")
            .field("name", &self.name_label.text())
            .field("kind", &self.kind_label.text())
            .finish_non_exhaustive()
    }
}

impl Default for MetaTableWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaTableWidget {
    /// Constructs a new [`MetaTableWidget`].
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.set_hexpand(true);

        let name_label = gtk4::Label::new(None);
        name_label.add_css_class("pn");
        name_label.set_halign(gtk4::Align::Center);
        name_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        container.append(&name_label);

        let kind_label = gtk4::Label::new(None);
        kind_label.add_css_class("pk");
        kind_label.set_halign(gtk4::Align::Center);
        container.append(&kind_label);

        let table_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        table_box.add_css_class("meta");
        table_box.set_hexpand(true);

        let row_kind = MetaRow::new("Kind");
        let row_size = MetaRow::new("Size");
        let row_modified = MetaRow::new("Modified");
        let row_where = MetaRow::new("Where");

        table_box.append(&row_kind.container);
        table_box.append(&row_size.container);
        table_box.append(&row_modified.container);
        table_box.append(&row_where.container);

        container.append(&table_box);

        Self {
            container,
            name_label,
            kind_label,
            table_box,
            row_kind,
            row_size,
            row_modified,
            row_where,
        }
    }

    /// Updates all metadata labels from the given [`FileMetadataSummary`].
    pub fn set_metadata(&self, meta: &FileMetadataSummary) {
        self.name_label.set_text(&meta.name);
        self.kind_label.set_text(&meta.kind);

        self.row_kind.set_value(&meta.kind);
        self.row_size.set_value(&meta.formatted_size);
        self.row_modified.set_value(&meta.formatted_modified);
        self.row_where.set_value(&meta.where_path);
    }

    /// Clears all metadata labels.
    pub fn clear(&self) {
        self.name_label.set_text("");
        self.kind_label.set_text("");
        self.row_kind.set_value("");
        self.row_size.set_value("");
        self.row_modified.set_value("");
        self.row_where.set_value("");
    }
}
