//! Spotlight window and overlay state management.

use gdk4::prelude::*;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

/// Initial default width of the Spotlight overlay window in pixels.
pub const INITIAL_WIDTH: i32 = 680;

/// Initial default height of the compact search-only overlay window in pixels.
pub const INITIAL_HEIGHT: i32 = 68;

/// Maximum vertical height allowed during search results expansion.
pub const MAX_HEIGHT: i32 = 580;

/// Standard height per item row in pixels.
pub const ROW_HEIGHT: i32 = 44;

/// Standard height for result section headers in pixels.
pub const HEADER_HEIGHT: i32 = 28;

/// Standard height of the bottom keyboard shortcut hint bar in pixels.
pub const FOOTER_HINT_HEIGHT: i32 = 36;

/// Default vertical margin percentage from top edge (18%).
pub const DEFAULT_TOP_MARGIN_PERCENT: i32 = 18;

/// Default animation duration for height transitions in milliseconds.
pub const ANIMATION_DURATION_MS: i64 = 120;

/// Default animation duration in microseconds.
pub const ANIMATION_DURATION_US: i64 = ANIMATION_DURATION_MS * 1000;

/// Calculates ease-out cubic curve value for progress in range [0.0, 1.0].
#[must_use]
pub fn ease_out_cubic(progress: f64) -> f64 {
    let p = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - p).powi(3)
}

/// Interpolates between `start` and `target` using ease-out cubic interpolation.
#[must_use]
pub fn interpolate_height(start: i32, target: i32, progress: f64) -> i32 {
    let eased = ease_out_cubic(progress);
    let diff = f64::from(target.saturating_sub(start));
    let current = diff.mul_add(eased, f64::from(start));
    if current.is_finite() && current >= 0.0 && current <= f64::from(i32::MAX) {
        #[allow(clippy::cast_possible_truncation)]
        let h = current.round() as i32;
        h
    } else {
        target
    }
}

/// Window lifecycle state representing the visible mode of the Spotlight overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayState {
    /// Overlay is unmapped, transparent, or hidden in the background.
    #[default]
    Dormant,
    /// Overlay is visible showing only the clean search bar.
    Empty,
    /// Overlay is expanded vertically displaying query search results.
    ResultsList,
    /// Overlay is expanded horizontally displaying results plus a side preview pane.
    SplitPreview,
}

impl OverlayState {
    /// Returns true if the overlay is in any visible state.
    #[must_use]
    pub const fn is_visible(self) -> bool {
        !matches!(self, Self::Dormant)
    }

    /// Verifies whether transitioning from `self` to `target` is valid.
    #[must_use]
    pub const fn can_transition_to(self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Dormant, Self::Dormant | Self::Empty)
                | (Self::Empty, Self::Empty | Self::ResultsList | Self::Dormant)
                | (
                    Self::ResultsList | Self::SplitPreview,
                    Self::ResultsList | Self::Empty | Self::SplitPreview | Self::Dormant,
                )
        )
    }
}

/// Helper to initialize layer shell on a GTK window.
pub fn init_for_window<W: LayerShell>(window: &W) {
    window.init_layer_shell();
}

/// Computes target window height based on number of visible rows and category headers.
#[must_use]
pub fn calculate_target_height(rows: usize, headers: usize) -> i32 {
    let rows_h = i32::try_from(rows)
        .unwrap_or(i32::MAX)
        .saturating_mul(ROW_HEIGHT);
    let headers_h = i32::try_from(headers)
        .unwrap_or(i32::MAX)
        .saturating_mul(HEADER_HEIGHT);
    let total = INITIAL_HEIGHT
        .saturating_add(rows_h)
        .saturating_add(headers_h)
        .saturating_add(FOOTER_HINT_HEIGHT);
    total.min(MAX_HEIGHT)
}

/// Computes the vertical margin offset in pixels (18-20% from top edge).
#[must_use]
pub fn calculate_top_margin(screen_height: i32) -> i32 {
    if screen_height <= 0 {
        194
    } else {
        screen_height
            .saturating_mul(DEFAULT_TOP_MARGIN_PERCENT)
            .checked_div(100)
            .unwrap_or(194)
    }
}

