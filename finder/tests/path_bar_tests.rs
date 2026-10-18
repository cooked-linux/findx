#![allow(clippy::unwrap_used)]
//! Integration and unit tests for Interactive Breadcrumbs Path Bar.

use std::path::{Path, PathBuf};

use finder::breadcrumbs::{
    BreadcrumbDisplayItem, BreadcrumbLineage, FinderPreferences, PathSegment, layout_segments,
    parse_uri_list, percent_decode, resolve_file_icon, resolve_folder_icon,
};
use finder::ui::window::{WindowKeyAction, map_key_to_window_action};

#[test]
fn test_ancestry_segment_decomposition_root() {
    let lineage = BreadcrumbLineage::from_path(Path::new("/"));
    assert_eq!(lineage.segments.len(), 1);
    assert_eq!(lineage.active_index, 0);

    let root = &lineage.segments[0];
    assert_eq!(root.name, "Computer");
    assert_eq!(root.path, PathBuf::from("/"));
    assert!(root.is_root);
    assert!(root.is_directory);
    assert!(!root.is_leaf);
    assert_eq!(root.icon_name, "drive-harddisk-symbolic");
}

#[test]
fn test_ancestry_segment_decomposition_directory() {
    let lineage = BreadcrumbLineage::from_path(Path::new("/home/user"));
    assert_eq!(lineage.segments.len(), 3);
    assert_eq!(lineage.active_index, 2);

    assert_eq!(lineage.segments[0].name, "Computer");
    assert_eq!(lineage.segments[0].path, PathBuf::from("/"));
    assert!(lineage.segments[0].is_root);
    assert!(lineage.segments[0].is_directory);
    assert!(!lineage.segments[0].is_leaf);

    assert_eq!(lineage.segments[1].name, "home");
    assert_eq!(lineage.segments[1].path, PathBuf::from("/home"));
    assert!(!lineage.segments[1].is_root);
    assert!(lineage.segments[1].is_directory);
    assert!(!lineage.segments[1].is_leaf);

    assert_eq!(lineage.segments[2].name, "user");
    assert_eq!(lineage.segments[2].path, PathBuf::from("/home/user"));
    assert!(!lineage.segments[2].is_root);
    assert!(lineage.segments[2].is_directory);
    assert!(!lineage.segments[2].is_leaf);
}

#[test]
fn test_ancestry_segment_decomposition_file_leaf() {
    let lineage = BreadcrumbLineage::from_path(Path::new("/home/user/workspace/src/main.rs"));
    assert_eq!(lineage.segments.len(), 6);
    assert_eq!(lineage.active_index, 5);

    assert_eq!(lineage.segments[0].name, "Computer");
    assert_eq!(lineage.segments[0].path, PathBuf::from("/"));

    assert_eq!(lineage.segments[1].name, "home");
    assert_eq!(lineage.segments[1].path, PathBuf::from("/home"));

    assert_eq!(lineage.segments[2].name, "user");
    assert_eq!(lineage.segments[2].path, PathBuf::from("/home/user"));

    assert_eq!(lineage.segments[3].name, "workspace");
    assert_eq!(
        lineage.segments[3].path,
        PathBuf::from("/home/user/workspace")
    );

    assert_eq!(lineage.segments[4].name, "src");
    assert_eq!(
        lineage.segments[4].path,
        PathBuf::from("/home/user/workspace/src")
    );
    assert!(lineage.segments[4].is_directory);
    assert!(!lineage.segments[4].is_leaf);

    let leaf = &lineage.segments[5];
    assert_eq!(leaf.name, "main.rs");
    assert_eq!(leaf.path, PathBuf::from("/home/user/workspace/src/main.rs"));
    assert!(!leaf.is_root);
    assert!(!leaf.is_directory);
    assert!(leaf.is_leaf);
    assert_eq!(leaf.icon_name, "text-x-generic-symbolic");
}

