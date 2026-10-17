//! Unified domain error hierarchy for the `finder` crate.

/// Domain errors encountered during Finder operation.
#[derive(Debug, thiserror::Error)]
pub enum FinderError {
    /// Filesystem error originated from underlying storage or filesystem operations.
    #[error("Filesystem error: {0}")]
    Fs(#[from] shared::errors::FsError),

    /// Standard I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Path requested for navigation is invalid or inaccessible.
    #[error("Invalid navigation path: {0}")]
    InvalidPath(String),

    /// Navigation history stack transition error.
    #[error("History navigation error: {0}")]
    History(String),

    /// UI initialization or widget hierarchy error.
    #[error("UI initialization error: {0}")]
    Ui(String),

    /// D-Bus IPC communication error.
    #[error("D-Bus IPC error: {0}")]
    Ipc(#[from] shared::errors::IpcError),
}

/// Convenience alias for results returning [`FinderError`].
pub type Result<T> = std::result::Result<T, FinderError>;
