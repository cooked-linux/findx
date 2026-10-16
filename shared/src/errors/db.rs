//! Database and persistence error definitions.

/// Errors arising from database access, migrations, or indexing operations.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// SQLite engine error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A schema migration step failed.
    #[error("database migration to version {version} failed: {message}")]
    MigrationFailed {
        /// Schema version target that failed to apply.
        version: u32,
        /// Diagnostic message explaining migration failure.
        message: String,
    },

    /// The database connection pool has no available connections.
    #[error("database connection pool exhausted")]
    PoolExhausted,

    /// Full-text search or tag index table corruption detected.
    #[error("corrupted search index: {0}")]
    CorruptedIndex(String),

    /// Serialization or deserialization error during database interaction.
    #[error("database serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Filesystem or I/O error during database initialization or file operations.
    #[error("database i/o error: {0}")]
    Io(#[from] std::io::Error),
}

impl DbError {
    /// Constructs a [`DbError::MigrationFailed`] variant.
    #[must_use]
    pub fn migration_failed(version: u32, message: impl Into<String>) -> Self {
        Self::MigrationFailed {
            version,
            message: message.into(),
        }
    }

    /// Constructs a [`DbError::CorruptedIndex`] variant.
    #[must_use]
    pub fn corrupted_index(message: impl Into<String>) -> Self {
        Self::CorruptedIndex(message.into())
    }
}
