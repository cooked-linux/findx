//! Specialized video and audio media playback viewport with transport controls.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::quicklook::types::MediaPreviewData;

/// Formats seconds into standard media timestamp format `MM:SS` or `HH:MM:SS`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn format_media_duration(total_seconds: f64) -> String {
    let s = total_seconds.max(0.0).round() as u64;
    let hours = s / 3600;
    let mins = (s % 3600) / 60;
    let secs = s % 60;

    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

/// Viewport widget rendering audio/video content with playback scrubber and transport bar.
#[derive(Debug, Clone)]
pub struct MediaViewport {
    /// Root overlay holding the media display canvas and bottom transport bar.
    pub overlay: gtk4::Overlay,
    /// Picture widget displaying video poster frame or album artwork.
    pub picture: gtk4::Picture,
    /// Fallback / audio placeholder container.
    pub audio_placeholder: gtk4::Box,
    /// Stack switching between video picture and audio placeholder.
    pub display_stack: gtk4::Stack,
    /// Bottom transport controls bar container.
    pub transport_box: gtk4::Box,
    /// Play / Pause toggle button.
    pub btn_play_pause: gtk4::Button,
    /// Interactive time seek scale.
    pub time_slider: gtk4::Scale,
    /// Current elapsed time position label.
    pub pos_label: gtk4::Label,
    /// Total duration label.
    pub dur_label: gtk4::Label,
    /// Volume menu button.
    pub btn_volume: gtk4::MenuButton,
    /// Volume level scale in popover.
    pub volume_slider: gtk4::Scale,
    /// Playing state tracker.
    pub is_playing: Rc<RefCell<bool>>,
    /// Total duration in seconds.
    pub duration_secs: Rc<RefCell<f64>>,
    /// Current playback position in seconds.
    pub position_secs: Rc<RefCell<f64>>,
    /// Volume level (0.0 to 1.0).
    pub volume_level: Rc<RefCell<f64>>,
}

impl Default for MediaViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaViewport {
    /// Constructs a new [`MediaViewport`].
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn new() -> Self {
        // Video Picture Canvas
        let picture = gtk4::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk4::ContentFit::Contain)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .hexpand(true)
            .vexpand(true)
            .build();

        // Audio Placeholder Canvas
        let audio_placeholder = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(16)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .hexpand(true)
            .vexpand(true)
            .build();

        let audio_icon = gtk4::Image::builder()
            .icon_name("audio-x-generic")
            .pixel_size(128)
            .halign(gtk4::Align::Center)
            .build();

        let audio_desc = gtk4::Label::builder()
            .label("Audio Track")
            .css_classes(["quicklook-title"])
            .halign(gtk4::Align::Center)
            .build();

        audio_placeholder.append(&audio_icon);
        audio_placeholder.append(&audio_desc);

        let display_stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .hexpand(true)
            .vexpand(true)
            .build();

        display_stack.add_named(&picture, Some("video"));
        display_stack.add_named(&audio_placeholder, Some("audio"));
        display_stack.set_visible_child_name("video");

