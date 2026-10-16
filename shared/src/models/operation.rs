//! Batch filesystem operations and progress reporting models.

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::errors::ModelError;

/// The type of file manipulation operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OperationKind {
    /// Copy files or directories to a destination.
    Copy,
    /// Move/rename files or directories to a destination.
    Move,
    /// Duplicate files in-place with unique naming.
    Duplicate,
    /// Move items to the Freedesktop Trash.
    Trash,
    /// Restore items from the Trash to their original locations.
    Restore,
    /// Permanently delete items without moving to Trash.
    Delete,
}

impl OperationKind {
    /// Human-readable title of the operation (e.g. `"Copy"`).
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Copy => "Copy",
            Self::Move => "Move",
            Self::Duplicate => "Duplicate",
            Self::Trash => "Move to Trash",
            Self::Restore => "Restore",
            Self::Delete => "Delete",
        }
    }

    /// Present continuous verb participle (e.g. `"Copying"`).
    #[must_use]
    pub const fn verb_present(&self) -> &'static str {
        match self {
            Self::Copy => "Copying",
            Self::Move => "Moving",
            Self::Duplicate => "Duplicating",
            Self::Trash => "Trashing",
            Self::Restore => "Restoring",
            Self::Delete => "Deleting",
        }
    }

    /// Past tense verb (e.g. `"Copied"`).
    #[must_use]
    pub const fn verb_past(&self) -> &'static str {
        match self {
            Self::Copy => "Copied",
            Self::Move => "Moved",
            Self::Duplicate => "Duplicated",
            Self::Trash => "Trashed",
            Self::Restore => "Restored",
            Self::Delete => "Deleted",
        }
    }

    /// Indicates whether this operation kind inherently requires a target destination directory.
    #[must_use]
    pub const fn requires_destination(&self) -> bool {
        match self {
            Self::Copy | Self::Move => true,
            Self::Duplicate | Self::Trash | Self::Restore | Self::Delete => false,
        }
    }
}

impl fmt::Display for OperationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for OperationKind {
    type Err = ModelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "copy" => Ok(Self::Copy),
            "move" => Ok(Self::Move),
            "duplicate" => Ok(Self::Duplicate),
            "trash" | "move to trash" => Ok(Self::Trash),
            "restore" => Ok(Self::Restore),
            "delete" | "remove" => Ok(Self::Delete),
            _ => Err(ModelError::invalid_operation(format!(
                "unrecognized operation kind: '{s}'"
            ))),
        }
    }
}

/// Execution state of a batch file operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OperationState {
    /// Operation is waiting in the worker queue.
    Queued,
    /// Pre-flight scanning and size estimation in progress.
    Preparing,
    /// Operation is actively transferring/processing files.
    Running,
    /// Operation is temporarily paused by the user.
    Paused,
    /// Operation was cancelled and is rolling back/cleaning up.
    Cancelled,
    /// Operation finished successfully.
    Completed,
    /// Operation encountered an unrecoverable failure.
    Failed,
}

impl OperationState {
    /// Returns `true` if the operation is in an active (in-progress) state.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self, Self::Preparing | Self::Running)
    }

    /// Returns `true` if the operation has finished (successfully or unsuccessfully).
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }

    /// Human-readable state label.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Preparing => "Preparing",
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Cancelled => "Cancelled",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }
}

impl fmt::Display for OperationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// Single file transfer entry within an operation manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TransferItem {
    /// Source file or directory path.
    pub source: PathBuf,
    /// Target destination path.
    pub destination: Option<PathBuf>,
    /// Expected byte size of the item.
    pub size_bytes: u64,
    /// Number of bytes transferred so far.
    pub bytes_transferred: u64,
    /// Whether the transfer for this individual item is complete.
    pub completed: bool,
}

impl TransferItem {
    /// Creates a new [`TransferItem`].
    #[must_use]
    pub fn new(source: impl Into<PathBuf>, destination: Option<PathBuf>, size_bytes: u64) -> Self {
        Self {
            source: source.into(),
            destination,
            size_bytes,
            bytes_transferred: 0,
            completed: false,
        }
    }
}

/// A batch operation task requested by the user or UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BatchOperation {
    /// Unique task identifier.
    pub id: String,
    /// Kind of filesystem operation to execute.
    pub kind: OperationKind,
    /// Source paths being operated on.
    pub sources: Vec<PathBuf>,
    /// Target destination directory, if applicable.
    pub destination: Option<PathBuf>,
}

impl BatchOperation {
    /// Constructs a new [`BatchOperation`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        kind: OperationKind,
        sources: Vec<PathBuf>,
        destination: Option<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            sources,
            destination,
        }
    }

    /// Validates internal consistency of the operation request.
    ///
    /// # Errors
    /// Returns [`ModelError::Validation`] if sources are empty or required destination is missing.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.sources.is_empty() {
            return Err(ModelError::validation("batch operation sources cannot be empty"));
        }
        for src in &self.sources {
            if src.as_os_str().is_empty() {
                return Err(ModelError::invalid_path(src.clone()));
            }
        }
        if self.kind.requires_destination() && self.destination.is_none() {
            return Err(ModelError::validation(format!(
                "operation '{}' requires a target destination",
                self.kind
            )));
        }
        Ok(())
    }

    /// Returns the destination path reference, if present.
    #[must_use]
    pub fn destination_path(&self) -> Option<&Path> {
        self.destination.as_deref()
    }
}

