//! Freedesktop Trash management compliant with the XDG Trash specification.
//!
//! Provides APIs for moving items to trash, restoring them, listing trash contents,
//! purging/emptying trash, and parsing `.trashinfo` metadata files.

use std::path::{Path, PathBuf};

use crate::errors::{FsError, Result};
use crate::fs::clone::clone_file;

/// A trashed item representing a deleted file in the Freedesktop Trash.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TrashedItem {
    /// Identifier: absolute path to the `.trashinfo` metadata file.
    pub id: String,
    /// The display name of the deleted file or folder.
    pub name: String,
    /// The original absolute path before being moved to trash.
    pub original_path: PathBuf,
    /// ISO 8601 deletion date string.
    pub deletion_date: Option<String>,
}

/// Parsed metadata from a `.trashinfo` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashInfo {
    /// The original path of the file before deletion.
    pub original_path: PathBuf,
    /// The deletion date in ISO 8601 format.
    pub deletion_date: Option<String>,
}

/// Decodes an RFC 2396 percent-encoded UTF-8 string (e.g. `%20` -> ` `).
fn decode_percent(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut iter = input.bytes();

    while let Some(b) = iter.next() {
        if b == b'%' {
            let h1 = iter.next();
            let h2 = iter.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                let s = [c1, c2];
                if let Ok(hex_str) = std::str::from_utf8(&s)
                    && let Ok(byte_val) = u8::from_str_radix(hex_str, 16)
                {
                    bytes.push(byte_val);
                    continue;
                }
                bytes.push(b'%');
                bytes.push(c1);
                bytes.push(c2);
            } else {
                bytes.push(b'%');
                if let Some(c) = h1 {
                    bytes.push(c);
                }
            }
        } else {
            bytes.push(b);
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

/// Parses a Freedesktop `.trashinfo` file content.
///
/// Format per Freedesktop Trash spec:
/// ```ini
/// [Trash Info]
/// Path=/path/to/original/file
/// DeletionDate=2026-10-15T21:30:00
/// ```
///
/// # Errors
/// Returns an error if the header `[Trash Info]` or the `Path` key is missing.
pub fn parse_trash_info(content: &str) -> Result<TrashInfo> {
    let mut has_header = false;
    let mut original_path: Option<PathBuf> = None;
    let mut deletion_date: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed == "[Trash Info]" {
            has_header = true;
            continue;
        }

        if let Some((key, val)) = trimmed.split_once('=') {
            let key = key.trim();
            let val = val.trim();
            match key {
                "Path" => {
                    let decoded = decode_percent(val);
                    original_path = Some(PathBuf::from(decoded));
                }
                "DeletionDate" => {
                    deletion_date = Some(val.to_string());
                }
                _ => {}
            }
        }
    }

    if !has_header {
        return Err(FsError::TrashError("missing [Trash Info] header in .trashinfo".into()).into());
    }

    let original_path = original_path
        .ok_or_else(|| FsError::TrashError("missing Path key in .trashinfo".into()))?;

    Ok(TrashInfo {
        original_path,
        deletion_date,
    })
}

/// Moves a file or directory to the Freedesktop Trash.
///
/// # Errors
/// Returns an error if the path does not exist or moving to trash fails.
pub fn move_to_trash(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(FsError::not_found(path).into());
    }

    trash::delete(path).map_err(|e| FsError::TrashError(e.to_string()))?;
    Ok(())
}

/// Restores a previously trashed item to its original location.
///
/// Attempts restoration via the `trash` crate first. If the item is not registered
/// in the global list, falls back to direct restoration using its `.trashinfo` file.
///
/// # Errors
/// Returns an error if restoration fails or if the item cannot be found.
pub fn restore_from_trash(trashed_item: &TrashedItem) -> Result<()> {
    // 1. Try restoring via the standard trash crate
    if let Ok(items) = trash::os_limited::list()
        && let Some(matched) = items
            .into_iter()
            .find(|it| it.id.to_string_lossy() == trashed_item.id)
    {
        trash::os_limited::restore_all(vec![matched])
            .map_err(|e| FsError::TrashError(e.to_string()))?;
        return Ok(());
    }

    // 2. Fallback: manual restoration from `.trashinfo` and sibling `files/` directory
    let info_path = Path::new(&trashed_item.id);
    if info_path.exists() {
        let content = std::fs::read_to_string(info_path).map_err(|e| FsError::io(info_path, e))?;
        let info = parse_trash_info(&content)?;

        let trash_dir = info_path.parent().and_then(Path::parent);
        let file_name = info_path.file_stem().unwrap_or_default();

        if let Some(dir) = trash_dir {
            let src = dir.join("files").join(file_name);
            if src.exists() {
                if let Some(parent) = info.original_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| FsError::io(parent, e))?;
                }

                if std::fs::rename(&src, &info.original_path).is_err() {
                    clone_file(&src, &info.original_path)?;
                    let _ = std::fs::remove_file(&src);
                }

                let _ = std::fs::remove_file(info_path);
                return Ok(());
            }
        }
    }

    Err(FsError::TrashError(format!(
        "could not restore trashed item '{}': file not found in trash",
        trashed_item.name
    ))
    .into())
}

