//! Sidebar tags navigation, virtual `tags://` directory resolution, and filter query mechanics.

use std::collections::HashSet;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use shared::db::Database;
use shared::models::{FileItem, FileMetadata, TagColor};

/// Canonical URI scheme prefix for virtual tag directories.
pub const VIRTUAL_TAGS_SCHEME: &str = "tags://";

/// Constructs a virtual filesystem path for `tag_name` (e.g. `tags://Work`).
#[must_use]
pub fn make_tag_virtual_path(tag_name: &str) -> PathBuf {
    PathBuf::from(format!("{VIRTUAL_TAGS_SCHEME}{tag_name}"))
}

/// Checks whether `path` represents a virtual tag directory (`tags://...`).
#[must_use]
pub fn is_tag_virtual_path(path: &Path) -> bool {
    path.to_string_lossy().starts_with(VIRTUAL_TAGS_SCHEME)
}

/// Parses the tag name from a virtual tag directory path.
#[must_use]
pub fn parse_tag_virtual_path(path: &Path) -> Option<String> {
    let s = path.to_string_lossy();
    s.strip_prefix(VIRTUAL_TAGS_SCHEME)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Queries all filesystem paths matching `tag_name` from SQLite database.
#[must_use]
pub fn query_matching_tag_paths(conn: &Connection, tag_name: &str) -> Vec<PathBuf> {
    let mut matching_paths = HashSet::new();

    // 1. Query by text tag name in tag_cache
    if let Ok(mut stmt) =
        conn.prepare("SELECT path FROM tag_cache WHERE tag_name = ?1 ORDER BY path ASC;")
        && let Ok(rows) = stmt.query_map(rusqlite::params![tag_name], |row| row.get::<_, String>(0))
    {
        for path_res in rows.flatten() {
            matching_paths.insert(PathBuf::from(path_res));
        }
    }

    // 2. Query file_tags central table if present
    if let Ok(paths) = crate::metadata::tags_sync::query_files_by_tag(conn, tag_name) {
        for p in paths {
            matching_paths.insert(p);
        }
    }

    // 3. If tag_name matches a canonical color, also query by color code
    if let Some(color) = TagColor::from_name(tag_name) {
        if let Ok(color_paths) = crate::metadata::tags_sync::query_files_by_color(conn, color) {
            for p in color_paths {
                matching_paths.insert(p);
            }
        }
        if let Ok(mut stmt) =
            conn.prepare("SELECT path FROM tag_cache WHERE tag_color = ?1 ORDER BY path ASC;")
            && let Ok(rows) = stmt.query_map(rusqlite::params![color.display_name()], |row| {
                row.get::<_, String>(0)
            })
        {
            for path_res in rows.flatten() {
                matching_paths.insert(PathBuf::from(path_res));
            }
        }
    }

    let mut result: Vec<PathBuf> = matching_paths.into_iter().collect();
    result.sort();
    result
}

/// Loads full [`FileItem`] records for all files matching `tag_name`.
#[must_use]
pub fn query_tagged_files(tag_name: &str) -> Vec<FileItem> {
    let paths = Database::open_default()
        .map_or_else(|_| Vec::new(), |db| query_matching_tag_paths(&db, tag_name));

    let mut items = Vec::new();
    for p in paths {
        if let Ok(meta) = std::fs::symlink_metadata(&p) {
            let file_name = p.file_name().map_or_else(
                || p.to_string_lossy().to_string(),
                |n| n.to_string_lossy().to_string(),
            );
            let is_dir = meta.is_dir();
            let is_symlink = meta.is_symlink();
            let symlink_target = if is_symlink {
                std::fs::read_link(&p).ok()
            } else {
                None
            };
            let tags = crate::ui::tagging::badges::TagBadgeCache::global().get_or_load(&p);

            let file_meta = FileMetadata {
                size_bytes: meta.len(),
                modified: meta.modified().ok(),
                accessed: meta.accessed().ok(),
                created: meta.created().ok(),
                permissions: meta.permissions().mode(),
                inode: meta.ino(),
                device_id: meta.dev(),
                mime_type: if is_dir {
                    "inode/directory".to_string()
                } else {
                    shared::mime::detect_mime_type(&p)
                        .unwrap_or_else(|_| "application/octet-stream".to_string())
                },
                is_hidden: file_name.starts_with('.'),
                is_executable: (meta.permissions().mode() & 0o111) != 0,
            };

            items.push(FileItem::new(
                p,
                file_name,
                is_dir,
                is_symlink,
                symlink_target,
                file_meta,
                tags,
            ));
        }
    }

    items
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::NamedTempFile;

    #[test]
    fn test_virtual_path_helpers() {
        let path = make_tag_virtual_path("Work");
        assert_eq!(path.to_string_lossy(), "tags://Work");
        assert!(is_tag_virtual_path(&path));

        let extracted = parse_tag_virtual_path(&path);
        assert_eq!(extracted.as_deref(), Some("Work"));

        let normal = Path::new("/home/user/Documents");
        assert!(!is_tag_virtual_path(normal));
        assert_eq!(parse_tag_virtual_path(normal), None);
    }

    #[test]
    fn test_query_matching_tag_paths_from_sqlite() {
        let conn = Connection::open_in_memory().unwrap();
        crate::metadata::tags_sync::init_file_tags_table(&conn).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tag_cache (
                 path TEXT NOT NULL,
                 tag_name TEXT NOT NULL,
                 tag_color TEXT,
                 mtime INTEGER
             );
             INSERT INTO tag_cache VALUES ('/path/file1.txt', 'ClientA', 'Blue', 100);
             INSERT INTO tag_cache VALUES ('/path/file2.txt', 'ClientA', 'Green', 200);
             INSERT INTO tag_cache VALUES ('/path/file3.txt', 'Internal', 'Red', 300);
            ",
        )
        .unwrap();

        let client_a = query_matching_tag_paths(&conn, "ClientA");
        assert_eq!(client_a.len(), 2);
        assert_eq!(client_a[0], PathBuf::from("/path/file1.txt"));
        assert_eq!(client_a[1], PathBuf::from("/path/file2.txt"));

        let red_paths = query_matching_tag_paths(&conn, "Red");
        assert_eq!(red_paths.len(), 1);
        assert_eq!(red_paths[0], PathBuf::from("/path/file3.txt"));
    }

    #[test]
    fn test_query_tagged_files_with_actual_files() {
        let file1 = NamedTempFile::new().unwrap();
        let file2 = NamedTempFile::new().unwrap();

        let tag = shared::models::FileTag::new("ProjectAlpha", Some(TagColor::Yellow));
        crate::ui::tagging::menu::apply_tag_to_paths(
            &[file1.path().to_path_buf(), file2.path().to_path_buf()],
            &tag,
        );

        if let Ok(db) = Database::open_default() {
            let matched = query_matching_tag_paths(&db, "ProjectAlpha");
            assert!(matched.contains(&file1.path().to_path_buf()));
            assert!(matched.contains(&file2.path().to_path_buf()));
        }
    }
}
