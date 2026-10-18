//! Header Bar Operations Queue popover and active task badge controller.

use gtk4::glib;
use gtk4::prelude::*;
use shared::models::operation::BatchOperation;

use super::progress_dialog::{format_operation_title, operation_kind_icon};
use crate::operations::queue::{ActiveTask, OperationsQueue};

/// Controller managing the badge indicator and styling on the header bar `queue_btn`.
#[derive(Debug, Clone)]
pub struct QueueBadgeController {
    queue_btn: gtk4::Button,
    base_label: String,
}

impl QueueBadgeController {
    /// Instantiates a new [`QueueBadgeController`] wrapping the header bar's queue button.
    #[must_use]
    pub fn new(queue_btn: gtk4::Button) -> Self {
        let base_label = queue_btn
            .label()
            .map_or_else(|| "Queue".to_string(), |l| l.to_string());
        Self {
            queue_btn,
            base_label,
        }
    }

    /// Updates button label and CSS classes reflecting the active operation count.
    pub fn update(&self, active_count: usize) {
        if active_count > 0 {
            self.queue_btn
                .set_label(&format!("{} ({active_count})", self.base_label));
            self.queue_btn.add_css_class("queue-btn-active");
            self.queue_btn.set_tooltip_text(Some(&format!(
                "{active_count} file operation(s) in progress"
            )));
        } else {
            self.queue_btn.set_label(&self.base_label);
            self.queue_btn.remove_css_class("queue-btn-active");
            self.queue_btn
                .set_tooltip_text(Some("File Operations Queue"));
        }
    }

    /// Returns the current badge count based on button text parsing.
    #[must_use]
    pub fn current_badge_count(&self) -> usize {
        let text = self.queue_btn.label().unwrap_or_default();
        if let Some(start) = text.find('(')
            && let Some(end) = text.find(')')
            && end > start + 1
        {
            text[start + 1..end].parse().unwrap_or(0)
        } else {
            0
        }
    }
}

/// Popover widget attached to `header_bar.queue_btn` listing active and queued transfers.
#[derive(Debug, Clone)]
pub struct OperationsQueuePopover {
    /// Top-level popover.
    pub popover: gtk4::Popover,
    /// `ListBox` containing active and pending operation rows.
    pub list_box: gtk4::ListBox,
    /// Label shown when no transfers are in progress.
    pub empty_label: gtk4::Label,
    /// Badge controller tracking active task counts.
    pub badge_controller: QueueBadgeController,
    /// Reference to the header bar queue button.
    pub queue_btn: gtk4::Button,
}

impl OperationsQueuePopover {
    /// Instantiates a new [`OperationsQueuePopover`] attached to `queue_btn`.
    #[must_use]
    pub fn new(queue_btn: &gtk4::Button) -> Self {
        let popover = gtk4::Popover::builder()
            .autohide(true)
            .css_classes(["operations-queue-popover"])
            .build();
        popover.set_parent(queue_btn);

        let root_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(8)
            .margin_start(8)
            .margin_end(8)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        let title_label = gtk4::Label::builder()
            .label("File Transfers")
            .halign(gtk4::Align::Start)
            .css_classes(["operations-queue-title"])
            .build();
        root_box.append(&title_label);

        let empty_label = gtk4::Label::builder()
            .label("No active file transfers")
            .halign(gtk4::Align::Center)
            .css_classes(["operations-queue-empty"])
            .build();
        root_box.append(&empty_label);

        let list_box = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .visible(false)
            .build();

        let scrolled = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .max_content_height(280)
            .propagate_natural_height(true)
            .child(&list_box)
            .build();
        root_box.append(&scrolled);

        popover.set_child(Some(&root_box));

        let badge_controller = QueueBadgeController::new(queue_btn.clone());

        let popover_clone = popover.clone();
        queue_btn.connect_clicked(move |_| {
            popover_clone.popup();
        });

        Self {
            popover,
            list_box,
            empty_label,
            badge_controller,
            queue_btn: queue_btn.clone(),
        }
    }

    /// Attaches the popover to `queue_btn`, initializes content, and subscribes to lifecycle events.
    #[must_use]
    pub fn attach(queue_btn: &gtk4::Button, queue: &OperationsQueue) -> Self {
        let popover = Self::new(queue_btn);
        popover.refresh(queue);
        popover.start_event_listener(queue);
        popover
    }

    /// Spawns a `GLib` main-context async listener reacting to `OperationEvent` broadcasts.
    pub fn start_event_listener(&self, queue: &OperationsQueue) {
        let events = queue.subscribe_events();
        let popover_self = self.clone();
        let queue_clone = queue.clone();

        glib::spawn_future_local(async move {
            while events.recv().await.is_ok() {
                popover_self.refresh(&queue_clone);
            }
        });
    }

    /// Detaches the popover from its parent button.
    pub fn unparent(&self) {
        self.popover.unparent();
    }

