//! Specialized image viewport supporting pinch-to-zoom, scroll-wheel zoom, and fit toggling.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk;
use gtk4::prelude::*;

use crate::quicklook::types::ImagePreviewData;

/// Viewport widget for rendering raster and vector images with interactive zoom and pan.
#[derive(Debug, Clone)]
pub struct ImageViewport {
    /// Outer scrolled window providing pan navigation when zoomed in.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Center picture widget displaying the image raster.
    pub picture: gtk4::Picture,
    /// Current zoom scale factor (1.0 = 100% original size).
    pub zoom_factor: Rc<RefCell<f64>>,
    /// Original image dimensions `(width, height)`.
    pub original_size: Rc<RefCell<(u32, u32)>>,
    /// True when fitted to window bounding box, false when explicit zoom factor is applied.
    pub is_fit_to_window: Rc<RefCell<bool>>,
    /// Current image file path.
    pub current_path: Rc<RefCell<Option<PathBuf>>>,
}

impl Default for ImageViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageViewport {
    pub const MIN_ZOOM: f64 = 0.1;
    pub const MAX_ZOOM: f64 = 10.0;

    /// Constructs a new [`ImageViewport`].
    #[must_use]
    pub fn new() -> Self {
        let picture = gtk4::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk4::ContentFit::Contain)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .hexpand(true)
            .vexpand(true)
            .build();

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&picture)
            .build();

        let zoom_factor = Rc::new(RefCell::new(1.0));
        let original_size = Rc::new(RefCell::new((0, 0)));
        let is_fit_to_window = Rc::new(RefCell::new(true));
        let current_path = Rc::new(RefCell::new(None));

        let viewport = Self {
            scrolled_window,
            picture,
            zoom_factor,
            original_size,
            is_fit_to_window,
            current_path,
        };

        viewport.wire_zoom_gestures();
        viewport
    }

    /// Returns the root widget to embed in containers.
    #[must_use]
    pub const fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scrolled_window
    }

    /// Loads preview data from [`ImagePreviewData`].
    pub fn load_data(&self, data: &ImagePreviewData) {
        self.set_image(&data.image_path, data.width, data.height);
    }

    /// Sets the displayed image path and dimensions.
    pub fn set_image(&self, path: &Path, width: u32, height: u32) {
        *self.current_path.borrow_mut() = Some(path.to_path_buf());
        *self.original_size.borrow_mut() = (width, height);
        *self.zoom_factor.borrow_mut() = 1.0;
        *self.is_fit_to_window.borrow_mut() = true;

        let file = gio::File::for_path(path);
        self.picture.set_file(Some(&file));
        self.apply_zoom_mode();
    }

    /// Toggles between "Fit to Window" and "100% Original Size".
    pub fn toggle_fit_or_original(&self) {
        let mut fit = self.is_fit_to_window.borrow_mut();
        *fit = !*fit;
        if *fit {
            *self.zoom_factor.borrow_mut() = 1.0;
        }
        drop(fit);
        self.apply_zoom_mode();
    }

    /// Sets an explicit zoom factor, clamped to bounds.
    pub fn set_zoom_factor(&self, factor: f64) {
        let clamped = factor.clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        *self.zoom_factor.borrow_mut() = clamped;
        *self.is_fit_to_window.borrow_mut() = false;
        self.apply_zoom_mode();
    }

    /// Applies the current zoom state to the underlying `GtkPicture`.
    #[allow(clippy::cast_possible_truncation)]
    fn apply_zoom_mode(&self) {
        let fit = *self.is_fit_to_window.borrow();
        let (orig_w, orig_h) = *self.original_size.borrow();

        if fit || orig_w == 0 || orig_h == 0 {
            self.picture.set_can_shrink(true);
            self.picture.set_content_fit(gtk4::ContentFit::Contain);
            self.picture.set_size_request(-1, -1);
        } else {
            let scale = *self.zoom_factor.borrow();
            let target_w = (f64::from(orig_w) * scale).round() as i32;
            let target_h = (f64::from(orig_h) * scale).round() as i32;

            self.picture.set_can_shrink(false);
            self.picture.set_content_fit(gtk4::ContentFit::Fill);
            self.picture.set_size_request(target_w, target_h);
        }
    }

    /// Connects pinch-to-zoom, scroll-wheel zoom, and double-click gestures.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    fn wire_zoom_gestures(&self) {
        // 1. Double-click toggles Fit to Window vs 100% Original Size
        let click_gesture = gtk4::GestureClick::builder()
            .button(gdk::BUTTON_PRIMARY)
            .build();

        let pic_weak = self.picture.downgrade();
        let fit_flag = Rc::clone(&self.is_fit_to_window);
        let zoom_state = Rc::clone(&self.zoom_factor);
        let size_state = Rc::clone(&self.original_size);

        click_gesture.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2
                && let Some(pic) = pic_weak.upgrade()
            {
                let mut fit = fit_flag.borrow_mut();
                *fit = !*fit;
                let (orig_w, orig_h) = *size_state.borrow();

                if *fit || orig_w == 0 || orig_h == 0 {
                    pic.set_can_shrink(true);
                    pic.set_content_fit(gtk4::ContentFit::Contain);
                    pic.set_size_request(-1, -1);
                } else {
                    *zoom_state.borrow_mut() = 1.0;
                    pic.set_can_shrink(false);
                    pic.set_content_fit(gtk4::ContentFit::Fill);
                    pic.set_size_request(orig_w.cast_signed(), orig_h.cast_signed());
                }
            }
        });
        self.scrolled_window.add_controller(click_gesture);

        // 2. Pinch gesture (touchpad / touchscreen zoom)
        let pinch_gesture = gtk4::GestureZoom::new();
        let pic_weak_pinch = self.picture.downgrade();
        let fit_pinch = Rc::clone(&self.is_fit_to_window);
        let zoom_pinch = Rc::clone(&self.zoom_factor);
        let size_pinch = Rc::clone(&self.original_size);

        pinch_gesture.connect_scale_changed(move |gesture, scale| {
            if let Some(pic) = pic_weak_pinch.upgrade() {
                let (orig_w, orig_h) = *size_pinch.borrow();
                if orig_w > 0 && orig_h > 0 {
                    *fit_pinch.borrow_mut() = false;
                    let current = *zoom_pinch.borrow();
                    let new_zoom = (current * scale).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
                    *zoom_pinch.borrow_mut() = new_zoom;

                    let target_w = (f64::from(orig_w) * new_zoom).round() as i32;
                    let target_h = (f64::from(orig_h) * new_zoom).round() as i32;

                    pic.set_can_shrink(false);
                    pic.set_content_fit(gtk4::ContentFit::Fill);
                    pic.set_size_request(target_w, target_h);
                }
            }
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        self.scrolled_window.add_controller(pinch_gesture);

        // 3. Scroll wheel with Ctrl (Ctrl + Scroll Up/Down)
        let scroll_controller =
            gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        let pic_weak_scroll = self.picture.downgrade();
        let fit_scroll = Rc::clone(&self.is_fit_to_window);
        let zoom_scroll = Rc::clone(&self.zoom_factor);
        let size_scroll = Rc::clone(&self.original_size);

        scroll_controller.connect_scroll(move |ctrl, _, dy| {
            let modifiers = ctrl.current_event_state();
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                if let Some(pic) = pic_weak_scroll.upgrade() {
                    let (orig_w, orig_h) = *size_scroll.borrow();
                    if orig_w > 0 && orig_h > 0 {
                        *fit_scroll.borrow_mut() = false;
                        let delta = if dy < 0.0 { 1.15 } else { 0.85 };
                        let current = *zoom_scroll.borrow();
                        let new_zoom = (current * delta).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
                        *zoom_scroll.borrow_mut() = new_zoom;

                        let target_w = (f64::from(orig_w) * new_zoom).round() as i32;
                        let target_h = (f64::from(orig_h) * new_zoom).round() as i32;

                        pic.set_can_shrink(false);
                        pic.set_content_fit(gtk4::ContentFit::Fill);
                        pic.set_size_request(target_w, target_h);
                    }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.scrolled_window.add_controller(scroll_controller);
    }
}
