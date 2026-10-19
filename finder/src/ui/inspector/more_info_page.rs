//! Format-specific "More Info", Comments, and Origin accordion groups.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::prelude::*;

use crate::metadata::types::{DeepMetadata, ImageExifMeta, MediaStreamMeta, PdfDocMeta};
use crate::metadata::xattr::ExtendedAttributesService;

/// Formats media duration in seconds into `MM:SS` or `HH:MM:SS`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn format_duration(secs: f64) -> String {
    if secs <= 0.0 {
        return "00:00".to_string();
    }
    let total_secs = secs.round() as u64;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let remainder_secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{remainder_secs:02}")
    } else {
        format!("{mins:02}:{remainder_secs:02}")
    }
}

/// "More Info" property group displaying deep format-specific metadata.
#[derive(Clone)]
pub struct MoreInfoGroup {
    /// Root libadwaita preferences group.
    pub group: libadwaita::PreferencesGroup,
    rows_box: gtk4::Box,
}

impl MoreInfoGroup {
    /// Constructs a new [`MoreInfoGroup`].
    #[must_use]
    pub fn new() -> Self {
        let group = libadwaita::PreferencesGroup::builder()
            .title("More Info")
            .build();

        let rows_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(2)
            .build();
        group.add(&rows_box);

        Self { group, rows_box }
    }

    /// Clears any dynamic metadata rows.
    fn clear_rows(&self) {
        while let Some(child) = self.rows_box.first_child() {
            self.rows_box.remove(&child);
        }
    }

    fn add_row(&self, title: &str, subtitle: &str) {
        let row = libadwaita::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .build();
        self.rows_box.append(&row);
    }

    /// Binds photographic EXIF metadata.
    fn bind_image(&self, exif: &ImageExifMeta) {
        if exif.pixel_width > 0 && exif.pixel_height > 0 {
            self.add_row(
                "Dimensions",
                &format!("{} × {}", exif.pixel_width, exif.pixel_height),
            );
        }
        let camera = match (&exif.camera_make, &exif.camera_model) {
            (Some(make), Some(model)) => {
                if model.starts_with(make) {
                    model.clone()
                } else {
                    format!("{make} {model}")
                }
            }
            (Some(make), None) => make.clone(),
            (None, Some(model)) => model.clone(),
            (None, None) => String::new(),
        };
        if !camera.is_empty() {
            self.add_row("Device", &camera);
        }
        if let Some(fl) = exif.focal_length {
            self.add_row("Focal Length", &format!("{fl:.1} mm"));
        }
        if let Some(fnumb) = exif.f_number {
            self.add_row("F-Number", &format!("f/{fnumb:.1}"));
        }
        if let Some(iso) = exif.iso_speed {
            self.add_row("ISO Speed", &iso.to_string());
        }
        if let Some(ref exp) = exif.exposure_time {
            self.add_row("Exposure", exp);
        }
        if let Some(ref cp) = exif.color_profile {
            self.add_row("Color Profile", cp);
        }
    }

    /// Binds technical audio/video stream properties.
    fn bind_media(&self, stream: &MediaStreamMeta) {
        if stream.duration_secs > 0.0 {
            self.add_row("Duration", &format_duration(stream.duration_secs));
        }
        if let (Some(w), Some(h)) = (stream.video_width, stream.video_height) {
            self.add_row("Resolution", &format!("{w} × {h}"));
        }
        let codecs = match (&stream.video_codec, &stream.audio_codec) {
            (Some(v), Some(a)) => format!("{v} / {a}"),
            (Some(v), None) => v.clone(),
            (None, Some(a)) => a.clone(),
            (None, None) => String::new(),
        };
        if !codecs.is_empty() {
            self.add_row("Codecs", &codecs);
        }
        if let Some(bitrate) = stream.audio_bitrate {
            self.add_row("Audio Bitrate", &format!("{} kbps", bitrate / 1000));
        }
        if let Some(rate) = stream.sample_rate {
            self.add_row("Sample Rate", &format!("{} kHz", rate / 1000));
        }
        if let Some(ch) = stream.channels {
            let label = match ch {
                1 => "1 (Mono)".to_string(),
                2 => "2 (Stereo)".to_string(),
                6 => "6 (5.1 Surround)".to_string(),
                8 => "8 (7.1 Surround)".to_string(),
                other => other.to_string(),
            };
            self.add_row("Channels", &label);
        }
    }

