//! SQLite connection manager and PRAGMA configuration.

use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::migrations::apply_migrations;
use crate::errors::DbError;

/// Standard PRAGMAs configured on every database connection.
pub const PRAGMAS: &str = "\
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -20000;
";

/// Resolves the default database path (`~/.local/share/findx/findx.db`)
/// conforming to the XDG Base Directory specification.
///
/// # Errors
///
/// Returns a [`DbError`] if the XDG directory cannot be resolved or created.
pub fn default_db_path() -> Result<PathBuf, DbError> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("findx")
        .map_err(|e| DbError::CorruptedIndex(format!("xdg base directory error: {e}")))?;
    let db_path = xdg_dirs.place_data_file("findx.db")?;
    Ok(db_path)
}

/// Applies required PRAGMAs to an active SQLite connection.
///
/// # Errors
///
/// Returns a [`DbError`] if executing the PRAGMA batch fails.
pub fn configure_connection(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(PRAGMAS)?;
    Ok(())
}

/// Managed SQLite database instance wrapping a [`rusqlite::Connection`].
#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens the default system database at `~/.local/share/findx/findx.db`,
    /// configures PRAGMAs, and applies pending migrations.
    ///
    /// # Errors
    ///
    /// Returns a [`DbError`] if the connection cannot be established or migrations fail.
    pub fn open_default() -> Result<Self, DbError> {
        let path = default_db_path()?;
        Self::open(path)
    }

    /// Opens or creates an embedded database at `path`, configures PRAGMAs,
    /// and applies pending migrations.
    ///
    /// # Errors
    ///
    /// Returns a [`DbError`] if opening SQLite fails or migrations fail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        let mut conn = Connection::open(path)?;
        configure_connection(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Opens a fast in-memory SQLite database, configures PRAGMAs,
    /// and applies pending schema migrations.
    ///
    /// Primarily intended for isolated unit and integration testing.
    ///
    /// # Errors
    ///
    /// Returns a [`DbError`] if in-memory database creation or migrations fail.
    pub fn open_in_memory() -> Result<Self, DbError> {
        let mut conn = Connection::open_in_memory()?;
        configure_connection(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory database with configured PRAGMAs without applying migrations.
    ///
    /// Useful for testing migration engines from a clean slate.
    ///
    /// # Errors
    ///
    /// Returns a [`DbError`] if in-memory database creation fails.
    pub fn open_in_memory_unmigrated() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        configure_connection(&conn)?;
        Ok(Self { conn })
    }

    /// Executes pending schema migrations on this database instance.
    ///
    /// # Errors
    ///
    /// Returns a [`DbError`] if migration execution fails.
    pub fn migrate(&mut self) -> Result<usize, DbError> {
        apply_migrations(&mut self.conn)
    }

    /// Returns an immutable reference to the underlying [`rusqlite::Connection`].
    #[must_use]
    pub const fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Returns a mutable reference to the underlying [`rusqlite::Connection`].
    pub const fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Consumes the database wrapper and extracts the inner [`rusqlite::Connection`].
    #[must_use]
    pub fn into_inner(self) -> Connection {
        self.conn
    }
}

impl Deref for Database {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.conn
    }
}

impl DerefMut for Database {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.conn
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

    #[test]
    fn test_open_in_memory_pragmas() {
        let db = Database::open_in_memory().expect("open_in_memory should succeed");

        let sync_mode: i64 = db
            .query_row("PRAGMA synchronous;", [], |row| row.get(0))
            .expect("query PRAGMA synchronous");
        assert_eq!(sync_mode, 1, "synchronous must be NORMAL (1)");

        let foreign_keys: i64 = db
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .expect("query PRAGMA foreign_keys");
        assert_eq!(foreign_keys, 1, "foreign_keys must be ON (1)");

        let busy_timeout: i64 = db
            .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
            .expect("query PRAGMA busy_timeout");
        assert_eq!(busy_timeout, 5000, "busy_timeout must be 5000");

        let cache_size: i64 = db
            .query_row("PRAGMA cache_size;", [], |row| row.get(0))
            .expect("query PRAGMA cache_size");
        assert_eq!(cache_size, -20000, "cache_size must be -20000");
    }

    #[test]
    fn test_open_file_based_db() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let db_path = temp_dir.path().join("test.db");

        let mut db = Database::open(&db_path).expect("open on disk");

        let journal_mode: String = db
            .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
            .expect("query journal_mode");
        assert_eq!(
            journal_mode.to_uppercase(),
            "WAL",
            "journal_mode must be WAL on disk"
        );

        // Migrate again should return 0 since already applied
        let applied = db.migrate().expect("idempotent migration");
        assert_eq!(applied, 0);
    }

    #[test]
    fn test_default_db_path_resolution() {
        let path = default_db_path().expect("default_db_path");
        assert!(path.ends_with("findx/findx.db"));
    }

    #[test]
    fn test_open_in_memory_unmigrated() {
        let mut db = Database::open_in_memory_unmigrated().expect("open unmigrated");
        let applied = db.migrate().expect("migrate");
        assert_eq!(applied, 2, "first migration should apply 2 versions");
    }
}