#[test]
fn test_ancestry_segment_decomposition_explicit_selection() {
    let lineage = BreadcrumbLineage::resolve(
        Path::new("/home/user/documents"),
        Some(Path::new("/home/user/documents/report.pdf")),
    );
    assert_eq!(lineage.segments.len(), 5);
    assert_eq!(lineage.active_index, 4);

    let doc_dir = &lineage.segments[3];
    assert_eq!(doc_dir.name, "documents");
    assert!(doc_dir.is_directory);
    assert!(!doc_dir.is_leaf);

    let leaf = &lineage.segments[4];
    assert_eq!(leaf.name, "report.pdf");
    assert_eq!(leaf.path, PathBuf::from("/home/user/documents/report.pdf"));
    assert!(leaf.is_leaf);
    assert!(!leaf.is_directory);
    assert_eq!(leaf.icon_name, "application-pdf-symbolic");
}

#[test]
fn test_overflow_collapse_calculation_narrow() {
    let segments = vec![
        PathSegment::new(
            "Computer",
            "/",
            "drive-harddisk-symbolic",
            true,
            true,
            false,
        ),
        PathSegment::new("home", "/home", "folder-symbolic", true, false, false),
        PathSegment::new("user", "/home/user", "folder-symbolic", true, false, false),
        PathSegment::new(
            "projects",
            "/home/user/projects",
            "folder-symbolic",
            true,
            false,
            false,
        ),
        PathSegment::new(
            "findx",
            "/home/user/projects/findx",
            "folder-symbolic",
            true,
            false,
            false,
        ),
        PathSegment::new(
            "src",
            "/home/user/projects/findx/src",
            "folder-symbolic",
            true,
            false,
            false,
        ),
        PathSegment::new(
            "main.rs",
            "/home/user/projects/findx/src/main.rs",
            "text-x-generic-symbolic",
            false,
            false,
            true,
        ),
    ];

    // Narrow width forces collapse
    let items = layout_segments(&segments, 180, 60);
    assert_eq!(items.len(), 3);

    // Left root anchor preserved
    assert_eq!(
        items[0].segment().map(|s| s.name.as_str()),
        Some("Computer")
    );
    assert_eq!(
        items[0].segment().map(|s| &s.path),
        Some(&PathBuf::from("/"))
    );

    // Intermediate collapsed into overflow ellipsis
    assert!(items[1].is_overflow());
    let hidden = items[1].hidden_segments().unwrap();
    assert_eq!(hidden.len(), 5);
    assert_eq!(hidden[0].name, "home");
    assert_eq!(hidden[1].name, "user");
    assert_eq!(hidden[2].name, "projects");
    assert_eq!(hidden[3].name, "findx");
    assert_eq!(hidden[4].name, "src");

    // Right leaf anchor preserved
    assert_eq!(items[2].segment().map(|s| s.name.as_str()), Some("main.rs"));
    assert_eq!(
        items[2].segment().map(|s| &s.path),
        Some(&PathBuf::from("/home/user/projects/findx/src/main.rs"))
    );
}

#[test]
fn test_overflow_collapse_calculation_wide() {
    let segments = vec![
        PathSegment::new(
            "Computer",
            "/",
            "drive-harddisk-symbolic",
            true,
            true,
            false,
        ),
        PathSegment::new("home", "/home", "folder-symbolic", true, false, false),
        PathSegment::new("user", "/home/user", "folder-symbolic", true, false, false),
    ];

    // Wide container fits all
    let items = layout_segments(&segments, 1200, 60);
    assert_eq!(items.len(), 3);
    assert!(!items.iter().any(BreadcrumbDisplayItem::is_overflow));
    assert_eq!(
        items[0].segment().map(|s| s.name.as_str()),
        Some("Computer")
    );
    assert_eq!(items[1].segment().map(|s| s.name.as_str()), Some("home"));
    assert_eq!(items[2].segment().map(|s| s.name.as_str()), Some("user"));
}

#[test]
fn test_overflow_collapse_two_or_fewer_segments_never_collapses() {
    let two_segments = vec![
        PathSegment::new(
            "Computer",
            "/",
            "drive-harddisk-symbolic",
            true,
            true,
            false,
        ),
        PathSegment::new("home", "/home", "folder-symbolic", true, false, false),
    ];

    let items = layout_segments(&two_segments, 40, 60);
    assert_eq!(items.len(), 2);
    assert!(!items.iter().any(BreadcrumbDisplayItem::is_overflow));
}