    /// Binds document properties for PDF files.
    fn bind_pdf(&self, pdf: &PdfDocMeta) {
        if pdf.page_count > 0 {
            let s = if pdf.page_count == 1 {
                "1 page".to_string()
            } else {
                format!("{} pages", pdf.page_count)
            };
            self.add_row("Page Count", &s);
        }
        if let Some(ref ver) = pdf.pdf_version {
            self.add_row("PDF Version", ver);
        }
        if let Some(ref title) = pdf.title {
            self.add_row("Title", title);
        }
        if let Some(ref author) = pdf.author {
            self.add_row("Author", author);
        }
        if let Some(ref creator) = pdf.creator {
            self.add_row("Creator", creator);
        }
    }

    /// Binds metadata from single inspected node.
    pub fn bind_single(&self, meta: &DeepMetadata) {
        self.clear_rows();
        let mut has_data = false;

        if let Some(ref exif) = meta.image_meta {
            self.bind_image(exif);
            has_data = true;
        } else if let Some(ref stream) = meta.media_meta {
            self.bind_media(stream);
            has_data = true;
        } else if let Some(ref pdf) = meta.pdf_meta {
            self.bind_pdf(pdf);
            has_data = true;
        }

        self.group.set_visible(has_data);
    }

    /// Hides more info accordion for multi-selection.
    pub fn bind_multi(&self) {
        self.clear_rows();
        self.group.set_visible(false);
    }
}

impl Default for MoreInfoGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// "Comments" property group reading and persisting `user.xdg.comment`.
#[derive(Clone)]
pub struct CommentsGroup {
    /// Root libadwaita preferences group.
    pub group: libadwaita::PreferencesGroup,
    text_view: gtk4::TextView,
    current_paths: Rc<RefCell<Vec<PathBuf>>>,
    updating_text: Rc<RefCell<bool>>,
}

