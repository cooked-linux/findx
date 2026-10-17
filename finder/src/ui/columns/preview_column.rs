//! Terminal leaf preview column displaying rich file metadata, thumbnail, and quick actions.

use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use gtk4::prelude::*;
use shared::models::{FileItem, TagColor};

use crate::grouping::kind::classify_kind;
use crate::views::columns::models::{MAX_COLUMN_WIDTH, MIN_COLUMN_WIDTH};

/// Callback signature for preview actions receiving the inspected [`FileItem`].
pub type PreviewActionCallback = Box<dyn Fn(&FileItem)>;

struct PreviewInner {
    item: FileItem,
    on_open: RefCell<Option<PreviewActionCallback>>,
    on_quick_look: RefCell<Option<PreviewActionCallback>>,
    on_reveal: RefCell<Option<PreviewActionCallback>>,
}

impl fmt::Debug for PreviewInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewInner")
            .field("item", &self.item)
            .finish_non_exhaustive()
    }
}

/// Widget displaying rich preview card, asset thumbnail, metadata, and quick action buttons.
#[derive(Debug, Clone)]
pub struct ColumnPreviewWidget {
    /// Root scrolled window container.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Root vertical box container.
    pub container: gtk4::Box,
    /// Primary file asset thumbnail or fallback icon image.
    pub picture: gtk4::Picture,
    inner: Rc<PreviewInner>,
}

impl ColumnPreviewWidget {
    /// Constructs a new [`ColumnPreviewWidget`] for the specified [`FileItem`].
    #[must_use]
    pub fn new(item: FileItem, width_px: i32) -> Self {
        let clamped_width = width_px.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);

        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .css_classes(["preview-column"])
            .valign(gtk4::Align::Start)
            .halign(gtk4::Align::Fill)
            .hexpand(true)
            .vexpand(true)
            .spacing(12)
            .build();

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .width_request(clamped_width)
            .css_classes(["column-scrolled-window"])
            .child(&container)
            .build();

