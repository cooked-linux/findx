//! Domain errors for the archive creation and compression engine.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that may occur during archive creation or compression.
#[derive(Debug, Error)]
pub enum ArchiveError {
    /// Standard I/O failure on a disk path.
    #[error("I/O error at '{path}': {source}")]
    Io {
        /// Filesystem path where the error occurred.
        path: PathBuf,
        /// Underlying I/O error source.
        #[source]
        source: std::io::Error,
    },
    /// Low-level libarchive FFI failure.
    #[error("libarchive error ({code}): {message}")]
    Libarchive {
        /// Return code or errno from libarchive.
        code: i32,
        /// Diagnostic message from libarchive.
        message: String,
    },
    /// The user explicitly cancelled compression.
    #[error("Operation was cancelled by user")]
    Cancelled,
    /// No source items were selected for compression.
    #[error("Source selection cannot be empty")]
    EmptySelection,
    /// Destination folder does not exist or is invalid.
    #[error("Destination directory '{0}' does not exist or is not a directory")]
    InvalidDestination(PathBuf),
    /// Target filesystem has insufficient space for compression.
    #[error(
        "Insufficient disk space on destination '{path}': required {required} bytes, available {available} bytes"
    )]
    InsufficientSpace {
        /// Destination path checked.
        path: PathBuf,
        /// Required byte estimate.
        required: u64,
        /// Available bytes on filesystem.
        available: u64,
    },
    /// Failed to clean up staging temporary file upon cancellation or error.
    #[error("Temporary staging cleanup failed for '{path}': {source}")]
    CleanupFailed {
        /// Staging path that could not be removed.
        path: PathBuf,
        /// Underlying I/O error source.
        #[source]
        source: std::io::Error,
    },
    /// Failure during preflight filesystem walk or stat.
    #[error("Preflight scan failed for '{path}': {message}")]
    PreflightFailed {
        /// Path that caused preflight failure.
        path: PathBuf,
        /// Diagnostic details.
        message: String,
    },
    /// Unrecognized or unsupported archive format string.
    #[error("Invalid archive format: {0}")]
    InvalidFormat(String),
}

/// Convenience alias for `std::result::Result<T, ArchiveError>`.
pub type Result<T> = std::result::Result<T, ArchiveError>;
