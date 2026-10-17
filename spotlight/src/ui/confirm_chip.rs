//! Destructive action inline confirmation chip with countdown timer.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

/// Default countdown timeout in seconds for destructive actions.
pub const CONFIRM_TIMEOUT_SECS: u32 = 5;

/// Headless status state representing confirmation progress.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConfirmationStatus {
    /// No confirmation pending.
    #[default]
    Idle,
    /// Confirmation active with countdown timer.
    Awaiting {
        /// Unique system action identifier.
        action_id: String,
        /// Human-readable title of the action (e.g., `"Shut Down"`).
        action_title: String,
        /// Remaining seconds before auto-cancellation.
        seconds_left: u32,
    },
    /// Action was explicitly confirmed by user.
    Confirmed(String),
    /// Action was cancelled by user or timed out.
    Cancelled,
}

/// Headless state tracker for destructive action confirmations.
#[derive(Debug, Clone, Default)]
pub struct ConfirmState {
    status: ConfirmationStatus,
}

impl ConfirmState {
    /// Creates a new `ConfirmState` in `Idle` status.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            status: ConfirmationStatus::Idle,
        }
    }

    /// Requests confirmation for a destructive action.
    pub fn request(&mut self, action_id: impl Into<String>, title: impl Into<String>) {
        self.status = ConfirmationStatus::Awaiting {
            action_id: action_id.into(),
            action_title: title.into(),
            seconds_left: CONFIRM_TIMEOUT_SECS,
        };
    }

    /// Decrements the countdown timer by one second.
    ///
    /// Returns `true` if the confirmation is still pending, or `false` if it timed out.
    pub fn tick(&mut self) -> bool {
        if let ConfirmationStatus::Awaiting {
            ref mut seconds_left,
            ..
        } = self.status
        {
            if *seconds_left > 1 {
                *seconds_left -= 1;
                true
            } else {
                self.status = ConfirmationStatus::Cancelled;
                false
            }
        } else {
            false
        }
    }

    /// Confirms the active pending action.
    ///
    /// Returns the confirmed action ID if a confirmation was pending.
    pub fn confirm(&mut self) -> Option<String> {
        if let ConfirmationStatus::Awaiting { ref action_id, .. } = self.status {
            let id = action_id.clone();
            self.status = ConfirmationStatus::Confirmed(id.clone());
            Some(id)
        } else {
            None
        }
    }

    /// Cancels the pending action confirmation.
    pub fn cancel(&mut self) {
        self.status = ConfirmationStatus::Cancelled;
    }

    /// Returns `true` if awaiting confirmation.
    #[must_use]
    pub const fn is_awaiting(&self) -> bool {
        matches!(self.status, ConfirmationStatus::Awaiting { .. })
    }

    /// Returns current confirmation status.
    #[must_use]
    pub const fn status(&self) -> &ConfirmationStatus {
        &self.status
    }

    /// Returns remaining seconds if awaiting confirmation.
    #[must_use]
    pub const fn seconds_left(&self) -> Option<u32> {
        if let ConfirmationStatus::Awaiting { seconds_left, .. } = self.status {
            Some(seconds_left)
        } else {
            None
        }
    }
}

type ConfirmCallback = Box<dyn Fn(String)>;
type CancelCallback = Box<dyn Fn()>;

/// GTK4 Confirmation Chip Widget presenting `[↵ Confirm <Action> (5s)] [Esc: Cancel]`.
#[derive(Clone)]
pub struct ConfirmChipWidget {
    /// Container holding the confirmation buttons.
    pub container: gtk4::Box,
    /// Confirm button displaying countdown.
    pub confirm_button: gtk4::Button,
    /// Cancel button.
    pub cancel_button: gtk4::Button,
    state: Rc<RefCell<ConfirmState>>,
    timer_source: Rc<RefCell<Option<glib::SourceId>>>,
    on_confirm_cb: Rc<RefCell<Option<ConfirmCallback>>>,
    on_cancel_cb: Rc<RefCell<Option<CancelCallback>>>,
}

