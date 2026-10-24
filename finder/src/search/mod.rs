//! In-app search and scope switching subsystem.

pub mod error;
pub mod executor;
pub mod highlight;
pub mod parser;
pub mod scope;
pub mod scope_bar;
pub mod types;

pub use error::{Result, SearchError};
pub use executor::{
    SearchCancellation, SearchExecutionParams, execute_local_folder_search, execute_search,
};
pub use highlight::{
    escape_pango, format_highlighted_snippet, highlight_matched_text, highlight_query,
};
pub use parser::parse_query;
pub use scope::{
    ScopeBarState, SearchHistory, SearchHistoryEntry, clear_history, history_file_path,
    load_history, recent_queries, record_query,
};
pub use scope_bar::ScopeBarWidget;
pub use types::{
    AttributeFilter, DateConstraint, ParsedQuery, SearchMode, SearchResultItem, SearchScope,
    SearchToken, SizeFilter, SizeOperator,
};
