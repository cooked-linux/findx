//! Deep metadata extraction and inspection domain errors.

use std::path::PathBuf;
use thiserror::Error;

/// Domain error type representing failures encountered during deep metadata inspection.
#[derive(Debug, Error)]
pub enum MetadataError {
    /// Filesystem I/O error reading metadata for a path.
    #[error("I/O error reading metadata for '{path}': {source}")]
    Io {
        /// Target path where I/O failure occurred.
        path: PathBuf,
        /// Underlying standard I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Extended attribute error at a given path.
    #[error("Extended attribute error at '{0}': {1}")]
    Xattr(PathBuf, #[source] std::io::Error),
    /// Format-specific parsing error.
    #[error("Failed to parse metadata payload for '{path}': {message}")]
    ParseFailed {
        /// Target path of file being parsed.
        path: PathBuf,
        /// Detail message describing parse failure.
        message: String,
    },
    /// SQLite tag cache or fallback database error.
    #[error("Database error in tag index: {0}")]
    Database(#[from] rusqlite::Error),
    /// Permission denied modifying metadata or extended attributes.
    #[error("Permission denied modifying metadata on '{path}'")]
    PermissionDenied {
        /// Target path where permission was denied.
        path: PathBuf,
    },
}

/// Convenience alias for results returned by metadata operations.
pub type Result<T> = std::result::Result<T, MetadataError>;

impl MetadataError {
    /// Constructs a new [`MetadataError::Io`] variant.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Constructs a new [`MetadataError::Xattr`] variant.
    pub fn xattr(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Xattr(path.into(), source)
    }

    /// Constructs a new [`MetadataError::ParseFailed`] variant.
    pub fn parse_failed(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::ParseFailed {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Constructs a new [`MetadataError::PermissionDenied`] variant.
    pub fn permission_denied(path: impl Into<PathBuf>) -> Self {
        Self::PermissionDenied { path: path.into() }
    }
}
