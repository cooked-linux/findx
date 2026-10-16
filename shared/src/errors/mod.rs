//! Unified domain error hierarchy for the `findx` shared crate.
//!
//! Subsystems (filesystem, extended attributes, database, IPC, models)
//! provide dedicated typed errors, all unified under [`SharedError`].

pub mod db;
pub mod fs;
pub mod ipc;
pub mod model;
pub mod xattr;

pub use db::DbError;
pub use fs::FsError;
pub use ipc::IpcError;
pub use model::ModelError;
pub use xattr::XattrError;

/// Root domain error unifying all subsystem errors across `findx`.
#[derive(Debug, thiserror::Error)]
pub enum SharedError {
    /// Filesystem-related errors (I/O, symlinks, trash, permissions).
    #[error(transparent)]
    Fs(#[from] FsError),

    /// Extended attribute errors.
    #[error(transparent)]
    Xattr(#[from] XattrError),

    /// Database or persistence errors.
    #[error(transparent)]
    Db(#[from] DbError),

    /// Inter-process communication or D-Bus errors.
    #[error(transparent)]
    Ipc(#[from] IpcError),

    /// Domain model validation and classification errors.
    #[error(transparent)]
    Model(#[from] ModelError),
}

/// Convenience alias for `std::result::Result<T, SharedError>`.
pub type Result<T> = std::result::Result<T, SharedError>;

impl From<std::io::Error> for SharedError {
    fn from(err: std::io::Error) -> Self {
        Self::Fs(FsError::from(err))
    }
}

impl From<rusqlite::Error> for SharedError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Db(DbError::from(err))
    }
}

impl From<zbus::Error> for SharedError {
    fn from(err: zbus::Error) -> Self {
        Self::Ipc(IpcError::from(err))
    }
}

impl From<trash::Error> for SharedError {
    fn from(err: trash::Error) -> Self {
        Self::Fs(FsError::from(err))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::path::PathBuf;

    // Helper functions testing transparent `?` bubbling
    fn io_failure() -> std::result::Result<(), std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "disk read denied",
        ))
    }

    fn bubble_to_fs() -> std::result::Result<(), FsError> {
        io_failure()?;
        Ok(())
    }

    fn bubble_to_shared() -> std::result::Result<(), SharedError> {
        bubble_to_fs()?;
        Ok(())
    }

    fn direct_io_to_shared() -> std::result::Result<(), SharedError> {
        io_failure()?;
        Ok(())
    }

    #[test]
    fn test_question_mark_bubbling_chain() {
        let Err(err) = bubble_to_shared() else {
            unreachable!("must fail with error");
        };

        match err {
            SharedError::Fs(FsError::Io { path, source }) => {
                assert_eq!(path, PathBuf::new());
                assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
            }
            _ => unreachable!("expected SharedError::Fs(FsError::Io)"),
        }

        // Test direct I/O conversion to SharedError
        let Err(direct_err) = direct_io_to_shared() else {
            unreachable!("must fail with error");
        };
        assert!(matches!(direct_err, SharedError::Fs(FsError::Io { .. })));
    }

    #[test]
    fn test_error_source_chain_traversal() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let fs_err = FsError::io(PathBuf::from("/etc/findx.conf"), io_err);
        let shared_err = SharedError::from(fs_err);

        // Verify source traversal through transparent SharedError
        let source_dyn = shared_err
            .source()
            .expect("shared error must traverse to inner source");
        let downcasted = source_dyn
            .downcast_ref::<std::io::Error>()
            .expect("source must downcast to std::io::Error");
        assert_eq!(downcasted.kind(), std::io::ErrorKind::NotFound);

        // Errors without sources should return None
        let not_found_err = FsError::not_found(PathBuf::from("/tmp/missing"));
        assert!(not_found_err.source().is_none());
    }

    #[test]
    fn test_fs_error_messages() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let err = FsError::io(PathBuf::from("/tmp/test.txt"), io_err);
        assert_eq!(err.to_string(), "i/o error at '/tmp/test.txt'");

        let err = FsError::NotFound(PathBuf::from("/tmp/missing.txt"));
        assert_eq!(err.to_string(), "path not found: '/tmp/missing.txt'");

        let err = FsError::PermissionDenied(PathBuf::from("/root/secret"));
        assert_eq!(err.to_string(), "permission denied: '/root/secret'");

        let err = FsError::CrossDeviceMove {
            src: PathBuf::from("/mnt/a"),
            target: PathBuf::from("/mnt/b"),
        };
        assert_eq!(
            err.to_string(),
            "cross-device move from '/mnt/a' to '/mnt/b'"
        );

        let err = FsError::InvalidSymlink {
            path: PathBuf::from("/symlink"),
            target: PathBuf::from("/target"),
        };
        assert_eq!(
            err.to_string(),
            "invalid symlink at '/symlink' pointing to '/target'"
        );

        let err = FsError::DirectoryCycle {
            path: PathBuf::from("/a/b"),
            cycle_point: PathBuf::from("/a"),
        };
        assert_eq!(
            err.to_string(),
            "directory cycle detected at '/a/b' pointing to '/a'"
        );

        let err = FsError::ReflinkUnsupported {
            src: PathBuf::from("/dev/sda1"),
            target: PathBuf::from("/dev/sdb1"),
        };
        assert_eq!(
            err.to_string(),
            "reflink cloning unsupported from '/dev/sda1' to '/dev/sdb1'"
        );