#[test]
fn test_uri_drop_string_parsing() {
    let uri_payload = "\
# Dropped URIs from desktop environment
file:///home/user/Documents/notes.txt
file:///home/user/Pictures/screenshot%202026.png
file:///home/user/Dev/caf%C3%A9.rs

file://localhost/var/log/syslog
";

    let paths = parse_uri_list(uri_payload);
    assert_eq!(paths.len(), 4);
    assert_eq!(paths[0], PathBuf::from("/home/user/Documents/notes.txt"));
    assert_eq!(
        paths[1],
        PathBuf::from("/home/user/Pictures/screenshot 2026.png")
    );
    assert_eq!(paths[2], PathBuf::from("/home/user/Dev/café.rs"));
    assert_eq!(paths[3], PathBuf::from("/var/log/syslog"));
}

#[test]
fn test_percent_decode_helper() {
    assert_eq!(percent_decode("simple"), "simple");
    assert_eq!(percent_decode("Hello%20World"), "Hello World");
    assert_eq!(percent_decode("path%2Fto%2Ffile"), "path/to/file");
    assert_eq!(percent_decode("na%C3%AFve"), "naïve");
}

#[test]
fn test_preferences_persistence_roundtrip() {
    let prefs = FinderPreferences {
        pathbar_visible: false,
        calculate_all_sizes: true,
        show_transfer_dialog: false,
    };
    let json = serde_json::to_string(&prefs).unwrap();
    let deserialized: FinderPreferences = serde_json::from_str(&json).unwrap();
    assert_eq!(prefs, deserialized);
    assert!(!deserialized.pathbar_visible);
    assert!(deserialized.calculate_all_sizes);
    assert!(!deserialized.show_transfer_dialog);

    let default_prefs = FinderPreferences::default();
    assert!(default_prefs.pathbar_visible);
    assert!(!default_prefs.calculate_all_sizes);
    assert!(default_prefs.show_transfer_dialog);
}

#[test]
fn test_map_key_to_window_action_toggle_path_bar() {
    // Ctrl + Alt + P
    assert_eq!(
        map_key_to_window_action(
            gdk4::Key::p,
            gdk4::ModifierType::CONTROL_MASK | gdk4::ModifierType::ALT_MASK
        ),
        Some(WindowKeyAction::TogglePathBar)
    );
    assert_eq!(
        map_key_to_window_action(
            gdk4::Key::P,
            gdk4::ModifierType::CONTROL_MASK | gdk4::ModifierType::ALT_MASK
        ),
        Some(WindowKeyAction::TogglePathBar)
    );

    // Super (Cmd) + Alt + P
    assert_eq!(
        map_key_to_window_action(
            gdk4::Key::p,
            gdk4::ModifierType::SUPER_MASK | gdk4::ModifierType::ALT_MASK
        ),
        Some(WindowKeyAction::TogglePathBar)
    );
    assert_eq!(
        map_key_to_window_action(
            gdk4::Key::P,
            gdk4::ModifierType::SUPER_MASK | gdk4::ModifierType::ALT_MASK
        ),
        Some(WindowKeyAction::TogglePathBar)
    );
}

#[test]
fn test_folder_and_file_icon_resolution() {
    assert_eq!(
        resolve_folder_icon(Path::new("/home/user/Music"), "Music"),
        "folder-music-symbolic"
    );
    assert_eq!(
        resolve_folder_icon(Path::new("/home/user/Pictures"), "Pictures"),
        "folder-pictures-symbolic"
    );
    assert_eq!(
        resolve_folder_icon(Path::new("/home/user/Videos"), "Videos"),
        "folder-videos-symbolic"
    );

    assert_eq!(
        resolve_file_icon(Path::new("/path/to/archive.zip")),
        "package-x-generic-symbolic"
    );
    assert_eq!(
        resolve_file_icon(Path::new("/path/to/photo.jpg")),
        "image-x-generic-symbolic"
    );
    assert_eq!(
        resolve_file_icon(Path::new("/path/to/song.mp3")),
        "audio-x-generic-symbolic"
    );
    assert_eq!(
        resolve_file_icon(Path::new("/path/to/video.mp4")),
        "video-x-generic-symbolic"
    );
    assert_eq!(
        resolve_file_icon(Path::new("/path/to/document.pdf")),
        "application-pdf-symbolic"
    );
}
