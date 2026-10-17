//! Lexicographical dictionary and quick answers engine.

pub mod lookup;
pub mod models;
pub mod stemmer;
pub mod storage;

pub use lookup::{normalize_key, parse_trigger, resolve_query};
pub use models::{DefinitionSense, DictionaryCard};
pub use stemmer::Lemmatizer;
pub use storage::DictionaryStorage;

/// Domain error variants for the dictionary engine.
#[derive(Debug, thiserror::Error)]
pub enum DictError {
    /// Failure during SQLite database interaction.
    #[error("Database error in dictionary engine: {0}")]
    Db(#[from] rusqlite::Error),

    /// Failure during JSON serialization or deserialization.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// Requested dictionary headword or entry could not be found.
    #[error("Dictionary entry not found: {0}")]
    NotFound(String),

    /// I/O error encountered while resolving filesystem paths.
    #[error("I/O error in dictionary engine: {0}")]
    Io(#[from] std::io::Error),
}
