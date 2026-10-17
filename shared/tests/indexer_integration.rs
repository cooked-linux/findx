//! Integration tests for filesystem crawler, bounded text extractor,
//! inotify watcher, batch ingestion pipeline, and SQLite FTS5 hybrid search engine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::significant_drop_tightening
)]

use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use shared::db::apply_migrations;
use shared::indexer::{
    CrawlerConfig, DirectoryCrawler, FileWatcher, FsEvent, IndexPipeline, MAX_INDEX_FILE_SIZE,
    SearchQueryBuilder, extract_file_content_sync, search_indexed_files,
};

#[test]
fn test_crawler_mock_hierarchy_skips_blacklist_and_avoids_cycles() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path().to_path_buf();

    // 1. Regular files
    let src_dir = root.join("src");
    std::fs::create_dir(&src_dir).unwrap();
    let main_rs = src_dir.join("main.rs");
    {
        let mut f = File::create(&main_rs).unwrap();
        writeln!(f, "fn main() {{ println!(\"Hello indexer\"); }}").unwrap();
    }

    let docs_dir = root.join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    let guide_md = docs_dir.join("guide.md");
    {
        let mut f = File::create(&guide_md).unwrap();
        writeln!(f, "# Guide to FindX Crawler").unwrap();
    }

    // 2. Blacklisted directory: .git
    let git_dir = root.join(".git");
    std::fs::create_dir(&git_dir).unwrap();
    let git_head = git_dir.join("HEAD");
    {
        let mut f = File::create(&git_head).unwrap();
        writeln!(f, "ref: refs/heads/main").unwrap();
    }

    // 3. Blacklisted directory: node_modules
    let nm_dir = root.join("node_modules");
    std::fs::create_dir(&nm_dir).unwrap();
    let nm_pkg = nm_dir.join("package.json");
    {
        let mut f = File::create(&nm_pkg).unwrap();
        writeln!(f, "{{\"name\": \"ignored\"}}").unwrap();
    }

    // 4. Cyclical symlink pointing back to root
    let cycle_link = src_dir.join("recursive_link");
    let _ = std::os::unix::fs::symlink(&root, &cycle_link);

    // 5. Symlink to existing file (deduplication tracking)
    let symlink_to_file = docs_dir.join("link_to_main.rs");
    let _ = std::os::unix::fs::symlink(&main_rs, &symlink_to_file);

    // Execute crawler
    let config = CrawlerConfig::new(vec![root]);
    let crawler = DirectoryCrawler::new(config);

    let files = crawler.crawl_all().unwrap();
    let discovered_paths: Vec<_> = files.iter().map(|f| &f.path).collect();

    // Verify expected files were discovered (either main.rs or link_to_main.rs deduplicated)
    assert!(discovered_paths.iter().any(|p| {
        let name = p.file_name().unwrap().to_string_lossy();
        name == "main.rs" || name == "link_to_main.rs"
    }));
    assert!(discovered_paths.iter().any(|p| p.ends_with("guide.md")));

    // Verify blacklisted files were NOT discovered
    assert!(!discovered_paths.iter().any(|p| p.ends_with("HEAD")));
    assert!(!discovered_paths.iter().any(|p| p.ends_with("package.json")));
    assert!(
        !discovered_paths
            .iter()
            .any(|p| p.to_string_lossy().contains(".git"))
    );
    assert!(
        !discovered_paths
            .iter()
            .any(|p| p.to_string_lossy().contains("node_modules"))
    );

    // Cycle detection prevented infinite loop; symlink to main.rs deduplicated
    assert_eq!(files.len(), 2);
}

#[test]
fn test_content_extraction_caps_at_2mb() {
    let temp_dir = tempfile::tempdir().unwrap();

    // File > 2MB (2MB + 128 bytes)
    let large_file = temp_dir.path().join("large_payload.txt");
    {
        let f = File::create(&large_file).unwrap();
        f.set_len(MAX_INDEX_FILE_SIZE + 128).unwrap();
    }

    let extracted_large = extract_file_content_sync(&large_file).unwrap();
    assert!(
        extracted_large.is_empty(),
        "Content above 2MB ceiling must not be extracted"
    );

    // File < 2MB (50KB)
    let normal_file = temp_dir.path().join("normal_payload.rs");
    {
        let mut f = File::create(&normal_file).unwrap();
        writeln!(f, "pub fn calculate_frecency() -> f64 {{ 42.0 }}").unwrap();
    }

    let extracted_normal = extract_file_content_sync(&normal_file).unwrap();
    assert!(extracted_normal.contains("calculate_frecency"));
}

