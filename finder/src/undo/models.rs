//! Domain models and data structures for transactional undo/redo file operations.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// Type of filesystem operation tracked in the undo history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UndoOperationType {
    /// Moving files or directories to a different location.
    Move,
    /// Copying files or directories.
    Copy,
    /// Duplicating files or directories in-place.
    Duplicate,
    /// Renaming a file or directory.
    Rename,
    /// Moving files or directories to the Freedesktop Trash.
    Trash,
    /// Creating a new folder and populating it with selected items.
    NewFolderWithSelection,
}

/// Metadata record for an individual file or directory affected by an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOperationRecord {
    /// Original source path before forward operation.
    pub source_path: PathBuf,
    /// Resulting destination path after forward operation.
    pub destination_path: PathBuf,
    /// Inode number at time of operation (for identity verification).
    pub inode_number: u64,
    /// Filesystem device ID.
    pub device_id: u64,
    /// Timestamp when forward operation executed.
    pub timestamp: SystemTime,
    /// Freedesktop trashinfo relative path (for Trash operations).
    pub trash_info_rel_path: Option<PathBuf>,
}

impl FileOperationRecord {
    /// Creates a new [`FileOperationRecord`].
    #[must_use]
    pub const fn new(
        source_path: PathBuf,
        destination_path: PathBuf,
        inode_number: u64,
        device_id: u64,
        timestamp: SystemTime,
        trash_info_rel_path: Option<PathBuf>,
    ) -> Self {
        Self {
            source_path,
            destination_path,
            inode_number,
            device_id,
            timestamp,
            trash_info_rel_path,
        }
    }
}

/// Atomic transaction encapsulating one or more file operations that can be undone or redone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoTransaction {
    /// Unique transaction identifier (UUID v4 or nanoid).
    pub id: String,
    /// Operation category.
    pub op_type: UndoOperationType,
    /// Dynamic UI menu label (e.g. "Move of 3 Items").
    pub description: String,
    /// Individual file records comprising this atomic transaction.
    pub records: Vec<FileOperationRecord>,
    /// Timestamp when transaction was recorded.
    pub created_at: SystemTime,
}

impl UndoTransaction {
    /// Creates a new [`UndoTransaction`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        op_type: UndoOperationType,
        description: impl Into<String>,
        records: Vec<FileOperationRecord>,
        created_at: SystemTime,
    ) -> Self {
        Self {
            id: id.into(),
            op_type,
            description: description.into(),
            records,
            created_at,
        }
    }

    /// Formats the UI menu label for undoing this transaction.
    #[must_use]
    pub fn undo_label(&self) -> String {
        format!("Undo {}", self.description)
    }

    /// Formats the UI menu label for redoing this transaction.
    #[must_use]
    pub fn redo_label(&self) -> String {
        format!("Redo {}", self.description)
    }
}
