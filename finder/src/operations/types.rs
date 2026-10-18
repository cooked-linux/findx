//! Operations domain types, control structures, and event declarations.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use shared::models::operation::{ProgressReport, TransferItem};

/// Task synchronization and control handle for pausing and cancelling operations.
#[derive(Debug, Clone)]
pub struct TaskControl {
    /// Atomic flag indicating whether the task is currently paused.
    pub is_paused: Arc<AtomicBool>,
    /// Atomic flag indicating whether cancellation has been requested.
    pub is_cancelled: Arc<AtomicBool>,
}

impl Default for TaskControl {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskControl {
    /// Constructs a new [`TaskControl`] with unpaused and uncancelled state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            is_paused: Arc::new(AtomicBool::new(false)),
            is_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns `true` if this task has been requested to pause.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Acquire)
    }

    /// Sets the pause state to `true`.
    pub fn pause(&self) {
        self.is_paused.store(true, Ordering::Release);
    }

    /// Resumes the task by clearing the pause flag.
    pub fn resume(&self) {
        self.is_paused.store(false, Ordering::Release);
    }

    /// Returns `true` if this task has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(Ordering::Acquire)
    }

    /// Signals cancellation of the task.
    pub fn cancel(&self) {
        self.is_cancelled.store(true, Ordering::Release);
    }
}

/// Pre-flight manifest summarizing items, total bytes, and cross-device classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightManifest {
    /// Total count of individual files and directories scheduled.
    pub total_items: u64,
    /// Total size in bytes of all regular files scheduled.
    pub total_bytes: u64,
    /// Detailed list of transfer items to process.
    pub items: Vec<TransferItem>,
    /// Whether any items cross filesystem device boundaries, requiring block copies.
    pub requires_cross_device_copy: bool,
}

/// Lifecycle and progress events emitted by the file operations engine.
#[derive(Debug, Clone, PartialEq)]
pub enum OperationEvent {
    /// Incremental progress during the pre-flight filesystem scanning phase.
    PreflightProgress {
        /// Identifier of the operation.
        operation_id: String,
        /// Number of items scanned so far.
        items_scanned: u64,
        /// Cumulative uncompressed bytes discovered so far.
        bytes_scanned: u64,
    },
    /// Pre-flight inspection completed with the full manifest calculated.
    PreflightCompleted {
        /// Identifier of the operation.
        operation_id: String,
        /// Computed manifest.
        manifest: PreflightManifest,
    },
    /// Real-time progress update during active transfer.
    Progress(ProgressReport),
    /// A single item (file or directory) was successfully processed.
    ItemCompleted {
        /// Identifier of the operation.
        operation_id: String,
        /// Source path processed.
        source: PathBuf,
        /// Destination path written, if applicable.
        destination: Option<PathBuf>,
    },
    /// The operation was paused.
    Paused {
        /// Identifier of the operation.
        operation_id: String,
    },
    /// The operation was resumed from a paused state.
    Resumed {
        /// Identifier of the operation.
        operation_id: String,
    },
    /// The operation was cancelled and cleaned up.
    Cancelled {
        /// Identifier of the operation.
        operation_id: String,
    },
    /// The operation completed all scheduled tasks cleanly.
    Completed {
        /// Identifier of the operation.
        operation_id: String,
    },
    /// The operation encountered an unrecoverable failure.
    Failed {
        /// Identifier of the operation.
        operation_id: String,
        /// Human-readable error description.
        error: String,
    },
}
