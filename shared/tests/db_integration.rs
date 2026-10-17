//! Integration tests for embedded SQLite database, versioned schema migrations,
//! FTS5 full-text search with BM25 and snippets, frecency tracking, and tag cache.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::cast_possible_truncation
)]

use rusqlite::params;
use shared::db::{
    Database, FtsDocument, FtsQueryBuilder, TagCacheEntry, aggregate_event_counts,
    count_all_events, count_all_tags, count_documents, count_events_in_window, current_version,
    delete_document, get_events_for_item, index_document, list_all_tags, list_applied_migrations,
    prune_events_before, query_paths_by_tag, query_tags_for_path, record_event, remove_stale_paths,
    remove_tag, remove_tags_for_path, search, sync_path_tags, upsert_tag,
};

#[test]
fn test_integration_in_memory_initialization_and_migrations() {
    // 1. Test unmigrated initialization
    let mut db_unmigrated = Database::open_in_memory_unmigrated().expect("open unmigrated");
    assert_eq!(
        current_version(&db_unmigrated).expect("check version before migration"),
        None
    );

    let applied = db_unmigrated.migrate().expect("apply migrations");
    assert_eq!(applied, 2, "2 migrations should be applied");
    assert_eq!(
        current_version(&db_unmigrated).expect("check version after migration"),
        Some(2)
    );

    // 2. Test standard in-memory initialization (auto-migrated)
    let mut db = Database::open_in_memory().expect("open_in_memory");
    assert_eq!(current_version(&db).expect("current_version"), Some(2));

    // Subsequent migration execution should be idempotent
    let second_pass = db.migrate().expect("idempotent migration");
    assert_eq!(second_pass, 0, "no additional migrations should apply");

    // 3. Verify PRAGMAs
    let foreign_keys: i64 = db
        .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
        .expect("PRAGMA foreign_keys");
    assert_eq!(foreign_keys, 1);

    let busy_timeout: i64 = db
        .query_row("PRAGMA busy_timeout;", [], |row| row.get(0))
        .expect("PRAGMA busy_timeout");
    assert_eq!(busy_timeout, 5000);

    let cache_size: i64 = db
        .query_row("PRAGMA cache_size;", [], |row| row.get(0))
        .expect("PRAGMA cache_size");
    assert_eq!(cache_size, -20000);

    // 4. Verify applied migrations log
    let history = list_applied_migrations(&db).expect("list migrations");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].0, 1);
    assert_eq!(history[1].0, 2);
    assert!(!history[0].1.is_empty());
    assert!(!history[1].1.is_empty());
}

#[test]
fn test_integration_fts5_indexing_and_prefix_queries() {
    let db = Database::open_in_memory().expect("open_in_memory");

    let doc_report = FtsDocument::new(
        "/home/alice/Documents/file_annual_report_2026.pdf",
        "file_annual_report_2026.pdf",
        "Comprehensive annual financial statement summarizing distributed microservices revenue and growth metrics.",
        "application/pdf",
        "finance,annual,audit",
    );

    let doc_source = FtsDocument::new(
        "/home/alice/Projects/findx/file_crawler.rs",
        "file_crawler.rs",
        "pub async fn crawl_filesystem() -> Result<()> { log::info!(\"Distributed filesystem crawler\"); }",
        "text/rust",
        "code,indexer",
    );

    let doc_notes = FtsDocument::new(
        "/home/alice/Notes/meeting_agenda.txt",
        "meeting_agenda.txt",
        "Discuss team quarterly deliverables and system architecture roadmap.",
        "text/plain",
        "work,notes",
    );

    index_document(&db, &doc_report).expect("index doc_report");
    index_document(&db, &doc_source).expect("index doc_source");
    index_document(&db, &doc_notes).expect("index doc_notes");

    assert_eq!(count_documents(&db).expect("count"), 3);

    // 1. Prefix query matching filename: "file*"
    let prefix_results = search(&db, "file*", 10).expect("search file*");
    assert_eq!(
        prefix_results.len(),
        2,
        "prefix search 'file*' should match both files starting with 'file'"
    );

    // Verify snippet content has matches
    for hit in &prefix_results {
        assert!(hit.filename.starts_with("file_"));
    }

    // 2. Prefix query matching body content: "distrib*"
    let distrib_results = FtsQueryBuilder::new("distrib*")
        .snippet_config(2, "<b>", "</b>", "...", 12)
        .execute(&db)
        .expect("search distrib*");

    assert_eq!(
        distrib_results.len(),
        2,
        "both documents containing 'distributed' should match"
    );
    for hit in &distrib_results {
        assert!(hit.snippet.to_lowercase().contains("<b>distribut"));
    }

    // 3. MIME type filtered search
    let code_results = FtsQueryBuilder::new("file*")
        .mime_type("text/rust")
        .execute(&db)
        .expect("search file* with text/rust filter");

    assert_eq!(code_results.len(), 1);
    assert_eq!(code_results[0].filename, "file_crawler.rs");
    assert_eq!(code_results[0].mime_type, "text/rust");

    // 4. Update / Re-index document at existing path
    let doc_source_updated = FtsDocument::new(
        "/home/alice/Projects/findx/file_crawler.rs",
        "file_crawler.rs",
        "pub async fn crawl_filesystem() -> Result<()> { log::info!(\"Upgraded crawler logic\"); }",
        "text/rust",
        "code,indexer,v2",
    );
    index_document(&db, &doc_source_updated).expect("re-index");
    assert_eq!(count_documents(&db).expect("count after re-index"), 3);

    let upgraded_results = search(&db, "upgraded", 10).expect("search upgraded");
    assert_eq!(upgraded_results.len(), 1);
    assert_eq!(
        upgraded_results[0].path,
        "/home/alice/Projects/findx/file_crawler.rs"
    );

    // 5. Delete document
    let deleted_count =
        delete_document(&db, "/home/alice/Projects/findx/file_crawler.rs").expect("delete");
    assert_eq!(deleted_count, 1);
    assert_eq!(count_documents(&db).expect("count after delete"), 2);

    let search_deleted = search(&db, "upgraded", 10).expect("search deleted");
    assert!(search_deleted.is_empty());
}

