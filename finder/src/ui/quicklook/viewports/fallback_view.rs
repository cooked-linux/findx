//! Specialized fallback metadata card viewport for unsupported or generic file types.

use std::os::unix::fs::MetadataExt;

use gtk4::prelude::*;

use crate::quicklook::types::FallbackCardData;
use crate::ui::columns::{format_byte_size, format_exact_bytes, format_system_time};

/// Viewport widget rendering a large themed asset icon and comprehensive file attributes card.
#[derive(Debug, Clone)]
pub struct FallbackViewport {
    /// Outer container box.
    pub container: gtk4::Box,
    /// 128px prominent Freedesktop themed icon.
    pub icon_image: gtk4::Image,
    /// Bold primary filename label.
    pub title_label: gtk4::Label,
    /// Details grid table.
    pub details_grid: gtk4::Grid,
    /// Value label for Kind.
    pub kind_val: gtk4::Label,
    /// Value label for Logical Size.
    pub size_val: gtk4::Label,
    /// Value label for Physical Size on Disk.
    pub physical_size_val: gtk4::Label,
    /// Value label for Date Modified.
    pub modified_val: gtk4::Label,
}

impl Default for FallbackViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl FallbackViewport {
    /// Constructs a new [`FallbackViewport`].
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(20)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["quicklook-fallback-card"])
            .build();

        // 1. 128px Centered Asset Icon
        let icon_image = gtk4::Image::builder()
            .icon_name("text-x-generic")
            .pixel_size(128)
            .halign(gtk4::Align::Center)
            .build();
        container.append(&icon_image);

        // 2. Primary File Name (16px bold)
        let title_label = gtk4::Label::builder()
            .label("File")
            .css_classes(["quicklook-fallback-title"])
            .halign(gtk4::Align::Center)
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .max_width_chars(40)
            .build();
        container.append(&title_label);

        // 3. File details table (Kind, Logical Size, Physical Size, Modified)
        let details_grid = gtk4::Grid::builder()
            .row_spacing(8)
            .column_spacing(16)
            .halign(gtk4::Align::Center)
            .build();

        let add_row = |grid: &gtk4::Grid, row: i32, key: &str| -> gtk4::Label {
            let key_lbl = gtk4::Label::builder()
                .label(key)
                .css_classes(["quicklook-fallback-meta"])
                .halign(gtk4::Align::End)
                .build();
            let val_lbl = gtk4::Label::builder()
                .label("—")
                .css_classes(["quicklook-title"])
                .halign(gtk4::Align::Start)
                .selectable(true)
                .build();
            grid.attach(&key_lbl, 0, row, 1, 1);
            grid.attach(&val_lbl, 1, row, 1, 1);
            val_lbl
        };

        let kind_val = add_row(&details_grid, 0, "Kind");
        let size_val = add_row(&details_grid, 1, "Logical Size");
        let physical_size_val = add_row(&details_grid, 2, "Size on Disk");
        let modified_val = add_row(&details_grid, 3, "Modified");

        container.append(&details_grid);

        Self {
            container,
            icon_image,
            title_label,
            details_grid,
            kind_val,
            size_val,
            physical_size_val,
            modified_val,
        }
    }

    /// Returns the root widget to embed in containers.
    #[must_use]
    pub const fn widget(&self) -> &gtk4::Box {
        &self.container
    }

    /// Loads and renders preview metadata from [`FallbackCardData`].
    pub fn load_data(&self, card: &FallbackCardData) {
        self.icon_image.set_icon_name(Some(&card.icon_name));
        self.title_label.set_label(&card.file_name);

        self.kind_val.set_label(&card.mime_type);

        let logical_str = format!(
            "{} ({})",
            format_byte_size(card.file_size),
            format_exact_bytes(card.file_size)
        );
        self.size_val.set_label(&logical_str);

        // Query physical size on disk via 512-byte blocks allocation
        let physical_bytes =
            std::fs::metadata(&card.path).map_or(card.file_size, |meta| meta.blocks() * 512);
        let physical_str = format!(
            "{} ({})",
            format_byte_size(physical_bytes),
            format_exact_bytes(physical_bytes)
        );
        self.physical_size_val.set_label(&physical_str);

        self.modified_val
            .set_label(&format_system_time(card.modified));
    }
}