/// Real-time progress metric report emitted by the operations engine.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProgressReport {
    /// Identifier of the operation task being reported.
    pub operation_id: String,
    /// Number of individual items completed so far.
    pub completed_items: u64,
    /// Total count of items scheduled in the operation manifest.
    pub total_items: u64,
    /// Number of bytes transferred or processed so far.
    pub transferred_bytes: u64,
    /// Total bytes scheduled in the operation.
    pub total_bytes: u64,
    /// Instantaneous transfer rate in bytes per second.
    pub bytes_per_second: f64,
    /// Estimated time remaining in seconds, if known.
    pub estimated_remaining_seconds: Option<f64>,
    /// Current path actively being processed.
    pub current_file: Option<PathBuf>,
}

impl ProgressReport {
    /// Constructs a new [`ProgressReport`].
    #[must_use]
    #[expect(clippy::too_many_arguments, reason = "progress metric snapshot reporting")]
    pub fn new(
        operation_id: impl Into<String>,
        completed_items: u64,
        total_items: u64,
        transferred_bytes: u64,
        total_bytes: u64,
        bytes_per_second: f64,
        estimated_remaining_seconds: Option<f64>,
        current_file: Option<PathBuf>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            completed_items,
            total_items,
            transferred_bytes,
            total_bytes,
            bytes_per_second,
            estimated_remaining_seconds,
            current_file,
        }
    }

    /// Computes overall progress ratio from `0.0` to `1.0`.
    ///
    /// Prefers byte ratio if `total_bytes > 0`, otherwise falls back to item ratio.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "progress ratio requires floating-point division"
    )]
    pub fn progress_ratio(&self) -> f64 {
        if self.total_bytes > 0 {
            (self.transferred_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
        } else if self.total_items > 0 {
            (self.completed_items as f64 / self.total_items as f64).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Computes overall progress as a percentage between `0.0` and `100.0`.
    #[must_use]
    pub fn progress_percent(&self) -> f64 {
        self.progress_ratio() * 100.0
    }

    /// Returns `true` if all items and bytes have been transferred.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.total_items > 0
            && self.completed_items >= self.total_items
            && self.transferred_bytes >= self.total_bytes
    }

    /// Formats the current transfer rate into a human-readable throughput string
    /// (e.g. `"145.2 MB/s"`, `"12.4 KB/s"`).
    #[must_use]
    pub fn formatted_throughput(&self) -> String {
        let bps = self.bytes_per_second;
        if bps >= 1_000_000_000.0 {
            format!("{:.1} GB/s", bps / 1_000_000_000.0)
        } else if bps >= 1_000_000.0 {
            format!("{:.1} MB/s", bps / 1_000_000.0)
        } else if bps >= 1_000.0 {
            format!("{:.1} KB/s", bps / 1_000.0)
        } else {
            format!("{bps:.0} B/s")
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_kind_attributes() {
        assert_eq!(OperationKind::Copy.display_name(), "Copy");
        assert_eq!(OperationKind::Copy.verb_present(), "Copying");
        assert_eq!(OperationKind::Copy.verb_past(), "Copied");
        assert!(OperationKind::Copy.requires_destination());
        assert!(!OperationKind::Trash.requires_destination());

        assert_eq!("move".parse::<OperationKind>().unwrap(), OperationKind::Move);
        assert_eq!("delete".parse::<OperationKind>().unwrap(), OperationKind::Delete);
        assert!("invalid_op".parse::<OperationKind>().is_err());
    }

    #[test]
    fn test_operation_state_transitions() {
        assert!(OperationState::Running.is_active());
        assert!(OperationState::Preparing.is_active());
        assert!(!OperationState::Queued.is_active());
        assert!(OperationState::Completed.is_terminal());
        assert!(OperationState::Failed.is_terminal());
        assert!(OperationState::Cancelled.is_terminal());
        assert!(!OperationState::Paused.is_terminal());
    }

    #[test]
    fn test_batch_operation_validation() {
        let op_valid = BatchOperation::new(
            "op-1",
            OperationKind::Copy,
            vec![PathBuf::from("/tmp/src.txt")],
            Some(PathBuf::from("/tmp/dest")),
        );
        assert!(op_valid.validate().is_ok());

        let op_empty_sources = BatchOperation::new(
            "op-2",
            OperationKind::Copy,
            vec![],
            Some(PathBuf::from("/tmp/dest")),
        );
        assert!(op_empty_sources.validate().is_err());

        let op_missing_dest = BatchOperation::new(
            "op-3",
            OperationKind::Move,
            vec![PathBuf::from("/tmp/src.txt")],
            None,
        );
        assert!(op_missing_dest.validate().is_err());
    }

    #[test]
    fn test_progress_report_metrics() {
        let report = ProgressReport::new(
            "op-1",
            5,
            10,
            500_000_000,
            1_000_000_000,
            150_000_000.0,
            Some(3.3),
            Some(PathBuf::from("/data/file.iso")),
        );

        assert_eq!(report.progress_ratio(), 0.5);
        assert_eq!(report.progress_percent(), 50.0);
        assert!(!report.is_complete());
        assert_eq!(report.formatted_throughput(), "150.0 MB/s");

        let done_report = ProgressReport::new(
            "op-1",
            10,
            10,
            1_000_000_000,
            1_000_000_000,
            0.0,
            None,
            None,
        );
        assert!(done_report.is_complete());
        assert_eq!(done_report.progress_ratio(), 1.0);
    }

    #[test]
    fn test_operation_serde() {
        let op = BatchOperation::new(
            "op-123",
            OperationKind::Duplicate,
            vec![PathBuf::from("/home/user/doc.txt")],
            None,
        );
        let json = serde_json::to_string(&op).unwrap();
        let deserialized: BatchOperation = serde_json::from_str(&json).unwrap();
        assert_eq!(op, deserialized);
    }
}
