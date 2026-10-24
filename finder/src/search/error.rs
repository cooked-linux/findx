//! Search domain errors and result types.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Database error: {0}")]
    Db(#[from] shared::errors::DbError),
    #[error("Indexer error: {0}")]
    Indexer(#[from] shared::indexer::IndexerError),
    #[error("Query parse error in '{query}': {message}")]
    Parse { query: String, message: String },
    #[error("Invalid search operator '{operator}': {reason}")]
    InvalidOperator { operator: String, reason: String },
    #[error("Search query execution was cancelled")]
    Cancelled,
    #[error("Search history serialization error: {0}")]
    History(String),
    #[error("Filesystem traversal aborted: {message}")]
    Traversal { message: String },
}

impl From<rusqlite::Error> for SearchError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Db(shared::errors::DbError::from(err))
    }
}

pub type Result<T> = std::result::Result<T, SearchError>;