#[test]
fn test_pipeline_insert_and_query_bm25_and_snippets() {
    let mut conn = Connection::open_in_memory().unwrap();
    apply_migrations(&mut conn).unwrap();
    let conn_arc = Arc::new(Mutex::new(conn));

    let mut pipeline = IndexPipeline::new(Arc::clone(&conn_arc));
    let temp_dir = tempfile::tempdir().unwrap();

    let file_intro = temp_dir.path().join("findx_intro.md");
    {
        let mut f = File::create(&file_intro).unwrap();
        writeln!(
            f,
            "# FindX\nFindX provides lightning fast sub-50ms hybrid search with full-text snippets and BM25 ranking."
        )
        .unwrap();
    }

    let file_notes = temp_dir.path().join("search_notes.txt");
    {
        let mut f = File::create(&file_notes).unwrap();
        writeln!(
            f,
            "Research notes regarding reciprocal rank fusion and search algorithms."
        )
        .unwrap();
    }

    let file_unrelated = temp_dir.path().join("car_specs.txt");
    {
        let mut f = File::create(&file_unrelated).unwrap();
        writeln!(f, "Vehicles, engines, transmissions, and tires.").unwrap();
    }

    pipeline.enqueue_upsert(file_intro.clone()).unwrap();
    pipeline.enqueue_upsert(file_notes.clone()).unwrap();
    pipeline.enqueue_upsert(file_unrelated).unwrap();
    pipeline.flush().unwrap();

    let conn = conn_arc.lock().unwrap();

    // Query for 'search'
    let hits = search_indexed_files(&conn, "search", 10).unwrap();
    // Builder query execution
    let builder_hits = SearchQueryBuilder::new("hybrid")
        .limit(5)
        .execute(&conn)
        .unwrap();
    drop(conn);

    assert_eq!(hits.len(), 2);

    // search_notes.txt starts with 'search' so it receives the 100.0 filename prefix bonus
    assert_eq!(hits[0].path, file_notes);
    assert_eq!(hits[1].path, file_intro);
    assert!(hits[0].rank_score > 100.0);

    // Snippet verify
    assert!(hits[0].snippet.is_some() || hits[1].snippet.is_some());
    let snip = hits[1].snippet.as_ref().unwrap();
    assert!(snip.contains("<b>search</b>") || snip.contains("search"));

    assert_eq!(builder_hits.len(), 1);
    assert_eq!(builder_hits[0].path, file_intro);
}

#[test]
fn test_pipeline_file_deletion_removes_both_tables() {
    let mut conn = Connection::open_in_memory().unwrap();
    apply_migrations(&mut conn).unwrap();
    let conn_arc = Arc::new(Mutex::new(conn));

    let mut pipeline = IndexPipeline::new(Arc::clone(&conn_arc));
    let temp_dir = tempfile::tempdir().unwrap();

    let file_path = temp_dir.path().join("temporary_delete.txt");
    {
        let mut f = File::create(&file_path).unwrap();
        writeln!(f, "Temporary document to be deleted.").unwrap();
    }

    // 1. Ingest
    pipeline.enqueue_upsert(file_path.clone()).unwrap();
    pipeline.flush().unwrap();

    {
        let conn = conn_arc.lock().unwrap();
        let records_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM file_records;", [], |r| r.get(0))
            .unwrap();
        let fts_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM files_fts;", [], |r| r.get(0))
            .unwrap();
        drop(conn);

        assert_eq!(records_count, 1);
        assert_eq!(fts_count, 1);
    }

    // 2. Delete file and trigger deletion event
    std::fs::remove_file(&file_path).unwrap();
    pipeline.enqueue_event(FsEvent::Deleted(file_path)).unwrap();
    pipeline.flush().unwrap();

    {
        let conn = conn_arc.lock().unwrap();
        let records_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM file_records;", [], |r| r.get(0))
            .unwrap();
        let fts_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM files_fts;", [], |r| r.get(0))
            .unwrap();
        drop(conn);

        assert_eq!(records_count, 0);
        assert_eq!(fts_count, 0);
    }
}

#[tokio::test]
async fn test_pipeline_watcher_event_integration() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path().to_path_buf();

    let mut conn = Connection::open_in_memory().unwrap();
    apply_migrations(&mut conn).unwrap();
    let conn_arc = Arc::new(Mutex::new(conn));

    let mut pipeline = IndexPipeline::new(Arc::clone(&conn_arc));
    let mut watcher =
        FileWatcher::with_debounce(std::slice::from_ref(&root), Duration::from_millis(50)).unwrap();

    // Create a new file in watched directory
    let live_file = root.join("live_event.md");
    {
        let mut f = File::create(&live_file).unwrap();
        writeln!(f, "# Live inotify document for FindX.").unwrap();
    }

    // Wait for debounced FsEvent::Created
    let event = tokio::time::timeout(Duration::from_millis(600), watcher.recv())
        .await
        .unwrap()
        .unwrap();

    pipeline.enqueue_event(event).unwrap();
    pipeline.flush().unwrap();

    {
        let conn = conn_arc.lock().unwrap();
        let hits = search_indexed_files(&conn, "inotify", 5).unwrap();
        drop(conn);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, live_file);
    }
}
