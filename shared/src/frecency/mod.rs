//! Headless frecency (frequency + recency) engagement and decay ranking engine.
//!
//! Evaluates application and file engagement using an exponential half-life decay model,
//! query keyword boosts, and composite score ranking.

pub mod decay;
pub mod scorer;
pub mod tracker;

pub use decay::{
    DEFAULT_HALF_LIFE_SECS, EventKind, MAX_RETENTION_SECS, MIN_DECAY_THRESHOLD, compute_event_score,
};
pub use scorer::{
    CompositeScorer, DEFAULT_FRECENCY_WEIGHT, DEFAULT_TOP_HIT_EXACT_MATCH_SCORE,
    DEFAULT_TOP_HIT_LEAD_MARGIN, DEFAULT_TOP_HIT_MIN_TEXT_SCORE, RankedCandidate, rank_and_sort,
};
pub use tracker::{DEFAULT_SCORE_WINDOW_SECS, FrecencyTracker};

/// Domain error variants for the frecency ranking and tracking subsystem.
#[derive(Debug, thiserror::Error)]
pub enum FrecencyError {
    /// SQLite engine failure during event recording or querying.
    #[error("Database error in frecency engine: {0}")]
    Db(#[from] rusqlite::Error),

    /// Timestamp invalid or out of acceptable bounds.
    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    /// Numeric overflow, division by zero, or NaN encountered during score calculation.
    #[error("Calculation overflow or NaN encountered")]
    NumericError,

    /// Failure to acquire lock on the database connection mutex.
    #[error("Database lock acquisition failed: {0}")]
    LockError(String),
}

impl From<crate::errors::DbError> for FrecencyError {
    fn from(err: crate::errors::DbError) -> Self {
        match err {
            crate::errors::DbError::Sqlite(e) => Self::Db(e),
            other => Self::Db(rusqlite::Error::ToSqlConversionFailure(Box::new(other))),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_frecency_error_display() {
        let err = FrecencyError::InvalidTimestamp("epoch negative".to_string());
        assert_eq!(err.to_string(), "Invalid timestamp: epoch negative");

        let err = FrecencyError::NumericError;
        assert_eq!(err.to_string(), "Calculation overflow or NaN encountered");

        let err = FrecencyError::LockError("mutex poisoned".to_string());
        assert_eq!(
            err.to_string(),
            "Database lock acquisition failed: mutex poisoned"
        );

        let rusqlite_err = rusqlite::Error::ExecuteReturnedResults;
        let err = FrecencyError::from(rusqlite_err);
        assert!(
            err.to_string()
                .starts_with("Database error in frecency engine:")
        );
    }
}