#[test]
fn test_integration_frecency_engagement_records() {
    let db = Database::open_in_memory().expect("open_in_memory");

    // 1. Record events across multiple items and types
    let ts_base = 1_700_000_000_i64;

    record_event(
        &db,
        "org.gnome.Terminal.desktop",
        "app",
        "launch",
        ts_base + 100,
        Some("term"),
    )
    .expect("record terminal launch");

    record_event(
        &db,
        "org.gnome.Terminal.desktop",
        "app",
        "launch",
        ts_base + 200,
        Some("terminal"),
    )
    .expect("record terminal launch 2");

    record_event(
        &db,
        "org.gnome.Terminal.desktop",
        "app",
        "launch",
        ts_base + 300,
        None,
    )
    .expect("record terminal launch 3");

    record_event(
        &db,
        "org.mozilla.firefox.desktop",
        "app",
        "launch",
        ts_base + 250,
        Some("browser"),
    )
    .expect("record firefox launch");

    record_event(
        &db,
        "/home/alice/Documents/budget.xlsx",
        "file",
        "open",
        ts_base + 150,
        None,
    )
    .expect("record file open");

    record_event(
        &db,
        "/home/alice/Documents/budget.xlsx",
        "file",
        "reveal",
        ts_base + 180,
        None,
    )
    .expect("record file reveal");

    assert_eq!(count_all_events(&db).expect("count"), 6);

    // 2. Retrieve events for an item with timestamp filter
    let term_events = get_events_for_item(&db, "org.gnome.Terminal.desktop", ts_base + 150)
        .expect("get terminal events");
    assert_eq!(term_events.len(), 2);
    assert_eq!(term_events[0].timestamp, ts_base + 300); // Descending order
    assert_eq!(term_events[1].timestamp, ts_base + 200);
    assert_eq!(term_events[1].query.as_deref(), Some("terminal"));

    // 3. Count in time window
    let window_count = count_events_in_window(
        &db,
        "org.gnome.Terminal.desktop",
        ts_base + 100,
        ts_base + 250,
    )
    .expect("count in window");
    assert_eq!(window_count, 2);

    // 4. Aggregates: top items by frequency
    let top_apps = aggregate_event_counts(&db, Some("app"), ts_base, 10).expect("top apps");
    assert_eq!(top_apps[0].item_id, "org.gnome.Terminal.desktop");
    assert_eq!(top_apps[0].count, 3);
    assert_eq!(top_apps[1].item_id, "org.mozilla.firefox.desktop");
    assert_eq!(top_apps[1].count, 1);

    let top_files = aggregate_event_counts(&db, Some("file"), ts_base, 10).expect("top files");
    assert_eq!(top_files.len(), 2); // 1 open, 1 reveal

    // 5. Pruning older events
    let pruned = prune_events_before(&db, ts_base + 200).expect("prune");
    assert_eq!(
        pruned, 3,
        "events strictly before ts_base + 200 should be pruned"
    );
    assert_eq!(count_all_events(&db).expect("count after prune"), 3);
}

