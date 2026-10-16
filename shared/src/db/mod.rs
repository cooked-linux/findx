//! SQLite database engine, schema migrations, and indexing tables.

pub mod connection;
pub mod frecency;
pub mod fts;
pub mod migrations;
pub mod tags;

pub use connection::{configure_connection, default_db_path, Database, PRAGMAS};
pub use frecency::{
    aggregate_event_counts, count_all_events, count_events_in_window, get_events_for_item,
    prune_events_before, record_event, record_event_now, FrecencyAggregate, FrecencyEvent,
};
pub use fts::{
    count_documents, delete_document, index_document, sanitize_query, search, FtsDocument,
    FtsQueryBuilder, FtsSearchResult,
};
pub use migrations::{
    apply_migrations, current_version, list_applied_migrations, Migration, MIGRATIONS,
};
pub use tags::{
    count_all_tags, list_all_tags, query_paths_by_tag, query_tags_for_path, remove_stale_paths,
    remove_tag, remove_tags_for_path, sync_path_tags, upsert_tag, TagCacheEntry,
};
