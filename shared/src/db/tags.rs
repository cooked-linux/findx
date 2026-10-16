//! Tag cache table management and fast lookup helpers.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::errors::DbError;

/// A cached tag entry associating a filesystem path with a tag and optional color.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagCacheEntry {
    /// Absolute filesystem path.
    pub path: String,
    /// Name of the tag (e.g. `Work`, `Important`).
    pub tag_name: String,
    /// Optional color code or hex string (e.g. `Red`, `#ff3b30`).
    pub tag_color: Option<String>,
    /// Last modified time of the file in epoch seconds.
    pub mtime: i64,
}

impl TagCacheEntry {
    /// Creates a new tag cache entry.
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        tag_name: impl Into<String>,
        tag_color: Option<String>,
        mtime: i64,
    ) -> Self {
        Self {
            path: path.into(),
            tag_name: tag_name.into(),
            tag_color,
            mtime,
        }
    }
}

/// Upserts a single tag entry into the cache index.
///
/// If `(path, tag_name)` already exists, its color and mtime are updated.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite execution fails.
pub fn upsert_tag(conn: &Connection, entry: &TagCacheEntry) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO tag_cache (path, tag_name, tag_color, mtime) \
         VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(path, tag_name) DO UPDATE SET \
             tag_color = excluded.tag_color, \
             mtime = excluded.mtime;",
        params![entry.path, entry.tag_name, entry.tag_color, entry.mtime],
    )?;
    Ok(())
}

