//! Domain model error definitions.

use std::path::PathBuf;

/// Errors arising during domain model parsing, validation, or transformation.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// Validation failed for a model field or entity.
    #[error("validation failed: {0}")]
    Validation(String),

    /// Tag color string or hex value is invalid.
    #[error("invalid tag color: '{0}'")]
    InvalidTagColor(String),

    /// Tag extended attribute format is malformed.
    #[error("invalid tag format: '{0}'")]
    InvalidTagFormat(String),

    /// Unknown or unsupported MIME category classification.
    #[error("invalid MIME category: '{0}'")]
    InvalidMimeCategory(String),

    /// File operation state machine transition is invalid.
    #[error("invalid operation state: {0}")]
    InvalidOperation(String),

    /// Invalid filesystem path provided to domain model.
    #[error("invalid path: '{}'", .0.display())]
    InvalidPath(PathBuf),

    /// Serialization or deserialization error for domain model.
    #[error("model serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl ModelError {
    /// Constructs a [`ModelError::Validation`] variant.
    #[must_use]
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// Constructs a [`ModelError::InvalidTagColor`] variant.
    #[must_use]
    pub fn invalid_tag_color(color: impl Into<String>) -> Self {
        Self::InvalidTagColor(color.into())
    }

    /// Constructs a [`ModelError::InvalidTagFormat`] variant.
    #[must_use]
    pub fn invalid_tag_format(format: impl Into<String>) -> Self {
        Self::InvalidTagFormat(format.into())
    }

    /// Constructs a [`ModelError::InvalidMimeCategory`] variant.
    #[must_use]
    pub fn invalid_mime_category(category: impl Into<String>) -> Self {
        Self::InvalidMimeCategory(category.into())
    }

    /// Constructs a [`ModelError::InvalidOperation`] variant.
    #[must_use]
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }

    /// Constructs a [`ModelError::InvalidPath`] variant.
    #[must_use]
    pub fn invalid_path(path: impl Into<PathBuf>) -> Self {
        Self::InvalidPath(path.into())
    }
}
