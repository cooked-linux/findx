//! Modal conflict resolution dialog displaying side-by-side comparison cards and context-aware actions.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use gtk4::prelude::*;
use libadwaita::prelude::*;
use tokio::sync::oneshot;

use crate::operations::conflict::{
    ConflictAction, ConflictContext, ConflictItemMeta, ConflictResolution, RelativeTimeStatus,
};

/// Styling appearance for conflict resolution action buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictButtonStyle {
    /// Suggested primary style (e.g. Merge for dirs, Keep Both for files).
    Suggested,
    /// Destructive alert style (e.g. overwriting newer item or replacing dir).
    Destructive,
    /// Flat secondary action style.
    Flat,
    /// Default standard button style.
    Default,
}

/// Description of an action button rendered in the conflict resolution dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictButtonConfig {
    /// Conflict action variant.
    pub action: ConflictAction,
    /// Display label text.
    pub label: &'static str,
    /// Button appearance style.
    pub style: ConflictButtonStyle,
}

/// Relative age badge descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativeAgeBadge {
    /// Item was modified after the comparison counterpart.
    Newer,
    /// Item was modified before the comparison counterpart.
    Older,
    /// Items have identical modification timestamps within tolerance.
    None,
}

impl RelativeAgeBadge {
    /// Returns the user-facing badge label text.
    #[must_use]
    pub const fn label(&self) -> Option<&'static str> {
        match self {
            Self::Newer => Some("Newer"),
            Self::Older => Some("Older"),
            Self::None => None,
        }
    }

    /// Returns the CSS class assigned to the badge widget.
    #[must_use]
    pub const fn css_class(&self) -> Option<&'static str> {
        match self {
            Self::Newer => Some("conflict-age-badge-newer"),
            Self::Older => Some("conflict-age-badge-older"),
            Self::None => None,
        }
    }
}

/// Determines the relative age badges for destination and source items.
#[must_use]
pub const fn determine_age_badges(
    status: RelativeTimeStatus,
) -> (RelativeAgeBadge, RelativeAgeBadge) {
    match status {
        RelativeTimeStatus::IncomingNewer => (RelativeAgeBadge::Older, RelativeAgeBadge::Newer),
        RelativeTimeStatus::IncomingOlder => (RelativeAgeBadge::Newer, RelativeAgeBadge::Older),
        RelativeTimeStatus::IdenticalTime => (RelativeAgeBadge::None, RelativeAgeBadge::None),
    }
}

/// Determines the contextual action buttons to present based on collision type and age.
#[must_use]
pub fn determine_action_buttons(
    is_directory_collision: bool,
    relative_time: RelativeTimeStatus,
) -> Vec<ConflictButtonConfig> {
    if is_directory_collision {
        vec![
            ConflictButtonConfig {
                action: ConflictAction::Merge,
                label: "Merge",
                style: ConflictButtonStyle::Suggested,
            },
            ConflictButtonConfig {
                action: ConflictAction::Replace,
                label: "Replace",
                style: ConflictButtonStyle::Destructive,
            },
            ConflictButtonConfig {
                action: ConflictAction::KeepBoth,
                label: "Keep Both",
                style: ConflictButtonStyle::Default,
            },
            ConflictButtonConfig {
                action: ConflictAction::Skip,
                label: "Skip",
                style: ConflictButtonStyle::Flat,
            },
            ConflictButtonConfig {
                action: ConflictAction::Stop,
                label: "Stop",
                style: ConflictButtonStyle::Flat,
            },
        ]
    } else {
        let replace_style = if relative_time == RelativeTimeStatus::IncomingOlder {
            ConflictButtonStyle::Destructive
        } else {
            ConflictButtonStyle::Default
        };

        vec![
            ConflictButtonConfig {
                action: ConflictAction::Replace,
                label: "Replace",
                style: replace_style,
            },
            ConflictButtonConfig {
                action: ConflictAction::KeepBoth,
                label: "Keep Both",
                style: ConflictButtonStyle::Suggested,
            },
            ConflictButtonConfig {
                action: ConflictAction::Skip,
                label: "Skip",
                style: ConflictButtonStyle::Flat,
            },
            ConflictButtonConfig {
                action: ConflictAction::Stop,
                label: "Stop",
                style: ConflictButtonStyle::Flat,
            },
        ]
    }
}