        // 1. Large asset icon / thumbnail (64x80px)
        let picture = gtk4::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk4::ContentFit::Contain)
            .width_request(64)
            .height_request(80)
            .halign(gtk4::Align::Center)
            .build();

        // Fallback generic icon until thumbnail loads
        let fallback_icon = gtk4::Image::builder()
            .icon_name(Self::resolve_icon_name(&item))
            .pixel_size(64)
            .halign(gtk4::Align::Center)
            .build();

        let thumb_stack = gtk4::Stack::builder()
            .halign(gtk4::Align::Center)
            .width_request(64)
            .height_request(80)
            .build();
        thumb_stack.add_named(&fallback_icon, Some("icon"));
        thumb_stack.add_named(&picture, Some("picture"));
        thumb_stack.set_visible_child_name("icon");
        container.append(&thumb_stack);

        // 2. Prominent filename label (15px font, bold)
        let title_label = gtk4::Label::builder()
            .label(item.file_name())
            .halign(gtk4::Align::Center)
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .css_classes(["preview-title"])
            .build();
        container.append(&title_label);

        // 3. Subtitle: Kind · Human-readable file size (e.g. `Rust Source · 4.2 KB`)
        let kind_text = classify_kind(&item).title;
        let size_text = format_byte_size(item.metadata.size_bytes);
        let subtitle_text = format!("{kind_text} · {size_text}");
        let subtitle_label = gtk4::Label::builder()
            .label(&subtitle_text)
            .halign(gtk4::Align::Center)
            .css_classes(["preview-subtitle"])
            .build();
        container.append(&subtitle_label);

        // 4. Metadata card (`.preview-card` styling, 12px border radius, subtle border)
        let card = Self::build_metadata_card(&item);
        container.append(&card);

        // 5. Quick actions footer
        let (footer_box, open_btn, ql_btn, reveal_btn) = Self::build_quick_actions();
        container.append(&footer_box);

        let inner = Rc::new(PreviewInner {
            item,
            on_open: RefCell::new(None),
            on_quick_look: RefCell::new(None),
            on_reveal: RefCell::new(None),
        });

        let widget = Self {
            scrolled_window,
            container,
            picture,
            inner,
        };

        widget.wire_actions(&open_btn, &ql_btn, &reveal_btn);
        widget.load_thumbnail_async(thumb_stack);

        widget
    }

    /// Sets the width of the preview column, bounded by limits.
    pub fn set_width(&self, width: i32) {
        let clamped = width.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
        self.scrolled_window.set_width_request(clamped);
    }

    /// Returns the inspected file item.
    #[must_use]
    pub fn item(&self) -> &FileItem {
        &self.inner.item
    }

    /// Registers a callback triggered when clicking the "Open" button.
    pub fn connect_open<F: Fn(&FileItem) + 'static>(&self, callback: F) {
        *self.inner.on_open.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback triggered when clicking the "Quick Look" button.
    pub fn connect_quick_look<F: Fn(&FileItem) + 'static>(&self, callback: F) {
        *self.inner.on_quick_look.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback triggered when clicking the "Reveal" button.
    pub fn connect_reveal<F: Fn(&FileItem) + 'static>(&self, callback: F) {
        *self.inner.on_reveal.borrow_mut() = Some(Box::new(callback));
    }

    fn build_metadata_card(item: &FileItem) -> gtk4::Box {
        let card = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .css_classes(["preview-card"])
            .spacing(4)
            .build();

        // Modified date
        let mod_str = format_system_time(item.metadata.modified);
        card.append(&Self::build_card_row("Modified", &mod_str));

        // Created date
        let created_str = format_system_time(item.metadata.created);
        card.append(&Self::build_card_row("Created", &created_str));

        // Exact byte size
        let exact_size = format_exact_bytes(item.metadata.size_bytes);
        card.append(&Self::build_card_row("Size", &exact_size));

        // Tags
        let tags_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .build();
        let tags_label = gtk4::Label::builder()
            .label("Tags")
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .css_classes(["dim-label"])
            .build();
        tags_row.append(&tags_label);

        let colors = item.tag_colors();
        if colors.is_empty() {
            let empty_tag = gtk4::Label::builder()
                .label("—")
                .halign(gtk4::Align::End)
                .css_classes(["dim-label"])
                .build();
            tags_row.append(&empty_tag);
        } else {
            let dots_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(5)
                .valign(gtk4::Align::Center)
                .halign(gtk4::Align::End)
                .build();
            for color in colors {
                let dot = gtk4::Box::builder()
                    .css_classes(["tg", Self::tag_color_css_class(color)])
                    .build();
                dots_box.append(&dot);
            }
            tags_row.append(&dots_box);
        }
        card.append(&tags_row);

        card
    }

    fn build_card_row(label: &str, value: &str) -> gtk4::Box {
        let row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .build();

        let lbl = gtk4::Label::builder()
            .label(label)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .css_classes(["dim-label"])
            .build();

        let val = gtk4::Label::builder()
            .label(value)
            .halign(gtk4::Align::End)
            .build();

        row.append(&lbl);
        row.append(&val);
        row
    }

    fn build_quick_actions() -> (gtk4::Box, gtk4::Button, gtk4::Button, gtk4::Button) {
        let footer = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk4::Align::Center)
            .margin_top(4)
            .build();

        let open_btn = gtk4::Button::builder()
            .label("Open")
            .css_classes(["preview-btn", "primary"])
            .build();

        let ql_btn = gtk4::Button::builder()
            .label("Quick Look")
            .css_classes(["preview-btn"])
            .build();

        let reveal_btn = gtk4::Button::builder()
            .label("Reveal")
            .css_classes(["preview-btn"])
            .build();

        footer.append(&open_btn);
        footer.append(&ql_btn);
        footer.append(&reveal_btn);

        (footer, open_btn, ql_btn, reveal_btn)
    }

    fn wire_actions(
        &self,
        open_btn: &gtk4::Button,
        ql_btn: &gtk4::Button,
        reveal_btn: &gtk4::Button,
    ) {
        let inner = Rc::clone(&self.inner);
        open_btn.connect_clicked(move |_| {
            if let Some(ref cb) = *inner.on_open.borrow() {
                cb(&inner.item);
            } else {
                Self::default_open_file(&inner.item.path);
            }
        });

        let inner = Rc::clone(&self.inner);
        ql_btn.connect_clicked(move |_| {
            if let Some(ref cb) = *inner.on_quick_look.borrow() {
                cb(&inner.item);
            }
        });

        let inner = Rc::clone(&self.inner);
        reveal_btn.connect_clicked(move |_| {
            if let Some(ref cb) = *inner.on_reveal.borrow() {
                cb(&inner.item);
            } else {
                Self::default_reveal_file(&inner.item.path);
            }
        });
    }

    fn default_open_file(path: &Path) {
        let uri = format!("file://{}", path.to_string_lossy());
        let _ = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE);
    }

    fn default_reveal_file(path: &Path) {
        if let Some(parent) = path.parent() {
            let uri = format!("file://{}", parent.to_string_lossy());
            let _ = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE);
        }
    }

    fn load_thumbnail_async(&self, thumb_stack: gtk4::Stack) {
        let pic = self.picture.clone();
        let file_path = self.inner.item.path.clone();
        let mime = self.inner.item.metadata.mime_type.clone();

        glib::MainContext::default().spawn_local(async move {
            let thumb_res = tokio::task::spawn_blocking(move || {
                shared::thumbnail::get_or_generate_thumbnail(
                    &file_path,
                    shared::thumbnail::ThumbnailSize::Normal,
                    Some(&mime),
                )
            })
            .await;

            if let Ok(Ok(thumb_path)) = thumb_res {
                pic.set_filename(Some(thumb_path));
                thumb_stack.set_visible_child_name("picture");
            }
        });
    }

    const fn resolve_icon_name(item: &FileItem) -> &'static str {
        if item.is_dir() {
            "folder-symbolic"
        } else if item.is_symlink() && item.symlink_target.is_none() {
            "emblem-unreadable"
        } else {
            "text-x-generic-symbolic"
        }
    }

    const fn tag_color_css_class(color: TagColor) -> &'static str {
        match color {
            TagColor::Red => "red",
            TagColor::Orange => "orange",
            TagColor::Yellow => "yellow",
            TagColor::Green => "green",
            TagColor::Blue => "blue",
            TagColor::Purple => "purple",
            TagColor::Gray => "gray",
        }
    }
}

