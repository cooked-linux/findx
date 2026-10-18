//! Floating batch transfer progress window with animated progress bar and throughput metrics.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;
use libadwaita::prelude::*;
use shared::models::operation::{OperationKind, ProgressReport};

use crate::operations::queue::OperationsQueue;

/// Size threshold in bytes (100 MB) for requiring user confirmation before cancellation.
pub const LARGE_OPERATION_BYTES_THRESHOLD: u64 = 100 * 1024 * 1024;

/// Item count threshold (50 items) for requiring user confirmation before cancellation.
pub const LARGE_OPERATION_ITEMS_THRESHOLD: u64 = 50;

/// Returns `true` if an operation meets the size or item threshold to warrant cancellation confirmation.
#[must_use]
pub const fn is_large_operation(total_bytes: u64, total_items: u64) -> bool {
    total_bytes >= LARGE_OPERATION_BYTES_THRESHOLD || total_items >= LARGE_OPERATION_ITEMS_THRESHOLD
}

/// Returns the standard symbolic icon name associated with a given [`OperationKind`].
#[must_use]
pub const fn operation_kind_icon(kind: OperationKind) -> &'static str {
    match kind {
        OperationKind::Copy | OperationKind::Duplicate => "folder-copy-symbolic",
        OperationKind::Move => "edit-cut-symbolic",
        OperationKind::Trash | OperationKind::Delete => "user-trash-symbolic",
        OperationKind::Restore => "edit-undo-symbolic",
    }
}

/// Formats a human-readable title for a transfer operation.
#[must_use]
pub fn format_operation_title(
    kind: OperationKind,
    total_items: u64,
    destination_name: Option<&str>,
) -> String {
    let items_str = if total_items == 1 {
        "1 item".to_string()
    } else {
        format!("{total_items} items")
    };

    match (kind, destination_name) {
        (OperationKind::Copy, Some(dest)) => format!("Copying {items_str} to '{dest}'"),
        (OperationKind::Copy, None) => format!("Copying {items_str}"),
        (OperationKind::Move, Some(dest)) => format!("Moving {items_str} to '{dest}'"),
        (OperationKind::Move, None) => format!("Moving {items_str}"),
        (OperationKind::Duplicate, _) => format!("Duplicating {items_str}"),
        (OperationKind::Trash, _) => format!("Moving {items_str} to Trash"),
        (OperationKind::Restore, Some(dest)) => format!("Restoring {items_str} to '{dest}'"),
        (OperationKind::Restore, None) => format!("Restoring {items_str}"),
        (OperationKind::Delete, _) => format!("Deleting {items_str}"),
    }
}

/// Formats transfer throughput and ETA metrics label string.
#[must_use]
pub fn format_transfer_metrics(
    transferred_bytes: u64,
    total_bytes: u64,
    bytes_per_second: f64,
    estimated_remaining_seconds: Option<f64>,
) -> String {
    let transferred_str = glib::format_size(transferred_bytes);
    let total_str = glib::format_size(total_bytes);

    let speed_str = if bytes_per_second > 0.0 {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let bps = bytes_per_second as u64;
        format!("{}/s", glib::format_size(bps))
    } else {
        "—".to_string()
    };

    let eta_str = match estimated_remaining_seconds {
        Some(sec) if sec <= 5.0 => "Less than 5 seconds remaining".to_string(),
        Some(sec) if sec < 60.0 => format!("About {sec:.0} seconds remaining"),
        Some(sec) if sec < 3600.0 => {
            let mins = (sec / 60.0).round();
            if mins <= 1.0 {
                "About 1 minute remaining".to_string()
            } else {
                format!("About {mins:.0} minutes remaining")
            }
        }
        Some(sec) => {
            let hours = (sec / 3600.0 * 10.0).round() / 10.0;
            format!("About {hours:.1} hours remaining")
        }
        None => "Estimating…".to_string(),
    };

    if total_bytes > 0 {
        format!("{transferred_str} of {total_str} — {speed_str} — {eta_str}")
    } else {
        format!("{transferred_str} — {speed_str} — {eta_str}")
    }
}

