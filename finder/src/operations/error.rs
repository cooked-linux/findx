//! Operations domain error types.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during batch file operations.
#[derive(Debug, Error)]
pub enum OperationError {
    /// An I/O error occurred on a specific file system path.
    #[error("I/O error at '{path}': {source}")]
    Io {
        /// File system path where the error occurred.
        path: PathBuf,
        /// Underlying I/O error source.
        #[source]
        source: std::io::Error,
    },
    /// Insufficient disk space on the target destination.
    #[error(
        "Insufficient disk space on destination '{path}': required {required_bytes} bytes, available {available_bytes} bytes"
    )]
    InsufficientSpace {
        /// Target destination path.
        path: PathBuf,
        /// Bytes required for operation.
        required_bytes: u64,
        /// Bytes available on device.
        available_bytes: u64,
    },
    /// The operation was explicitly cancelled by the user.
    #[error("Operation was cancelled by user")]
    Cancelled,
    /// Operation timed out or was aborted.
    #[error("Operation timed out or aborted")]
    Aborted(String),
    /// Invalid operation parameters supplied.
    #[error("Invalid operation parameter: {0}")]
    Invalid(String),
    /// Permission denied on the path.
    #[error("Permission denied at '{path}': requires elevation")]
    PermissionDenied {
        /// Path where permission was denied.
        path: PathBuf,
    },
    /// A symlink recursion loop was detected.
    #[error("Symlink loop detected at '{path}'")]
    SymlinkLoop {
        /// Path at which symlink recursion was detected.
        path: PathBuf,
    },
    /// Error originating from shared models.
    #[error("Shared model error: {0}")]
    Model(#[from] shared::errors::ModelError),
    /// Error originating from shared filesystem layer.
    #[error("Filesystem error: {0}")]
    Fs(#[from] shared::errors::FsError),
    /// Error originating from shared crate.
    #[error("Shared error: {0}")]
    Shared(#[from] shared::errors::SharedError),
}

/// Convenience alias for `std::result::Result<T, OperationError>`.
pub type Result<T> = std::result::Result<T, OperationError>;