/// Formats byte count into human-readable metric string (e.g. `4.2 KB`, `1.5 MB`).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn format_byte_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < MB {
        format!("{:.1} KB", b / KB)
    } else if b < GB {
        format!("{:.1} MB", b / MB)
    } else if b < TB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.1} TB", b / TB)
    }
}

/// Formats exact byte count with digit separator commas (e.g. `4,312 bytes`).
#[must_use]
pub fn format_exact_bytes(bytes: u64) -> String {
    let s = bytes.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    for (i, ch) in chars.into_iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    format!("{result} bytes")
}

/// Formats a [`SystemTime`] into a human-readable date/time string.
#[must_use]
pub fn format_system_time(time: Option<SystemTime>) -> String {
    let Some(t) = time else {
        return "—".to_string();
    };

    let dt: DateTime<Local> = DateTime::from(t);
    let now = Local::now();

    if dt.date_naive() == now.date_naive() {
        format!("Today, {}", dt.format("%H:%M"))
    } else {
        dt.format("%b %-d, %Y").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_byte_size() {
        assert_eq!(format_byte_size(500), "500 B");
        assert_eq!(format_byte_size(1024), "1.0 KB");
        assert_eq!(format_byte_size(4300), "4.2 KB");
        assert_eq!(format_byte_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_byte_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_format_exact_bytes() {
        assert_eq!(format_exact_bytes(0), "0 bytes");
        assert_eq!(format_exact_bytes(999), "999 bytes");
        assert_eq!(format_exact_bytes(1000), "1,000 bytes");
        assert_eq!(format_exact_bytes(4312), "4,312 bytes");
        assert_eq!(format_exact_bytes(1_000_000), "1,000,000 bytes");
    }
}
