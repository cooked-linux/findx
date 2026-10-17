//! Search provider trait definition and error types.

use std::time::Instant;

use shared::models::SearchRecord;

use crate::arbiter::dispatcher::QueryScope;

/// Errors that can occur during provider search operations.
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// The search was superseded by a newer query or explicitly cancelled.
    #[error("Search cancelled or superseded")]
    Cancelled,

    /// The provider failed to finish before its allotted deadline.
    #[error("Search timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// An internal failure occurred inside the search provider.
    #[error("Internal provider failure: {0}")]
    Failure(String),
}

/// Asynchronous search provider interface for candidate retrieval.
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    /// Unique provider identifier (e.g. "applications", "calculator").
    fn id(&self) -> &'static str;

    /// Human-readable provider name for logging and diagnostics.
    fn display_name(&self) -> &'static str;

    /// Executes a search query within the allotted deadline.
    ///
    /// # Errors
    /// Returns a [`ProviderError`] if the query is cancelled, times out, or fails.
    async fn search(
        &self,
        query: &str,
        filter: Option<&str>,
        deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError>;

    /// Returns `true` if this provider is an ultra-fast synchronous/in-memory provider
    /// (e.g. Calculator, In-memory Trie, System Actions) expected to return in <3ms.
    fn is_fast_path(&self) -> bool {
        false
    }

    /// Checks whether this provider should execute for the given query scope filter.
    fn handles_scope(&self, scope: QueryScope) -> bool {
        match scope {
            QueryScope::All => true,
            QueryScope::Apps => matches!(self.id(), "applications" | "app" | "launcher"),
            QueryScope::Files => matches!(self.id(), "files" | "file" | "indexer" | "fts5"),
            QueryScope::Calculator => matches!(self.id(), "calculator" | "calc"),
            QueryScope::Dictionary => matches!(self.id(), "dictionary" | "dict"),
            QueryScope::Web => matches!(self.id(), "web" | "websearch" | "web_search"),
        }
    }
}
