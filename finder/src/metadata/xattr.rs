//! Extended attributes service and fallback SQLite storage.
//!
//! Manages POSIX extended attributes in the `user.*` namespace:
//! - `user.xdg.comment`: Freeform text comment
//! - `user.xdg.origin.url`: Download origin URL
//! - `user.xdg.origin.referrer`: Download origin referrer URL
//! - `user.xdg.tags`: List of tags applied to the file
//! - `user.xdg.color`: Canonical color label index or name
//!
//! Provides automatic fallback storage in `~/.local/share/findx/xattr_fallback.sqlite`
//! when target filesystems lack extended attribute support (`ENOTSUP` / `EOPNOTSUPP`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rusqlite::{Connection, params};
use shared::models::{FileTag, TagColor};

use super::error::{MetadataError, Result};
use super::types::ExtendedAttrsMeta;

/// Canonical attribute key for user comments.
pub const XATTR_COMMENT: &str = "user.xdg.comment";
/// Canonical attribute key for download origin URL.
pub const XATTR_ORIGIN_URL: &str = "user.xdg.origin.url";
/// Canonical attribute key for download origin referrer URL.
pub const XATTR_ORIGIN_REFERRER: &str = "user.xdg.origin.referrer";
/// Canonical attribute key for user tags.
pub const XATTR_TAGS: &str = "user.xdg.tags";
/// Canonical attribute key for color labels.
pub const XATTR_COLOR: &str = "user.xdg.color";
/// Canonical attribute key for hiding file extension.
pub const XATTR_HIDE_EXTENSION: &str = "user.xdg.hide_extension";

static OVERRIDE_FALLBACK_DB_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Resolves the path to the fallback SQLite database.
#[must_use]
pub fn default_fallback_db_path() -> PathBuf {
    if let Some(custom) = OVERRIDE_FALLBACK_DB_PATH.get() {
        return custom.clone();
    }

    if let Ok(dirs) = xdg::BaseDirectories::with_prefix("findx")
        && let Ok(path) = dirs.place_data_file("xattr_fallback.sqlite")
    {
        return path;
    }

    dirs_fallback_path()
}

fn dirs_fallback_path() -> PathBuf {
    if let Some(mut data_dir) = dirs_next_data_dir() {
        data_dir.push("findx");
        let _ = std::fs::create_dir_all(&data_dir);
        data_dir.push("xattr_fallback.sqlite");
        return data_dir;
    }

    std::env::temp_dir().join("findx_xattr_fallback.sqlite")
}

fn dirs_next_data_dir() -> Option<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(data_home));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
}

/// Allows overriding the fallback database location (primarily for isolated test fixtures).
pub fn set_override_fallback_db_path(path: PathBuf) {
    let _ = OVERRIDE_FALLBACK_DB_PATH.set(path);
}

/// Checks whether an I/O error indicates that extended attributes are unsupported.
fn is_enotsup(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::Unsupported
        || err.raw_os_error() == Some(libc::ENOTSUP)
        || err.raw_os_error() == Some(libc::EOPNOTSUPP)
}

fn open_fallback_db(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|e| MetadataError::io(parent, e))?;
    }

    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE IF NOT EXISTS xattr_fallback (
             file_path TEXT NOT NULL,
             attr_key TEXT NOT NULL,
             attr_value TEXT NOT NULL,
             PRIMARY KEY (file_path, attr_key)
         );",
    )?;
    Ok(conn)
}

fn get_fallback_attr(db_path: &Path, file_path: &Path, key: &str) -> Result<Option<String>> {
    let conn = open_fallback_db(db_path)?;
    let mut stmt = conn
        .prepare("SELECT attr_value FROM xattr_fallback WHERE file_path = ?1 AND attr_key = ?2;")?;
    let path_str = file_path.to_string_lossy();
    let mut rows = stmt.query(params![path_str, key])?;
    if let Some(row) = rows.next()? {
        let val: String = row.get(0)?;
        Ok(Some(val))
    } else {
        Ok(None)
    }
}

fn set_fallback_attr(db_path: &Path, file_path: &Path, key: &str, value: &str) -> Result<()> {
    let conn = open_fallback_db(db_path)?;
    let path_str = file_path.to_string_lossy();
    conn.execute(
        "INSERT INTO xattr_fallback (file_path, attr_key, attr_value)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(file_path, attr_key) DO UPDATE SET attr_value = excluded.attr_value;",
        params![path_str, key, value],
    )?;
    Ok(())
}

