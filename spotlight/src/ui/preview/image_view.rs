//! High-resolution image and vector PDF page preview component.
//!
//! Renders image thumbnails and PDF page 1 within a styled 220x158px card container
//! conforming to the `.thumb` elevation and border-radius specifications.

use std::fmt;
use std::path::Path;

use gtk4::prelude::*;

/// Fixed thumbnail card dimensions matching design specifications.
pub const THUMB_WIDTH: i32 = 220;
pub const THUMB_HEIGHT: i32 = 158;

/// Image and PDF thumbnail viewer widget.
#[derive(Clone)]
pub struct ImagePreviewWidget {
    /// Outer container box with `.thumb` styling.
    pub container: gtk4::Box,
    /// GTK Picture widget displaying scaled thumbnail image.
    pub picture: gtk4::Picture,
    /// Fallback icon widget displayed when image is unavailable.
    pub fallback_icon: gtk4::Image,
    /// Current image path if loaded.
    current_path: std::rc::Rc<std::cell::RefCell<Option<std::path::PathBuf>>>,
}

impl fmt::Debug for ImagePreviewWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImagePreviewWidget")
            .field("container", &self.container)
            .field("current_path", &self.current_path.borrow())
            .finish_non_exhaustive()
    }
}

impl Default for ImagePreviewWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl ImagePreviewWidget {
    /// Constructs a new [`ImagePreviewWidget`].
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("thumb");
        container.set_size_request(THUMB_WIDTH, THUMB_HEIGHT);
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.set_overflow(gtk4::Overflow::Hidden);

        let picture = gtk4::Picture::new();
        picture.set_can_shrink(true);
        picture.set_content_fit(gtk4::ContentFit::Contain);
        picture.set_size_request(THUMB_WIDTH, THUMB_HEIGHT);
        picture.set_halign(gtk4::Align::Center);
        picture.set_valign(gtk4::Align::Center);

        let fallback_icon = gtk4::Image::from_icon_name("image-x-generic");
        fallback_icon.set_pixel_size(64);
        fallback_icon.set_halign(gtk4::Align::Center);
        fallback_icon.set_valign(gtk4::Align::Center);
        fallback_icon.set_size_request(THUMB_WIDTH, THUMB_HEIGHT);
        fallback_icon.set_visible(false);

        container.append(&picture);
        container.append(&fallback_icon);

        Self {
            container,
            picture,
            fallback_icon,
            current_path: std::rc::Rc::new(std::cell::RefCell::new(None)),
        }
    }

    /// Loads and displays an image from the given filesystem path.
    pub fn set_image_path(&self, path: &Path) {
        *self.current_path.borrow_mut() = Some(path.to_path_buf());
        self.picture.set_filename(Some(path));
        self.picture.set_visible(true);
        self.fallback_icon.set_visible(false);
    }

    /// Displays a themed symbolic fallback icon instead of an image.
    pub fn set_fallback_icon(&self, icon_name: &str) {
        *self.current_path.borrow_mut() = None;
        self.fallback_icon.set_icon_name(Some(icon_name));
        self.picture.set_paintable(gtk4::gdk::Paintable::NONE);
        self.picture.set_visible(false);
        self.fallback_icon.set_visible(true);
    }

    /// Clears the image view.
    pub fn clear(&self) {
        *self.current_path.borrow_mut() = None;
        self.picture.set_paintable(gtk4::gdk::Paintable::NONE);
        self.picture.set_visible(false);
        self.fallback_icon.set_visible(false);
    }

    /// Returns the currently displayed image path if any.
    #[must_use]
    pub fn current_path(&self) -> Option<std::path::PathBuf> {
        self.current_path.borrow().clone()
    }
}
