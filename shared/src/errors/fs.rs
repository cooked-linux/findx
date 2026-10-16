//! Filesystem-related error definitions.

use std::path::PathBuf;

/// Errors arising from filesystem operations, paths, symlinks, or trash management.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    /// Underlying I/O error associated with a specific filesystem path.
    #[error("i/o error at '{}'", path.display())]
    Io {
        /// Path where the I/O error occurred.
        path: PathBuf,
        /// Underlying I/O error source.
        #[source]
        source: std::io::Error,
    },

    /// The requested path was not found on disk.
    #[error("path not found: '{}'", .0.display())]
    NotFound(PathBuf),

    /// Operation was rejected due to insufficient permissions.
    #[error("permission denied: '{}'", .0.display())]
    PermissionDenied(PathBuf),

    /// File move operation attempted across different filesystem boundaries without fallback.
    #[error("cross-device move from '{}' to '{}'", src.display(), target.display())]
    CrossDeviceMove {
        /// Source path on the originating filesystem.
        src: PathBuf,
        /// Destination path on the target filesystem.
        target: PathBuf,
    },

    /// Symlink target is invalid, broken, or cyclical.
    #[error("invalid symlink at '{}' pointing to '{}'", path.display(), target.display())]
    InvalidSymlink {
        /// Location of the symlink.
        path: PathBuf,
        /// Target path referenced by the symlink.
        target: PathBuf,
    },

    /// Recursive directory traversal encountered a cyclical reference.
    #[error("directory cycle detected at '{}' pointing to '{}'", path.display(), cycle_point.display())]
    DirectoryCycle {
        /// Directory where traversal detected recursion.
        path: PathBuf,
        /// Previously visited ancestor path completing the cycle.
        cycle_point: PathBuf,
    },

    /// Reflink copy (copy-on-write) is not supported by the underlying filesystem.
    #[error("reflink cloning unsupported from '{}' to '{}'", src.display(), target.display())]
    ReflinkUnsupported {
        /// Source path for reflink copy.
        src: PathBuf,
        /// Target path for reflink copy.
        target: PathBuf,
    },

    /// Error occurred while moving items to or from the Freedesktop Trash.
    #[error("trash error: {0}")]
    TrashError(String),
}

impl FsError {
    /// Constructs an [`FsError::Io`] variant wrapping an I/O error with its associated path.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Constructs an [`FsError::NotFound`] variant for the specified path.
    #[must_use]
    pub fn not_found(path: impl Into<PathBuf>) -> Self {
        Self::NotFound(path.into())
    }

    /// Constructs an [`FsError::PermissionDenied`] variant for the specified path.
    #[must_use]
    pub fn permission_denied(path: impl Into<PathBuf>) -> Self {
        Self::PermissionDenied(path.into())
    }
}

impl From<std::io::Error> for FsError {
    fn from(source: std::io::Error) -> Self {
        Self::Io {
            path: PathBuf::new(),
            source,
        }
    }
}

impl From<trash::Error> for FsError {
    fn from(err: trash::Error) -> Self {
        Self::TrashError(err.to_string())
    }
}
