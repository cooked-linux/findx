//! SQLite tag index cache synchronization.
//!
//! Synchronizes extended attribute file tags with the SQLite indexing database
//! to enable instant tag and color lookups across filesystems without filesystem crawls.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, params};
use shared::models::{FileTag, TagColor};

use super::error::{MetadataError, Result};

/// Connection alias for the shared database.
pub type DatabasePool = Connection;

/// Ensures the `file_tags` table and color index exist in the database.
///
/// # Errors
/// Returns [`MetadataError::Database`] if statement execution fails.
pub fn init_file_tags_table(pool: &DatabasePool) -> Result<()> {
    pool.execute_batch(
        "CREATE TABLE IF NOT EXISTS file_tags (
             file_path TEXT PRIMARY KEY,
             dev_id INTEGER,
             inode INTEGER,
             tags_json TEXT,
             color_code INTEGER,
             indexed_mtime INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_tag_color ON file_tags(color_code);",
    )?;
    Ok(())
}

/// Synchronizes tags for `path` into the central SQLite tag cache.
///
/// If `tags` is empty, any existing row for `path` is removed.
///
/// # Errors
/// Returns [`MetadataError`] if file metadata or database statements fail.
pub fn sync_file_tags(pool: &DatabasePool, path: &Path, tags: &[FileTag]) -> Result<()> {
    init_file_tags_table(pool)?;

    let path_str = path.to_string_lossy();
    if tags.is_empty() {
        return remove_file_tags(pool, path);
    }

    let meta = std::fs::symlink_metadata(path).map_err(|e| MetadataError::io(path, e))?;
    let dev_id = i64::try_from(meta.dev()).unwrap_or(0);
    let inode = i64::try_from(meta.ino()).unwrap_or(0);
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));

    let tags_json = serde_json::to_string(tags).map_err(|e| MetadataError::ParseFailed {
        path: path.to_path_buf(),
        message: format!("Failed to serialize tags to JSON: {e}"),
    })?;

    let color_code = tags
        .iter()
        .find_map(|t| t.color)
        .map(|c| i64::from(c.color_index()));

    pool.execute(
        "INSERT INTO file_tags (file_path, dev_id, inode, tags_json, color_code, indexed_mtime)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(file_path) DO UPDATE SET
             dev_id = excluded.dev_id,
             inode = excluded.inode,
             tags_json = excluded.tags_json,
             color_code = excluded.color_code,
             indexed_mtime = excluded.indexed_mtime;",
        params![path_str, dev_id, inode, tags_json, color_code, mtime],
    )?;

    Ok(())
}

/// Purges the cached tag record for `path` from the database.
///
/// # Errors
/// Returns [`MetadataError::Database`] if execution fails.
pub fn remove_file_tags(pool: &DatabasePool, path: &Path) -> Result<()> {
    init_file_tags_table(pool)?;
    let path_str = path.to_string_lossy();
    pool.execute(
        "DELETE FROM file_tags WHERE file_path = ?1;",
        params![path_str],
    )?;
    Ok(())
}

/// Queries all filesystem paths associated with a given tag name.
///
/// # Errors
/// Returns [`MetadataError::Database`] if query execution fails.
pub fn query_files_by_tag(pool: &DatabasePool, tag_name: &str) -> Result<Vec<PathBuf>> {
    init_file_tags_table(pool)?;

    let mut stmt =
        pool.prepare("SELECT file_path, tags_json FROM file_tags ORDER BY file_path ASC;")?;

    let mut matched_paths = Vec::new();
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let path_str: String = row.get(0)?;
        let tags_json: String = row.get(1)?;

        if let Ok(tags) = serde_json::from_str::<Vec<FileTag>>(&tags_json)
            && tags.iter().any(|t| t.name.eq_ignore_ascii_case(tag_name))
        {
            matched_paths.push(PathBuf::from(path_str));
        }
    }

    Ok(matched_paths)
}

/// Queries all filesystem paths labeled with a specific canonical color.
///
/// # Errors
/// Returns [`MetadataError::Database`] if query execution fails.
pub fn query_files_by_color(pool: &DatabasePool, color: TagColor) -> Result<Vec<PathBuf>> {
    init_file_tags_table(pool)?;

    let code = i64::from(color.color_index());
    let mut stmt = pool
        .prepare("SELECT file_path FROM file_tags WHERE color_code = ?1 ORDER BY file_path ASC;")?;

    let mut paths = Vec::new();
    let mut rows = stmt.query(params![code])?;

    while let Some(row) = rows.next()? {
        let path_str: String = row.get(0)?;
        paths.push(PathBuf::from(path_str));
    }

    Ok(paths)
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

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_file_tags_table(&conn).unwrap();
        conn
    }

    #[test]
    fn test_sync_and_query_tags() {
        let db = setup_test_db();
        let file1 = NamedTempFile::new().unwrap();
        let file2 = NamedTempFile::new().unwrap();

        let tags1 = vec![
            FileTag::new("ProjectA", Some(TagColor::Blue)),
            FileTag::new("Finance", None),
        ];
        let tags2 = vec![
            FileTag::new("ProjectA", Some(TagColor::Green)),
            FileTag::new("Archive", None),
        ];

        sync_file_tags(&db, file1.path(), &tags1).unwrap();
        sync_file_tags(&db, file2.path(), &tags2).unwrap();

        let project_a_files = query_files_by_tag(&db, "ProjectA").unwrap();
        assert_eq!(project_a_files.len(), 2);

        let finance_files = query_files_by_tag(&db, "Finance").unwrap();
        assert_eq!(finance_files.len(), 1);
        assert_eq!(finance_files[0], file1.path());

        let blue_files = query_files_by_color(&db, TagColor::Blue).unwrap();
        assert_eq!(blue_files.len(), 1);
        assert_eq!(blue_files[0], file1.path());

        let green_files = query_files_by_color(&db, TagColor::Green).unwrap();
        assert_eq!(green_files.len(), 1);
        assert_eq!(green_files[0], file2.path());
    }

    #[test]
    fn test_remove_file_tags() {
        let db = setup_test_db();
        let file = NamedTempFile::new().unwrap();
        let tags = vec![FileTag::new("Work", Some(TagColor::Red))];

        sync_file_tags(&db, file.path(), &tags).unwrap();
        assert_eq!(query_files_by_tag(&db, "Work").unwrap().len(), 1);

        remove_file_tags(&db, file.path()).unwrap();
        assert_eq!(query_files_by_tag(&db, "Work").unwrap().len(), 0);
        assert_eq!(query_files_by_color(&db, TagColor::Red).unwrap().len(), 0);
    }

    #[test]
    fn test_sync_empty_tags_removes_entry() {
        let db = setup_test_db();
        let file = NamedTempFile::new().unwrap();
        let tags = vec![FileTag::new("Draft", None)];

        sync_file_tags(&db, file.path(), &tags).unwrap();
        assert_eq!(query_files_by_tag(&db, "Draft").unwrap().len(), 1);

        // Sync with empty slice
        sync_file_tags(&db, file.path(), &[]).unwrap();
        assert_eq!(query_files_by_tag(&db, "Draft").unwrap().len(), 0);
    }
}