/// Formats a `SystemTime` modification timestamp into a human-readable local date string.
#[must_use]
pub fn format_modified_time(modified: Option<SystemTime>) -> String {
    modified.map_or_else(
        || "Unknown date".to_string(),
        |time| {
            let dt: chrono::DateTime<chrono::Local> = time.into();
            dt.format("%b %-d, %Y at %-I:%M %p").to_string()
        },
    )
}

/// Guesses a standard Freedesktop icon name from a path and directory flag.
#[must_use]
pub fn guess_icon_for_path(path: &Path, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder";
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);
    match ext.as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") => "image-x-generic",
        Some("mp4" | "mkv" | "mov" | "avi" | "webm") => "video-x-generic",
        Some("mp3" | "flac" | "wav" | "aac" | "ogg") => "audio-x-generic",
        Some("pdf") => "application-pdf",
        Some("zip" | "tar" | "gz" | "bz2" | "xz" | "7z") => "package-x-generic",
        Some("rs" | "c" | "cpp" | "py" | "js" | "ts" | "html" | "css") => "text-x-script",
        _ => "text-x-generic",
    }
}

/// Modal conflict resolution sheet dialog with side-by-side comparison cards.
#[derive(Debug, Clone)]
pub struct ConflictResolutionDialog {
    /// Top-level modal window.
    pub window: libadwaita::Window,
    /// Heading title label.
    pub title_label: gtk4::Label,
    /// Subtitle prompt label.
    pub subtitle_label: gtk4::Label,
    /// "Apply to all" batch toggle checkbox.
    pub apply_to_all_check: gtk4::CheckButton,
    /// Action button widgets.
    pub buttons: Vec<gtk4::Button>,
    /// Dedicated reference to Stop / Skip button for Alt modifier toggling.
    pub stop_button: Option<gtk4::Button>,
    /// Dedicated reference to Keep Both button for focus transitions.
    pub keep_both_button: Option<gtk4::Button>,
    /// Context describing colliding source and destination items.
    pub context: ConflictContext,
    responder: Arc<Mutex<Option<oneshot::Sender<ConflictResolution>>>>,
    is_alt_held: Arc<Mutex<bool>>,
}

impl ConflictResolutionDialog {
    /// Constructs a new [`ConflictResolutionDialog`].
    #[must_use]
    pub fn new(context: ConflictContext, responder: oneshot::Sender<ConflictResolution>) -> Self {
        let responder_arc = Arc::new(Mutex::new(Some(responder)));
        let is_alt_held_arc = Arc::new(Mutex::new(false));

        let window = libadwaita::Window::builder()
            .title("File Conflict")
            .modal(true)
            .resizable(false)
            .default_width(520)
            .build();

        let root_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(16)
            .margin_start(20)
            .margin_end(20)
            .margin_top(20)
            .margin_bottom(20)
            .build();

        let (header_box, title_label, subtitle_label) = Self::build_header(&context);
        root_box.append(&header_box);

        let (dest_badge, src_badge) = determine_age_badges(context.relative_time);
        let cards_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(12)
            .homogeneous(true)
            .build();

        let dest_card = Self::build_card("Existing Item", &context.destination, dest_badge);
        let src_card = Self::build_card("Incoming Item", &context.source, src_badge);
        cards_box.append(&dest_card);
        cards_box.append(&src_card);
        root_box.append(&cards_box);

        let apply_to_all_check = gtk4::CheckButton::builder()
            .label("Apply to all")
            .halign(gtk4::Align::Start)
            .build();
        root_box.append(&apply_to_all_check);

        let (button_bar, buttons, stop_button, keep_both_button) =
            Self::build_button_bar(&context, &responder_arc, &apply_to_all_check, &window);
        root_box.append(&button_bar);

        window.set_content(Some(&root_box));

        let dialog = Self {
            window,
            title_label,
            subtitle_label,
            apply_to_all_check,
            buttons,
            stop_button,
            keep_both_button,
            context,
            responder: responder_arc,
            is_alt_held: is_alt_held_arc,
        };

        dialog.setup_keyboard_controller();
        dialog.setup_close_handler();

        dialog
    }

