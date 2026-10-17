//! Integration tests for split-pane preview inspector, metadata table, and geometry transitions.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::similar_names,
    clippy::duration_suboptimal_units
)]

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

use image::{ImageBuffer, Rgb};
use shared::thumbnail::{ThumbnailSize, get_or_generate_thumbnail};
use spotlight::preview::loader::{
    MAX_CODE_LINES, MAX_LINE_LENGTH, TRUNCATION_NOTICE, format_breadcrumbs, format_human_bytes,
    format_kind_display, format_timestamp, highlight_code_lines, inspect_metadata,
    read_capped_lines,
};
use spotlight::ui::{
    EXPANDED_WIDTH, INITIAL_WIDTH, OverlayState, SPLIT_LIST_WIDTH, SpotlightWindow,
    WIDTH_ANIMATION_DURATION_MS, ease_out_cubic, interpolate_width,
};

#[test]
fn test_metadata_formatting_human_bytes() {
    assert_eq!(format_human_bytes(0), "0 B");
    assert_eq!(format_human_bytes(512), "512 B");
    assert_eq!(format_human_bytes(1023), "1023 B");

    // Kilobytes
    let one_kb = 1024;
    assert!(format_human_bytes(one_kb).contains("KB"));
    let twelve_kb = 12 * 1024;
    assert_eq!(format_human_bytes(twelve_kb), "12.0 KB");

    // Megabytes (2.4 MB = 2.4 * 1024 * 1024 = 2,516,582 bytes)
    let two_point_four_mb = 2_516_582;
    assert_eq!(format_human_bytes(two_point_four_mb), "2.4 MB");

    // Gigabytes
    let two_gb = 2 * 1024 * 1024 * 1024;
    assert_eq!(format_human_bytes(two_gb), "2.0 GB");

    // Terabytes
    let one_tb = 1024 * 1024 * 1024 * 1024;
    assert_eq!(format_human_bytes(one_tb), "1.0 TB");
}

#[test]
fn test_metadata_formatting_timestamps_and_breadcrumbs() {
    // Timestamp formatting
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_504_000); // Oct 2026
    let formatted = format_timestamp(time);
    assert!(
        formatted.contains("2026"),
        "expected year 2026 in timestamp, got: {formatted}"
    );

    // Breadcrumb formatting
    let sample_path = Path::new("/var/log/findx/tests/report.pdf");
    let breadcrumbs = format_breadcrumbs(sample_path);
    assert_eq!(breadcrumbs, "findx / tests");

    let root_file = Path::new("/file.txt");
    assert_eq!(format_breadcrumbs(root_file), "/");

    // Kind formatting
    assert_eq!(
        format_kind_display("application/pdf", Path::new("invoice.pdf")),
        "PDF Document"
    );
    assert_eq!(
        format_kind_display("image/png", Path::new("screenshot.png")),
        "PNG Image"
    );
    assert_eq!(
        format_kind_display("image/jpeg", Path::new("photo.jpg")),
        "JPEG Image"
    );
    assert_eq!(
        format_kind_display("text/rust", Path::new("main.rs")),
        "Rust Source Code"
    );
    assert_eq!(
        format_kind_display("text/x-python", Path::new("script.py")),
        "Python Script"
    );
    assert_eq!(
        format_kind_display("inode/directory", Path::new("/home/user/Documents")),
        "Folder"
    );
}

#[test]
fn test_thumbnail_cache_loading_via_shared_thumbnail() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let image_path = temp_dir.path().join("test_sample.png");

    // Create a 128x128 solid red PNG image
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(128, 128, Rgb([255, 0, 0]));
    img.save(&image_path).expect("save test png");

    // Query or generate thumbnail
    let thumb_res = get_or_generate_thumbnail(&image_path, ThumbnailSize::Large, Some("image/png"));
    assert!(
        thumb_res.is_ok(),
        "thumbnail generation failed: {:?}",
        thumb_res.err()
    );

    let thumb_path = thumb_res.expect("thumb path");
    assert!(thumb_path.is_file(), "thumbnail file must exist on disk");

    // Synchronous metadata inspection
    let meta_res = inspect_metadata(&image_path, Some("image/png"));
    assert!(meta_res.is_ok(), "metadata inspection failed");

    let meta = meta_res.expect("metadata");
    assert_eq!(meta.name, "test_sample.png");
    assert_eq!(meta.kind, "PNG Image");
    assert!(meta.size_bytes > 0);
}

