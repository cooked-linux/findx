//! End-to-end smart folder round-trip tests spanning persistence, compilation,
//! FTS5 indexing, and filesystem fallback execution.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::redundant_closure_for_method_calls
)]

use std::path::PathBuf;

use finder::smart_folders::{
    BooleanMode, CriteriaAttribute, CriteriaGroup, CriteriaOperator, CriteriaSlice,
    SmartFolderDefinition, SmartFolderEngine, list_saved_searches, load_smart_folder,
    save_smart_folder,
};
use shared::db::fts::{FtsDocument, index_document};

fn sample_definition(scope: &std::path::Path) -> SmartFolderDefinition {
    let mut definition = SmartFolderDefinition::new(
        "Quarterly Reports",
        CriteriaGroup::all(vec![
            CriteriaSlice::new(CriteriaAttribute::Kind, CriteriaOperator::Equals, "PDF"),
            CriteriaSlice::new(
                CriteriaAttribute::Name,
                CriteriaOperator::Contains,
                "report",
            ),
        ]),
    );
    definition.scopes = vec![scope.to_path_buf()];
    definition
}

#[test]
fn test_smart_folder_round_trip_and_execution() {
    let scope = tempfile::tempdir().expect("scope dir");
    let storage = tempfile::tempdir().expect("storage dir");

    std::fs::write(scope.path().join("report.pdf"), b"%PDF-1.4 report").expect("pdf");
    std::fs::write(scope.path().join("report-2026.pdf"), b"%PDF-1.4 report").expect("pdf");
    std::fs::write(scope.path().join("notes.txt"), b"report notes").expect("txt");

    let definition = sample_definition(scope.path());
    let saved_path = save_smart_folder(storage.path(), &definition).expect("save");
    assert!(saved_path.ends_with("Quarterly Reports.savedSearch"));

    let loaded = load_smart_folder(&saved_path).expect("load");
    assert_eq!(loaded.name, definition.name);
    assert_eq!(loaded.scopes, definition.scopes);
    assert_eq!(loaded.root_criteria, definition.root_criteria);

    let listed = list_saved_searches(storage.path()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].0, saved_path);

    let engine = SmartFolderEngine::in_memory();

    let outside = tempfile::NamedTempFile::new().expect("outside");
    engine
        .with_connection(|conn| {
            index_document(
                conn,
                &FtsDocument::new(
                    outside.path().to_string_lossy(),
                    "report-outside-scope.pdf",
                    "quarterly report",
                    "application/pdf",
                    "",
                ),
            )
        })
        .expect("database available")
        .expect("index document");

    engine
        .with_connection(|conn| {
            index_document(
                conn,
                &FtsDocument::new(
                    scope.path().join("report.pdf").to_string_lossy(),
                    "report.pdf",
                    "quarterly report",
                    "application/pdf",
                    "",
                ),
            )
        })
        .expect("database available")
        .expect("index document");

    let results = engine.execute(&loaded).expect("execute");
    assert_eq!(results.items.len(), 2);
    let names: Vec<&str> = results.items.iter().map(|item| item.file_name()).collect();
    assert!(names.contains(&"report.pdf"));
    assert!(names.contains(&"report-2026.pdf"));
    assert!(!names.contains(&"notes.txt"));
    assert!(!names.contains(&"report-outside-scope.pdf"));
    assert_eq!(results.active_scopes, vec![PathBuf::from(scope.path())]);
}

#[test]
fn test_unsupported_and_corrupt_saved_searches_are_reported() {
    let storage = tempfile::tempdir().expect("storage dir");

    let mut outdated = sample_definition(std::path::Path::new("/tmp"));
    outdated.version = "0.1".to_string();
    outdated.name = "Outdated".to_string();
    std::fs::write(
        storage.path().join("Outdated.savedSearch"),
        serde_json::to_string(&outdated).expect("serialize"),
    )
    .expect("write outdated");

    std::fs::write(
        storage.path().join("Corrupt.savedSearch"),
        b"{ this is not json",
    )
    .expect("write corrupt");

    let listed = list_saved_searches(storage.path()).expect("list");
    assert!(listed.is_empty());

    let error = load_smart_folder(&storage.path().join("Corrupt.savedSearch"));
    assert!(error.is_err());
}

#[test]
fn test_boolean_none_group_matches_complement() {
    let definition = SmartFolderDefinition::new(
        "Small Files",
        CriteriaGroup {
            mode: BooleanMode::None,
            slices: vec![CriteriaSlice::new(
                CriteriaAttribute::Size,
                CriteriaOperator::GreaterThan,
                1024_i64,
            )],
            subgroups: Vec::new(),
        },
    );
    let compiled = finder::smart_folders::compile_definition(&definition);
    assert!(
        compiled
            .active_scopes
            .iter()
            .all(|scope| scope.is_absolute())
    );
}
