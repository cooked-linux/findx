//! Quick Look floating preview window shell, header bar, and viewport coordinator.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gdk4::prelude::*;
use gtk4::prelude::*;
use libadwaita::prelude::*;

use crate::quicklook::QuickLookEngine;
use crate::quicklook::types::PreviewPayload;
use crate::ui::quicklook::carousel::{IndexSheetPopover, QuickLookCarousel};
use crate::ui::quicklook::prefs::QuickLookConfig;
use crate::ui::quicklook::theme::install_quicklook_theme;
use crate::ui::quicklook::viewports::{
    FallbackViewport, ImageViewport, MediaViewport, PdfViewport, TextViewport,
};

/// Top-level floating Quick Look preview window wrapping [`libadwaita::Window`].
#[derive(Clone)]
pub struct QuickLookWindow {
    /// Underlying top-level libadwaita window.
    pub window: libadwaita::Window,
    /// Root overlay allowing header to float smoothly over viewports.
    pub overlay: gtk4::Overlay,
    /// Header revealer supporting smooth slide/fade in fullscreen.
    pub header_revealer: gtk4::Revealer,
    /// Draggable header bar container.
    pub header_box: gtk4::Box,
    /// Previous item navigation button `[◀]`.
    pub btn_prev: gtk4::Button,
    /// Next item navigation button `[▶]`.
    pub btn_next: gtk4::Button,
    /// Item position counter label (e.g. `"1 of 6"`).
    pub counter_label: gtk4::Label,
    /// Centered filename label with middle ellipsis truncation.
    pub title_label: gtk4::Label,
    /// "Open with <App>" action button.
    pub btn_open: gtk4::Button,
    /// Index sheet mosaic grid toggle button `[⊞]`.
    pub btn_index_sheet: gtk4::Button,
    /// Fullscreen presentation toggle button `[⛶]`.
    pub btn_fullscreen: gtk4::Button,
    /// Close window button `[✕]`.
    pub btn_close: gtk4::Button,
    /// Main content stack supporting crossfade transitions between specialized viewports.
    pub stack: gtk4::Stack,
    /// Specialized raster/vector image viewport.
    pub image_viewport: ImageViewport,
    /// Specialized syntax-highlighted text/code viewport.
    pub text_viewport: TextViewport,
    /// Specialized vector PDF document viewport.
    pub pdf_viewport: PdfViewport,
    /// Specialized video/audio media viewport.
    pub media_viewport: MediaViewport,
    /// Specialized fallback metadata card viewport.
    pub fallback_viewport: FallbackViewport,
    /// Viewport container box inside the overlay.
    pub viewport_box: gtk4::Box,
    /// Internal fullscreen tracking state.
    pub is_fullscreen: Rc<RefCell<bool>>,
    /// Multi-file selection carousel tracker.
    pub carousel: Rc<RefCell<QuickLookCarousel>>,
    /// Index sheet mosaic thumbnail popover.
    pub index_sheet: IndexSheetPopover,
    /// Quick Look generation engine reference.
    pub engine: Arc<QuickLookEngine>,
    /// User preferences and window geometry config.
    pub config: Rc<RefCell<QuickLookConfig>>,
    /// Timer handle for auto-hiding the header in fullscreen mode.
    hide_header_timer_source: Rc<RefCell<Option<glib::SourceId>>>,
}

