//! Error types and result definitions for the undo/redo engine.

use std::path::PathBuf;
use std::time::SystemTime;

use thiserror::Error;

/// Errors that can occur during transactional undo and redo operations.
#[derive(Debug, Error)]
pub enum UndoError {
    /// No transaction available on the undo stack.
    #[error("no transaction available to undo")]
    UndoStackEmpty,

    /// No transaction available on the redo stack.
    #[error("no transaction available to redo")]
    RedoStackEmpty,

    /// Inode or device verification failed for a file before inverse execution.
    #[error(
        "inode verification failed for '{}': expected dev={}, ino={}; found dev={}, ino={}",
        path.display(),
        expected_dev,
        expected_ino,
        actual_dev,
        actual_ino
    )]
    InodeMismatch {
        /// File path where verification failed.
        path: PathBuf,
        /// Expected filesystem device ID.
        expected_dev: u64,
        /// Expected filesystem inode number.
        expected_ino: u64,
        /// Actual filesystem device ID discovered on disk.
        actual_dev: u64,
        /// Actual filesystem inode number discovered on disk.
        actual_ino: u64,
    },

    /// The target item does not exist on disk.
    #[error("item not found at '{}'", .0.display())]
    ItemNotFound(PathBuf),

    /// A destination collision occurred and conflict policy prevented overwrite.
    #[error("destination already occupied at '{}'", .0.display())]
    Conflict(PathBuf),

    /// Restoration from trash failed.
    #[error("failed to restore '{}' from trash: {reason}", path.display())]
    TrashRestorationFailed {
        /// Target path that could not be restored.
        path: PathBuf,
        /// Reason restoration failed.
        reason: String,
    },

    /// File was modified after the forward copy operation.
    #[error("file at '{}' was modified after operation", path.display())]
    FileModifiedAfterCopy {
        /// Path of the modified file.
        path: PathBuf,
        /// Expected modification timestamp recorded at operation time.
        recorded_mtime: SystemTime,
        /// Current modification timestamp found on disk.
        current_mtime: SystemTime,
    },

    /// Standard I/O error occurred.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// Filesystem layer error from shared crate.
    #[error("filesystem error: {0}")]
    Fs(#[from] shared::errors::FsError),

    /// Shared error from shared crate.
    #[error("shared error: {0}")]
    Shared(#[from] shared::errors::SharedError),

    /// Serialization error occurred while reading or writing the journal.
    #[error("journal serialization error: {0}")]
    JournalSerialization(#[from] serde_json::Error),

    /// General operational failure.
    #[error("{0}")]
    OperationFailed(String),
}

/// Convenience type alias for Results returning [`UndoError`].
pub type Result<T, E = UndoError> = std::result::Result<T, E>;
