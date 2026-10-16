//! Extended attributes (xattrs) management for Linux filesystem.
//!
//! Handles tags, color labels, user comments, and origin download URLs
//! using standard `user.xdg.*` namespaces.

use std::path::Path;

use crate::errors::{FsError, Result, XattrError};
use crate::models::{FileTag, TagColor};

/// Attribute name for user tags.
pub const XATTR_TAGS: &str = "user.xdg.tags";

/// Attribute name for user color label.
pub const XATTR_COLOR: &str = "user.xdg.color";

/// Attribute name for user comments.
pub const XATTR_COMMENT: &str = "user.xdg.comment";

/// Attribute name for download origin URL.
pub const XATTR_ORIGIN_URL: &str = "user.xdg.origin.url";

/// Maximum allowable size for an extended attribute value (64 KB).
pub const MAX_XATTR_SIZE: usize = 65_536;

/// Checks whether an I/O error indicates that extended attributes are unsupported.
fn is_enotsup(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::Unsupported
        || err.raw_os_error() == Some(libc::ENOTSUP)
        || err.raw_os_error() == Some(libc::EOPNOTSUPP)
}

/// Checks whether an I/O error indicates attribute missing (ENODATA / ENOATTR).
fn is_enodata(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(libc::ENODATA)
}

/// Retrieves an extended attribute string value, safely handling missing attributes
/// and filesystems lacking extended attribute support.
fn get_xattr_string(path: &Path, attr: &str) -> Result<Option<String>> {
    if !path.exists() {
        return Err(FsError::not_found(path).into());
    }

    match xattr::get(path, attr) {
        Ok(Some(bytes)) => {
            let s = String::from_utf8(bytes)
                .map_err(|e| XattrError::invalid_encoding(attr, e))?;
            Ok(Some(s))
        }
        Ok(None) => Ok(None),
        Err(err) => {
            if is_enotsup(&err) || is_enodata(&err) {
                Ok(None)
            } else if err.kind() == std::io::ErrorKind::NotFound {
                Err(FsError::not_found(path).into())
            } else if err.kind() == std::io::ErrorKind::PermissionDenied {
                Err(FsError::permission_denied(path).into())
            } else {
                Err(XattrError::io(path, attr, err).into())
            }
        }
    }
}

/// Sets an extended attribute string value with size boundary validation.
fn set_xattr_string(path: &Path, attr: &str, value: &str) -> Result<()> {
    if !path.exists() {
        return Err(FsError::not_found(path).into());
    }

    let bytes = value.as_bytes();
    if bytes.len() > MAX_XATTR_SIZE {
        return Err(XattrError::value_too_large(attr, bytes.len(), MAX_XATTR_SIZE).into());
    }

    match xattr::set(path, attr, bytes) {
        Ok(()) => Ok(()),
        Err(err) => {
            if is_enotsup(&err) {
                Err(XattrError::unsupported_filesystem(path).into())
            } else if err.kind() == std::io::ErrorKind::NotFound {
                Err(FsError::not_found(path).into())
            } else if err.kind() == std::io::ErrorKind::PermissionDenied {
                Err(FsError::permission_denied(path).into())
            } else {
                Err(XattrError::io(path, attr, err).into())
            }
        }
    }
}

/// Removes an extended attribute from a file, ignoring missing attributes.
fn remove_xattr(path: &Path, attr: &str) -> Result<()> {
    if !path.exists() {
        return Err(FsError::not_found(path).into());
    }

    match xattr::remove(path, attr) {
        Ok(()) => Ok(()),
        Err(err) => {
            if is_enotsup(&err) || is_enodata(&err) || err.kind() == std::io::ErrorKind::NotFound {
                Ok(())
            } else if err.kind() == std::io::ErrorKind::PermissionDenied {
                Err(FsError::permission_denied(path).into())
            } else {
                Err(XattrError::io(path, attr, err).into())
            }
        }
    }
}

/// Retrieves file tags stored in extended attributes (`user.xdg.tags` and `user.xdg.color`).
///
/// # Errors
/// Returns an error if the path does not exist or an unexpected I/O error occurs.
/// Filesystems without xattr support gracefully return an empty list.
pub fn get_tags(path: &Path) -> Result<Vec<FileTag>> {
    let raw_tags = get_xattr_string(path, XATTR_TAGS)?;
    let raw_color = get_xattr_string(path, XATTR_COLOR)?;

    let color_opt = raw_color.as_deref().and_then(|c| {
        c.trim()
            .parse::<u8>()
            .ok()
            .and_then(TagColor::from_index)
            .or_else(|| c.trim().parse::<TagColor>().ok())
    });

    let mut tags = match raw_tags {
        Some(content) if !content.trim().is_empty() => FileTag::parse_xdg_string(&content),
        _ => Vec::new(),
    };

    if let Some(color) = color_opt {
        if tags.is_empty() {
            tags.push(FileTag::new(color.display_name(), Some(color)));
        } else if tags[0].color.is_none() {
            tags[0].color = Some(color);
        }
    }

    Ok(tags)
}