fn remove_fallback_attr(db_path: &Path, file_path: &Path, key: &str) -> Result<()> {
    let conn = open_fallback_db(db_path)?;
    let path_str = file_path.to_string_lossy();
    conn.execute(
        "DELETE FROM xattr_fallback WHERE file_path = ?1 AND attr_key = ?2;",
        params![path_str, key],
    )?;
    Ok(())
}

/// Service managing extended attributes with symlink protection and fallback database support.
#[derive(Debug, Default, Clone)]
pub struct ExtendedAttributesService {
    fallback_path: Option<PathBuf>,
}

impl ExtendedAttributesService {
    /// Creates a default [`ExtendedAttributesService`] instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fallback_path: None,
        }
    }

    /// Creates an instance using a custom fallback database path.
    #[must_use]
    pub fn with_fallback_path(path: impl Into<PathBuf>) -> Self {
        Self {
            fallback_path: Some(path.into()),
        }
    }

    fn fallback_db(&self) -> PathBuf {
        self.fallback_path
            .clone()
            .unwrap_or_else(default_fallback_db_path)
    }

    // -----------------------------------------------------------------------
    // Internal symlink-safe raw helpers
    // -----------------------------------------------------------------------

    fn read_raw_xattr(&self, path: &Path, attr: &str) -> Result<Option<String>> {
        match xattr::get(path, attr) {
            Ok(Some(bytes)) => {
                let s = String::from_utf8(bytes).map_err(|e| MetadataError::ParseFailed {
                    path: path.to_path_buf(),
                    message: format!("Attribute '{attr}' contains invalid UTF-8: {e}"),
                })?;
                Ok(Some(s))
            }
            Ok(None) => Ok(None),
            Err(err) => {
                if is_enotsup(&err) {
                    get_fallback_attr(&self.fallback_db(), path, attr)
                } else if err.kind() == std::io::ErrorKind::NotFound {
                    Err(MetadataError::io(path, err))
                } else if err.kind() == std::io::ErrorKind::PermissionDenied {
                    Err(MetadataError::permission_denied(path))
                } else {
                    Err(MetadataError::xattr(path, err))
                }
            }
        }
    }

    fn write_raw_xattr(&self, path: &Path, attr: &str, val: &str) -> Result<()> {
        match xattr::set(path, attr, val.as_bytes()) {
            Ok(()) => Ok(()),
            Err(err) => {
                if is_enotsup(&err) {
                    set_fallback_attr(&self.fallback_db(), path, attr, val)
                } else if err.kind() == std::io::ErrorKind::NotFound {
                    Err(MetadataError::io(path, err))
                } else if err.kind() == std::io::ErrorKind::PermissionDenied {
                    Err(MetadataError::permission_denied(path))
                } else {
                    Err(MetadataError::xattr(path, err))
                }
            }
        }
    }

    fn delete_raw_xattr(&self, path: &Path, attr: &str) -> Result<()> {
        match xattr::remove(path, attr) {
            Ok(()) => Ok(()),
            Err(err) => {
                if is_enotsup(&err) {
                    remove_fallback_attr(&self.fallback_db(), path, attr)
                } else if err.kind() == std::io::ErrorKind::NotFound {
                    // Attribute missing or file missing
                    if !path.exists() && std::fs::symlink_metadata(path).is_err() {
                        Err(MetadataError::io(path, err))
                    } else {
                        Ok(())
                    }
                } else if err.kind() == std::io::ErrorKind::PermissionDenied {
                    Err(MetadataError::permission_denied(path))
                } else {
                    // Raw ENODATA / ENOATTR on remove is normal when attribute didn't exist
                    if err.raw_os_error() == Some(libc::ENODATA) {
                        Ok(())
                    } else {
                        Err(MetadataError::xattr(path, err))
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Public instance methods
    // -----------------------------------------------------------------------

    /// Retrieves the user comment stored in `user.xdg.comment`.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_comment_inst(&self, path: &Path) -> Result<String> {
        let val = self.read_raw_xattr(path, XATTR_COMMENT)?;
        Ok(val.unwrap_or_default())
    }

    /// Sets or removes the user comment stored in `user.xdg.comment`.
    ///
    /// If `comment` is empty or only whitespace, the attribute is removed.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_comment_inst(&self, path: &Path, comment: &str) -> Result<()> {
        let trimmed = comment.trim();
        if trimmed.is_empty() {
            self.delete_raw_xattr(path, XATTR_COMMENT)
        } else {
            self.write_raw_xattr(path, XATTR_COMMENT, trimmed)
        }
    }

    /// Retrieves the origin download URL and referrer from `user.xdg.origin.*`.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_origin_inst(&self, path: &Path) -> Result<(Option<String>, Option<String>)> {
        let url = self.read_raw_xattr(path, XATTR_ORIGIN_URL)?;
        let referrer = self.read_raw_xattr(path, XATTR_ORIGIN_REFERRER)?;
        Ok((url, referrer))
    }

    /// Sets or removes the origin download URL and referrer.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_origin_inst(
        &self,
        path: &Path,
        url: Option<&str>,
        referrer: Option<&str>,
    ) -> Result<()> {
        match url {
            Some(u) if !u.trim().is_empty() => {
                self.write_raw_xattr(path, XATTR_ORIGIN_URL, u.trim())?;
            }
            _ => {
                self.delete_raw_xattr(path, XATTR_ORIGIN_URL)?;
            }
        }

        match referrer {
            Some(r) if !r.trim().is_empty() => {
                self.write_raw_xattr(path, XATTR_ORIGIN_REFERRER, r.trim())?;
            }
            _ => {
                self.delete_raw_xattr(path, XATTR_ORIGIN_REFERRER)?;
            }
        }

        Ok(())
    }

    /// Reads and parses user tags from `user.xdg.tags` and `user.xdg.color`.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_tags_inst(&self, path: &Path) -> Result<Vec<FileTag>> {
        let raw_tags = self.read_raw_xattr(path, XATTR_TAGS)?;
        let raw_color = self.read_raw_xattr(path, XATTR_COLOR)?;

        let color_opt = raw_color.and_then(|c| {
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

    /// Sets file tags and color in extended attributes (`user.xdg.tags` and `user.xdg.color`).
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_tags_inst(&self, path: &Path, tags: &[FileTag]) -> Result<()> {
        crate::ui::tagging::badges::invalidate_badge_cache(path);
        if tags.is_empty() {
            self.delete_raw_xattr(path, XATTR_TAGS)?;
            self.delete_raw_xattr(path, XATTR_COLOR)?;
            return Ok(());
        }

        let formatted = FileTag::to_macos_string(tags);
        self.write_raw_xattr(path, XATTR_TAGS, &formatted)?;

        let first_color = tags.iter().find_map(|t| t.color);
        if let Some(color) = first_color {
            self.write_raw_xattr(path, XATTR_COLOR, &color.color_index().to_string())?;
        } else {
            self.delete_raw_xattr(path, XATTR_COLOR)?;
        }

        Ok(())
    }

    /// Reads the extension hiding preference from `user.xdg.hide_extension`.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_hide_extension_inst(&self, path: &Path) -> Result<bool> {
        let val = self.read_raw_xattr(path, XATTR_HIDE_EXTENSION)?;
        Ok(val.is_some_and(|s| s.trim() == "1" || s.trim().eq_ignore_ascii_case("true")))
    }

    /// Sets or removes the extension hiding preference stored in `user.xdg.hide_extension`.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_hide_extension_inst(&self, path: &Path, hide: bool) -> Result<()> {
        if hide {
            self.write_raw_xattr(path, XATTR_HIDE_EXTENSION, "1")
        } else {
            self.delete_raw_xattr(path, XATTR_HIDE_EXTENSION)
        }
    }

    /// Reads all extended attributes and aggregates them into [`ExtendedAttrsMeta`].
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_all_inst(&self, path: &Path) -> Result<ExtendedAttrsMeta> {
        let comment = self.get_comment_inst(path)?;
        let (origin_url, origin_referrer) = self.get_origin_inst(path)?;
        let tags = self.get_tags_inst(path)?;
        let color_code = tags.iter().find_map(|t| t.color);

        Ok(ExtendedAttrsMeta {
            comment,
            origin_url,
            origin_referrer,
            tags,
            color_code,
        })
    }

    // -----------------------------------------------------------------------
    // Static convenience API delegating to default instance
    // -----------------------------------------------------------------------

    /// Retrieves user comment using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_comment(path: &Path) -> Result<String> {
        Self::new().get_comment_inst(path)
    }

    /// Sets user comment using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_comment(path: &Path, comment: &str) -> Result<()> {
        Self::new().set_comment_inst(path, comment)
    }

    /// Retrieves origin URLs using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_origin(path: &Path) -> Result<(Option<String>, Option<String>)> {
        Self::new().get_origin_inst(path)
    }

    /// Sets origin URLs using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_origin(path: &Path, url: Option<&str>, referrer: Option<&str>) -> Result<()> {
        Self::new().set_origin_inst(path, url, referrer)
    }

    /// Reads tags using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_tags(path: &Path) -> Result<Vec<FileTag>> {
        Self::new().get_tags_inst(path)
    }

    /// Writes tags using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_tags(path: &Path, tags: &[FileTag]) -> Result<()> {
        Self::new().set_tags_inst(path, tags)
    }

    /// Reads extension hiding preference using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_hide_extension(path: &Path) -> Result<bool> {
        Self::new().get_hide_extension_inst(path)
    }

    /// Sets extension hiding preference using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn set_hide_extension(path: &Path, hide: bool) -> Result<()> {
        Self::new().set_hide_extension_inst(path, hide)
    }

    /// Aggregates all extended attributes using default service configuration.
    ///
    /// # Errors
    /// Returns [`MetadataError`] if I/O or permissions fail.
    pub fn get_all(path: &Path) -> Result<ExtendedAttrsMeta> {
        Self::new().get_all_inst(path)
    }
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

        let initial = ExtendedAttributesService::get_comment(path).unwrap();
        assert!(initial.is_empty());

        ExtendedAttributesService::set_comment(path, "Important project document").unwrap();
        let comment = ExtendedAttributesService::get_comment(path).unwrap();
        assert_eq!(comment, "Important project document");

        ExtendedAttributesService::set_comment(path, "").unwrap();
        let cleared = ExtendedAttributesService::get_comment(path).unwrap();
        assert!(cleared.is_empty());
    }

    #[test]
    fn test_origin_urls_lifecycle() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        let (initial_url, initial_ref) = ExtendedAttributesService::get_origin(path).unwrap();
        assert!(initial_url.is_none());
        assert!(initial_ref.is_none());

        ExtendedAttributesService::set_origin(
            path,
            Some("https://example.com/download.tar.gz"),
            Some("https://example.com/releases"),
        )
        .unwrap();

        let (url, referrer) = ExtendedAttributesService::get_origin(path).unwrap();
        assert_eq!(url.as_deref(), Some("https://example.com/download.tar.gz"));
        assert_eq!(referrer.as_deref(), Some("https://example.com/releases"));

        ExtendedAttributesService::set_origin(path, None, None).unwrap();
        let (cleared_url, cleared_ref) = ExtendedAttributesService::get_origin(path).unwrap();
        assert!(cleared_url.is_none());
        assert!(cleared_ref.is_none());
    }

    #[test]
    fn test_tags_lifecycle() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();

        let initial_tags = ExtendedAttributesService::get_tags(path).unwrap();
        assert!(initial_tags.is_empty());

        let tags = vec![
            FileTag::new("Work", Some(TagColor::Blue)),
            FileTag::new("Urgent", Some(TagColor::Red)),
        ];
        ExtendedAttributesService::set_tags(path, &tags).unwrap();

        let loaded = ExtendedAttributesService::get_tags(path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Work");
        assert_eq!(loaded[0].color, Some(TagColor::Blue));
        assert_eq!(loaded[1].name, "Urgent");
        assert_eq!(loaded[1].color, Some(TagColor::Red));

        ExtendedAttributesService::set_tags(path, &[]).unwrap();
        let cleared_tags = ExtendedAttributesService::get_tags(path).unwrap();
        assert!(cleared_tags.is_empty());
    }

    #[test]
    fn test_fallback_sqlite_storage() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_fallback.sqlite");
        let fake_path = Path::new("/proc/virtual_test_file");

        set_fallback_attr(&db_path, fake_path, XATTR_COMMENT, "Virtual note").unwrap();
        let note = get_fallback_attr(&db_path, fake_path, XATTR_COMMENT).unwrap();
        assert_eq!(note.as_deref(), Some("Virtual note"));

        remove_fallback_attr(&db_path, fake_path, XATTR_COMMENT).unwrap();
        let cleared = get_fallback_attr(&db_path, fake_path, XATTR_COMMENT).unwrap();
        assert!(cleared.is_none());
    }
}