impl CommentsGroup {
    /// Constructs a new [`CommentsGroup`].
    #[must_use]
    pub fn new() -> Self {
        let group = libadwaita::PreferencesGroup::builder()
            .title("Comments")
            .build();

        let text_view = gtk4::TextView::builder()
            .wrap_mode(gtk4::WrapMode::WordChar)
            .left_margin(8)
            .right_margin(8)
            .top_margin(8)
            .bottom_margin(8)
            .hexpand(true)
            .vexpand(false)
            .height_request(80)
            .build();

        let scrolled = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .min_content_height(80)
            .max_content_height(140)
            .child(&text_view)
            .build();

        let clamp = libadwaita::Clamp::builder()
            .maximum_size(440)
            .child(&scrolled)
            .build();

        group.add(&clamp);

        let current_paths: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let updating_text = Rc::new(RefCell::new(false));

        // Commit helper
        let paths_commit = Rc::clone(&current_paths);
        let text_view_weak = text_view.downgrade();
        let updating_commit = Rc::clone(&updating_text);

        let commit_fn = Rc::new(move || {
            if *updating_commit.borrow() {
                return;
            }
            if let Some(tv) = text_view_weak.upgrade() {
                let buffer = tv.buffer();
                let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                let paths = paths_commit.borrow();
                for path in paths.iter() {
                    let _ = ExtendedAttributesService::set_comment(path, &text);
                }
            }
        });

        // Focus-out auto commit
        let focus_controller = gtk4::EventControllerFocus::new();
        let commit_focus = Rc::clone(&commit_fn);
        focus_controller.connect_leave(move |_| {
            commit_focus();
        });
        text_view.add_controller(focus_controller);

        // Ctrl+Enter auto commit
        let key_controller = gtk4::EventControllerKey::new();
        let commit_key = Rc::clone(&commit_fn);
        key_controller.connect_key_pressed(move |_ctrl, keyval, _code, state| {
            let is_ctrl = state.contains(gdk4::ModifierType::CONTROL_MASK);
            if is_ctrl && (keyval == gdk4::Key::Return || keyval == gdk4::Key::KP_Enter) {
                commit_key();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        text_view.add_controller(key_controller);

        Self {
            group,
            text_view,
            current_paths,
            updating_text,
        }
    }

    /// Binds single node comment.
    pub fn bind_single(&self, meta: &DeepMetadata) {
        *self.current_paths.borrow_mut() = vec![meta.path.clone()];
        *self.updating_text.borrow_mut() = true;
        self.text_view.buffer().set_text(&meta.extended.comment);
        *self.updating_text.borrow_mut() = false;
        self.group.set_visible(true);
    }

    /// Binds multi-selection paths for batch commenting.
    pub fn bind_multi(&self, paths: &[PathBuf]) {
        *self.current_paths.borrow_mut() = paths.to_vec();
        *self.updating_text.borrow_mut() = true;
        self.text_view.buffer().set_text("");
        *self.updating_text.borrow_mut() = false;
        self.group.set_visible(true);
    }
}

impl Default for CommentsGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// "Origin" group displaying `user.xdg.origin.url` and referrer.
#[derive(Clone)]
pub struct OriginGroup {
    /// Root libadwaita preferences group.
    pub group: libadwaita::PreferencesGroup,
    url_row: libadwaita::ActionRow,
    referrer_row: libadwaita::ActionRow,
    current_url: Rc<RefCell<Option<String>>>,
}

impl OriginGroup {
    /// Constructs a new [`OriginGroup`].
    #[must_use]
    pub fn new() -> Self {
        let group = libadwaita::PreferencesGroup::builder()
            .title("Where from")
            .build();

        let url_row = libadwaita::ActionRow::builder()
            .title("Download URL")
            .build();

        let open_btn = gtk4::Button::builder()
            .icon_name("web-browser-symbolic")
            .tooltip_text("Open URL in browser")
            .valign(gtk4::Align::Center)
            .css_classes(["flat"])
            .build();
        url_row.add_suffix(&open_btn);

        let referrer_row = libadwaita::ActionRow::builder().title("Referrer").build();

        group.add(&url_row);
        group.add(&referrer_row);

        let current_url: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        let url_clone = Rc::clone(&current_url);
        open_btn.connect_clicked(move |btn| {
            let root = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
            if let (Some(url), Some(root)) = (url_clone.borrow().as_ref(), root.as_ref()) {
                gtk4::UriLauncher::new(url).launch(
                    Some(root),
                    None as Option<&gio::Cancellable>,
                    |_| {},
                );
            }
        });

        Self {
            group,
            url_row,
            referrer_row,
            current_url,
        }
    }

    /// Binds download origin attributes from metadata.
    pub fn bind_single(&self, meta: &DeepMetadata) {
        if let Some(ref url) = meta.extended.origin_url {
            *self.current_url.borrow_mut() = Some(url.clone());
            self.url_row.set_subtitle(url);

            if let Some(ref ref_url) = meta.extended.origin_referrer {
                self.referrer_row.set_subtitle(ref_url);
                self.referrer_row.set_visible(true);
            } else {
                self.referrer_row.set_visible(false);
            }

            self.group.set_visible(true);
        } else {
            *self.current_url.borrow_mut() = None;
            self.group.set_visible(false);
        }
    }

    /// Hides origin group for multi-selection.
    pub fn bind_multi(&self) {
        *self.current_url.borrow_mut() = None;
        self.group.set_visible(false);
    }
}

impl Default for OriginGroup {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_variants() {
        assert_eq!(format_duration(0.0), "00:00");
        assert_eq!(format_duration(45.0), "00:45");
        assert_eq!(format_duration(225.0), "03:45");
        assert_eq!(format_duration(3665.0), "01:01:05");
    }
}
