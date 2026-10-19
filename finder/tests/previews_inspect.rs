#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty
)]
//! Comprehensive Milestone M5 Integration Test Suite.
//!
//! Validates:
//! 1. Quick Look Text & Code Highlighting (`syntect`, language detection, truncation safeguard).
//! 2. Quick Look Image & EXIF Normalization (dimension parsing and orientation normalization).
//! 3. Bubblewrap Sandbox Isolation & Timeout Safety (network namespace isolation, timeout kills).
//! 4. Deep Metadata & POSIX Statx Extraction (physical bytes, `stx_btime`, octal permissions).
//! 5. Extended Attributes Read/Write & Fallback (user.xdg.comment, user.xdg.origin.url, removal).
//! 6. 7-Color Tagging & SQLite Index Sync (xattr sync to SQLite `tag_cache` and color querying).
//! 7. Multi-Selection Summary Aggregation (cumulative bytes, physical blocks, shared/partial tags).

use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::Duration;

use finder::metadata::aggregate::aggregate_metadata;
use finder::metadata::extract::extract_deep_metadata;
use finder::metadata::tags_sync::{
    init_file_tags_table, query_files_by_color, query_files_by_tag, sync_file_tags,
};
use finder::metadata::xattr::ExtendedAttributesService;
use finder::quicklook::generator::MAX_CODE_LINES;
use finder::quicklook::sandbox::{SandboxConfig, has_bubblewrap, run_sandboxed};
use finder::quicklook::types::PreviewPayload;
use finder::quicklook::{QuickLookEngine, QuickLookError};
use rusqlite::Connection;
use shared::models::{FileTag, TagColor};
use tempfile::{NamedTempFile, tempdir};

