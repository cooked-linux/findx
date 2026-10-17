//! Event tracker and SQLite storage for frecency events.
//!
//! Tracks application and file engagement history, calculates aggregated
//! decayed frecency scores, and handles query keyword engagement bonuses.

use rusqlite::Connection;
use std::sync::{Arc, Mutex, MutexGuard};

use super::FrecencyError;
use super::decay::{DEFAULT_HALF_LIFE_SECS, EventKind, compute_event_score};

/// Retention window for score calculations: 30 days in seconds.
pub const DEFAULT_SCORE_WINDOW_SECS: u64 = 30 * 86_400;

/// Headless frecency tracker backed by a thread-safe SQLite connection.
#[derive(Debug, Clone)]
pub struct FrecencyTracker {
    db: Arc<Mutex<Connection>>,
    half_life_secs: f64,
}

impl FrecencyTracker {
    /// Creates a new `FrecencyTracker` wrapping an `Arc<Mutex<Connection>>`
    /// using the default 4-day half-life.
    #[must_use]
    pub const fn new(db: Arc<Mutex<Connection>>) -> Self {
        Self {
            db,
            half_life_secs: DEFAULT_HALF_LIFE_SECS,
        }
    }

    /// Creates a new `FrecencyTracker` with a custom decay half-life in seconds.
    #[must_use]
    pub const fn with_half_life(db: Arc<Mutex<Connection>>, half_life_secs: f64) -> Self {
        Self { db, half_life_secs }
    }

    /// Returns the half-life configuration in seconds.
    #[must_use]
    pub const fn half_life_secs(&self) -> f64 {
        self.half_life_secs
    }