/// Atomically replaces all tags for a specific path.
///
/// # Errors
///
/// Returns a [`DbError`] if any transactional statement fails.
pub fn sync_path_tags(
    conn: &mut Connection,
    path: &str,
    tags: &[(&str, Option<&str>)],
    mtime: i64,
) -> Result<(), DbError> {
    let tx = conn.transaction()?;

    tx.execute("DELETE FROM tag_cache WHERE path = ?1;", params![path])?;

    for (tag_name, tag_color) in tags {
        tx.execute(
            "INSERT INTO tag_cache (path, tag_name, tag_color, mtime) \
             VALUES (?1, ?2, ?3, ?4);",
            params![path, tag_name, tag_color, mtime],
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Removes all cached tags associated with `path`.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite execution fails.
pub fn remove_tags_for_path(conn: &Connection, path: &str) -> Result<usize, DbError> {
    let affected = conn.execute("DELETE FROM tag_cache WHERE path = ?1;", params![path])?;
    Ok(affected)
}

/// Removes a specific tag from a given `path`.
///
/// Returns `true` if a row was found and removed.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite execution fails.
pub fn remove_tag(conn: &Connection, path: &str, tag_name: &str) -> Result<bool, DbError> {
    let affected = conn.execute(
        "DELETE FROM tag_cache WHERE path = ?1 AND tag_name = ?2;",
        params![path, tag_name],
    )?;
    Ok(affected > 0)
}

/// Queries all filesystem paths indexed under `tag_name`, ordered alphabetically.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite query fails.
pub fn query_paths_by_tag(conn: &Connection, tag_name: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare("SELECT path FROM tag_cache WHERE tag_name = ?1 ORDER BY path ASC;")?;
    let rows = stmt.query_map(params![tag_name], |row| row.get(0))?;

    let mut paths = Vec::new();
    for row in rows {
        paths.push(row?);
    }
    Ok(paths)
}

/// Retrieves all tag cache entries associated with `path`, ordered by tag name.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite query fails.
pub fn query_tags_for_path(conn: &Connection, path: &str) -> Result<Vec<TagCacheEntry>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT path, tag_name, tag_color, mtime \
         FROM tag_cache \
         WHERE path = ?1 \
         ORDER BY tag_name ASC;",
    )?;

    let rows = stmt.query_map(params![path], |row| {
        Ok(TagCacheEntry {
            path: row.get(0)?,
            tag_name: row.get(1)?,
            tag_color: row.get(2)?,
            mtime: row.get(3)?,
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

/// Lists all distinct tag names along with their file occurrence count, ordered by occurrence count descending.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite query fails.
pub fn list_all_tags(conn: &Connection) -> Result<Vec<(String, u64)>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT tag_name, COUNT(path) AS cnt \
         FROM tag_cache \
         GROUP BY tag_name \
         ORDER BY cnt DESC, tag_name ASC;",
    )?;

    let rows = stmt.query_map([], |row| {
        let count_i64: i64 = row.get(1)?;
        Ok((row.get(0)?, u64::try_from(count_i64).unwrap_or(0)))
    })?;

    let mut list = Vec::new();
    for row in rows {
        list.push(row?);
    }
    Ok(list)
}

/// Deletes all tag records for a set of stale or deleted paths in a single transaction.
///
/// Returns the total number of deleted tag records.
///
/// # Errors
///
/// Returns a [`DbError`] if transaction fails.
pub fn remove_stale_paths<T: AsRef<str>>(
    conn: &mut Connection,
    stale_paths: &[T],
) -> Result<usize, DbError> {
    if stale_paths.is_empty() {
        return Ok(0);
    }

    let tx = conn.transaction()?;
    let mut total_deleted = 0;

    for path in stale_paths {
        total_deleted += tx.execute("DELETE FROM tag_cache WHERE path = ?1;", params![path.as_ref()])?;
    }

    tx.commit()?;
    Ok(total_deleted)
}

/// Returns the total number of cached tag records in the database.
///
/// # Errors
///
/// Returns a [`DbError`] if querying SQLite fails.
pub fn count_all_tags(conn: &Connection) -> Result<usize, DbError> {
    let count: i64 = conn.query_row("SELECT COUNT(1) FROM tag_cache;", [], |row| row.get(0))?;
    Ok(usize::try_from(count).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::apply_migrations;

    fn setup_tags_db() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in memory");
        apply_migrations(&mut conn).expect("apply migrations");
        conn
    }

    #[test]
    fn test_upsert_and_query_tags() {
        let conn = setup_tags_db();

        let entry1 = TagCacheEntry::new("/path/to/doc.pdf", "Work", Some("Blue".into()), 1_700_000_000);
        let entry2 = TagCacheEntry::new("/path/to/doc.pdf", "Invoice", Some("Green".into()), 1_700_000_000);
        let entry3 = TagCacheEntry::new("/path/to/other.txt", "Work", Some("Blue".into()), 1_700_000_100);

        upsert_tag(&conn, &entry1).expect("upsert 1");
        upsert_tag(&conn, &entry2).expect("upsert 2");
        upsert_tag(&conn, &entry3).expect("upsert 3");

        assert_eq!(count_all_tags(&conn).expect("count"), 3);

        // Query paths by tag "Work"
        let work_paths = query_paths_by_tag(&conn, "Work").expect("query paths");
        assert_eq!(work_paths, vec!["/path/to/doc.pdf", "/path/to/other.txt"]);

        // Query tags for path "/path/to/doc.pdf"
        let doc_tags = query_tags_for_path(&conn, "/path/to/doc.pdf").expect("query tags");
        assert_eq!(doc_tags.len(), 2);
        assert_eq!(doc_tags[0].tag_name, "Invoice");
        assert_eq!(doc_tags[1].tag_name, "Work");

        // Update tag color on conflict
        let entry1_updated = TagCacheEntry::new("/path/to/doc.pdf", "Work", Some("Red".into()), 1_700_000_200);
        upsert_tag(&conn, &entry1_updated).expect("update 1");
        assert_eq!(count_all_tags(&conn).expect("count"), 3);

        let updated_tags = query_tags_for_path(&conn, "/path/to/doc.pdf").expect("query updated");
        let work_tag = updated_tags.iter().find(|t| t.tag_name == "Work").expect("work tag");
        assert_eq!(work_tag.tag_color.as_deref(), Some("Red"));
        assert_eq!(work_tag.mtime, 1_700_000_200);
    }

    #[test]
    fn test_sync_tags_and_remove() {
        let mut conn = setup_tags_db();

        let tags = [("ProjectA", Some("Yellow")), ("Draft", None)];
        sync_path_tags(&mut conn, "/path/report.docx", &tags, 12345).expect("sync");

        let entries = query_tags_for_path(&conn, "/path/report.docx").expect("query");
        assert_eq!(entries.len(), 2);

        // Sync again with only Draft
        let tags_updated = [("Draft", Some("Orange"))];
        sync_path_tags(&mut conn, "/path/report.docx", &tags_updated, 12350).expect("sync updated");

        let entries_after = query_tags_for_path(&conn, "/path/report.docx").expect("query after");
        assert_eq!(entries_after.len(), 1);
        assert_eq!(entries_after[0].tag_name, "Draft");
        assert_eq!(entries_after[0].tag_color.as_deref(), Some("Orange"));

        // Remove single tag
        let removed = remove_tag(&conn, "/path/report.docx", "Draft").expect("remove tag");
        assert!(removed);
        assert_eq!(count_all_tags(&conn).expect("count"), 0);
    }

    #[test]
    fn test_list_all_tags_and_remove_stale() {
        let mut conn = setup_tags_db();

        upsert_tag(&conn, &TagCacheEntry::new("/a", "T1", None, 1)).expect("up");
        upsert_tag(&conn, &TagCacheEntry::new("/b", "T1", None, 1)).expect("up");
        upsert_tag(&conn, &TagCacheEntry::new("/c", "T1", None, 1)).expect("up");
        upsert_tag(&conn, &TagCacheEntry::new("/a", "T2", None, 1)).expect("up");

        let tag_list = list_all_tags(&conn).expect("list");
        assert_eq!(tag_list[0], ("T1".to_string(), 3));
        assert_eq!(tag_list[1], ("T2".to_string(), 1));

        // Remove stale paths
        let deleted = remove_stale_paths(&mut conn, &["/a", "/c"]).expect("remove stale");
        assert_eq!(deleted, 3); // 2 from /a, 1 from /c
        assert_eq!(count_all_tags(&conn).expect("count"), 1); // 1 remaining on /b
    }
}

