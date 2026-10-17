//! Strongly typed errors for the spotlight application.

use thiserror::Error;

/// Errors produced by Spotlight operations.
#[derive(Debug, Error)]
pub enum SpotlightError {
    /// Failure during global hotkey registration or activation.
    #[error("hotkey error: {0}")]
    Hotkey(String),

    /// Inter-process communication or D-Bus portal error.
    #[error("IPC error: {0}")]
    Ipc(#[from] shared::errors::SharedError),

    /// GTK4, libadwaita, or Layer Shell initialization error.
    #[error("GTK error: {0}")]
    Gtk(String),
}

/// Specialized Result alias for Spotlight operations.
pub type Result<T> = std::result::Result<T, SpotlightError>;
