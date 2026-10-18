//! Quick Look error types and results.

use std::path::PathBuf;
use thiserror::Error;

/// Domain error variants for Quick Look preview operations.
#[derive(Debug, Error)]
pub enum QuickLookError {
    /// File I/O failure when reading, writing, or inspecting preview targets.
    #[error("I/O error at '{path}': {source}")]
    Io {
        /// Target path where the I/O error occurred.
        path: PathBuf,
        /// Underlying standard I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Isolated Bubblewrap or sub-process execution failure.
    #[error("Sandbox execution failed for '{path}': {message}")]
    Sandbox {
        /// Target file path being processed in sandbox.
        path: PathBuf,
        /// Detail or stderr message from sandbox failure.
        message: String,
    },

    /// Preview generation exceeded its configured deadline.
    #[error("Preview generation timed out after {timeout_ms}ms for '{path}'")]
    Timeout {
        /// Target file path that timed out.
        path: PathBuf,
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// File format cannot be parsed by any registered generator.
    #[error("Unsupported MIME format: {mime_type}")]
    UnsupportedFormat {
        /// The unsupported MIME type string.
        mime_type: String,
    },

    /// Request was superseded by a newer preview navigation request.
    #[error("Operation was cancelled by newer request")]
    Cancelled,

    /// Format-specific decoding failure.
    #[error("Decoding failed for '{path}': {message}")]
    DecodingFailed {
        /// Target file path that failed decoding.
        path: PathBuf,
        /// Error message or decoder detail.
        message: String,
    },

    /// Thumbnail cache or Freedesktop thumbnail generator error.
    #[error("Thumbnail error: {0}")]
    Thumbnail(#[from] shared::thumbnail::ThumbnailError),
}

impl QuickLookError {
    /// Helper to construct an `Io` error.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Helper to construct a `Sandbox` error.
    #[must_use]
    pub fn sandbox(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Sandbox {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Helper to construct a `Timeout` error.
    #[must_use]
    pub fn timeout(path: impl Into<PathBuf>, timeout_ms: u64) -> Self {
        Self::Timeout {
            path: path.into(),
            timeout_ms,
        }
    }

    /// Helper to construct an `UnsupportedFormat` error.
    #[must_use]
    pub fn unsupported(mime_type: impl Into<String>) -> Self {
        Self::UnsupportedFormat {
            mime_type: mime_type.into(),
        }
    }

    /// Helper to construct a `DecodingFailed` error.
    #[must_use]
    pub fn decoding_failed(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::DecodingFailed {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// Convenience alias for Quick Look fallible results.
pub type Result<T> = std::result::Result<T, QuickLookError>;