    /// Returns a clone of the underlying database connection handle.
    #[must_use]
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.db)
    }

    /// Locks the underlying SQLite connection safely without unwrapping.
    fn lock_conn(&self) -> Result<MutexGuard<'_, Connection>, FrecencyError> {
        self.db
            .lock()
            .map_err(|e| FrecencyError::LockError(e.to_string()))
    }

    /// Records an engagement event for an item.
    ///
    /// # Errors
    ///
    /// Returns [`FrecencyError::Db`] if SQLite insertion fails, or
    /// [`FrecencyError::InvalidTimestamp`] if the timestamp cannot be converted to `i64`.
    pub fn record_event(
        &self,
        item_id: &str,
        item_type: &str,
        kind: EventKind,
        query: Option<&str>,
        timestamp: u64,
    ) -> Result<(), FrecencyError> {
        let conn = self.lock_conn()?;
        let ts_i64 =
            i64::try_from(timestamp).map_err(|e| FrecencyError::InvalidTimestamp(e.to_string()))?;
        crate::db::frecency::record_event(&conn, item_id, item_type, kind.as_str(), ts_i64, query)?;
        Ok(())
    }

    /// Records an engagement event using the current system wall-clock time.
    ///
    /// # Errors
    ///
    /// Returns [`FrecencyError`] if system time conversion or SQLite recording fails.
    pub fn record_event_now(
        &self,
        item_id: &str,
        item_type: &str,
        kind: EventKind,
        query: Option<&str>,
    ) -> Result<(), FrecencyError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| FrecencyError::InvalidTimestamp(e.to_string()))?
            .as_secs();
        self.record_event(item_id, item_type, kind, query, now)
    }

    /// Calculates the decayed frecency score for an item based on events
    /// within the last 30 days relative to `now`.
    ///
    /// # Errors
    ///
    /// Returns [`FrecencyError`] if SQLite querying fails or numeric overflow occurs.
    pub fn calculate_score(&self, item_id: &str, now: u64) -> Result<f64, FrecencyError> {
        let since = i64::try_from(now.saturating_sub(DEFAULT_SCORE_WINDOW_SECS))
            .map_err(|e| FrecencyError::InvalidTimestamp(e.to_string()))?;
        let events = {
            let conn = self.lock_conn()?;
            crate::db::frecency::get_events_for_item(&conn, item_id, since)?
        };

        let mut total_score = 0.0;
        for ev in events {
            let ev_time = u64::try_from(ev.timestamp)
                .map_err(|e| FrecencyError::InvalidTimestamp(e.to_string()))?;
            let kind = EventKind::parse(&ev.event_type).unwrap_or(EventKind::Open);
            let score = compute_event_score(ev_time, now, kind, self.half_life_secs);
            total_score += score;
        }

        if total_score.is_nan() || total_score.is_infinite() {
            return Err(FrecencyError::NumericError);
        }

        Ok(total_score)
    }

    /// Calculates a query keyword boost multiplier for an item.
    ///
    /// Returns a bonus multiplier between `1.0` (no prior matches) and `1.5` (strong match history)
    /// if past recorded events for `item_id` matched the identical query string.
    ///
    /// # Errors
    ///
    /// Returns [`FrecencyError`] if database querying fails.
    #[allow(clippy::cast_precision_loss)]
    pub fn calculate_keyword_boost(
        &self,
        item_id: &str,
        query: &str,
        now: u64,
    ) -> Result<f64, FrecencyError> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Ok(1.0);
        }

        let since = i64::try_from(now.saturating_sub(DEFAULT_SCORE_WINDOW_SECS))
            .map_err(|e| FrecencyError::InvalidTimestamp(e.to_string()))?;
        let events = {
            let conn = self.lock_conn()?;
            crate::db::frecency::get_events_for_item(&conn, item_id, since)?
        };

        let mut match_count = 0usize;
        for ev in events {
            if let Some(ref past_query) = ev.query
                && past_query.trim().eq_ignore_ascii_case(trimmed_query)
            {
                match_count += 1;
            }
        }

        if match_count == 0 {
            Ok(1.0)
        } else {
            // Baseline 1.0 + 0.2 per match, capped at 1.5x (up to 1.5x)
            let bonus = (match_count as f64 * 0.2).min(0.5);
            Ok(1.0 + bonus)
        }
    }

    /// Calculates the decayed frecency score for an item, optionally amplified
    /// by the keyword match bonus multiplier if a query is provided.
    ///
    /// # Errors
    ///
    /// Returns [`FrecencyError`] if score or boost computation fails.
    pub fn calculate_score_with_query(
        &self,
        item_id: &str,
        query: Option<&str>,
        now: u64,
    ) -> Result<f64, FrecencyError> {
        let base_score = self.calculate_score(item_id, now)?;
        if let Some(q) = query {
            let boost = self.calculate_keyword_boost(item_id, q, now)?;
            Ok(base_score * boost)
        } else {
            Ok(base_score)
        }
    }

    /// Prunes stale frecency events recorded strictly before `cutoff_epoch_secs`.
    ///
    /// Returns the number of deleted records.
    ///
    /// # Errors
    ///
    /// Returns [`FrecencyError`] if deletion in SQLite fails.
    pub fn prune_stale_events(&self, cutoff_epoch_secs: u64) -> Result<usize, FrecencyError> {
        let cutoff_i64 = i64::try_from(cutoff_epoch_secs)
            .map_err(|e| FrecencyError::InvalidTimestamp(e.to_string()))?;
        let conn = self.lock_conn()?;
        let affected = crate::db::frecency::prune_events_before(&conn, cutoff_i64)?;
        drop(conn);
        Ok(affected)
    }

    /// Returns the total count of frecency records in the database.
    ///
    /// # Errors
    ///
    /// Returns [`FrecencyError`] if querying SQLite fails.
    pub fn count_all_events(&self) -> Result<usize, FrecencyError> {
        let conn = self.lock_conn()?;
        let count = crate::db::frecency::count_all_events(&conn)?;
        drop(conn);
        Ok(count)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn setup_test_tracker() -> FrecencyTracker {
        let mut conn = Connection::open_in_memory().expect("open in memory");
        crate::db::migrations::apply_migrations(&mut conn).expect("apply migrations");
        FrecencyTracker::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn test_record_and_calculate_score() {
        let tracker = setup_test_tracker();
        let now = 1_700_000_000;

        // Record a launch at `now`
        tracker
            .record_event("app.desktop", "app", EventKind::Launch, None, now)
            .expect("record launch");

        let score = tracker.calculate_score("app.desktop", now).expect("score");
        assert!((score - 100.0).abs() < 1e-6);

        // Record another event 4 days earlier: weight 100 * 0.8 * 0.5 = 40.0
        let four_days_ago = now - (4 * 86_400);
        tracker
            .record_event("app.desktop", "app", EventKind::Open, None, four_days_ago)
            .expect("record open");

        let combined = tracker.calculate_score("app.desktop", now).expect("score");
        assert!((combined - 140.0).abs() < 1e-6);

        // An unrelated item should have 0.0 score
        let empty_score = tracker
            .calculate_score("other.desktop", now)
            .expect("score");
        assert_eq!(empty_score, 0.0);
    }

    #[test]
    fn test_keyword_boost_scoring() {
        let tracker = setup_test_tracker();
        let now = 1_700_000_000;

        tracker
            .record_event(
                "browser.desktop",
                "app",
                EventKind::Launch,
                Some("firefox"),
                now,
            )
            .expect("record");

        // Query match "firefox" should give 1.2x boost
        let boost = tracker
            .calculate_keyword_boost("browser.desktop", "firefox", now)
            .expect("boost");
        assert!((boost - 1.2).abs() < 1e-6);

        // Case insensitivity check
        let boost_caps = tracker
            .calculate_keyword_boost("browser.desktop", "FIREFOX", now)
            .expect("boost caps");
        assert!((boost_caps - 1.2).abs() < 1e-6);

        // Different query gives 1.0x (no boost)
        let boost_other = tracker
            .calculate_keyword_boost("browser.desktop", "chrome", now)
            .expect("boost other");
        assert_eq!(boost_other, 1.0);

        // Empty query gives 1.0x
        let boost_empty = tracker
            .calculate_keyword_boost("browser.desktop", "   ", now)
            .expect("boost empty");
        assert_eq!(boost_empty, 1.0);

        // Record 2 more matching events: 1.0 + 3 * 0.2 = 1.6 -> capped to 1.5x
        tracker
            .record_event(
                "browser.desktop",
                "app",
                EventKind::Launch,
                Some("firefox"),
                now,
            )
            .expect("record 2");
        tracker
            .record_event(
                "browser.desktop",
                "app",
                EventKind::Launch,
                Some("firefox"),
                now,
            )
            .expect("record 3");

        let boost_capped = tracker
            .calculate_keyword_boost("browser.desktop", "firefox", now)
            .expect("boost capped");
        assert!((boost_capped - 1.5).abs() < 1e-6);

        // Combined score with query
        let boosted_score = tracker
            .calculate_score_with_query("browser.desktop", Some("firefox"), now)
            .expect("boosted score");
        let base_score = tracker
            .calculate_score("browser.desktop", now)
            .expect("base score");
        assert!((boosted_score - (base_score * 1.5)).abs() < 1e-6);
    }

    #[test]
    fn test_prune_stale_events() {
        let tracker = setup_test_tracker();
        let now = 1_700_000_000;

        tracker
            .record_event("old.desktop", "app", EventKind::Launch, None, 1000)
            .expect("rec 1");
        tracker
            .record_event("new.desktop", "app", EventKind::Launch, None, now)
            .expect("rec 2");

        assert_eq!(tracker.count_all_events().expect("count"), 2);

        let pruned = tracker.prune_stale_events(5000).expect("prune");
        assert_eq!(pruned, 1);
        assert_eq!(tracker.count_all_events().expect("count"), 1);

        // The old item now has 0 score
        let old_score = tracker.calculate_score("old.desktop", now).expect("score");
        assert_eq!(old_score, 0.0);
    }

    #[test]
    fn test_record_now() {
        let tracker = setup_test_tracker();
        tracker
            .record_event_now("today.desktop", "app", EventKind::Launch, Some("test"))
            .expect("record now");
        assert_eq!(tracker.count_all_events().expect("count"), 1);
    }
}