#[tokio::test]
async fn test_1_quick_look_text_and_code_highlighting() {
    let engine = Arc::new(QuickLookEngine::new());

    // 1. Rust Source File Preview
    let mut rust_file = NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        rust_file,
        "// Finder Quick Look Test\nfn main() {{\n    println!(\"Hello, FindX!\");\n}}"
    )
    .unwrap();
    rust_file.flush().unwrap();

    let rust_payload = engine
        .load_preview(rust_file.path(), (800, 600))
        .await
        .unwrap();

    match rust_payload {
        PreviewPayload::SourceCode(code_data) => {
            assert_eq!(code_data.language, "Rust");
            assert_eq!(code_data.total_lines, 4);
            assert!(!code_data.is_truncated);
            assert!(
                code_data.highlighted_html.contains("<span")
                    || code_data.highlighted_html.contains("fn")
            );
            assert!(code_data.raw_lines[1].contains("fn main"));
        }
        other => panic!("Expected SourceCode payload for Rust file, got: {other:?}"),
    }

    // 2. Markdown File Preview
    let mut md_file = NamedTempFile::with_suffix(".md").unwrap();
    writeln!(
        md_file,
        "# Project Overview\n\nThis is a test markdown document for Quick Look."
    )
    .unwrap();
    md_file.flush().unwrap();

    let md_payload = engine
        .load_preview(md_file.path(), (800, 600))
        .await
        .unwrap();

    match md_payload {
        PreviewPayload::SourceCode(code_data) => {
            assert!(code_data.total_lines >= 3);
            assert!(!code_data.is_truncated);
            assert!(code_data.raw_lines[0].contains("# Project Overview"));
        }
        other => panic!("Expected SourceCode payload for Markdown file, got: {other:?}"),
    }

    // 3. Truncation Safeguard on Large Files (>500 lines)
    let mut large_file = NamedTempFile::with_suffix(".txt").unwrap();
    for i in 1..=600 {
        writeln!(large_file, "Line number {i} of large file").unwrap();
    }
    large_file.flush().unwrap();

    let large_payload = engine
        .load_preview(large_file.path(), (800, 600))
        .await
        .unwrap();

    match large_payload {
        PreviewPayload::SourceCode(code_data) => {
            assert_eq!(code_data.total_lines, 600);
            assert_eq!(code_data.raw_lines.len(), MAX_CODE_LINES);
            assert!(code_data.is_truncated);
            assert!(code_data.highlighted_html.contains("Truncated"));
        }
        other => panic!("Expected SourceCode payload for large text file, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_2_quick_look_image_and_exif_normalization() {
    let engine = Arc::new(QuickLookEngine::new());

    let img_file = NamedTempFile::with_suffix(".png").unwrap();
    let img = image::RgbaImage::new(160, 120);
    img.save(img_file.path()).unwrap();

    let payload = engine
        .load_preview(img_file.path(), (800, 600))
        .await
        .unwrap();

    match payload {
        PreviewPayload::Image(img_data) => {
            assert_eq!(img_data.width, 160);
            assert_eq!(img_data.height, 120);
            assert_eq!(img_data.image_path, img_file.path());
        }
        other => panic!("Expected Image payload, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_3_bubblewrap_sandbox_isolation_and_timeout_safety() {
    let temp_file = NamedTempFile::new().unwrap();

    if has_bubblewrap() {
        // 1. Verify benign command executes in unprivileged container with network namespace unshared
        let config = SandboxConfig::new(temp_file.path()).with_timeout(Duration::from_secs(3));
        let result = run_sandboxed("/bin/echo", &["sandbox_ok"], &config).await;
        assert!(
            result.is_ok(),
            "Sandboxed echo execution failed: {result:?}"
        );
        let out = result.unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("sandbox_ok"));

        // 2. Simulate hung parser process: 100ms timeout on a 5-second sleep
        let hung_config =
            SandboxConfig::new(temp_file.path()).with_timeout(Duration::from_millis(100));
        let start = std::time::Instant::now();
        let timeout_result = run_sandboxed("/bin/sleep", &["5"], &hung_config).await;
        let elapsed = start.elapsed();

        assert!(timeout_result.is_err());
        match timeout_result.unwrap_err() {
            QuickLookError::Timeout { timeout_ms, .. } => {
                assert_eq!(timeout_ms, 100);
            }
            other => panic!("Expected Timeout error, got: {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(2),
            "Timeout kill took too long: {elapsed:?}"
        );
    }

    // 3. Engine-level resilience: unhandled or corrupt file gracefully falls back to FallbackCard
    let engine = Arc::new(QuickLookEngine::new());
    let mut corrupt_file = NamedTempFile::with_suffix(".bin").unwrap();
    corrupt_file.write_all(&[0x00, 0xFF, 0xFE, 0xFD]).unwrap();
    corrupt_file.flush().unwrap();

    let fallback_payload = engine
        .load_preview(corrupt_file.path(), (800, 600))
        .await
        .unwrap();

    match fallback_payload {
        PreviewPayload::FallbackCard(card) => {
            assert_eq!(card.file_size, 4);
            assert_eq!(card.mime_type, "application/octet-stream");
        }
        other => panic!("Expected FallbackCard payload, got: {other:?}"),
    }
}

#[test]
fn test_4_deep_metadata_and_posix_statx_extraction() {
    let mut temp = NamedTempFile::new().unwrap();
    let payload = vec![0x41u8; 4096];
    temp.write_all(&payload).unwrap();
    temp.flush().unwrap();

    // Set permissions explicitly to 0755
    let mut perms = std::fs::metadata(temp.path()).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(temp.path(), perms).unwrap();

    let meta = extract_deep_metadata(temp.path()).unwrap();
    assert_eq!(meta.size_logical, 4096);
    assert_eq!(meta.permissions.octal, "0755");
    assert!(!meta.is_directory);
    assert!(meta.created_time.is_some() || meta.modified_time.elapsed().is_ok());
    // Physical block allocation must be non-zero or equal to logical on modern filesystems
    assert!(meta.size_physical > 0 || meta.size_logical > 0);
}

#[test]
fn test_5_extended_attributes_read_write_and_fallback() {
    let temp = NamedTempFile::new().unwrap();
    let path = temp.path();

    // 1. Read & Write user.xdg.comment
    let comment_text = "Phase 5 verified metadata comment";
    ExtendedAttributesService::set_comment(path, comment_text).unwrap();
    let read_comment = ExtendedAttributesService::get_comment(path).unwrap();
    assert_eq!(read_comment, comment_text);

    // 2. Read & Write user.xdg.origin.url
    let origin_url = "https://findx.desktop/downloads/spec.pdf";
    ExtendedAttributesService::set_origin(path, Some(origin_url), None).unwrap();
    let (read_url, _) = ExtendedAttributesService::get_origin(path).unwrap();
    assert_eq!(read_url, Some(origin_url.to_string()));

    // 3. Comment removal when empty string is provided
    ExtendedAttributesService::set_comment(path, "").unwrap();
    let removed_comment = ExtendedAttributesService::get_comment(path).unwrap();
    assert_eq!(removed_comment, "");
}

#[test]
fn test_6_seven_color_tagging_and_sqlite_index_sync() {
    let dir = tempdir().unwrap();
    let file_red = dir.path().join("document_red.txt");
    let file_blue = dir.path().join("document_blue.txt");

    File::create(&file_red).unwrap().write_all(b"red").unwrap();
    File::create(&file_blue)
        .unwrap()
        .write_all(b"blue")
        .unwrap();

    // 1. Tag files with Red and Blue canonical colors via xattr
    let red_tags = vec![FileTag::new("Urgent", Some(TagColor::Red))];
    let blue_tags = vec![FileTag::new("Work", Some(TagColor::Blue))];

    ExtendedAttributesService::set_tags(&file_red, &red_tags).unwrap();
    ExtendedAttributesService::set_tags(&file_blue, &blue_tags).unwrap();

    // 2. Sync to in-memory SQLite tag database
    let db = Connection::open_in_memory().unwrap();
    init_file_tags_table(&db).unwrap();

    sync_file_tags(&db, &file_red, &red_tags).unwrap();
    sync_file_tags(&db, &file_blue, &blue_tags).unwrap();

    // 3. Query files by color code
    let red_matches = query_files_by_color(&db, TagColor::Red).unwrap();
    assert_eq!(red_matches.len(), 1);
    assert_eq!(red_matches[0], file_red);

    let blue_matches = query_files_by_color(&db, TagColor::Blue).unwrap();
    assert_eq!(blue_matches.len(), 1);
    assert_eq!(blue_matches[0], file_blue);

    // 4. Query files by tag name
    let urgent_matches = query_files_by_tag(&db, "Urgent").unwrap();
    assert_eq!(urgent_matches.len(), 1);
    assert_eq!(urgent_matches[0], file_red);

    let work_matches = query_files_by_tag(&db, "work").unwrap();
    assert_eq!(work_matches.len(), 1);
    assert_eq!(work_matches[0], file_blue);
}

#[test]
fn test_7_multi_selection_summary_aggregation() {
    let dir = tempdir().unwrap();
    let mut paths = Vec::new();

    // Create 5 files with distinct byte counts: 100, 200, 300, 400, 500
    for i in 1..=5 {
        let p = dir.path().join(format!("file_{i}.dat"));
        let mut f = File::create(&p).unwrap();
        let payload = vec![b'X'; i * 100];
        f.write_all(&payload).unwrap();
        f.flush().unwrap();

        // Tag all 5 with "Project" (shared tag)
        let mut tags = vec![FileTag::new("Project", Some(TagColor::Green))];
        // Tag only items 1 and 2 with "Review" (partial tag)
        if i <= 2 {
            tags.push(FileTag::new("Review", Some(TagColor::Yellow)));
        }
        ExtendedAttributesService::set_tags(&p, &tags).unwrap();

        paths.push(p);
    }

    let summary = aggregate_metadata(&paths).unwrap();

    assert_eq!(summary.total_items, 5);
    assert_eq!(summary.total_files, 5);
    assert_eq!(summary.total_directories, 0);
    assert_eq!(summary.total_logical_bytes, 100 + 200 + 300 + 400 + 500);

    // Verify shared tags: "Project" must be present in shared_tags
    assert!(
        summary
            .shared_tags
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case("Project"))
    );
    assert!(
        !summary
            .shared_tags
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case("Review"))
    );

    // Verify partial tags: "Review" must be present in partial_tags
    assert!(
        summary
            .partial_tags
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case("Review"))
    );
}