    fn build_header(context: &ConflictContext) -> (gtk4::Box, gtk4::Label, gtk4::Label) {
        let filename = context
            .destination
            .path
            .file_name()
            .map_or_else(|| "item".to_string(), |n| n.to_string_lossy().to_string());

        let title_text = format!("An item named \"{filename}\" already exists in this location.");
        let subtitle_text = if context.is_directory_collision {
            "Do you want to merge it with the existing directory, or replace it?".to_string()
        } else {
            "Do you want to replace it with the one you're moving?".to_string()
        };

        let header_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .build();

        let title_label = gtk4::Label::builder()
            .label(&title_text)
            .halign(gtk4::Align::Start)
            .wrap(true)
            .css_classes(["operations-progress-title"])
            .build();
        header_box.append(&title_label);

        let subtitle_label = gtk4::Label::builder()
            .label(&subtitle_text)
            .halign(gtk4::Align::Start)
            .wrap(true)
            .css_classes(["operations-progress-subtitle"])
            .build();
        header_box.append(&subtitle_label);

        (header_box, title_label, subtitle_label)
    }

    fn build_button_bar(
        context: &ConflictContext,
        responder_arc: &Arc<Mutex<Option<oneshot::Sender<ConflictResolution>>>>,
        apply_to_all_check: &gtk4::CheckButton,
        window: &libadwaita::Window,
    ) -> (
        gtk4::Box,
        Vec<gtk4::Button>,
        Option<gtk4::Button>,
        Option<gtk4::Button>,
    ) {
        let button_configs =
            determine_action_buttons(context.is_directory_collision, context.relative_time);
        let button_bar = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk4::Align::End)
            .build();

        let mut buttons = Vec::with_capacity(button_configs.len());
        let mut stop_button = None;
        let mut keep_both_button = None;

        for config in button_configs {
            let btn = gtk4::Button::builder().label(config.label).build();
            match config.style {
                ConflictButtonStyle::Suggested => btn.add_css_class("suggested-action"),
                ConflictButtonStyle::Destructive => btn.add_css_class("destructive-action"),
                ConflictButtonStyle::Flat => btn.add_css_class("flat"),
                ConflictButtonStyle::Default => {}
            }

            if config.action == ConflictAction::Stop {
                stop_button = Some(btn.clone());
            } else if config.action == ConflictAction::KeepBoth {
                keep_both_button = Some(btn.clone());
            }

            let resp_clone = Arc::clone(responder_arc);
            let check_clone = apply_to_all_check.clone();
            let win_clone = window.clone();
            let action = config.action;

            btn.connect_clicked(move |clicked_btn| {
                let apply_all = check_clone.is_active();
                let actual_action = if action == ConflictAction::Stop
                    && clicked_btn.label().as_deref() == Some("Skip")
                {
                    ConflictAction::Skip
                } else {
                    action
                };

                Self::resolve_with_responder(&resp_clone, actual_action, apply_all);
                win_clone.close();
            });

            button_bar.append(&btn);
            buttons.push(btn);
        }