impl std::fmt::Debug for QuickLookWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickLookWindow")
            .field("is_fullscreen", &self.is_fullscreen)
            .field("carousel", &self.carousel)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl QuickLookWindow {
    /// Default window dimensions.
    pub const DEFAULT_WIDTH: i32 = 800;
    pub const DEFAULT_HEIGHT: i32 = 600;
    pub const HEADER_HEIGHT: i32 = 44;

    /// Constructs a new [`QuickLookWindow`].
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn new(parent: Option<&impl IsA<gtk4::Window>>, engine: Arc<QuickLookEngine>) -> Self {
        install_quicklook_theme();

        let config = QuickLookConfig::load();
        let (initial_w, initial_h) = (config.window_width, config.window_height);

        let window = libadwaita::Window::builder()
            .title("Quick Look")
            .default_width(initial_w)
            .default_height(initial_h)
            .css_classes(["quicklook-window"])
            .build();

        if let Some(parent_win) = parent {
            window.set_transient_for(Some(parent_win));
        }

        // Clamp window dimensions to display work area if monitor info is available
        if let Some(display) = gdk4::Display::default() {
            let monitors = display.monitors();
            if let Some(obj) = monitors.item(0)
                && let Ok(monitor) = obj.downcast::<gdk4::Monitor>()
            {
                let geom = monitor.geometry();
                let (clamped_w, clamped_h) = QuickLookConfig::clamp_dimensions(
                    initial_w,
                    initial_h,
                    geom.width(),
                    geom.height(),
                );
                window.set_default_size(clamped_w, clamped_h);
            }
        }

        // --- Left Header Navigation: Prev, Next, Counter ---
        let nav_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(4)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::Start)
            .build();

        let btn_prev = gtk4::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Previous item (Left Arrow)")
            .css_classes(["flat", "circular"])
            .sensitive(false)
            .build();

        let btn_next = gtk4::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text("Next item (Right Arrow)")
            .css_classes(["flat", "circular"])
            .sensitive(false)
            .build();

        let counter_label = gtk4::Label::builder()
            .label("")
            .css_classes(["quicklook-counter"])
            .valign(gtk4::Align::Center)
            .visible(false)
            .build();

        nav_box.append(&btn_prev);
        nav_box.append(&btn_next);
        nav_box.append(&counter_label);

        // --- Center Title ---
        let title_label = gtk4::Label::builder()
            .label("Quick Look")
            .ellipsize(gtk4::pango::EllipsizeMode::Middle)
            .halign(gtk4::Align::Center)
            .hexpand(true)
            .css_classes(["quicklook-title"])
            .build();

        // --- Right Action Buttons ---
        let actions_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(4)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::End)
            .build();

        let btn_open = gtk4::Button::builder()
            .label("Open")
            .icon_name("document-open-symbolic")
            .tooltip_text("Open with default application")
            .css_classes(["flat"])
            .build();

        let btn_index_sheet = gtk4::Button::builder()
            .icon_name("view-grid-symbolic")
            .tooltip_text("Index Sheet Mosaic")
            .css_classes(["flat", "circular"])
            .sensitive(false)
            .build();

        let btn_fullscreen = gtk4::Button::builder()
            .icon_name("view-fullscreen-symbolic")
            .tooltip_text("Toggle Fullscreen (F11)")
            .css_classes(["flat", "circular"])
            .build();

        let btn_close = gtk4::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Close (Esc or Space)")
            .css_classes(["flat", "circular"])
            .build();

        actions_box.append(&btn_open);
        actions_box.append(&btn_index_sheet);
        actions_box.append(&btn_fullscreen);
        actions_box.append(&btn_close);

        // --- Assembled Header Bar wrapped in draggable WindowHandle ---
        let header_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .css_classes(["quicklook-header"])
            .hexpand(true)
            .spacing(8)
            .build();

        header_box.append(&nav_box);
        header_box.append(&title_label);
        header_box.append(&actions_box);

        let window_handle = gtk4::WindowHandle::builder().child(&header_box).build();

        let header_revealer = gtk4::Revealer::builder()
            .child(&window_handle)
            .transition_type(gtk4::RevealerTransitionType::SlideDown)
            .reveal_child(true)
            .valign(gtk4::Align::Start)
            .halign(gtk4::Align::Fill)
            .build();

        // --- Viewports Stack ---
        let stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .transition_duration(200)
            .hexpand(true)
            .vexpand(true)
            .build();

        let image_viewport = ImageViewport::new();
        let text_viewport = TextViewport::new();
        let pdf_viewport = PdfViewport::new();
        let media_viewport = MediaViewport::new();
        let fallback_viewport = FallbackViewport::new();

        media_viewport.set_volume(config.volume_level);

        stack.add_named(image_viewport.widget(), Some("image"));
        stack.add_named(text_viewport.widget(), Some("code"));
        stack.add_named(pdf_viewport.widget(), Some("pdf"));
        stack.add_named(media_viewport.widget(), Some("media"));
        stack.add_named(fallback_viewport.widget(), Some("fallback"));

        let viewport_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .css_classes(["quicklook-viewport-container"])
            .hexpand(true)
            .vexpand(true)
            .margin_top(Self::HEADER_HEIGHT)
            .build();
        viewport_box.append(&stack);

        // --- Root Overlay ---
        let overlay = gtk4::Overlay::builder()
            .child(&viewport_box)
            .hexpand(true)
            .vexpand(true)
            .build();

        overlay.add_overlay(&header_revealer);

        window.set_content(Some(&overlay));

        let is_fullscreen = Rc::new(RefCell::new(false));
        let carousel = Rc::new(RefCell::new(QuickLookCarousel::new()));
        let index_sheet = IndexSheetPopover::new();
        index_sheet.popover.set_parent(&btn_index_sheet);

        let config_rc = Rc::new(RefCell::new(config));
        let hide_header_timer_source = Rc::new(RefCell::new(None));

        let ql_window = Self {
            window,
            overlay,
            header_revealer,
            header_box,
            btn_prev,
            btn_next,
            counter_label,
            title_label,
            btn_open,
            btn_index_sheet,
            btn_fullscreen,
            btn_close,
            stack,
            image_viewport,
            text_viewport,
            pdf_viewport,
            media_viewport,
            fallback_viewport,
            viewport_box,
            is_fullscreen,
            carousel,
            index_sheet,
            engine,
            config: config_rc,
            hide_header_timer_source,
        };

        ql_window.wire_core_signals();
        ql_window.wire_carousel_signals();
        ql_window.wire_fullscreen_motion();
        ql_window.wire_persistence_signals();

        ql_window
    }

    /// Wires basic window signals: close button, esc/space/arrow key bindings, and fullscreen toggle.
    fn wire_core_signals(&self) {
        let win_clone = self.window.clone();
        self.btn_close.connect_clicked(move |_| {
            win_clone.close();
        });

        // Fullscreen toggle button
        let win_weak = self.window.downgrade();
        let fs_state = Rc::clone(&self.is_fullscreen);
        let header_rev = self.header_revealer.clone();
        let view_box = self.viewport_box.clone();
        let btn_fs = self.btn_fullscreen.clone();
        let timer_source = Rc::clone(&self.hide_header_timer_source);

        self.btn_fullscreen.connect_clicked(move |_| {
            if let Some(win) = win_weak.upgrade() {
                let mut fs = fs_state.borrow_mut();
                *fs = !*fs;
                if *fs {
                    win.add_css_class("quicklook-fullscreen");
                    view_box.set_margin_top(0);
                    btn_fs.set_icon_name("view-restore-symbolic");
                    btn_fs.set_tooltip_text(Some("Exit Fullscreen (F11 / Esc)"));
                    win.fullscreen();
                } else {
                    if let Some(src) = timer_source.borrow_mut().take() {
                        src.remove();
                    }
                    win.remove_css_class("quicklook-fullscreen");
                    view_box.set_margin_top(Self::HEADER_HEIGHT);
                    header_rev.set_reveal_child(true);
                    btn_fs.set_icon_name("view-fullscreen-symbolic");
                    btn_fs.set_tooltip_text(Some("Toggle Fullscreen (F11)"));
                    win.unfullscreen();
                }
            }
        });

        // Key event controller for Escape, Space, F11, Left, Right
        let key_controller = gtk4::EventControllerKey::new();
        let win_weak2 = self.window.downgrade();
        let fs_state2 = Rc::clone(&self.is_fullscreen);
        let btn_fs_click = self.btn_fullscreen.clone();
        let btn_prev_click = self.btn_prev.clone();
        let btn_next_click = self.btn_next.clone();

        key_controller.connect_key_pressed(move |_, keyval, _, _| {
            if let Some(win) = win_weak2.upgrade() {
                match keyval {
                    gdk4::Key::Escape => {
                        if *fs_state2.borrow() {
                            btn_fs_click.emit_clicked();
                        } else {
                            win.close();
                        }
                        return glib::Propagation::Stop;
                    }
                    gdk4::Key::space => {
                        win.close();
                        return glib::Propagation::Stop;
                    }
                    gdk4::Key::F11 => {
                        btn_fs_click.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    gdk4::Key::Left if btn_prev_click.is_sensitive() => {
                        btn_prev_click.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    gdk4::Key::Right if btn_next_click.is_sensitive() => {
                        btn_next_click.emit_clicked();
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });

        self.window.add_controller(key_controller);
    }

    /// Wires carousel previous, next, and index sheet buttons.
    fn wire_carousel_signals(&self) {
        // Prev button
        let ql_prev = self.clone();
        self.btn_prev.connect_clicked(move |_| {
            let next_path = ql_prev.carousel.borrow_mut().previous().cloned();
            if let Some(path) = next_path {
                ql_prev.update_carousel_ui();
                ql_prev.load_active_preview(&path);
            }
        });

        // Next button
        let ql_next = self.clone();
        self.btn_next.connect_clicked(move |_| {
            let next_path = ql_next.carousel.borrow_mut().next().cloned();
            if let Some(path) = next_path {
                ql_next.update_carousel_ui();
                ql_next.load_active_preview(&path);
            }
        });

        // Index Sheet toggle button
        let ql_sheet = self.clone();
        self.btn_index_sheet.connect_clicked(move |_| {
            let car = ql_sheet.carousel.borrow();
            ql_sheet.index_sheet.populate(
                car.paths(),
                car.current_index(),
                Some(ql_sheet.engine.thumbnail_cache()),
            );
            ql_sheet.index_sheet.popover.popup();
        });

        // Item selected in Index Sheet
        let ql_item_sel = self.clone();
        self.index_sheet.connect_item_selected(move |index| {
            let target_path = ql_item_sel.carousel.borrow_mut().jump_to(index).cloned();
            if let Some(path) = target_path {
                ql_item_sel.update_carousel_ui();
                ql_item_sel.load_active_preview(&path);
            }
        });

        // "Open with <App>" button
        let car_open = Rc::clone(&self.carousel);
        self.btn_open.connect_clicked(move |_| {
            if let Some(path) = car_open.borrow().current_path() {
                let file = gio::File::for_path(path);
                let uri = file.uri();
                let _ = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
            }
        });
    }

    /// Connects motion controller on overlay to auto-hide floating header in fullscreen mode.
    fn wire_fullscreen_motion(&self) {
        let motion = gtk4::EventControllerMotion::new();
        let rev_weak = self.header_revealer.downgrade();
        let fs_flag = Rc::clone(&self.is_fullscreen);
        let timer_slot = Rc::clone(&self.hide_header_timer_source);

        motion.connect_motion(move |_, _, _| {
            if *fs_flag.borrow() {
                if let Some(src) = timer_slot.borrow_mut().take() {
                    src.remove();
                }
                if let Some(rev) = rev_weak.upgrade() {
                    rev.set_reveal_child(true);
                }
                let rev_timer = rev_weak.clone();
                let fs_timer = Rc::clone(&fs_flag);
                let slot_timer = Rc::clone(&timer_slot);
                let src_id = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(2500),
                    move || {
                        *slot_timer.borrow_mut() = None;
                        if *fs_timer.borrow()
                            && let Some(r) = rev_timer.upgrade()
                        {
                            r.set_reveal_child(false);
                        }
                    },
                );
                *timer_slot.borrow_mut() = Some(src_id);
            }
        });

        self.overlay.add_controller(motion);
    }

    /// Connects window close signal to persist dimensions and preferences to JSON asynchronously.
    fn wire_persistence_signals(&self) {
        let win_weak = self.window.downgrade();
        let config_rc = Rc::clone(&self.config);
        let media_vol = Rc::clone(&self.media_viewport.volume_level);
        let fs_state = Rc::clone(&self.is_fullscreen);

        self.window.connect_close_request(move |_| {
            if let Some(win) = win_weak.upgrade() {
                let (w, h) = win.default_size();
                let mut conf = config_rc.borrow_mut();
                conf.window_width = w;
                conf.window_height = h;
                conf.volume_level = *media_vol.borrow();
                conf.is_fullscreen = *fs_state.borrow();
                conf.save_async();
            }
            glib::Propagation::Proceed
        });
    }

    /// Updates header navigation counter label and button sensitivity.
    pub fn update_carousel_ui(&self) {
        let car = self.carousel.borrow();
        let total = car.len();
        let current_1_indexed = car.current_index() + 1;

        if total <= 1 {
            self.counter_label.set_visible(false);
            self.btn_prev.set_sensitive(false);
            self.btn_next.set_sensitive(false);
            self.btn_index_sheet.set_sensitive(false);
        } else {
            self.counter_label
                .set_label(&format!("{current_1_indexed} of {total}"));
            self.counter_label.set_visible(true);
            self.btn_prev.set_sensitive(true);
            self.btn_next.set_sensitive(true);
            self.btn_index_sheet.set_sensitive(true);
        }
    }

    /// Starts asynchronous preview loading for the active filesystem path.
    pub fn load_active_preview(&self, path: &Path) {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("File");
        self.set_title_text(file_name);

        // Update "Open with <App>" button
        let mime = shared::mime::detect_mime_type(path)
            .unwrap_or_else(|_| "application/octet-stream".to_string());
        if let Some(app_info) = gio::AppInfo::default_for_type(&mime, false) {
            let app_name = app_info.name().to_string();
            let icon = app_info.icon().and_then(|i| i.to_string());
            self.set_open_app(&app_name, icon.as_deref());
        } else {
            self.btn_open.set_label("Open");
            self.btn_open.set_icon_name("document-open-symbolic");
        }

        // Fast-path thumbnail resolution (<10ms)
        if let Some(fast_thumb) = self.engine.get_fast_path_thumbnail(path, Some(&mime)) {
            let file = gio::File::for_path(&fast_thumb);
            self.image_viewport.picture.set_file(Some(&file));
            self.stack.set_visible_child_name("image");
        }

        // Asynchronous full generator preview pipeline
        let engine = Arc::clone(&self.engine);
        let ql_weak = self.window.downgrade();
        let target_path = path.to_path_buf();
        let ql_self = self.clone();

        glib::MainContext::default().spawn_local(async move {
            if let Ok(payload) = engine.load_preview(&target_path, (800, 600)).await
                && ql_weak.upgrade().is_some()
                && ql_self.carousel.borrow().current_path() == Some(&target_path)
            {
                ql_self.display_payload(&payload);
            }
        });
    }

    /// Initiates a multi-file preview session and displays the floating window.
    pub fn show_preview(&self, files: Vec<PathBuf>, active_index: usize) {
        if files.is_empty() {
            return;
        }

        self.carousel.borrow_mut().set_files(files, active_index);
        self.update_carousel_ui();

        if let Some(path) = self.carousel.borrow().current_path().cloned() {
            self.load_active_preview(&path);
        }

        self.window.present();
    }

    /// Seamlessly shifts the preview to a new path (e.g. when arrowing through files in Finder).
    pub fn update_active_file(&self, path: &Path) {
        let mut car = self.carousel.borrow_mut();
        if let Some(pos) = car.paths().iter().position(|p| p == path) {
            let _ = car.jump_to(pos);
        } else {
            car.set_files(vec![path.to_path_buf()], 0);
        }
        drop(car);

        self.update_carousel_ui();
        self.load_active_preview(path);
    }

    /// Sets the preview title in the header bar.
    pub fn set_title_text(&self, title: &str) {
        self.title_label.set_label(title);
        self.window.set_title(Some(title));
    }

    /// Updates the multi-file counter display (e.g. `"1 of 6"`).
    pub fn set_counter(&self, current_1_indexed: usize, total: usize) {
        if total <= 1 {
            self.counter_label.set_visible(false);
            self.btn_prev.set_sensitive(false);
            self.btn_next.set_sensitive(false);
            self.btn_index_sheet.set_sensitive(false);
        } else {
            self.counter_label
                .set_label(&format!("{current_1_indexed} of {total}"));
            self.counter_label.set_visible(true);
            self.btn_prev.set_sensitive(true);
            self.btn_next.set_sensitive(true);
            self.btn_index_sheet.set_sensitive(true);
        }
    }

    /// Configures the "Open with <App>" action button.
    pub fn set_open_app(&self, app_name: &str, icon_name: Option<&str>) {
        self.btn_open.set_label(&format!("Open with {app_name}"));
        if let Some(icon) = icon_name {
            self.btn_open.set_icon_name(icon);
        }
        self.btn_open.set_visible(true);
    }

    /// Displays a preview payload by selecting the corresponding specialized viewport.
    pub fn display_payload(&self, payload: &PreviewPayload) {
        match payload {
            PreviewPayload::Image(data) => {
                self.image_viewport.load_data(data);
                self.stack.set_visible_child_name("image");
            }
            PreviewPayload::SourceCode(data) => {
                self.text_viewport.load_data(data);
                self.stack.set_visible_child_name("code");
            }
            PreviewPayload::Pdf(data) => {
                self.pdf_viewport.load_data(data);
                self.stack.set_visible_child_name("pdf");
            }
            PreviewPayload::Media(data) => {
                self.media_viewport.load_data(data);
                self.stack.set_visible_child_name("media");
            }
            PreviewPayload::FallbackCard(data) => {
                self.fallback_viewport.load_data(data);
                self.stack.set_visible_child_name("fallback");
            }
        }
    }

    /// Hides the Quick Look window.
    pub fn dismiss(&self) {
        self.window.close();
    }
}
