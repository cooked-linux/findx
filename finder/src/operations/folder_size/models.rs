//! Domain models and error types for asynchronous folder size calculation.

use std::path::PathBuf;
use std::time::SystemTime;

/// Calculation results representing recursive size and item counts for a directory tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FolderSizeResult {
    /// Physical allocated disk bytes (`st_blocks` * 512).
    pub total_bytes: u64,
    /// Total count of regular files within tree.
    pub file_count: u64,
    /// Total count of subdirectories within tree.
    pub folder_count: u64,
    /// Modification timestamp of the folder at calculation time.
    pub dir_mtime: SystemTime,
    /// When this cache record was calculated.
    pub calculated_at: SystemTime,
}

impl FolderSizeResult {
    /// Returns the combined total number of files and subdirectories.
    #[must_use]
    pub const fn total_items(&self) -> u64 {
        self.file_count + self.folder_count
    }

    /// Formats the physical byte size into human-readable text (e.g., "1.42 GB").
    #[must_use]
    pub fn formatted_size(&self) -> String {
        glib::format_size(self.total_bytes).to_string()
    }

    /// Formats size and item count into a standard summary string (e.g. "1.42 GB (3,412 items)").
    #[must_use]
    pub fn formatted_summary(&self) -> String {
        format!("{} ({} items)", self.formatted_size(), self.total_items())
    }
}

/// Scheduling priority for background folder calculation jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Priority {
    /// Off-screen prefetch or idle background sizing.
    #[default]
    Low = 0,
    /// Currently visible screen rows or active user inspection (e.g. Get Info).
    High = 1,
}

/// Errors that can occur during folder size calculation or caching.
#[derive(Debug, thiserror::Error)]
pub enum FolderSizeError {
    /// The target directory was not found.
    #[error("directory not found: {0}")]
    NotFound(PathBuf),

    /// Permission was denied to inspect the directory.
    #[error("permission denied: {0}")]
    PermissionDenied(PathBuf),

    /// The calculation was cancelled.
    #[error("folder size calculation cancelled")]
    Cancelled,

    /// An I/O error occurred while reading the filesystem.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// File or directory path where the error occurred.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// SQLite database error.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}
