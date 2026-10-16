//! Schema migration engine for versioned SQLite upgrades.

use rusqlite::{params, Connection};

use crate::errors::DbError;

/// Schema migration descriptor defining an incremental schema upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    /// Incremental version identifier.
    pub version: u32,
    /// Human-readable descriptor of the migration purpose.
    pub description: &'static str,
    /// Raw SQL statements to execute for this migration step.
    pub sql: &'static str,
}

/// Static registry of ordered schema migrations.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    description: "v1_initial",
    sql: include_str!("migrations/v1_initial.sql"),
}];

/// Applies all pending schema migrations sequentially within individual transactions.
///
/// Returns the number of new migrations successfully applied.
///
/// # Errors
///
/// Returns a [`DbError`] if creating the migration tracker table fails,
/// or if any individual migration script fails to execute.
pub fn apply_migrations(conn: &mut Connection) -> Result<usize, DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    let mut applied_count = 0;
    for migration in MIGRATIONS {
        let is_applied: bool = conn
            .query_row(
                "SELECT COUNT(1) > 0 FROM schema_migrations WHERE version = ?1;",
                params![migration.version],
                |row| row.get(0),
            )
            .map_err(DbError::from)?;

        if is_applied {
            continue;
        }

        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql).map_err(|e| {
            DbError::migration_failed(
                migration.version,
                format!("failed executing migration '{}': {e}", migration.description),
            )
        })?;

        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, datetime('now'));",
            params![migration.version],
        )?;

        tx.commit()?;
        applied_count += 1;
    }

    Ok(applied_count)
}

/// Retrieves the current highest applied schema version, or `None` if no migrations have been applied.
///
/// # Errors
///
/// Returns a [`DbError`] if querying SQLite metadata fails.
pub fn current_version(conn: &Connection) -> Result<Option<u32>, DbError> {
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(1) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations';",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(None);
    }

    let version: Option<u32> = conn
        .query_row(
            "SELECT MAX(version) FROM schema_migrations;",
            [],
            |row| row.get(0),
        )
        .map_err(DbError::from)?;

    Ok(version)
}

/// Lists all applied migrations with their application timestamps in ascending order of version.
///
/// # Errors
///
/// Returns a [`DbError`] if querying SQLite fails.
pub fn list_applied_migrations(conn: &Connection) -> Result<Vec<(u32, String)>, DbError> {
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(1) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations';",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !table_exists {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare("SELECT version, applied_at FROM schema_migrations ORDER BY version ASC;")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_migrations_fresh_db() {
        let mut conn = Connection::open_in_memory().expect("open_in_memory");
        assert_eq!(current_version(&conn).expect("current_version"), None);

        let applied = apply_migrations(&mut conn).expect("apply_migrations");
        assert_eq!(applied, 1, "should apply 1 migration");
        assert_eq!(current_version(&conn).expect("current_version"), Some(1));

        let history = list_applied_migrations(&conn).expect("list_applied_migrations");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, 1);
        assert!(!history[0].1.is_empty());

        // Re-applying should be a no-op
        let applied_again = apply_migrations(&mut conn).expect("re-apply");
        assert_eq!(applied_again, 0);
    }

    #[test]
    fn test_core_tables_created() {
        let mut conn = Connection::open_in_memory().expect("open_in_memory");
        apply_migrations(&mut conn).expect("apply_migrations");

        // Verify files_fts exists
        let fts_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM files_fts;", [], |row| row.get(0))
            .expect("files_fts table query");
        assert_eq!(fts_count, 0);

        // Verify frecency_events exists
        let frecency_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM frecency_events;", [], |row| row.get(0))
            .expect("frecency_events table query");
        assert_eq!(frecency_count, 0);

        // Verify tag_cache exists
        let tag_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM tag_cache;", [], |row| row.get(0))
            .expect("tag_cache table query");
        assert_eq!(tag_count, 0);

        // Verify indices exist
        let fts_idx: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'index' AND name = 'idx_frecency_item';",
                [],
                |row| row.get(0),
            )
            .expect("frecency index check");
        assert_eq!(fts_idx, 1);

        let tag_idx: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'index' AND name = 'idx_tag_name';",
                [],
                |row| row.get(0),
            )
            .expect("tag index check");
        assert_eq!(tag_idx, 1);
    }

    #[test]
    fn test_migration_failure_rollback() {
        let mut conn = Connection::open_in_memory().expect("open_in_memory");

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );",
        )
        .expect("create schema_migrations");

        // Simulate a broken migration within a transaction
        let err = conn
            .transaction()
            .and_then(|tx| {
                tx.execute_batch("THIS IS NOT VALID SQL;")?;
                tx.commit()
            })
            .unwrap_err();

        let db_err = DbError::migration_failed(999, err.to_string());
        assert!(matches!(db_err, DbError::MigrationFailed { version: 999, .. }));
    }
}