#[test]
fn test_integration_tag_cache_and_constraints() {
    let mut db = Database::open_in_memory().expect("open_in_memory");

    // 1. Test primary key constraint directly via SQLite
    db.execute(
        "INSERT INTO tag_cache (path, tag_name, tag_color, mtime) VALUES (?1, ?2, ?3, ?4);",
        params!["/docs/contract.pdf", "Legal", "Purple", 1_000_000],
    )
    .expect("first insert must succeed");

    let duplicate_insert = db.execute(
        "INSERT INTO tag_cache (path, tag_name, tag_color, mtime) VALUES (?1, ?2, ?3, ?4);",
        params!["/docs/contract.pdf", "Legal", "Red", 1_000_001],
    );
    assert!(
        duplicate_insert.is_err(),
        "inserting duplicate (path, tag_name) must fail due to PRIMARY KEY constraint"
    );

    // 2. Test upsert_tag handles conflict cleanly
    let updated_entry = TagCacheEntry::new(
        "/docs/contract.pdf",
        "Legal",
        Some("Red".to_string()),
        1_000_005,
    );
    upsert_tag(&db, &updated_entry).expect("upsert on conflict must succeed");

    let tags = query_tags_for_path(&db, "/docs/contract.pdf").expect("query tags");
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].tag_color.as_deref(), Some("Red"));
    assert_eq!(tags[0].mtime, 1_000_005);

    // 3. Multi-tag indexing and queries
    let entries = [
        TagCacheEntry::new(
            "/docs/contract.pdf",
            "Confidential",
            Some("Orange".into()),
            1_000_005,
        ),
        TagCacheEntry::new(
            "/docs/invoice.pdf",
            "Finance",
            Some("Green".into()),
            1_000_010,
        ),
        TagCacheEntry::new(
            "/docs/invoice.pdf",
            "Confidential",
            Some("Orange".into()),
            1_000_010,
        ),
        TagCacheEntry::new(
            "/photos/vacation.jpg",
            "Travel",
            Some("Blue".into()),
            1_000_020,
        ),
    ];
    for entry in &entries {
        upsert_tag(&db, entry).expect("insert entry");
    }

    assert_eq!(count_all_tags(&db).expect("count"), 5);

    // Query paths by tag name (ordered alphabetically)
    let confidential_paths = query_paths_by_tag(&db, "Confidential").expect("query confidential");
    assert_eq!(
        confidential_paths,
        vec!["/docs/contract.pdf", "/docs/invoice.pdf"]
    );

    // Query tags for path (ordered alphabetically by tag name)
    let contract_tags =
        query_tags_for_path(&db, "/docs/contract.pdf").expect("query contract tags");
    assert_eq!(contract_tags.len(), 2);
    assert_eq!(contract_tags[0].tag_name, "Confidential");
    assert_eq!(contract_tags[1].tag_name, "Legal");

    // List all distinct tags with file counts
    let tag_summary = list_all_tags(&db).expect("list all tags");
    assert_eq!(tag_summary[0], ("Confidential".to_string(), 2));

    // 4. Atomic path synchronization
    let new_tags = [("Finance", Some("Green")), ("Processed", None)];
    sync_path_tags(&mut db, "/docs/invoice.pdf", &new_tags, 1_000_050).expect("sync tags");

    let invoice_tags = query_tags_for_path(&db, "/docs/invoice.pdf").expect("query invoice tags");
    assert_eq!(invoice_tags.len(), 2);
    assert_eq!(invoice_tags[0].tag_name, "Finance");
    assert_eq!(invoice_tags[1].tag_name, "Processed");

    // 5. Remove single tag & remove all tags for path
    let removed = remove_tag(&db, "/docs/invoice.pdf", "Processed").expect("remove tag");
    assert!(removed);
    assert_eq!(
        query_tags_for_path(&db, "/docs/invoice.pdf")
            .expect("q")
            .len(),
        1
    );

    let removed_path = remove_tags_for_path(&db, "/docs/invoice.pdf").expect("remove path");
    assert_eq!(removed_path, 1);
    assert!(
        query_tags_for_path(&db, "/docs/invoice.pdf")
            .expect("q")
            .is_empty()
    );

    // 6. Batch remove stale paths
    let deleted_stale =
        remove_stale_paths(&mut db, &["/docs/contract.pdf", "/photos/vacation.jpg"])
            .expect("batch remove stale");
    assert_eq!(deleted_stale, 3);
    assert_eq!(count_all_tags(&db).expect("remaining tags"), 0);
}
