//! Frecency engagement tracking and aggregate query helpers.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::errors::DbError;

/// An engagement event recorded for an application or file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrecencyEvent {
    /// Unique database record identifier.
    pub id: i64,
    /// Target identifier (desktop app ID or filesystem path).
    pub item_id: String,
    /// Item classification (`app` or `file`).
    pub item_type: String,
    /// Action kind (`launch`, `open`, `reveal`).
    pub event_type: String,
    /// Event occurrence timestamp in epoch seconds.
    pub timestamp: i64,
    /// Optional search query text that led to this engagement.
    pub query: Option<String>,
}

/// Aggregated event count for an item and action type within a time window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrecencyAggregate {
    /// Target identifier.
    pub item_id: String,
    /// Item classification (`app` or `file`).
    pub item_type: String,
    /// Action kind (`launch`, `open`, `reveal`).
    pub event_type: String,
    /// Total number of matching events.
    pub count: u64,
}

/// Records a new frecency engagement event.
///
/// Returns the generated record `id`.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite insertion fails.
pub fn record_event(
    conn: &Connection,
    item_id: &str,
    item_type: &str,
    event_type: &str,
    timestamp: i64,
    query: Option<&str>,
) -> Result<i64, DbError> {
    conn.execute(
        "INSERT INTO frecency_events (item_id, item_type, event_type, timestamp, query) \
         VALUES (?1, ?2, ?3, ?4, ?5);",
        params![item_id, item_type, event_type, timestamp, query],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Records an engagement event using the current system clock time.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite insertion fails.
pub fn record_event_now(
    conn: &Connection,
    item_id: &str,
    item_type: &str,
    event_type: &str,
    query: Option<&str>,
) -> Result<i64, DbError> {
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap_or(0);

    record_event(conn, item_id, item_type, event_type, now, query)
}

/// Retrieves all events for a given `item_id` recorded at or after `since_timestamp`.
///
/// # Errors
///
/// Returns a [`DbError`] if querying SQLite fails.
pub fn get_events_for_item(
    conn: &Connection,
    item_id: &str,
    since_timestamp: i64,
) -> Result<Vec<FrecencyEvent>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, item_id, item_type, event_type, timestamp, query \
         FROM frecency_events \
         WHERE item_id = ?1 AND timestamp >= ?2 \
         ORDER BY timestamp DESC;",
    )?;

    let rows = stmt.query_map(params![item_id, since_timestamp], |row| {
        Ok(FrecencyEvent {
            id: row.get(0)?,
            item_id: row.get(1)?,
            item_type: row.get(2)?,
            event_type: row.get(3)?,
            timestamp: row.get(4)?,
            query: row.get(5)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Counts events for a specific item falling within `[start_ts, end_ts]`.
///
/// # Errors
///
/// Returns a [`DbError`] if querying SQLite fails.
pub fn count_events_in_window(
    conn: &Connection,
    item_id: &str,
    start_ts: i64,
    end_ts: i64,
) -> Result<u64, DbError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(1) \
         FROM frecency_events \
         WHERE item_id = ?1 AND timestamp >= ?2 AND timestamp <= ?3;",
        params![item_id, start_ts, end_ts],
        |row| row.get(0),
    )?;
    Ok(u64::try_from(count).unwrap_or(0))
}

/// Computes aggregated event counts for items since a given timestamp.
///
/// Results are ordered by frequency descending up to `limit`.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite query fails.
pub fn aggregate_event_counts(
    conn: &Connection,
    item_type: Option<&str>,
    since_timestamp: i64,
    limit: usize,
) -> Result<Vec<FrecencyAggregate>, DbError> {
    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

    let mut results = Vec::new();

    if let Some(kind) = item_type {
        let mut stmt = conn.prepare(
            "SELECT item_id, item_type, event_type, COUNT(1) AS cnt \
             FROM frecency_events \
             WHERE item_type = ?1 AND timestamp >= ?2 \
             GROUP BY item_id, item_type, event_type \
             ORDER BY cnt DESC \
             LIMIT ?3;",
        )?;

        let rows = stmt.query_map(params![kind, since_timestamp, limit_i64], |row| {
            let count_i64: i64 = row.get(3)?;
            Ok(FrecencyAggregate {
                item_id: row.get(0)?,
                item_type: row.get(1)?,
                event_type: row.get(2)?,
                count: u64::try_from(count_i64).unwrap_or(0),
            })
        })?;

        for row in rows {
            results.push(row?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT item_id, item_type, event_type, COUNT(1) AS cnt \
             FROM frecency_events \
             WHERE timestamp >= ?1 \
             GROUP BY item_id, item_type, event_type \
             ORDER BY cnt DESC \
             LIMIT ?2;",
        )?;

        let rows = stmt.query_map(params![since_timestamp, limit_i64], |row| {
            let count_i64: i64 = row.get(3)?;
            Ok(FrecencyAggregate {
                item_id: row.get(0)?,
                item_type: row.get(1)?,
                event_type: row.get(2)?,
                count: u64::try_from(count_i64).unwrap_or(0),
            })
        })?;

        for row in rows {
            results.push(row?);
        }
    }

    Ok(results)
}

/// Deletes all frecency events recorded strictly before `before_timestamp`.
///
/// # Errors
///
/// Returns a [`DbError`] if SQLite execution fails.
pub fn prune_events_before(conn: &Connection, before_timestamp: i64) -> Result<usize, DbError> {
    let affected = conn.execute(
        "DELETE FROM frecency_events WHERE timestamp < ?1;",
        params![before_timestamp],
    )?;
    Ok(affected)
}

/// Returns the total number of frecency records in the database.
///
/// # Errors
///
/// Returns a [`DbError`] if querying SQLite fails.
pub fn count_all_events(conn: &Connection) -> Result<usize, DbError> {
    let count: i64 = conn.query_row("SELECT COUNT(1) FROM frecency_events;", [], |row| {
        row.get(0)
    })?;
    Ok(usize::try_from(count).unwrap_or(0))
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
    use crate::db::migrations::apply_migrations;

    fn setup_frecency_db() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in memory");
        apply_migrations(&mut conn).expect("apply migrations");
        conn
    }

    #[test]
    fn test_record_and_get_events() {
        let conn = setup_frecency_db();

        let id1 = record_event(&conn, "firefox.desktop", "app", "launch", 1000, Some("web"))
            .expect("record event 1");
        let id2 = record_event(&conn, "firefox.desktop", "app", "launch", 2000, None)
            .expect("record event 2");
        let _id3 = record_event(&conn, "calc.desktop", "app", "launch", 1500, None)
            .expect("record event 3");

        assert!(id1 > 0);
        assert!(id2 > id1);

        assert_eq!(count_all_events(&conn).expect("count"), 3);

        // Retrieve events for firefox since timestamp 1000
        let events = get_events_for_item(&conn, "firefox.desktop", 1000).expect("get events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].timestamp, 2000); // Descending order
        assert_eq!(events[1].timestamp, 1000);
        assert_eq!(events[1].query.as_deref(), Some("web"));

        // Retrieve since 1500
        let recent = get_events_for_item(&conn, "firefox.desktop", 1500).expect("get recent");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].timestamp, 2000);
    }

    #[test]
    fn test_count_in_window_and_aggregates() {
        let conn = setup_frecency_db();

        record_event(&conn, "code.desktop", "app", "launch", 100, None).expect("rec");
        record_event(&conn, "code.desktop", "app", "launch", 200, None).expect("rec");
        record_event(&conn, "code.desktop", "app", "launch", 300, None).expect("rec");
        record_event(&conn, "code.desktop", "app", "open", 250, None).expect("rec");
        record_event(&conn, "/home/file.txt", "file", "open", 150, None).expect("rec");

        let window_cnt =
            count_events_in_window(&conn, "code.desktop", 150, 250).expect("count window");
        assert_eq!(window_cnt, 2); // timestamp 200 launch, 250 open

        // Aggregates for all item types
        let agg_all = aggregate_event_counts(&conn, None, 50, 10).expect("agg all");
        assert_eq!(agg_all[0].item_id, "code.desktop");
        assert_eq!(agg_all[0].event_type, "launch");
        assert_eq!(agg_all[0].count, 3);

        // Aggregates filtered by item_type = "file"
        let agg_file = aggregate_event_counts(&conn, Some("file"), 50, 10).expect("agg file");
        assert_eq!(agg_file.len(), 1);
        assert_eq!(agg_file[0].item_id, "/home/file.txt");
        assert_eq!(agg_file[0].count, 1);
    }

    #[test]
    fn test_record_now_and_prune() {
        let conn = setup_frecency_db();

        let id = record_event_now(&conn, "terminal.desktop", "app", "launch", Some("term"))
            .expect("record now");
        assert!(id > 0);

        record_event(&conn, "old.desktop", "app", "launch", 50, None).expect("record old");
        assert_eq!(count_all_events(&conn).expect("count"), 2);

        let pruned = prune_events_before(&conn, 100).expect("prune");
        assert_eq!(pruned, 1);
        assert_eq!(count_all_events(&conn).expect("count"), 1);
    }
}