        let err = FsError::TrashError("operation refused by user".to_string());
        assert_eq!(err.to_string(), "trash error: operation refused by user");
    }

    #[test]
    fn test_xattr_error_messages_and_sources() {
        let err = XattrError::unsupported_filesystem(PathBuf::from("/mnt/vfat"));
        assert_eq!(
            err.to_string(),
            "extended attributes not supported on filesystem for '/mnt/vfat'"
        );

        let err = XattrError::attribute_not_found(PathBuf::from("/file.txt"), "user.xdg.tags");
        assert_eq!(
            err.to_string(),
            "extended attribute 'user.xdg.tags' not found on '/file.txt'"
        );

        let invalid_utf8_bytes = vec![0xFF, 0xFE, 0xFD];
        let Err(utf8_err) = String::from_utf8(invalid_utf8_bytes) else {
            unreachable!("invalid bytes");
        };
        let err = XattrError::invalid_encoding("user.xdg.comment", utf8_err);
        assert_eq!(
            err.to_string(),
            "invalid UTF-8 encoding for extended attribute 'user.xdg.comment'"
        );
        assert!(err.source().is_some());

        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken");
        let err = XattrError::io(PathBuf::from("/file.txt"), "user.xdg.color", io_err);
        assert_eq!(
            err.to_string(),
            "i/o error for extended attribute 'user.xdg.color' on '/file.txt'"
        );
        assert!(err.source().is_some());

        let err = XattrError::value_too_large("user.xdg.tags", 70_000, 65_536);
        assert_eq!(
            err.to_string(),
            "extended attribute 'user.xdg.tags' value size (70000 bytes) exceeds limit of 65536 bytes"
        );
    }

    #[test]
    fn test_db_error_messages_and_sources() {
        let sqlite_err = rusqlite::Error::ExecuteReturnedResults;
        let err = DbError::from(sqlite_err);
        assert_eq!(
            err.to_string(),
            "sqlite error: Execute returned results - did you mean to call query?"
        );
        assert!(err.source().is_some());

        let err = DbError::migration_failed(3, "table already exists");
        assert_eq!(
            err.to_string(),
            "database migration to version 3 failed: table already exists"
        );

        let err = DbError::PoolExhausted;
        assert_eq!(err.to_string(), "database connection pool exhausted");

        let err = DbError::corrupted_index("fts5 index invalid segment offset");
        assert_eq!(
            err.to_string(),
            "corrupted search index: fts5 index invalid segment offset"
        );

        let Err(json_err) = serde_json::from_str::<serde_json::Value>("{ invalid }") else {
            unreachable!("invalid json");
        };
        let err = DbError::from(json_err);
        assert!(err.to_string().starts_with("database serialization error:"));
        assert!(err.source().is_some());
    }

    #[test]
    fn test_ipc_error_messages() {
        let err = IpcError::service_unavailable("org.freedesktop.FileManager1");
        assert_eq!(
            err.to_string(),
            "service unavailable: org.freedesktop.FileManager1"
        );

        let err = IpcError::timeout("ShowItems", 1500);
        assert_eq!(
            err.to_string(),
            "ipc call to 'ShowItems' timed out after 1500ms"
        );

        let err = IpcError::portal_error("user denied shortcut permission");
        assert_eq!(
            err.to_string(),
            "desktop portal error: user denied shortcut permission"
        );

        let err = IpcError::invalid_response("malformed signature in reply");
        assert_eq!(
            err.to_string(),
            "invalid ipc response: malformed signature in reply"
        );
    }

    #[test]
    fn test_model_error_messages() {
        let err = ModelError::validation("file size cannot be negative");
        assert_eq!(
            err.to_string(),
            "validation failed: file size cannot be negative"
        );

        let err = ModelError::invalid_tag_color("#ZZ9900");
        assert_eq!(err.to_string(), "invalid tag color: '#ZZ9900'");

        let err = ModelError::invalid_tag_format("tag missing delimiter");
        assert_eq!(
            err.to_string(),
            "invalid tag format: 'tag missing delimiter'"
        );

        let err = ModelError::invalid_mime_category("application/unknown");
        assert_eq!(
            err.to_string(),
            "invalid MIME category: 'application/unknown'"
        );

        let err = ModelError::invalid_operation("cannot restore active operation");
        assert_eq!(
            err.to_string(),
            "invalid operation state: cannot restore active operation"
        );

        let err = ModelError::invalid_path(PathBuf::from("../outside"));
        assert_eq!(err.to_string(), "invalid path: '../outside'");
    }

    #[test]
    fn test_shared_error_transparent_display() {
        let fs_err = FsError::NotFound(PathBuf::from("/dev/null/item"));
        let expected = fs_err.to_string();
        let shared: SharedError = fs_err.into();
        assert_eq!(shared.to_string(), expected);

        let xattr_err = XattrError::UnsupportedFilesystem(PathBuf::from("/proc"));
        let expected = xattr_err.to_string();
        let shared: SharedError = xattr_err.into();
        assert_eq!(shared.to_string(), expected);

        let db_err = DbError::PoolExhausted;
        let expected = db_err.to_string();
        let shared: SharedError = db_err.into();
        assert_eq!(shared.to_string(), expected);

        let ipc_err = IpcError::ServiceUnavailable("org.freedesktop.login1".into());
        let expected = ipc_err.to_string();
        let shared: SharedError = ipc_err.into();
        assert_eq!(shared.to_string(), expected);

        let model_err = ModelError::Validation("invalid name".into());
        let expected = model_err.to_string();
        let shared: SharedError = model_err.into();
        assert_eq!(shared.to_string(), expected);
    }
}
