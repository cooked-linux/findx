//! SQLite database engine, schema migrations, and indexing tables.

pub mod connection;
pub mod frecency;
pub mod fts;
pub mod migrations;
pub mod tags;

pub use connection::{Database, PRAGMAS, configure_connection, default_db_path};
pub use frecency::{
    FrecencyAggregate, FrecencyEvent, aggregate_event_counts, count_all_events,
    count_events_in_window, get_events_for_item, prune_events_before, record_event,
    record_event_now,
};
pub use fts::{
    FtsDocument, FtsQueryBuilder, FtsSearchResult, count_documents, delete_document,
    index_document, sanitize_query, search,
};
pub use migrations::{
    MIGRATIONS, Migration, apply_migrations, current_version, list_applied_migrations,
};
pub use tags::{
    TagCacheEntry, count_all_tags, list_all_tags, query_paths_by_tag, query_tags_for_path,
    remove_stale_paths, remove_tag, remove_tags_for_path, sync_path_tags, upsert_tag,
};
