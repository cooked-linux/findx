//! Domain error types for the Smart Folders and Saved Search subsystem.

use std::path::PathBuf;

use thiserror::Error;

/// Errors raised while compiling, executing, or persisting smart folders.
#[derive(Debug, Error)]
pub enum SmartFolderError {
    /// Filesystem I/O failure tied to a specific path.
    #[error("I/O error at '{path}': {source}")]
    Io {
        /// Path whose I/O operation failed.
        path: PathBuf,
        /// Underlying operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// Malformed or unreadable `.savedSearch` JSON document.
    #[error("Failed to parse .savedSearch JSON at '{path}': {message}")]
    Serialization {
        /// Path of the offending saved search file.
        path: PathBuf,
        /// Human-readable parse failure description.
        message: String,
    },
    /// The persisted schema version is not supported by this build.
    #[error("Unsupported schema version: '{found}' (expected '{expected}')")]
    InvalidSchemaVersion {
        /// Version string discovered in the file.
        found: String,
        /// Version string supported by this build.
        expected: String,
    },
    /// SQLite or database-layer failure.
    #[error("Database error: {message}")]
    Database {
        /// Diagnostic message describing the failure.
        message: String,
    },
    /// Criteria attribute, operator, or value combination is invalid.
    #[error("Invalid criteria attribute '{attribute}': {reason}")]
    InvalidCriteria {
        /// Attribute name that failed validation.
        attribute: String,
        /// Explanation of the validation failure.
        reason: String,
    },
    /// A configured search scope is missing or unreadable.
    #[error("Scope path does not exist or is unreadable: '{path}'")]
    ScopeNotFound {
        /// Missing or unreadable scope path.
        path: PathBuf,
    },
    /// The query was superseded by a newer monotonic query identifier.
    #[error("Query was superseded by newer query ID")]
    Cancelled,
}

impl SmartFolderError {
    /// Builds an [`SmartFolderError::Io`] with contextual path information.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Convenience result alias for smart folder operations.
pub type Result<T> = std::result::Result<T, SmartFolderError>;
