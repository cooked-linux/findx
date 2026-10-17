//! Inline calculator, unit converter, and currency exchange search provider.

use std::time::Instant;

use shared::calc::try_evaluate;
use shared::models::SearchRecord;

use crate::arbiter::dispatcher::QueryScope;
use crate::arbiter::provider::{ProviderError, SearchProvider};

/// Search provider evaluating arithmetic expressions, unit conversions, and currency rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CalculatorProvider;

impl CalculatorProvider {
    /// Creates a new [`CalculatorProvider`].
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl SearchProvider for CalculatorProvider {
    fn id(&self) -> &'static str {
        "calculator"
    }

    fn display_name(&self) -> &'static str {
        "Calculator & Converter"
    }

    fn is_fast_path(&self) -> bool {
        true
    }

    fn handles_scope(&self, scope: QueryScope) -> bool {
        matches!(scope, QueryScope::All | QueryScope::Calculator)
    }

    async fn search(
        &self,
        query: &str,
        _filter: Option<&str>,
        _deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError> {
        // Fast synchronous evaluation (<1ms) via Phase 1 engine
        try_evaluate(query).map_or_else(
            || Ok(vec![]),
            |calc_res| {
                let mut record = calc_res.to_search_record();
                // Assign high confidence score to promote calculation as Top Hit candidate
                record.score = 980.0;
                Ok(vec![record])
            },
        )
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
    use shared::models::{SearchCategory, SearchPayload};

    #[tokio::test]
    async fn test_calculator_provider_math() {
        let provider = CalculatorProvider::new();
        let results = provider
            .search("25 * 4 + 10", None, Instant::now())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        let record = &results[0];
        assert_eq!(record.title, "110");
        assert_eq!(record.score, 980.0);
        assert_eq!(record.category, SearchCategory::Calculations);
        if let SearchPayload::Calculation { expression, result } = &record.payload {
            assert_eq!(expression, "25 * 4 + 10");
            assert_eq!(result, "110");
        } else {
            panic!("expected Calculation payload");
        }
    }

    #[tokio::test]
    async fn test_calculator_provider_unit_conversion() {
        let provider = CalculatorProvider::new();
        let results = provider
            .search("10 miles in km", None, Instant::now())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        let record = &results[0];
        assert!(record.title.contains("16.09"));
        assert_eq!(record.score, 980.0);
        assert_eq!(record.category, SearchCategory::Calculations);
    }

    #[tokio::test]
    async fn test_calculator_provider_currency() {
        let provider = CalculatorProvider::new();
        let results = provider
            .search("100 USD to EUR", None, Instant::now())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        let record = &results[0];
        assert_eq!(record.score, 980.0);
        assert_eq!(record.category, SearchCategory::Calculations);
    }

    #[tokio::test]
    async fn test_calculator_provider_suppression() {
        let provider = CalculatorProvider::new();
        // Incomplete expressions return empty results silently
        let res_inc1 = provider
            .search("25 * ", None, Instant::now())
            .await
            .unwrap();
        assert!(res_inc1.is_empty());

        let res_inc2 = provider
            .search("10 + ", None, Instant::now())
            .await
            .unwrap();
        assert!(res_inc2.is_empty());

        // Plain numbers without operators return empty results to prevent search hijacking
        let res_plain = provider.search("1984", None, Instant::now()).await.unwrap();
        assert!(res_plain.is_empty());
    }

    #[test]
    fn test_calculator_provider_metadata() {
        let provider = CalculatorProvider::new();
        assert_eq!(provider.id(), "calculator");
        assert_eq!(provider.display_name(), "Calculator & Converter");
        assert!(provider.is_fast_path());
        assert!(provider.handles_scope(QueryScope::All));
        assert!(provider.handles_scope(QueryScope::Calculator));
        assert!(!provider.handles_scope(QueryScope::Apps));
        assert!(!provider.handles_scope(QueryScope::Files));
    }
}
