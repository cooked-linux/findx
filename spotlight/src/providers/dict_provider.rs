//! Offline lexical dictionary and quick answers search provider.
//!
//! Connects directly to [`DictionaryStorage`] and [`Lemmatizer`], resolving explicit
//! definition triggers (`define <word>`, `dict <word>`, `dict:<word>`) and implicit
//! vocabulary queries, emitting canonical [`SearchRecord`] items and accompanying web suggestions.

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use shared::dict::{DictionaryStorage, Lemmatizer, resolve_query};
use shared::models::{SearchCategory, SearchPayload, SearchRecord};

use crate::arbiter::dispatcher::QueryScope;
use crate::arbiter::provider::{ProviderError, SearchProvider};

/// Search provider resolving explicit definitions and vocabulary lookups from local dictionaries.
#[derive(Clone)]
pub struct DictionaryProvider {
    storage: Option<Arc<DictionaryStorage>>,
    lemmatizer: Arc<Lemmatizer>,
}

impl fmt::Debug for DictionaryProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DictionaryProvider")
            .field("has_storage", &self.storage.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for DictionaryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl DictionaryProvider {
    /// Creates a new [`DictionaryProvider`] looking up installed databases in standard locations.
    #[must_use]
    pub fn new() -> Self {
        let storage = DictionaryStorage::open_default()
            .ok()
            .flatten()
            .map(Arc::new);
        let lemmatizer = Arc::new(Lemmatizer::new());
        Self {
            storage,
            lemmatizer,
        }
    }

    /// Creates a [`DictionaryProvider`] with an explicitly provided [`DictionaryStorage`].
    #[must_use]
    pub fn with_storage(storage: Option<Arc<DictionaryStorage>>) -> Self {
        Self {
            storage,
            lemmatizer: Arc::new(Lemmatizer::new()),
        }
    }

    /// Creates a [`DictionaryProvider`] initialized from an owned [`DictionaryStorage`].
    #[must_use]
    pub fn from_storage(storage: DictionaryStorage) -> Self {
        Self::with_storage(Some(Arc::new(storage)))
    }

    /// Returns `true` if a local dictionary database is available.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        self.storage.is_some()
    }

    /// Returns a reference to the inner [`DictionaryStorage`], if available.
    #[must_use]
    pub const fn storage(&self) -> Option<&Arc<DictionaryStorage>> {
        self.storage.as_ref()
    }
}

