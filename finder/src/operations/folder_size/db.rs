//! Persistent SQLite storage for folder size calculation caching.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, params};

use super::models::FolderSizeResult;

/// Schema definition SQL for the folder size cache table and index.
pub const SCHEMA_SQL: &str = "\
CREATE TABLE IF NOT EXISTS folder_size_cache (
    path TEXT PRIMARY KEY,
    dev_id INTEGER NOT NULL,
    inode INTEGER NOT NULL,
    dir_mtime INTEGER NOT NULL,
    total_bytes INTEGER NOT NULL,
    file_count INTEGER NOT NULL,
    folder_count INTEGER NOT NULL,
    calculated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_folder_size_dev_ino ON folder_size_cache(dev_id, inode);
";

/// Standard PRAGMAs configured on persistent SQLite cache connections.
pub const PRAGMAS_SQL: &str = "\
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
";

/// Converts a [`SystemTime`] to nanoseconds since UNIX epoch for storage in SQLite.
#[must_use]
pub fn system_time_to_nanos(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

/// Converts nanoseconds since UNIX epoch back into a [`SystemTime`].
#[must_use]
pub fn nanos_to_system_time(nanos: i64) -> SystemTime {
    if nanos <= 0 {
        SystemTime::UNIX_EPOCH
    } else {
        let u_nanos = u64::try_from(nanos).unwrap_or(0);
        SystemTime::UNIX_EPOCH + Duration::from_nanos(u_nanos)
    }
}

/// Resolves the default persistent cache database path (`~/.cache/findx/folder_sizes.sqlite`)
/// conforming to the XDG Base Directory specification.
///
/// # Errors
///
/// Returns an error if the XDG cache path cannot be created or resolved.
pub fn default_cache_db_path() -> Result<PathBuf, std::io::Error> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("findx")
        .map_err(|e| std::io::Error::other(format!("xdg base directory error: {e}")))?;
    xdg_dirs
        .place_cache_file("folder_sizes.sqlite")
        .map_err(|e| std::io::Error::other(format!("xdg cache place error: {e}")))
}

/// Opens or creates the SQLite database at `path` (or default cache path if `None`)
/// and initializes the schema and PRAGMAs.
///
/// # Errors
///
/// Returns a [`rusqlite::Error`] if connection creation or schema setup fails.
pub fn open_cache_db(path: Option<&Path>) -> Result<Connection, rusqlite::Error> {
    let conn = if let Some(p) = path {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Connection::open(p)?
    } else {
        let default_path = default_cache_db_path()
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Connection::open(default_path)?
    };

    conn.execute_batch(PRAGMAS_SQL)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

/// Opens an in-memory SQLite database initialized with schema and PRAGMAs,
/// primarily for unit testing.
///
/// # Errors
///
/// Returns a [`rusqlite::Error`] if in-memory database creation fails.
pub fn open_in_memory_db() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch(PRAGMAS_SQL)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

/// Fetches a cached folder size record from the database.
///
/// # Errors
///
/// Returns a [`rusqlite::Error`] if the SQL query fails.
pub fn get_folder_size_record(
    conn: &Connection,
    path: &Path,
) -> Result<Option<(u64, u64, FolderSizeResult)>, rusqlite::Error> {
    let path_str = path.to_string_lossy();
    let mut stmt = conn.prepare_cached(
        "SELECT dev_id, inode, dir_mtime, total_bytes, file_count, folder_count, calculated_at
         FROM folder_size_cache WHERE path = ?1;",
    )?;

    let mut rows = stmt.query(params![path_str])?;
    if let Some(row) = rows.next()? {
        let dev_id_i64: i64 = row.get(0)?;
        let inode_i64: i64 = row.get(1)?;
        let dir_mtime_nanos: i64 = row.get(2)?;
        let total_bytes_i64: i64 = row.get(3)?;
        let file_count_i64: i64 = row.get(4)?;
        let folder_count_i64: i64 = row.get(5)?;
        let calculated_at_nanos: i64 = row.get(6)?;

        let dev_id = u64::try_from(dev_id_i64).unwrap_or(0);
        let inode = u64::try_from(inode_i64).unwrap_or(0);
        let total_bytes = u64::try_from(total_bytes_i64).unwrap_or(0);
        let file_count = u64::try_from(file_count_i64).unwrap_or(0);
        let folder_count = u64::try_from(folder_count_i64).unwrap_or(0);

        let result = FolderSizeResult {
            total_bytes,
            file_count,
            folder_count,
            dir_mtime: nanos_to_system_time(dir_mtime_nanos),
            calculated_at: nanos_to_system_time(calculated_at_nanos),
        };

        Ok(Some((dev_id, inode, result)))
    } else {
        Ok(None)
    }
}

/// Inserts or updates a cached folder size record in the database.
///
/// # Errors
///
/// Returns a [`rusqlite::Error`] if execution fails.
pub fn upsert_folder_size_record(
    conn: &Connection,
    path: &Path,
    dev_id: u64,
    inode: u64,
    result: &FolderSizeResult,
) -> Result<(), rusqlite::Error> {
    let path_str = path.to_string_lossy();
    let dev_id_i64 = i64::try_from(dev_id).unwrap_or(i64::MAX);
    let inode_i64 = i64::try_from(inode).unwrap_or(i64::MAX);
    let dir_mtime_nanos = system_time_to_nanos(result.dir_mtime);
    let total_bytes_i64 = i64::try_from(result.total_bytes).unwrap_or(i64::MAX);
    let file_count_i64 = i64::try_from(result.file_count).unwrap_or(i64::MAX);
    let folder_count_i64 = i64::try_from(result.folder_count).unwrap_or(i64::MAX);
    let calculated_at_nanos = system_time_to_nanos(result.calculated_at);

    let mut stmt = conn.prepare_cached(
        "INSERT INTO folder_size_cache (
            path, dev_id, inode, dir_mtime, total_bytes, file_count, folder_count, calculated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(path) DO UPDATE SET
            dev_id = excluded.dev_id,
            inode = excluded.inode,
            dir_mtime = excluded.dir_mtime,
            total_bytes = excluded.total_bytes,
            file_count = excluded.file_count,
            folder_count = excluded.folder_count,
            calculated_at = excluded.calculated_at;",
    )?;

    stmt.execute(params![
        path_str,
        dev_id_i64,
        inode_i64,
        dir_mtime_nanos,
        total_bytes_i64,
        file_count_i64,
        folder_count_i64,
        calculated_at_nanos,
    ])?;

    Ok(())
}

/// Deletes a cached folder size record by path.
///
/// # Errors
///
/// Returns a [`rusqlite::Error`] if execution fails.
pub fn delete_folder_size_record(conn: &Connection, path: &Path) -> Result<(), rusqlite::Error> {
    let path_str = path.to_string_lossy();
    let mut stmt = conn.prepare_cached("DELETE FROM folder_size_cache WHERE path = ?1;")?;
    stmt.execute(params![path_str])?;
    Ok(())
}