/// Floating batch transfer progress window wrapping an `adw::Window`.
#[derive(Debug, Clone)]
pub struct TransferProgressDialog {
    /// Top-level window.
    pub window: libadwaita::Window,
    /// Operation symbolic icon.
    pub icon_image: gtk4::Image,
    /// High-level summary title label.
    pub title_label: gtk4::Label,
    /// Subtitle label displaying active file being copied.
    pub subtitle_label: gtk4::Label,
    /// Smooth animated progress bar.
    pub progress_bar: gtk4::ProgressBar,
    /// Tabular metrics label (transferred, throughput, ETA).
    pub metrics_label: gtk4::Label,
    /// Pause/Resume toggle button.
    pub pause_resume_btn: gtk4::Button,
    /// Cancel operation button.
    pub cancel_btn: gtk4::Button,
    operation_id: Arc<Mutex<String>>,
    is_paused: Arc<Mutex<bool>>,
    is_completed: Arc<Mutex<bool>>,
    total_bytes: Arc<Mutex<u64>>,
    total_items: Arc<Mutex<u64>>,
}

impl TransferProgressDialog {
    /// Instantiates a new [`TransferProgressDialog`].
    #[must_use]
    pub fn new(
        operation_id: impl Into<String>,
        kind: OperationKind,
        total_items: u64,
        destination_name: Option<&str>,
    ) -> Self {
        let op_id = operation_id.into();
        let icon_name = operation_kind_icon(kind);
        let title_text = format_operation_title(kind, total_items, destination_name);

        let window = libadwaita::Window::builder()
            .title(&title_text)
            .default_width(440)
            .resizable(false)
            .deletable(true)
            .build();

        let root_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(10)
            .margin_start(16)
            .margin_end(16)
            .margin_top(14)
            .margin_bottom(14)
            .css_classes(["operations-progress-dialog"])
            .build();

        // Header Row: Icon + Title + Action Controls (Pause / Cancel)
        let header_row = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(10)
            .valign(gtk4::Align::Center)
            .build();

        let icon_image = gtk4::Image::builder()
            .icon_name(icon_name)
            .pixel_size(24)
            .valign(gtk4::Align::Center)
            .build();
        header_row.append(&icon_image);

        let title_label = gtk4::Label::builder()
            .label(&title_text)
            .halign(gtk4::Align::Start)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .css_classes(["operations-progress-title"])
            .build();
        header_row.append(&title_label);

        let pause_resume_btn = gtk4::Button::builder()
            .icon_name("media-playback-pause-symbolic")
            .tooltip_text("Pause Operation")
            .css_classes(["flat"])
            .valign(gtk4::Align::Center)
            .build();
        header_row.append(&pause_resume_btn);

        let cancel_btn = gtk4::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Cancel Operation")
            .css_classes(["flat"])
            .valign(gtk4::Align::Center)
            .build();
        header_row.append(&cancel_btn);

        root_box.append(&header_row);

        // Subtitle Row: Current active file
        let subtitle_label = gtk4::Label::builder()
            .label("Preparing operation…")
            .halign(gtk4::Align::Start)
            .ellipsize(gtk4::pango::EllipsizeMode::Middle)
            .css_classes(["operations-progress-subtitle"])
            .build();
        root_box.append(&subtitle_label);

        // Progress Bar
        let progress_bar = gtk4::ProgressBar::builder()
            .hexpand(true)
            .fraction(0.0)
            .build();
        root_box.append(&progress_bar);

        // Metrics Row
        let metrics_label = gtk4::Label::builder()
            .label("Estimating…")
            .halign(gtk4::Align::Start)
            .css_classes(["operations-progress-metrics"])
            .build();
        root_box.append(&metrics_label);

        window.set_content(Some(&root_box));

        Self {
            window,
            icon_image,
            title_label,
            subtitle_label,
            progress_bar,
            metrics_label,
            pause_resume_btn,
            cancel_btn,
            operation_id: Arc::new(Mutex::new(op_id)),
            is_paused: Arc::new(Mutex::new(false)),
            is_completed: Arc::new(Mutex::new(false)),
            total_bytes: Arc::new(Mutex::new(0)),
            total_items: Arc::new(Mutex::new(total_items)),
        }
    }

    /// Displays the progress dialog window.
    pub fn present(&self) {
        self.window.present();
    }

    /// Closes the progress dialog window.
    pub fn close(&self) {
        self.window.close();
    }