    /// Re-queries the `OperationsQueue` and rebuilds the popover rows and badge count.
    pub fn refresh(&self, queue: &OperationsQueue) {
        let active = queue.active_tasks();
        let pending = queue.pending_operations();
        let count = active.len();

        self.badge_controller.update(count);

        if active.is_empty() && pending.is_empty() {
            self.empty_label.set_visible(true);
            self.list_box.set_visible(false);
            while let Some(child) = self.list_box.first_child() {
                self.list_box.remove(&child);
            }
            return;
        }

        self.empty_label.set_visible(false);
        self.list_box.set_visible(true);

        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        // Active Tasks
        for task in active {
            let row = self.build_active_row(&task, queue);
            self.list_box.append(&row);
        }

        // Queued / Pending Tasks
        for op in pending {
            let row = self.build_pending_row(&op, queue);
            self.list_box.append(&row);
        }
    }

    fn build_active_row(&self, task: &ActiveTask, queue: &OperationsQueue) -> gtk4::Box {
        let row_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .css_classes(["operations-queue-row"])
            .build();

        let top_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk4::Align::Center)
            .build();

        let icon = gtk4::Image::builder()
            .icon_name(operation_kind_icon(task.kind))
            .pixel_size(16)
            .build();
        top_row.append(&icon);

        let dest_name = task
            .operation
            .destination
            .as_deref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str());

        let count = u64::try_from(task.operation.sources.len()).unwrap_or(0);
        let title_text = format_operation_title(task.kind, count, dest_name);

        let title = gtk4::Label::builder()
            .label(&title_text)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        top_row.append(&title);

        let ratio = task.last_report.as_ref().map_or(
            0.0,
            shared::models::operation::ProgressReport::progress_ratio,
        );
        let percent_text = format!("{:.0}%", ratio * 100.0);
        let percent_label = gtk4::Label::builder()
            .label(&percent_text)
            .halign(gtk4::Align::End)
            .css_classes(["operations-progress-metrics"])
            .build();
        top_row.append(&percent_label);

        let is_paused = task.control.is_paused();
        let pause_icon = if is_paused {
            "media-playback-start-symbolic"
        } else {
            "media-playback-pause-symbolic"
        };
        let pause_tip = if is_paused { "Resume" } else { "Pause" };

        let pause_btn = gtk4::Button::builder()
            .icon_name(pause_icon)
            .tooltip_text(pause_tip)
            .css_classes(["flat"])
            .build();

        let task_id_pause = task.operation_id.clone();
        let queue_pause = queue.clone();
        let popover_self_pause = self.clone();

        pause_btn.connect_clicked(move |_| {
            if is_paused {
                let _ = queue_pause.resume_operation(&task_id_pause);
            } else {
                let _ = queue_pause.pause_operation(&task_id_pause);
            }
            popover_self_pause.refresh(&queue_pause);
        });
        top_row.append(&pause_btn);

        let cancel_btn = gtk4::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Cancel")
            .css_classes(["flat"])
            .build();

        let task_id_cancel = task.operation_id.clone();
        let queue_cancel = queue.clone();
        let popover_self_cancel = self.clone();

        cancel_btn.connect_clicked(move |_| {
            let _ = queue_cancel.cancel_operation(&task_id_cancel);
            popover_self_cancel.refresh(&queue_cancel);
        });
        top_row.append(&cancel_btn);

        row_box.append(&top_row);

        let progress_bar = gtk4::ProgressBar::builder()
            .fraction(ratio)
            .hexpand(true)
            .build();
        row_box.append(&progress_bar);

        row_box
    }

    fn build_pending_row(&self, op: &BatchOperation, queue: &OperationsQueue) -> gtk4::Box {
        let row_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .css_classes(["operations-queue-row"])
            .build();

        let top_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk4::Align::Center)
            .build();

        let icon = gtk4::Image::builder()
            .icon_name(operation_kind_icon(op.kind))
            .pixel_size(16)
            .build();
        top_row.append(&icon);

        let dest_name = op
            .destination
            .as_deref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str());

        let count = u64::try_from(op.sources.len()).unwrap_or(0);
        let title_text = format_operation_title(op.kind, count, dest_name);

        let title = gtk4::Label::builder()
            .label(&title_text)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        top_row.append(&title);

        let status_label = gtk4::Label::builder()
            .label("Queued")
            .halign(gtk4::Align::End)
            .css_classes(["operations-progress-subtitle"])
            .build();
        top_row.append(&status_label);

        let cancel_btn = gtk4::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Cancel")
            .css_classes(["flat"])
            .build();

        let op_id_cancel = op.id.clone();
        let queue_cancel = queue.clone();
        let popover_self_cancel = self.clone();

        cancel_btn.connect_clicked(move |_| {
            let _ = queue_cancel.cancel_operation(&op_id_cancel);
            popover_self_cancel.refresh(&queue_cancel);
        });
        top_row.append(&cancel_btn);

        row_box.append(&top_row);

        let progress_bar = gtk4::ProgressBar::builder()
            .fraction(0.0)
            .hexpand(true)
            .build();
        row_box.append(&progress_bar);

        row_box
    }
}