/// Standard footer hint keyboard shortcuts (shortcut, label).
pub const FOOTER_HINT_SHORTCUTS: [(&str, &str); 3] = [
    ("↑↓", "Navigate"),
    ("↵", "Open"),
    ("Ctrl+↵", "Show in Folder"),
];

/// Builds a styled footer navigation hints bar container.
#[must_use]
pub fn build_footer_hint_box() -> gtk4::Box {
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
    container.add_css_class("hint");

    for (key, label_text) in FOOTER_HINT_SHORTCUTS {
        let item_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let key_label = gtk4::Label::new(None);
        key_label.set_markup(&format!("<b>{key}</b>"));
        let desc_label = gtk4::Label::new(Some(label_text));

        item_box.append(&key_label);
        item_box.append(&desc_label);
        container.append(&item_box);
    }

    container
}

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::search_bar::{KeyAction, SearchBarWidget};

/// Custom floating Spotlight window wrapping GTK4 and Wayland layer-shell.
#[derive(Debug, Clone)]
pub struct SpotlightWindow {
    /// Underlying GTK application window surface.
    pub window: gtk4::ApplicationWindow,
    /// Top-level root layout box.
    pub root_box: gtk4::Box,
    /// Header search bar container.
    pub search_bar: gtk4::Box,
    /// Header search bar widget.
    pub search_bar_widget: SearchBarWidget,
    /// Central results and previews box.
    pub content_box: gtk4::Box,
    /// Bottom shortcut hints footer bar.
    pub footer_hint: gtk4::Box,
    /// Flag indicating whether Wayland Layer-Shell protocol is active.
    pub is_wayland: bool,
    /// Current overlay lifecycle state.
    state: Rc<Cell<OverlayState>>,
    /// Currently rendered window height.
    current_height: Rc<Cell<i32>>,
    /// Monotonically increasing generation to cancel stale animation callbacks.
    animation_generation: Rc<Cell<u64>>,
    /// Saved XDG activation token for restoring focus on dismissal.
    activation_token: Rc<RefCell<Option<String>>>,
}

impl SpotlightWindow {
    /// Constructs a new `SpotlightWindow` with layer-shell or X11 splash fallback.
    #[must_use]
    pub fn new(app: Option<&gtk4::Application>) -> Self {
        let window = app.map_or_else(
            || gtk4::ApplicationWindow::builder().build(),
            |application| {
                gtk4::ApplicationWindow::builder()
                    .application(application)
                    .build()
            },
        );

        window.set_default_size(INITIAL_WIDTH, INITIAL_HEIGHT);

        let root_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        root_box.add_css_class("win");

        let search_bar_widget = SearchBarWidget::new();
        let search_bar = search_bar_widget.container.clone();

        let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

        let footer_hint = build_footer_hint_box();
        footer_hint.set_visible(false);

        root_box.append(&search_bar);
        root_box.append(&content_box);
        root_box.append(&footer_hint);
        window.set_child(Some(&root_box));

        let is_wayland = gtk4_layer_shell::is_supported();

        if is_wayland {
            init_for_window(&window);
            window.set_layer(Layer::Overlay);

            // Anchor bits: do not anchor any edges so window floats freely
            window.set_anchor(Edge::Top, false);
            window.set_anchor(Edge::Bottom, false);
            window.set_anchor(Edge::Left, false);
            window.set_anchor(Edge::Right, false);

            let screen_height = get_monitor_height(&window).unwrap_or(1080);
            let top_margin = calculate_top_margin(screen_height);
            window.set_margin(Edge::Top, top_margin);
        } else {
            // X11 fallback: undecorated splash / modal dialog
            window.set_decorated(false);
            window.set_modal(true);
        }

        super::theme::init_theme(&window);

        let current_height = Rc::new(Cell::new(INITIAL_HEIGHT));
        let animation_generation = Rc::new(Cell::new(0));
        let state = Rc::new(Cell::new(OverlayState::Dormant));
        let activation_token = Rc::new(RefCell::new(None));

        // Dismiss when losing focus or clicking outside
        let focus_controller = gtk4::EventControllerFocus::new();
        let state_clone = Rc::clone(&state);
        let window_weak = window.downgrade();
        focus_controller.connect_leave(move |_| {
            if let Some(win) = window_weak.upgrade()
                && state_clone.get().is_visible()
            {
                state_clone.set(OverlayState::Dormant);
                win.set_visible(false);
            }
        });
        window.add_controller(focus_controller);

        let click_gesture = gtk4::GestureClick::new();
        let state_click = Rc::clone(&state);
        let window_weak_click = window.downgrade();
        click_gesture.connect_pressed(move |_, _, _, y| {
            if y > f64::from(INITIAL_HEIGHT)
                && state_click.get() == OverlayState::Empty
                && let Some(win) = window_weak_click.upgrade()
            {
                state_click.set(OverlayState::Dormant);
                win.set_visible(false);
            }
        });
        window.add_controller(click_gesture);

        // Key actions: Escape to clear/dismiss, Toggle (Super+Space) to dismiss immediately
        let search_bar_keys = search_bar_widget.clone();
        let state_keys = Rc::clone(&state);
        let window_weak = window.downgrade();
        search_bar_widget.connect_key_action(move |action| match action {
            KeyAction::Escape => {
                if search_bar_keys.text().trim().is_empty() {
                    if let Some(win) = window_weak.upgrade() {
                        state_keys.set(OverlayState::Dormant);
                        win.set_visible(false);
                    }
                } else {
                    search_bar_keys.clear();
                }
            }
            KeyAction::Toggle => {
                if let Some(win) = window_weak.upgrade() {
                    state_keys.set(OverlayState::Dormant);
                    win.set_visible(false);
                }
            }
            _ => {}
        });

        Self {
            window,
            root_box,
            search_bar,
            search_bar_widget,
            content_box,
            footer_hint,
            is_wayland,
            state,
            current_height,
            animation_generation,
            activation_token,
        }
    }