    /// Updates dialog metrics and smooth progress bar from a [`ProgressReport`].
    pub fn update_progress(&self, report: &ProgressReport) {
        let is_completed = *self
            .is_completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if is_completed {
            return;
        }

        {
            let mut bytes_guard = self
                .total_bytes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *bytes_guard = report.total_bytes;
        }

        {
            let mut items_guard = self
                .total_items
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *items_guard = report.total_items;
        }

        self.progress_bar.set_fraction(report.progress_ratio());

        if let Some(ref path) = report.current_file {
            let filename = path
                .file_name()
                .map_or_else(|| path.to_string_lossy(), |f| f.to_string_lossy());
            self.subtitle_label.set_label(&filename);
        }

        let paused = *self
            .is_paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut metrics = format_transfer_metrics(
            report.transferred_bytes,
            report.total_bytes,
            report.bytes_per_second,
            report.estimated_remaining_seconds,
        );

        if paused {
            metrics.push_str(" (Paused)");
        }
        self.metrics_label.set_label(&metrics);
    }

    /// Toggles the paused presentation state of the dialog.
    pub fn set_paused(&self, paused: bool) {
        {
            let mut guard = self
                .is_paused
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard = paused;
        }

        if paused {
            self.pause_resume_btn
                .set_icon_name("media-playback-start-symbolic");
            self.pause_resume_btn
                .set_tooltip_text(Some("Resume Operation"));
            let current = self.metrics_label.text().to_string();
            if !current.ends_with("(Paused)") {
                self.metrics_label.set_label(&format!("{current} (Paused)"));
            }
        } else {
            self.pause_resume_btn
                .set_icon_name("media-playback-pause-symbolic");
            self.pause_resume_btn
                .set_tooltip_text(Some("Pause Operation"));
            let current = self.metrics_label.text().to_string();
            if let Some(stripped) = current.strip_suffix(" (Paused)") {
                self.metrics_label.set_label(stripped);
            }
        }
    }

    /// Marks the operation as complete and initiates smooth auto-dismissal.
    pub fn mark_completed(&self) {
        {
            let mut guard = self
                .is_completed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard = true;
        }

        self.progress_bar.set_fraction(1.0);
        self.subtitle_label.set_label("Completed");
        self.metrics_label
            .set_label("All files transferred cleanly");
        self.pause_resume_btn.set_sensitive(false);
        self.cancel_btn.set_sensitive(false);

        // Smooth fade out / dismiss after 800ms
        let win = self.window.clone();
        glib::timeout_add_local_once(Duration::from_millis(800), move || {
            win.close();
        });
    }

    /// Marks the operation as failed with an error description.
    pub fn mark_failed(&self, error: &str) {
        {
            let mut guard = self
                .is_completed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *guard = true;
        }

        self.subtitle_label.set_label("Operation failed");
        self.metrics_label.set_label(error);
        self.pause_resume_btn.set_sensitive(false);
    }

    /// Connects pause/resume and cancel button signals to an [`OperationsQueue`].
    pub fn connect_queue(&self, queue: OperationsQueue) {
        let op_id_arc = Arc::clone(&self.operation_id);
        let is_paused_arc = Arc::clone(&self.is_paused);
        let queue_pause = queue.clone();
        let dialog_self = self.clone();

        self.pause_resume_btn.connect_clicked(move |_| {
            let id = op_id_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let is_currently_paused = *is_paused_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            if is_currently_paused {
                if queue_pause.resume_operation(&id).is_ok() {
                    *is_paused_arc
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
                    dialog_self.set_paused(false);
                }
            } else if queue_pause.pause_operation(&id).is_ok() {
                *is_paused_arc
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                dialog_self.set_paused(true);
            }
        });

        let op_id_arc2 = Arc::clone(&self.operation_id);
        let total_bytes_arc = Arc::clone(&self.total_bytes);
        let total_items_arc = Arc::clone(&self.total_items);
        let win_clone = self.window.clone();

        self.cancel_btn.connect_clicked(move |_| {
            let id = op_id_arc2
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let total_b = *total_bytes_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let total_i = *total_items_arc
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            let queue_cancel = queue.clone();
            let win = win_clone.clone();

            if is_large_operation(total_b, total_i) {
                // Confirmation dialog for large operations
                let confirm_dialog = libadwaita::MessageDialog::builder()
                    .transient_for(&win)
                    .modal(true)
                    .heading("Stop file transfer?")
                    .body("Are you sure you want to cancel? Items already copied will remain in the destination.")
                    .build();

                confirm_dialog.add_response("cancel_op", "Stop Operation");
                confirm_dialog.set_response_appearance("cancel_op", libadwaita::ResponseAppearance::Destructive);
                confirm_dialog.add_response("continue", "Continue");
                confirm_dialog.set_response_appearance("continue", libadwaita::ResponseAppearance::Suggested);
                confirm_dialog.set_default_response(Some("continue"));
                confirm_dialog.set_close_response("continue");

                confirm_dialog.connect_response(None, move |_, response| {
                    if response == "cancel_op" {
                        let _ = queue_cancel.cancel_operation(&id);
                        win.close();
                    }
                });

                confirm_dialog.present();
            } else {
                let _ = queue_cancel.cancel_operation(&id);
                win.close();
            }
        });
    }

