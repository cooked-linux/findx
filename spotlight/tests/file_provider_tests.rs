//! Integration tests for `FileSearchProvider` and file search results.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]

use std::fs::{self, File};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use shared::db::apply_migrations;
use shared::indexer::IndexPipeline;
use shared::models::{SearchCategory, SearchPayload};
use spotlight::arbiter::dispatcher::QueryScope;
use spotlight::arbiter::provider::{ProviderError, SearchProvider};
use spotlight::providers::file_provider::FileSearchProvider;

fn setup_test_db() -> Arc<Mutex<Connection>> {
    let mut conn = Connection::open_in_memory().unwrap();
    apply_migrations(&mut conn).unwrap();
    Arc::new(Mutex::new(conn))
}

#[tokio::test]
async fn test_file_provider_filename_match_categorized_as_documents() {
    let conn_arc = setup_test_db();
    let temp_dir = tempfile::tempdir().unwrap();

    let doc_path = temp_dir.path().join("financial_statement_2026.pdf");
    {
        let mut f = File::create(&doc_path).unwrap();
        writeln!(f, "Q3 Balance sheet and corporate income details.").unwrap();
    }

    let mut pipeline = IndexPipeline::new(Arc::clone(&conn_arc));
    pipeline.enqueue_upsert(doc_path.clone()).unwrap();
    pipeline.flush().unwrap();

    let provider = FileSearchProvider::new(conn_arc, 10);
    assert_eq!(provider.id(), "files");
    assert_eq!(provider.display_name(), "Files");
    assert!(!provider.is_fast_path());
    assert!(provider.handles_scope(QueryScope::Files));
    assert!(provider.handles_scope(QueryScope::All));
    assert!(!provider.handles_scope(QueryScope::Apps));

    let deadline = Instant::now() + Duration::from_secs(5);
    let results = provider.search("financial", None, deadline).await.unwrap();

    assert!(!results.is_empty());
    let hit = &results[0];
    assert_eq!(hit.title, "financial_statement_2026.pdf");
    assert_eq!(hit.category, SearchCategory::Documents);
    assert_eq!(hit.icon_name, Some("x-office-document".to_string()));
    assert_eq!(
        hit.payload,
        SearchPayload::File {
            path: doc_path.clone()
        }
    );
}

#[tokio::test]
async fn test_file_provider_snippet_highlight_contains_bold_tag() {
    let conn_arc = setup_test_db();
    let temp_dir = tempfile::tempdir().unwrap();

    let notes_path = temp_dir.path().join("notes.txt");
    {
        let mut f = File::create(&notes_path).unwrap();
        writeln!(
            f,
            "Spotlight implements deep neural search with vector acceleration."
        )
        .unwrap();
    }

    let mut pipeline = IndexPipeline::new(Arc::clone(&conn_arc));
    pipeline.enqueue_upsert(notes_path.clone()).unwrap();
    pipeline.flush().unwrap();

    let provider = FileSearchProvider::new(conn_arc, 10);
    let deadline = Instant::now() + Duration::from_secs(5);

    // Search for a term that only appears in content ("neural")
    let results = provider.search("neural", None, deadline).await.unwrap();

    assert!(!results.is_empty());
    let hit = &results[0];
    assert_eq!(hit.title, "notes.txt");
    assert_eq!(hit.category, SearchCategory::Documents);
    assert!(hit.subtitle.is_some());

    let snippet = hit.subtitle.as_ref().unwrap();
    assert!(
        snippet.contains("<b>neural</b>") || snippet.contains("neural"),
        "Snippet should contain highlighted query term, got: {snippet}"
    );
}

#[tokio::test]
async fn test_file_provider_folder_categorization() {
    let conn_arc = setup_test_db();
    let temp_dir = tempfile::tempdir().unwrap();

    let folder_path = temp_dir.path().join("ProjectArchives");
    fs::create_dir(&folder_path).unwrap();

    let mut pipeline = IndexPipeline::new(Arc::clone(&conn_arc));
    pipeline.enqueue_upsert(folder_path.clone()).unwrap();
    pipeline.flush().unwrap();

    let provider = FileSearchProvider::new(conn_arc, 10);
    let deadline = Instant::now() + Duration::from_secs(5);

    let results = provider
        .search("ProjectArchives", None, deadline)
        .await
        .unwrap();

    assert!(!results.is_empty());
    let hit = &results[0];
    assert_eq!(hit.title, "ProjectArchives");
    assert_eq!(hit.category, SearchCategory::Folders);
    assert_eq!(hit.icon_name, Some("folder".to_string()));
    assert_eq!(hit.payload, SearchPayload::File { path: folder_path });
}

#[tokio::test]
async fn test_file_provider_empty_query_returns_empty() {
    let conn_arc = setup_test_db();
    let provider = FileSearchProvider::new(conn_arc, 10);
    let deadline = Instant::now() + Duration::from_secs(5);

    let results = provider.search("   ", None, deadline).await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_file_provider_deadline_timeout() {
    let conn_arc = setup_test_db();
    let provider = FileSearchProvider::new(conn_arc, 10);
    // Deadline in the past
    let deadline = Instant::now()
        .checked_sub(Duration::from_millis(100))
        .unwrap_or_else(Instant::now);

    let err = provider.search("test", None, deadline).await.unwrap_err();
    assert!(matches!(err, ProviderError::Timeout(_)));
}