    /// Returns current overlay state.
    #[must_use]
    pub fn state(&self) -> OverlayState {
        self.state.get()
    }

    /// Returns current rendered window height.
    #[must_use]
    pub fn current_height(&self) -> i32 {
        self.current_height.get()
    }

    /// Smoothly animates window height to target value using ease-out cubic over 120ms.
    pub fn animate_to_height(&self, target_height: i32) {
        let start_height = self.current_height.get();
        if start_height == target_height {
            return;
        }

        let next_gen = self.animation_generation.get().wrapping_add(1);
        self.animation_generation.set(next_gen);
        let target_gen = next_gen;

        let current_h = Rc::clone(&self.current_height);
        let generation = Rc::clone(&self.animation_generation);
        let start_time: Rc<Cell<Option<i64>>> = Rc::new(Cell::new(None));

        self.window.add_tick_callback(move |win, frame_clock| {
            if generation.get() != target_gen {
                return glib::ControlFlow::Break;
            }

            let now = frame_clock.frame_time();
            let start = start_time.get().unwrap_or_else(|| {
                start_time.set(Some(now));
                now
            });

            let elapsed = now.saturating_sub(start);
            if elapsed >= ANIMATION_DURATION_US {
                win.set_default_size(INITIAL_WIDTH, target_height);
                current_h.set(target_height);
                glib::ControlFlow::Break
            } else {
                let elapsed_u32 = u32::try_from(elapsed).unwrap_or(u32::MAX);
                let duration_u32 = u32::try_from(ANIMATION_DURATION_US).unwrap_or(120_000);
                let progress = f64::from(elapsed_u32) / f64::from(duration_u32);
                let h = interpolate_height(start_height, target_height, progress);
                win.set_default_size(INITIAL_WIDTH, h);
                current_h.set(h);
                glib::ControlFlow::Continue
            }
        });
    }

    /// Smoothly animates window height to accommodate search results.
    pub fn animate_to_results(&self, rows: usize, headers: usize) {
        let target = calculate_target_height(rows, headers);
        self.animate_to_height(target);
    }

    /// Smoothly animates window height back to compact initial empty state.
    pub fn animate_to_empty(&self) {
        self.animate_to_height(INITIAL_HEIGHT);
    }

    /// Transitions to a new overlay state if valid.
    #[must_use]
    pub fn set_state(&self, target: OverlayState) -> bool {
        let current = self.state.get();
        if current.can_transition_to(target) {
            self.state.set(target);
            self.apply_state_visuals(target);
            true
        } else {
            false
        }
    }