        (button_bar, buttons, stop_button, keep_both_button)
    }

    fn build_card(header: &str, meta: &ConflictItemMeta, age_badge: RelativeAgeBadge) -> gtk4::Box {
        let card = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(8)
            .css_classes(["conflict-dialog-card"])
            .build();

        let header_label = gtk4::Label::builder()
            .label(header)
            .halign(gtk4::Align::Start)
            .css_classes(["operations-progress-title"])
            .build();
        card.append(&header_label);

        let body_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(12)
            .valign(gtk4::Align::Center)
            .build();

        // Thumbnail / Icon Preview (64x64)
        let icon_img = if let Some(ref thumb) = meta.thumbnail_path
            && thumb.exists()
        {
            gtk4::Image::from_file(thumb)
        } else {
            let icon_name = guess_icon_for_path(&meta.path, meta.is_dir);
            gtk4::Image::from_icon_name(icon_name)
        };
        icon_img.set_pixel_size(64);
        body_row.append(&icon_img);

        // Details
        let details_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .build();

        let name = meta
            .path
            .file_name()
            .map_or_else(|| "—".to_string(), |n| n.to_string_lossy().to_string());
        let name_label = gtk4::Label::builder()
            .label(&name)
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::Middle)
            .css_classes(["operations-progress-title"])
            .build();
        details_box.append(&name_label);

        let size_text = if meta.is_dir {
            "Folder".to_string()
        } else {
            gtk4::glib::format_size(meta.size_bytes).to_string()
        };
        let size_label = gtk4::Label::builder()
            .label(&size_text)
            .halign(gtk4::Align::Start)
            .css_classes(["operations-progress-metrics"])
            .build();
        details_box.append(&size_label);

        // Date and Relative Age Badge
        let date_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(6)
            .build();

        let date_text = format_modified_time(meta.modified);
        let date_label = gtk4::Label::builder()
            .label(&date_text)
            .halign(gtk4::Align::Start)
            .css_classes(["operations-progress-subtitle"])
            .build();
        date_box.append(&date_label);

        if let (Some(badge_text), Some(css_class)) = (age_badge.label(), age_badge.css_class()) {
            let badge = gtk4::Label::builder()
                .label(badge_text)
                .css_classes([css_class])
                .build();
            date_box.append(&badge);
        }
        details_box.append(&date_box);

        body_row.append(&details_box);
        card.append(&body_row);

        card
    }

    fn setup_keyboard_controller(&self) {
        let key_controller = gtk4::EventControllerKey::new();
        let dialog_self = self.clone();

        key_controller.connect_key_pressed(move |_, keyval, _keycode, _state| {
            if keyval == gtk4::gdk::Key::Escape {
                dialog_self.trigger_action(ConflictAction::Stop);
                dialog_self.window.close();
                return gtk4::glib::Propagation::Stop;
            }

            if keyval == gtk4::gdk::Key::Alt_L || keyval == gtk4::gdk::Key::Alt_R {
                dialog_self.set_alt_pressed(true);
                return gtk4::glib::Propagation::Proceed;
            }

            if keyval == gtk4::gdk::Key::Return || keyval == gtk4::gdk::Key::KP_Enter {
                // Trigger focused button or default primary action
                for btn in &dialog_self.buttons {
                    if btn.has_focus() {
                        btn.emit_clicked();
                        return gtk4::glib::Propagation::Stop;
                    }
                }
                if let Some(ref keep_both) = dialog_self.keep_both_button {
                    keep_both.emit_clicked();
                    return gtk4::glib::Propagation::Stop;
                }
            }

            gtk4::glib::Propagation::Proceed
        });

        let dialog_self_rel = self.clone();
        key_controller.connect_key_released(move |_, keyval, _keycode, _state| {
            if keyval == gtk4::gdk::Key::Alt_L || keyval == gtk4::gdk::Key::Alt_R {
                dialog_self_rel.set_alt_pressed(false);
            }
        });

        self.window.add_controller(key_controller);
    }

    fn setup_close_handler(&self) {
        let resp_arc = Arc::clone(&self.responder);
        self.window.connect_close_request(move |_| {
            Self::resolve_with_responder(&resp_arc, ConflictAction::Stop, false);
            gtk4::glib::Propagation::Proceed
        });
    }

    /// Sets whether the Alt/Option modifier key is pressed, dynamically altering Stop to Skip.
    pub fn set_alt_pressed(&self, pressed: bool) {
        {
            let mut guard = self
                .is_alt_held
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard = pressed;
        }

        if let Some(ref stop_btn) = self.stop_button {
            if pressed {
                stop_btn.set_label("Skip");
                if let Some(ref keep_both) = self.keep_both_button {
                    keep_both.grab_focus();
                }
            } else {
                stop_btn.set_label("Stop");
            }
        }
    }

    /// Triggers resolution for a specified [`ConflictAction`] and closes the dialog.
    pub fn trigger_action(&self, action: ConflictAction) {
        let apply_to_all = self.apply_to_all_check.is_active();
        Self::resolve_with_responder(&self.responder, action, apply_to_all);
        self.window.close();
    }

    fn resolve_with_responder(
        responder: &Arc<Mutex<Option<oneshot::Sender<ConflictResolution>>>>,
        action: ConflictAction,
        apply_to_all: bool,
    ) {
        let mut guard = responder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(sender) = guard.take() {
            let resolution = ConflictResolution::new(action, apply_to_all);
            let _ = sender.send(resolution);
        }
    }

    /// Attaches the conflict dialog as modal sheet to the parent window.
    pub fn attach_to_parent(&self, parent: &impl IsA<gtk4::Window>) {
        self.window.set_transient_for(Some(parent));
    }

    /// Displays the modal conflict dialog.
    pub fn present(&self) {
        self.window.present();
    }

    /// Closes the modal conflict dialog.
    pub fn close(&self) {
        self.window.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_age_badges() {
        let (dst, src) = determine_age_badges(RelativeTimeStatus::IncomingNewer);
        assert_eq!(dst, RelativeAgeBadge::Older);
        assert_eq!(src, RelativeAgeBadge::Newer);

        let (dst, src) = determine_age_badges(RelativeTimeStatus::IncomingOlder);
        assert_eq!(dst, RelativeAgeBadge::Newer);
        assert_eq!(src, RelativeAgeBadge::Older);

        let (dst, src) = determine_age_badges(RelativeTimeStatus::IdenticalTime);
        assert_eq!(dst, RelativeAgeBadge::None);
        assert_eq!(src, RelativeAgeBadge::None);
    }

    #[test]
    fn test_determine_action_buttons_file_collision() {
        let buttons_newer = determine_action_buttons(false, RelativeTimeStatus::IncomingNewer);
        assert_eq!(buttons_newer.len(), 4);
        assert_eq!(buttons_newer[0].action, ConflictAction::Replace);
        assert_eq!(buttons_newer[0].style, ConflictButtonStyle::Default);
        assert_eq!(buttons_newer[1].action, ConflictAction::KeepBoth);
        assert_eq!(buttons_newer[1].style, ConflictButtonStyle::Suggested);

        let buttons_older = determine_action_buttons(false, RelativeTimeStatus::IncomingOlder);
        assert_eq!(buttons_older[0].action, ConflictAction::Replace);
        assert_eq!(buttons_older[0].style, ConflictButtonStyle::Destructive);
    }

    #[test]
    fn test_determine_action_buttons_directory_collision() {
        let buttons = determine_action_buttons(true, RelativeTimeStatus::IncomingNewer);
        assert_eq!(buttons.len(), 5);
        assert_eq!(buttons[0].action, ConflictAction::Merge);
        assert_eq!(buttons[0].style, ConflictButtonStyle::Suggested);
        assert_eq!(buttons[1].action, ConflictAction::Replace);
        assert_eq!(buttons[1].style, ConflictButtonStyle::Destructive);
    }
}