impl std::fmt::Debug for ConfirmChipWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfirmChipWidget")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Default for ConfirmChipWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfirmChipWidget {
    /// Constructs a new `ConfirmChipWidget`.
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        container.add_css_class("confirm-bar");
        container.set_halign(gtk4::Align::Center);
        container.set_visible(false);

        let confirm_button = gtk4::Button::new();
        confirm_button.add_css_class("confirm-chip-action");

        let cancel_button = gtk4::Button::with_label("Esc: Cancel");
        cancel_button.add_css_class("confirm-chip-cancel");

        container.append(&confirm_button);
        container.append(&cancel_button);

        let state = Rc::new(RefCell::new(ConfirmState::new()));
        let timer_source = Rc::new(RefCell::new(None));
        let on_confirm_cb: Rc<RefCell<Option<ConfirmCallback>>> = Rc::new(RefCell::new(None));
        let on_cancel_cb: Rc<RefCell<Option<CancelCallback>>> = Rc::new(RefCell::new(None));

        let widget = Self {
            container,
            confirm_button,
            cancel_button,
            state,
            timer_source,
            on_confirm_cb,
            on_cancel_cb,
        };

        // Wire confirm click
        let w_confirm = widget.clone();
        widget.confirm_button.connect_clicked(move |_| {
            w_confirm.confirm();
        });

        // Wire cancel click
        let w_cancel = widget.clone();
        widget.cancel_button.connect_clicked(move |_| {
            w_cancel.cancel();
        });

        widget
    }

    /// Surfaces the confirmation chip for `action_id` and begins 5s countdown.
    pub fn show_confirmation<F: Fn(String) + 'static, C: Fn() + 'static>(
        &self,
        action_id: &str,
        title: &str,
        on_confirm: F,
        on_cancel: C,
    ) {
        self.stop_timer();

        self.state.borrow_mut().request(action_id, title);
        *self.on_confirm_cb.borrow_mut() = Some(Box::new(on_confirm));
        *self.on_cancel_cb.borrow_mut() = Some(Box::new(on_cancel));

        let initial_label = format!("↵ Confirm {title} ({CONFIRM_TIMEOUT_SECS}s)");
        self.confirm_button.set_label(&initial_label);
        self.container.set_visible(true);

        let state_clone = Rc::clone(&self.state);
        let btn_clone = self.confirm_button.clone();
        let w_clone = self.clone();
        let action_title = title.to_string();

        let source = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let still_active = state_clone.borrow_mut().tick();
            if still_active {
                let left = state_clone
                    .borrow()
                    .seconds_left()
                    .unwrap_or(CONFIRM_TIMEOUT_SECS);
                btn_clone.set_label(&format!("↵ Confirm {action_title} ({left}s)"));
                glib::ControlFlow::Continue
            } else {
                w_clone.cancel();
                glib::ControlFlow::Break
            }
        });

        *self.timer_source.borrow_mut() = Some(source);
    }

    /// Stops any running countdown timer.
    fn stop_timer(&self) {
        if let Some(source) = self.timer_source.borrow_mut().take() {
            source.remove();
        }
    }

    /// Executes the pending confirmed action.
    pub fn confirm(&self) {
        self.stop_timer();
        self.container.set_visible(false);

        if let Some(action_id) = self.state.borrow_mut().confirm()
            && let Some(cb) = self.on_confirm_cb.borrow().as_ref()
        {
            cb(action_id);
        }
    }

    /// Cancels confirmation and dismisses the chip.
    pub fn cancel(&self) {
        self.stop_timer();
        self.state.borrow_mut().cancel();
        self.container.set_visible(false);

        if let Some(cb) = self.on_cancel_cb.borrow().as_ref() {
            cb();
        }
    }

    /// Returns `true` if confirmation is actively presented and awaiting user input.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state.borrow().is_awaiting() && self.container.is_visible()
    }

    /// Returns the underlying GTK container box.
    #[must_use]
    pub const fn container(&self) -> &gtk4::Box {
        &self.container
    }
}