    /// Convenience helper to set current file being transferred.
    pub fn set_current_file(&self, path: &Path) {
        let name = path
            .file_name()
            .map_or_else(|| path.to_string_lossy(), |f| f.to_string_lossy());
        self.subtitle_label.set_label(&name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_operation_title() {
        assert_eq!(
            format_operation_title(OperationKind::Copy, 1, Some("Downloads")),
            "Copying 1 item to 'Downloads'"
        );
        assert_eq!(
            format_operation_title(OperationKind::Copy, 15, Some("Documents")),
            "Copying 15 items to 'Documents'"
        );
        assert_eq!(
            format_operation_title(OperationKind::Move, 3, Some("Trash")),
            "Moving 3 items to 'Trash'"
        );
        assert_eq!(
            format_operation_title(OperationKind::Trash, 5, None),
            "Moving 5 items to Trash"
        );
        assert_eq!(
            format_operation_title(OperationKind::Duplicate, 2, None),
            "Duplicating 2 items"
        );
        assert_eq!(
            format_operation_title(OperationKind::Delete, 1, None),
            "Deleting 1 item"
        );
    }

    #[test]
    fn test_is_large_operation() {
        assert!(!is_large_operation(1024, 5));
        assert!(is_large_operation(LARGE_OPERATION_BYTES_THRESHOLD, 1));
        assert!(is_large_operation(1024, LARGE_OPERATION_ITEMS_THRESHOLD));
    }

    #[test]
    fn test_format_transfer_metrics() {
        let metrics =
            format_transfer_metrics(1_450_000_000, 12_800_000_000, 145_000_000.0, Some(45.0));
        assert!(metrics.contains("About 45 seconds remaining"));
        assert!(metrics.contains("145"));

        let short_eta = format_transfer_metrics(100, 200, 10.0, Some(3.0));
        assert!(short_eta.contains("Less than 5 seconds remaining"));

        let min_eta = format_transfer_metrics(100, 200, 10.0, Some(120.0));
        assert!(min_eta.contains("About 2 minutes remaining"));

        let hour_eta = format_transfer_metrics(100, 200, 10.0, Some(7200.0));
        assert!(hour_eta.contains("About 2.0 hours remaining"));

        let no_eta = format_transfer_metrics(100, 200, 0.0, None);
        assert!(no_eta.contains("Estimating…"));
    }

    #[test]
    fn test_operation_kind_icon() {
        assert_eq!(
            operation_kind_icon(OperationKind::Copy),
            "folder-copy-symbolic"
        );
        assert_eq!(
            operation_kind_icon(OperationKind::Move),
            "edit-cut-symbolic"
        );
        assert_eq!(
            operation_kind_icon(OperationKind::Trash),
            "user-trash-symbolic"
        );
        assert_eq!(
            operation_kind_icon(OperationKind::Delete),
            "user-trash-symbolic"
        );
        assert_eq!(
            operation_kind_icon(OperationKind::Duplicate),
            "folder-copy-symbolic"
        );
        assert_eq!(
            operation_kind_icon(OperationKind::Restore),
            "edit-undo-symbolic"
        );
    }
}