#[async_trait::async_trait]
impl SearchProvider for DictionaryProvider {
    fn id(&self) -> &'static str {
        "dictionary"
    }

    fn display_name(&self) -> &'static str {
        "Dictionary"
    }

    fn is_fast_path(&self) -> bool {
        true
    }

    fn handles_scope(&self, scope: QueryScope) -> bool {
        matches!(scope, QueryScope::All | QueryScope::Dictionary)
    }

    async fn search(
        &self,
        query: &str,
        _filter: Option<&str>,
        _deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError> {
        let Some(ref storage) = self.storage else {
            return Ok(Vec::new());
        };

        let Ok(Some(card)) = resolve_query(query, storage.as_ref(), &self.lemmatizer) else {
            return Ok(Vec::new()); // Graceful degradation: fail silently
        };

        let mut records = Vec::with_capacity(3);

        // 1. Primary canonical Dictionary Record
        let headword = card.headword.clone();
        records.push(card.to_search_record());

        // 2. Accompanying Web Suggestion rows:
        //    - "{word} meaning" (Web Suggestion)
        //    - "{word} examples" (Web Suggestion)
        let meaning_query = format!("{headword} meaning");
        let meaning_url = format!(
            "https://duckduckgo.com/?q={}",
            urlencoding::encode(&meaning_query)
        );
        records.push(SearchRecord::new(
            format!("web:dict:{}:meaning", headword.to_ascii_lowercase()),
            meaning_query,
            Some("Web Suggestion".to_string()),
            SearchCategory::WebSearch,
            Some("applications-internet".to_string()),
            200.0,
            SearchPayload::WebSearch { url: meaning_url },
        ));

        let examples_query = format!("{headword} examples");
        let examples_url = format!(
            "https://duckduckgo.com/?q={}",
            urlencoding::encode(&examples_query)
        );
        records.push(SearchRecord::new(
            format!("web:dict:{}:examples", headword.to_ascii_lowercase()),
            examples_query,
            Some("Web Suggestion".to_string()),
            SearchCategory::WebSearch,
            Some("applications-internet".to_string()),
            190.0,
            SearchPayload::WebSearch { url: examples_url },
        ));

        Ok(records)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use shared::dict::{DefinitionSense, DictionaryCard};
    use std::time::Duration;

    fn create_test_storage() -> DictionaryStorage {
        let storage = DictionaryStorage::open_in_memory().expect("open storage");
        let card = DictionaryCard::new(
            "serendipity",
            Some("/ˌserənˈdɪpədē/".to_string()),
            vec![DefinitionSense::new(
                "noun",
                "The occurrence and development of events by chance in a happy way.",
                Some("A fortunate stroke of serendipity.".to_string()),
            )],
            vec!["chance".to_string(), "fortune".to_string()],
            Some("Coined by Horace Walpole in 1754.".to_string()),
            450.0,
        );
        storage.insert_entry(&card).expect("insert card");
        storage
    }

    #[tokio::test]
    async fn test_provider_missing_storage_silent() {
        let provider = DictionaryProvider::with_storage(None);
        assert!(!provider.is_available());

        let results = provider
            .search(
                "define serendipity",
                None,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .expect("search succeeds");
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_provider_explicit_query() {
        let storage = create_test_storage();
        let provider = DictionaryProvider::from_storage(storage);
        assert!(provider.is_available());

        let results = provider
            .search(
                "define serendipity",
                None,
                Instant::now() + Duration::from_secs(1),
            )
            .await
            .expect("search succeeds");

        assert_eq!(results.len(), 3);

        // Record 0: Dictionary entry with score 900.0
        let dict_rec = &results[0];
        assert_eq!(dict_rec.title, "serendipity");
        assert_eq!(dict_rec.category, SearchCategory::Dictionary);
        assert_eq!(dict_rec.score, 900.0);
        assert_eq!(
            dict_rec.icon_name.as_deref(),
            Some("accessories-dictionary")
        );

        // Record 1: meaning web suggestion
        let meaning_rec = &results[1];
        assert_eq!(meaning_rec.title, "serendipity meaning");
        assert_eq!(meaning_rec.subtitle.as_deref(), Some("Web Suggestion"));
        assert_eq!(meaning_rec.category, SearchCategory::WebSearch);
        assert_eq!(meaning_rec.score, 200.0);

        // Record 2: examples web suggestion
        let examples_rec = &results[2];
        assert_eq!(examples_rec.title, "serendipity examples");
        assert_eq!(examples_rec.subtitle.as_deref(), Some("Web Suggestion"));
        assert_eq!(examples_rec.category, SearchCategory::WebSearch);
        assert_eq!(examples_rec.score, 190.0);
    }

    #[tokio::test]
    async fn test_provider_implicit_query() {
        let storage = create_test_storage();
        let provider = DictionaryProvider::from_storage(storage);

        let results = provider
            .search("serendipity", None, Instant::now() + Duration::from_secs(1))
            .await
            .expect("search succeeds");

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].score, 450.0);
    }

    #[test]
    fn test_provider_metadata() {
        let provider = DictionaryProvider::new();
        assert_eq!(provider.id(), "dictionary");
        assert_eq!(provider.display_name(), "Dictionary");
        assert!(provider.is_fast_path());
        assert!(provider.handles_scope(QueryScope::All));
        assert!(provider.handles_scope(QueryScope::Dictionary));
        assert!(!provider.handles_scope(QueryScope::Apps));
        assert!(!provider.handles_scope(QueryScope::Files));
        assert!(!provider.handles_scope(QueryScope::Calculator));
        assert!(!provider.handles_scope(QueryScope::Web));
    }
}