        // --- Bottom Transport Controls Bar ---
        let transport_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(10)
            .css_classes(["quicklook-media-transport"])
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::End)
            .margin_bottom(24)
            .build();

        let btn_play_pause = gtk4::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .tooltip_text("Play / Pause (Space)")
            .css_classes(["flat", "circular"])
            .build();

        let pos_label = gtk4::Label::builder()
            .label("00:00")
            .css_classes(["quicklook-counter"])
            .valign(gtk4::Align::Center)
            .build();

        let time_slider = gtk4::Scale::with_range(gtk4::Orientation::Horizontal, 0.0, 100.0, 1.0);
        time_slider.set_hexpand(true);
        time_slider.set_width_request(280);
        time_slider.set_draw_value(false);

        let dur_label = gtk4::Label::builder()
            .label("00:00")
            .css_classes(["quicklook-counter"])
            .valign(gtk4::Align::Center)
            .build();

        // Volume button with popover
        let volume_popover = gtk4::Popover::builder().build();
        let volume_slider = gtk4::Scale::with_range(gtk4::Orientation::Vertical, 0.0, 1.0, 0.05);
        volume_slider.set_inverted(true);
        volume_slider.set_height_request(100);
        volume_slider.set_value(0.85);
        volume_slider.set_draw_value(false);
        volume_popover.set_child(Some(&volume_slider));

        let btn_volume = gtk4::MenuButton::builder()
            .icon_name("audio-volume-high-symbolic")
            .tooltip_text("Volume")
            .css_classes(["flat", "circular"])
            .popover(&volume_popover)
            .build();

        transport_box.append(&btn_play_pause);
        transport_box.append(&pos_label);
        transport_box.append(&time_slider);
        transport_box.append(&dur_label);
        transport_box.append(&btn_volume);

        let overlay = gtk4::Overlay::builder()
            .child(&display_stack)
            .hexpand(true)
            .vexpand(true)
            .build();

        overlay.add_overlay(&transport_box);

        let is_playing = Rc::new(RefCell::new(false));
        let duration_secs = Rc::new(RefCell::new(0.0));
        let position_secs = Rc::new(RefCell::new(0.0));
        let volume_level = Rc::new(RefCell::new(0.85));

        let viewport = Self {
            overlay,
            picture,
            audio_placeholder,
            display_stack,
            transport_box,
            btn_play_pause,
            time_slider,
            pos_label,
            dur_label,
            btn_volume,
            volume_slider,
            is_playing,
            duration_secs,
            position_secs,
            volume_level,
        };

        viewport.wire_controls();
        viewport
    }

    /// Returns the root widget to embed in containers.
    #[must_use]
    pub const fn widget(&self) -> &gtk4::Overlay {
        &self.overlay
    }

    /// Loads preview metadata from [`MediaPreviewData`].
    pub fn load_data(&self, data: &MediaPreviewData) {
        *self.duration_secs.borrow_mut() = data.duration_secs;
        *self.position_secs.borrow_mut() = 0.0;
        *self.is_playing.borrow_mut() = false;

        self.btn_play_pause
            .set_icon_name("media-playback-start-symbolic");
        self.dur_label
            .set_label(&format_media_duration(data.duration_secs));
        self.pos_label.set_label("00:00");

        self.time_slider.set_range(0.0, data.duration_secs.max(1.0));
        self.time_slider.set_value(0.0);

        if data.is_video {
            if let Some(ref poster) = data.poster_image {
                let file = gio::File::for_path(poster);
                self.picture.set_file(Some(&file));
            } else {
                self.picture.set_paintable(None::<&gdk4::Paintable>);
            }
            self.display_stack.set_visible_child_name("video");
        } else {
            self.display_stack.set_visible_child_name("audio");
        }
    }

    /// Toggles play / pause state.
    pub fn toggle_play_pause(&self) {
        let mut playing = self.is_playing.borrow_mut();
        *playing = !*playing;
        if *playing {
            self.btn_play_pause
                .set_icon_name("media-playback-pause-symbolic");
        } else {
            self.btn_play_pause
                .set_icon_name("media-playback-start-symbolic");
        }
    }

    /// Sets the current playback position in seconds.
    pub fn set_position(&self, position: f64) {
        let dur = *self.duration_secs.borrow();
        let clamped = position.clamp(0.0, dur);
        *self.position_secs.borrow_mut() = clamped;
        self.time_slider.set_value(clamped);
        self.pos_label.set_label(&format_media_duration(clamped));
    }

    /// Sets the volume level (0.0 to 1.0).
    pub fn set_volume(&self, volume: f64) {
        let clamped = volume.clamp(0.0, 1.0);
        *self.volume_level.borrow_mut() = clamped;
        self.volume_slider.set_value(clamped);

        let icon = if clamped <= 0.01 {
            "audio-volume-muted-symbolic"
        } else if clamped < 0.5 {
            "audio-volume-low-symbolic"
        } else {
            "audio-volume-high-symbolic"
        };
        self.btn_volume.set_icon_name(icon);
    }

    /// Connects transport UI signals.
    fn wire_controls(&self) {
        let vp_toggle = self.clone();
        self.btn_play_pause.connect_clicked(move |_| {
            vp_toggle.toggle_play_pause();
        });

        let vp_seek = self.clone();
        self.time_slider.connect_value_changed(move |scale| {
            let val = scale.value();
            *vp_seek.position_secs.borrow_mut() = val;
            vp_seek.pos_label.set_label(&format_media_duration(val));
        });

        let vp_vol = self.clone();
        self.volume_slider.connect_value_changed(move |scale| {
            vp_vol.set_volume(scale.value());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_media_duration() {
        assert_eq!(format_media_duration(0.0), "00:00");
        assert_eq!(format_media_duration(74.0), "01:14");
        assert_eq!(format_media_duration(225.0), "03:45");
        assert_eq!(format_media_duration(3665.0), "01:01:05");
    }
}