#[test]
fn test_text_truncation_resource_limits() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let small_file = temp_dir.path().join("small.rs");

    // 1. Small file with 20 lines
    {
        let mut file = File::create(&small_file).expect("create small.rs");
        for i in 1..=20 {
            writeln!(file, "fn line_{i}() {{ println!(\"hello\"); }}").expect("write line");
        }
    }

    let (lines, is_large) = read_capped_lines(&small_file).expect("read small");
    assert_eq!(lines.len(), 20);
    assert!(!is_large);

    let (markup, is_truncated, total) = highlight_code_lines(&lines, "rs", false, false);
    assert_eq!(total, 20);
    assert!(!is_truncated);
    assert!(!markup.contains(TRUNCATION_NOTICE));
    assert!(markup.contains("<span")); // Syntax highlighting active

    // 2. Large file exceeding MAX_CODE_LINES (600 lines)
    let large_file = temp_dir.path().join("large.rs");
    {
        let mut file = File::create(&large_file).expect("create large.rs");
        for i in 1..=600 {
            writeln!(file, "let var_{i} = {i};").expect("write line");
        }
    }

    let (capped_lines, is_large) = read_capped_lines(&large_file).expect("read large");
    // BufReader take(501) reads at most 501 lines to detect truncation without unbounded reading
    assert_eq!(capped_lines.len(), MAX_CODE_LINES + 1);
    assert!(!is_large);

    let (markup, is_truncated, total) = highlight_code_lines(&capped_lines, "rs", true, false);
    assert_eq!(total, MAX_CODE_LINES + 1);
    assert!(is_truncated);
    assert!(markup.contains(TRUNCATION_NOTICE));

    // 3. Oversized line (> 1,000 characters) bypasses syntax highlighting
    let long_line = "x".repeat(MAX_LINE_LENGTH + 50);
    let lines_with_long = vec![long_line, "let a = 1;".to_string()];
    let (plain_markup, is_truncated, total) =
        highlight_code_lines(&lines_with_long, "rs", false, false);
    assert_eq!(total, 2);
    assert!(!is_truncated);
    // Bypassed highlighting must not have color spans
    assert!(!plain_markup.contains("<span foreground="));
}

#[test]
fn test_split_pane_geometry_state_transitions() {
    assert_eq!(INITIAL_WIDTH, 680);
    assert_eq!(EXPANDED_WIDTH, 960);
    assert_eq!(SPLIT_LIST_WIDTH, 312);
    assert_eq!(WIDTH_ANIMATION_DURATION_MS, 140);
    assert!((ease_out_cubic(0.5) - 0.875).abs() < 1e-6);

    // Eased curve interpolation
    assert_eq!(interpolate_width(680, 960, 0.0), 680);
    assert_eq!(interpolate_width(680, 960, 1.0), 960);
    let mid = interpolate_width(680, 960, 0.5);
    // Ease-out cubic: progress 0.5 gives 1 - (1 - 0.5)^3 = 0.875, so mid is > 900
    assert!(
        mid > 900,
        "ease-out cubic at 50% must cover majority of distance, got {mid}"
    );

    // Reverse interpolation (collapsing from 960 to 680)
    assert_eq!(interpolate_width(960, 680, 0.0), 960);
    assert_eq!(interpolate_width(960, 680, 1.0), 680);
    let mid_reverse = interpolate_width(960, 680, 0.5);
    assert!(
        mid_reverse < 750,
        "ease-out cubic collapse at 50% must rapidly shrink, got {mid_reverse}"
    );

    // State transitions
    let results_state = OverlayState::ResultsList;
    assert!(results_state.can_transition_to(OverlayState::SplitPreview));
    assert!(results_state.can_transition_to(OverlayState::Empty));
    assert!(results_state.can_transition_to(OverlayState::Dormant));

    let split_state = OverlayState::SplitPreview;
    assert!(split_state.is_visible());
    assert!(split_state.can_transition_to(OverlayState::ResultsList));
    assert!(split_state.can_transition_to(OverlayState::Empty));
    assert!(split_state.can_transition_to(OverlayState::Dormant));

    // Live widget lifecycle when GTK display is present
    if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err()) {
        return;
    }
    let window = SpotlightWindow::new(None);
    assert_eq!(window.current_width(), INITIAL_WIDTH);

    // Transition through Empty and ResultsList
    assert!(window.set_state(OverlayState::Empty));
    assert!(window.set_state(OverlayState::ResultsList));

    // Expand to split preview
    window.animate_to_split_preview();
    assert_eq!(window.state(), OverlayState::SplitPreview);

    // Collapse to single column results
    window.animate_to_results_width();
    assert_eq!(window.state(), OverlayState::ResultsList);

    // Toggle Quick Look
    window.toggle_quick_look();
    assert_eq!(window.state(), OverlayState::SplitPreview);

    window.toggle_quick_look();
    assert_eq!(window.state(), OverlayState::ResultsList);

    // Dismissal resets geometry
    window.dismiss();
    assert_eq!(window.state(), OverlayState::Dormant);
}