/// Sets file tags in extended attributes (`user.xdg.tags` and `user.xdg.color`).
///
/// Passing an empty slice removes both tag attributes.
///
/// # Errors
/// Returns an error if the target does not exist, lacks permissions, or does not support xattrs.
pub fn set_tags(path: &Path, tags: &[FileTag]) -> Result<()> {
    if tags.is_empty() {
        remove_tags(path)?;
        return Ok(());
    }

    let payload = FileTag::to_xdg_string(tags);
    set_xattr_string(path, XATTR_TAGS, &payload)?;

    let first_color = tags.iter().find_map(|t| t.color);
    if let Some(color) = first_color {
        set_xattr_string(path, XATTR_COLOR, &color.color_index().to_string())?;
    } else {
        remove_xattr(path, XATTR_COLOR)?;
    }

    Ok(())
}

/// Removes all tag attributes (`user.xdg.tags` and `user.xdg.color`) from the specified file.
///
/// # Errors
/// Returns an error if the target does not exist or lacks permissions.
pub fn remove_tags(path: &Path) -> Result<()> {
    remove_xattr(path, XATTR_TAGS)?;
    remove_xattr(path, XATTR_COLOR)?;
    Ok(())
}

/// Retrieves the user comment stored in `user.xdg.comment`.
///
/// # Errors
/// Returns an error if querying xattrs encounters an unrecoverable I/O error.
pub fn get_comment(path: &Path) -> Result<Option<String>> {
    get_xattr_string(path, XATTR_COMMENT)
}

/// Sets the user comment in `user.xdg.comment`.
///
/// Providing an empty or whitespace-only string removes the comment attribute.
///
/// # Errors
/// Returns an error if writing xattrs fails or is not supported.
pub fn set_comment(path: &Path, comment: &str) -> Result<()> {
    if comment.trim().is_empty() {
        remove_comment(path)
    } else {
        set_xattr_string(path, XATTR_COMMENT, comment)
    }
}

/// Removes the user comment attribute (`user.xdg.comment`) from the specified file.
///
/// # Errors
/// Returns an error if the path does not exist or lacks permissions.
pub fn remove_comment(path: &Path) -> Result<()> {
    remove_xattr(path, XATTR_COMMENT)
}

/// Retrieves the origin download URL stored in `user.xdg.origin.url`.
///
/// # Errors
/// Returns an error if querying xattrs encounters an unrecoverable I/O error.
pub fn get_origin_url(path: &Path) -> Result<Option<String>> {
    get_xattr_string(path, XATTR_ORIGIN_URL)
}

/// Sets the origin download URL in `user.xdg.origin.url`.
///
/// Providing an empty or whitespace-only URL removes the origin attribute.
///
/// # Errors
/// Returns an error if writing xattrs fails or is not supported.
pub fn set_origin_url(path: &Path, url: &str) -> Result<()> {
    if url.trim().is_empty() {
        remove_origin_url(path)
    } else {
        set_xattr_string(path, XATTR_ORIGIN_URL, url)
    }
}

/// Removes the origin URL attribute (`user.xdg.origin.url`) from the specified file.
///
/// # Errors
/// Returns an error if the path does not exist or lacks permissions.
pub fn remove_origin_url(path: &Path) -> Result<()> {
    remove_xattr(path, XATTR_ORIGIN_URL)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_comment_lifecycle() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        assert_eq!(get_comment(path).unwrap(), None);

        set_comment(path, "Important project notes").unwrap();
        assert_eq!(
            get_comment(path).unwrap().as_deref(),
            Some("Important project notes")
        );

        set_comment(path, "").unwrap();
        assert_eq!(get_comment(path).unwrap(), None);

        set_comment(path, "Re-added comment").unwrap();
        remove_comment(path).unwrap();
        assert_eq!(get_comment(path).unwrap(), None);
    }

    #[test]
    fn test_origin_url_lifecycle() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        assert_eq!(get_origin_url(path).unwrap(), None);

        let url = "https://example.com/downloads/package.tar.gz";
        set_origin_url(path, url).unwrap();
        assert_eq!(get_origin_url(path).unwrap().as_deref(), Some(url));

        remove_origin_url(path).unwrap();
        assert_eq!(get_origin_url(path).unwrap(), None);
    }

    #[test]
    fn test_tags_lifecycle() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        assert!(get_tags(path).unwrap().is_empty());

        let tags = vec![
            FileTag::new("Work", Some(TagColor::Red)),
            FileTag::new("Financial", None),
        ];

        set_tags(path, &tags).unwrap();
        let retrieved = get_tags(path).unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].name, "Work");
        assert_eq!(retrieved[0].color, Some(TagColor::Red));
        assert_eq!(retrieved[1].name, "Financial");
        assert_eq!(retrieved[1].color, None);

        set_tags(path, &[]).unwrap();
        assert!(get_tags(path).unwrap().is_empty());
    }

    #[test]
    fn test_xattr_value_too_large() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();
        let huge_comment = "a".repeat(MAX_XATTR_SIZE + 10);

        let err = set_comment(path, &huge_comment).unwrap_err();
        match err {
            crate::errors::SharedError::Xattr(XattrError::ValueTooLarge { size, limit, .. }) => {
                assert_eq!(size, MAX_XATTR_SIZE + 10);
                assert_eq!(limit, MAX_XATTR_SIZE);
            }
            other => panic!("expected ValueTooLarge, got: {other:?}"),
        }
    }

    #[test]
    fn test_nonexistent_file() {
        let missing = Path::new("/tmp/findx_nonexistent_123456789.txt");
        assert!(get_comment(missing).is_err());
        assert!(set_comment(missing, "test").is_err());
        assert!(get_tags(missing).is_err());
        assert!(set_tags(missing, &[FileTag::new("tag", None)]).is_err());
    }
}