/// Lists all items currently in the trash across Freedesktop-compliant directories.
///
/// # Errors
/// Returns an error if querying the trash directory fails.
pub fn list_trash_items() -> Result<Vec<TrashedItem>> {
    let items = trash::os_limited::list().map_err(|e| FsError::TrashError(e.to_string()))?;

    let trashed_items = items
        .into_iter()
        .map(|item| {
            let id = item.id.to_string_lossy().into_owned();
            let name = item.name.to_string_lossy().into_owned();
            let original_path = item.original_path();

            let deletion_date = std::fs::read_to_string(&id)
                .ok()
                .and_then(|c| parse_trash_info(&c).ok())
                .and_then(|info| info.deletion_date);

            TrashedItem {
                id,
                name,
                original_path,
                deletion_date,
            }
        })
        .collect();

    Ok(trashed_items)
}

/// Permanently empties all items in the trash.
///
/// # Errors
/// Returns an error if purging trash files fails.
pub fn empty_trash() -> Result<()> {
    let items = trash::os_limited::list().map_err(|e| FsError::TrashError(e.to_string()))?;
    trash::os_limited::purge_all(items).map_err(|e| FsError::TrashError(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_trash_info_standard() {
        let content = "\
[Trash Info]
Path=/home/user/documents/report.pdf
DeletionDate=2026-10-15T21:30:00
";
        let info = parse_trash_info(content).unwrap();
        assert_eq!(
            info.original_path,
            PathBuf::from("/home/user/documents/report.pdf")
        );
        assert_eq!(
            info.deletion_date.as_deref(),
            Some("2026-10-15T21:30:00")
        );
    }

    #[test]
    fn test_parse_trash_info_percent_encoded() {
        let content = "\
[Trash Info]
Path=/home/user/My%20Folder/File%231%20%28Draft%29.txt
DeletionDate=2026-10-15T10:15:30Z
";
        let info = parse_trash_info(content).unwrap();
        assert_eq!(
            info.original_path,
            PathBuf::from("/home/user/My Folder/File#1 (Draft).txt")
        );
        assert_eq!(
            info.deletion_date.as_deref(),
            Some("2026-10-15T10:15:30Z")
        );
    }

    #[test]
    fn test_parse_trash_info_missing_header() {
        let content = "Path=/tmp/file.txt\nDeletionDate=2026-10-15T00:00:00\n";
        assert!(parse_trash_info(content).is_err());
    }

    #[test]
    fn test_parse_trash_info_missing_path() {
        let content = "[Trash Info]\nDeletionDate=2026-10-15T00:00:00\n";
        assert!(parse_trash_info(content).is_err());
    }

    #[test]
    fn test_trash_nonexistent_file() {
        let missing = Path::new("/tmp/findx_nonexistent_trash_file_98765.txt");
        assert!(move_to_trash(missing).is_err());
    }

    #[test]
    fn test_trash_and_restore_cycle() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        std::fs::write(&path, b"temporary trash test payload").unwrap();

        assert!(path.exists());
        move_to_trash(&path).unwrap();
        assert!(!path.exists(), "file should be gone from original path");

        let items = list_trash_items().unwrap();
        let found = items.iter().find(|i| i.original_path == path);
        assert!(found.is_some(), "trashed item should appear in trash items list");

        if let Some(item) = found {
            restore_from_trash(item).unwrap();
            assert!(path.exists(), "file should be restored to original path");
            let content = std::fs::read(&path).unwrap();
            assert_eq!(content, b"temporary trash test payload");
        }
    }
}
