//! Query arbiter, multi-provider search dispatcher, and ranking synthesizers.

pub mod dispatcher;
pub mod normalizer;
pub mod provider;

pub use dispatcher::{
    ParsedQuery, QueryDispatcher, QueryScope, StreamingUpdate, parse_query_scope,
};
pub use normalizer::{
    DEFAULT_MAX_PER_CATEGORY, DEFAULT_TOP_HIT_THRESHOLD, SelectionState, flatten_records,
    synthesize_results,
};
pub use provider::{ProviderError, SearchProvider};

pub use shared::models::{
    AggregatedSearchResults, CategorySection, SearchCategory, SearchPayload, SearchRecord,
};
