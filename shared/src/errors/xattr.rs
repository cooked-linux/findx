//! Extended attribute error definitions.

use std::path::PathBuf;

/// Errors arising from reading, writing, or decoding extended attributes (xattrs).
#[derive(Debug, thiserror::Error)]
pub enum XattrError {
    /// Filesystem does not support extended attributes.
    #[error("extended attributes not supported on filesystem for '{}'", .0.display())]
    UnsupportedFilesystem(PathBuf),

    /// The specified extended attribute does not exist on the target path.
    #[error("extended attribute '{attr}' not found on '{}'", path.display())]
    AttributeNotFound {
        /// Path where the attribute was queried.
        path: PathBuf,
        /// Name of the missing attribute.
        attr: String,
    },

    /// Value retrieved from the extended attribute is not valid UTF-8.
    #[error("invalid UTF-8 encoding for extended attribute '{attr}'")]
    InvalidEncoding {
        /// Name of the attribute with invalid encoding.
        attr: String,
        /// Underlying UTF-8 decoding error source.
        #[source]
        source: std::string::FromUtf8Error,
    },

    /// I/O error occurred during extended attribute access.
    #[error("i/o error for extended attribute '{attr}' on '{}'", path.display())]
    Io {
        /// Target path where the operation failed.
        path: PathBuf,
        /// Attribute name involved in the operation.
        attr: String,
        /// Underlying I/O error source.
        #[source]
        source: std::io::Error,
    },

    /// Attribute value exceeds the maximum allowable size (e.g. 64KB for user xattrs).
    #[error("extended attribute '{attr}' value size ({size} bytes) exceeds limit of {limit} bytes")]
    ValueTooLarge {
        /// Attribute name whose value exceeded limits.
        attr: String,
        /// Actual size of the value in bytes.
        size: usize,
        /// Maximum allowable size in bytes.
        limit: usize,
    },
}

impl XattrError {
    /// Constructs an [`XattrError::UnsupportedFilesystem`] variant.
    #[must_use]
    pub fn unsupported_filesystem(path: impl Into<PathBuf>) -> Self {
        Self::UnsupportedFilesystem(path.into())
    }

    /// Constructs an [`XattrError::AttributeNotFound`] variant.
    #[must_use]
    pub fn attribute_not_found(path: impl Into<PathBuf>, attr: impl Into<String>) -> Self {
        Self::AttributeNotFound {
            path: path.into(),
            attr: attr.into(),
        }
    }

    /// Constructs an [`XattrError::InvalidEncoding`] variant.
    #[must_use]
    pub fn invalid_encoding(attr: impl Into<String>, source: std::string::FromUtf8Error) -> Self {
        Self::InvalidEncoding {
            attr: attr.into(),
            source,
        }
    }

    /// Constructs an [`XattrError::Io`] variant.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, attr: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            attr: attr.into(),
            source,
        }
    }

    /// Constructs an [`XattrError::ValueTooLarge`] variant.
    #[must_use]
    pub fn value_too_large(attr: impl Into<String>, size: usize, limit: usize) -> Self {
        Self::ValueTooLarge {
            attr: attr.into(),
            size,
            limit,
        }
    }
}
