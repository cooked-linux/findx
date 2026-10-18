//! Domain models, action types, and resolution contexts for conflict handling.

use std::path::PathBuf;
use std::time::SystemTime;

/// User or policy action to resolve a naming or path collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ConflictAction {
    /// Completely overwrite the target file or directory with incoming item.
    Replace,
    /// Rename incoming item with an auto-incrementing suffix (e.g. "File (1).txt").
    KeepBoth,
    /// Recursively combine directory trees, keeping newer files when items collide.
    Merge,
    /// Bypass this conflicting item and continue with remaining batch items.
    Skip,
    /// Immediately terminate and abort the entire batch operation.
    Stop,
}

/// Relative temporal status comparing source and destination modification times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RelativeTimeStatus {
    /// The incoming source item was modified after the destination item.
    IncomingNewer,
    /// The incoming source item was modified before the destination item.
    IncomingOlder,
    /// The timestamps are identical within a 1.0 second tolerance window.
    IdenticalTime,
}

/// Metadata snapshot for an item involved in a conflict collision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConflictItemMeta {
    /// File system path of the item.
    pub path: PathBuf,
    /// Whether the item is a directory.
    pub is_dir: bool,
    /// File size in bytes (0 for directories).
    pub size_bytes: u64,
    /// Modification time, if available.
    pub modified: Option<SystemTime>,
    /// Optional thumbnail path for UI preview.
    pub thumbnail_path: Option<PathBuf>,
}

impl ConflictItemMeta {
    /// Constructs a new [`ConflictItemMeta`].
    #[must_use]
    pub const fn new(
        path: PathBuf,
        is_dir: bool,
        size_bytes: u64,
        modified: Option<SystemTime>,
        thumbnail_path: Option<PathBuf>,
    ) -> Self {
        Self {
            path,
            is_dir,
            size_bytes,
            modified,
            thumbnail_path,
        }
    }
}

/// Detailed context comparing conflicting source and destination items.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ConflictContext {
    /// Identifier of the active batch operation.
    pub operation_id: String,
    /// Metadata for incoming source item.
    pub source: ConflictItemMeta,
    /// Metadata for existing destination item.
    pub destination: ConflictItemMeta,
    /// Whether this is a directory-against-directory collision.
    pub is_directory_collision: bool,
    /// Relative age comparison between source and destination.
    pub relative_time: RelativeTimeStatus,
}

impl ConflictContext {
    /// Constructs a new [`ConflictContext`].
    #[must_use]
    pub const fn new(
        operation_id: String,
        source: ConflictItemMeta,
        destination: ConflictItemMeta,
        is_directory_collision: bool,
        relative_time: RelativeTimeStatus,
    ) -> Self {
        Self {
            operation_id,
            source,
            destination,
            is_directory_collision,
            relative_time,
        }
    }
}

/// Resolution decision chosen by user or batch policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ConflictResolution {
    /// Resolution action chosen.
    pub action: ConflictAction,
    /// Whether to apply this decision to all remaining collisions in the batch.
    pub apply_to_all: bool,
}

impl ConflictResolution {
    /// Constructs a new [`ConflictResolution`].
    #[must_use]
    pub const fn new(action: ConflictAction, apply_to_all: bool) -> Self {
        Self {
            action,
            apply_to_all,
        }
    }
}