    /// Applies widget visibility and classes corresponding to `state`.
    fn apply_state_visuals(&self, state: OverlayState) {
        match state {
            OverlayState::Dormant => {
                self.footer_hint.set_visible(false);
                self.content_box.set_visible(false);
                self.search_bar.remove_css_class("dv");
            }
            OverlayState::Empty => {
                self.footer_hint.set_visible(true);
                self.content_box.set_visible(false);
                self.search_bar.remove_css_class("dv");
            }
            OverlayState::ResultsList | OverlayState::SplitPreview => {
                self.footer_hint.set_visible(true);
                self.content_box.set_visible(true);
                self.search_bar.add_css_class("dv");
            }
        }
    }

    /// Returns true if window is currently visible.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.state.get().is_visible() && self.window.is_visible()
    }

    /// Presents the Spotlight overlay window and grabs keyboard focus.
    pub fn present(&self) {
        if self.is_wayland {
            self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        }
        let _ = self.set_state(OverlayState::Empty);
        self.search_bar_widget.clear();
        self.search_bar_widget.select_all();
        self.window.present();
        let _ = self.search_bar_widget.grab_focus();
    }

    /// Toggles overlay between presented (Empty) and dismissed (Dormant).
    pub fn toggle(&self) {
        if self.is_visible() {
            self.dismiss();
        } else {
            self.present();
        }
    }

    /// Sets the XDG activation token for restoring focus to previous application.
    pub fn set_activation_token(&self, token: Option<String>) {
        *self.activation_token.borrow_mut() = token;
    }

    /// Gets current XDG activation token if set.
    #[must_use]
    pub fn activation_token(&self) -> Option<String> {
        self.activation_token.borrow().clone()
    }

    /// Dismisses the Spotlight overlay and releases keyboard focus.
    pub fn dismiss(&self) {
        if self.is_wayland {
            self.window.set_keyboard_mode(KeyboardMode::None);
        }
        self.animate_to_empty();
        let _ = self.set_state(OverlayState::Dormant);
        self.window.set_visible(false);
    }
}

/// Attempts to retrieve monitor vertical height from window display.
fn get_monitor_height(window: &gtk4::ApplicationWindow) -> Option<i32> {
    let display = gtk4::prelude::WidgetExt::display(window);
    let monitors = display.monitors();
    let first = monitors.item(0)?;
    let monitor = first.downcast::<gdk4::Monitor>().ok()?;
    Some(monitor.geometry().height())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_geometry_constants() {
        assert_eq!(INITIAL_WIDTH, 680);
        assert_eq!(INITIAL_HEIGHT, 68);
        assert_eq!(MAX_HEIGHT, 580);
    }

    #[test]
    fn test_calculate_target_height_formula() {
        assert_eq!(calculate_target_height(0, 0), 68 + 36);
        assert_eq!(calculate_target_height(1, 0), 68 + 44 + 36);
        assert_eq!(calculate_target_height(1, 1), 68 + 44 + 28 + 36);
        assert_eq!(calculate_target_height(10, 2), 580);
    }

    #[test]
    fn test_ease_out_cubic_bounds() {
        assert!((ease_out_cubic(0.0) - 0.0).abs() < 1e-6);
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_out_cubic(0.5) > 0.8);
    }

    #[test]
    fn test_interpolate_height() {
        assert_eq!(interpolate_height(68, 580, 0.0), 68);
        assert_eq!(interpolate_height(68, 580, 1.0), 580);
        let mid = interpolate_height(68, 580, 0.5);
        assert!(mid > 500);
    }

    #[test]
    fn test_footer_hints_shortcuts() {
        assert_eq!(FOOTER_HINT_SHORTCUTS.len(), 3);
        assert_eq!(FOOTER_HINT_SHORTCUTS[0], ("↑↓", "Navigate"));
        assert_eq!(FOOTER_HINT_SHORTCUTS[1], ("↵", "Open"));
        assert_eq!(FOOTER_HINT_SHORTCUTS[2], ("Ctrl+↵", "Show in Folder"));
    }
}
