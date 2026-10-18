//! Specialized multi-page vector PDF document viewport.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk;
use gtk4::prelude::*;

use crate::quicklook::types::PdfPreviewData;

/// Callback invoked when user requests flipping to a new 1-indexed page.
pub type PageChangeCallback = Box<dyn Fn(usize)>;

/// Viewport widget rendering PDF document pages with a floating page navigation pill overlay.
#[derive(Clone)]
pub struct PdfViewport {
    /// Root overlay holding the scrolled page raster and floating navigation pill.
    pub overlay: gtk4::Overlay,
    /// Center picture displaying the rendered raster page.
    pub picture: gtk4::Picture,
    /// Scrolled container for the page picture.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Floating pill container box.
    pub pill_box: gtk4::Box,
    /// Previous page button `[◀]`.
    pub btn_prev: gtk4::Button,
    /// Next page button `[▶]`.
    pub btn_next: gtk4::Button,
    /// Page counter label (e.g. `"Page 2 of 14"`).
    pub page_label: gtk4::Label,
    /// Current 1-indexed page number.
    pub current_page: Rc<RefCell<usize>>,
    /// Total page count.
    pub page_count: Rc<RefCell<usize>>,
    /// Currently loaded PDF file path.
    pub pdf_path: Rc<RefCell<Option<PathBuf>>>,
    /// Page change event listener.
    on_page_change: Rc<RefCell<Option<PageChangeCallback>>>,
}

impl std::fmt::Debug for PdfViewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfViewport")
            .field("current_page", &self.current_page)
            .field("page_count", &self.page_count)
            .field("pdf_path", &self.pdf_path)
            .finish_non_exhaustive()
    }
}

impl Default for PdfViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfViewport {
    /// Constructs a new [`PdfViewport`].
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

        // Floating pill navigation overlay: [◀] Page 1 of 14 [▶]
        let pill_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .css_classes(["quicklook-pill-overlay"])
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::End)
            .margin_bottom(20)
            .build();

        let btn_prev = gtk4::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Previous page (PageUp)")
            .css_classes(["flat", "circular"])
            .sensitive(false)
            .build();

        let page_label = gtk4::Label::builder()
            .label("Page 1 of 1")
            .valign(gtk4::Align::Center)
            .css_classes(["quicklook-counter"])
            .build();

        let btn_next = gtk4::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text("Next page (PageDown)")
            .css_classes(["flat", "circular"])
            .sensitive(false)
            .build();

        pill_box.append(&btn_prev);
        pill_box.append(&page_label);
        pill_box.append(&btn_next);

        let overlay = gtk4::Overlay::builder()
            .child(&scrolled_window)
            .hexpand(true)
            .vexpand(true)
            .build();

        overlay.add_overlay(&pill_box);

        let current_page = Rc::new(RefCell::new(1));
        let page_count = Rc::new(RefCell::new(1));
        let pdf_path = Rc::new(RefCell::new(None));
        let on_page_change = Rc::new(RefCell::new(None));

        let viewport = Self {
            overlay,
            picture,
            scrolled_window,
            pill_box,
            btn_prev,
            btn_next,
            page_label,
            current_page,
            page_count,
            pdf_path,
            on_page_change,
        };

        viewport.wire_controls();
        viewport
    }

    /// Returns the root widget to embed in containers.
    #[must_use]
    pub const fn widget(&self) -> &gtk4::Overlay {
        &self.overlay
    }

    /// Connects a callback invoked when the page changes.
    pub fn connect_page_changed<F: Fn(usize) + 'static>(&self, callback: F) {
        *self.on_page_change.borrow_mut() = Some(Box::new(callback));
    }

    /// Loads preview data from [`PdfPreviewData`].
    pub fn load_data(&self, data: &PdfPreviewData) {
        *self.current_page.borrow_mut() = data.current_page;
        *self.page_count.borrow_mut() = data.page_count;

        let file = gio::File::for_path(&data.rendered_page_path);
        self.picture.set_file(Some(&file));

        self.update_indicator();
    }

    /// Sets the page numbers and loads the page raster image directly.
    pub fn set_page(&self, current: usize, total: usize, image_path: Option<&Path>) {
        *self.current_page.borrow_mut() = current;
        *self.page_count.borrow_mut() = total;

        if let Some(path) = image_path {
            let file = gio::File::for_path(path);
            self.picture.set_file(Some(&file));
        }

        self.update_indicator();
    }

    /// Returns the current active 1-indexed page.
    #[must_use]
    pub fn current_page(&self) -> usize {
        *self.current_page.borrow()
    }

    /// Returns the total number of pages in the active document.
    #[must_use]
    pub fn page_count(&self) -> usize {
        *self.page_count.borrow()
    }

    /// Updates the label text and prev/next button sensitivity.
    fn update_indicator(&self) {
        let current = *self.current_page.borrow();
        let total = *self.page_count.borrow();

        self.page_label
            .set_label(&format!("Page {current} of {total}"));
        self.btn_prev.set_sensitive(current > 1);
        self.btn_next.set_sensitive(current < total);

        // Hide pill overlay if single-page document
        self.pill_box.set_visible(total > 1);
    }

    /// Advances to the next page if available.
    pub fn next_page(&self) {
        let current = *self.current_page.borrow();
        let total = *self.page_count.borrow();
        if current < total {
            let new_page = current + 1;
            *self.current_page.borrow_mut() = new_page;
            self.update_indicator();
            if let Some(ref cb) = *self.on_page_change.borrow() {
                cb(new_page);
            }
        }
    }

    /// Navigates to the previous page if available.
    pub fn previous_page(&self) {
        let current = *self.current_page.borrow();
        if current > 1 {
            let new_page = current - 1;
            *self.current_page.borrow_mut() = new_page;
            self.update_indicator();
            if let Some(ref cb) = *self.on_page_change.borrow() {
                cb(new_page);
            }
        }
    }

    /// Wires button clicks and keyboard event controllers.
    fn wire_controls(&self) {
        let vp_prev = self.clone();
        self.btn_prev.connect_clicked(move |_| {
            vp_prev.previous_page();
        });

        let vp_next = self.clone();
        self.btn_next.connect_clicked(move |_| {
            vp_next.next_page();
        });

        // Keyboard navigation: PageUp / PageDown
        let key_controller = gtk4::EventControllerKey::new();
        let vp_key = self.clone();

        key_controller.connect_key_pressed(move |_, keyval, _, _| match keyval {
            gdk::Key::Page_Up => {
                vp_key.previous_page();
                glib::Propagation::Stop
            }
            gdk::Key::Page_Down => {
                vp_key.next_page();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        self.overlay.add_controller(key_controller);
    }
}
