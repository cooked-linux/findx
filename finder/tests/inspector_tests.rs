#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::assert_is_empty
)]

use std::io::Write;
use tempfile::{NamedTempFile, tempdir};

use gtk4::prelude::*;

use finder::metadata::aggregate::aggregate_metadata;
use finder::metadata::xattr::ExtendedAttributesService;
use finder::ui::inspector::general_page::{
    format_dual_size, format_inspector_time, rename_target, set_file_locked,
};
use finder::ui::inspector::icon_custom::{
    FOLDER_CUSTOM_ICON_NAME, FOLDER_DIRECTORY_ENTRY, remove_custom_folder_icon,
    resolve_custom_folder_icon,
};
use finder::ui::inspector::more_info_page::format_duration;
use finder::ui::inspector::permissions_page::{bits_to_privilege, privilege_to_bits};
use finder::ui::inspector::{GetInfoDialog, InspectorMode};

#[test]
fn test_inspector_icon_folder_custom_icon_lifecycle() {
    let dir = tempdir().unwrap();
    let folder_path = dir.path().to_path_buf();

    // Initially no custom icon
    assert!(resolve_custom_folder_icon(&folder_path).is_none());

    // Write custom icon file and .directory entry
    let icon_path = folder_path.join(FOLDER_CUSTOM_ICON_NAME);
    std::fs::write(&icon_path, b"\x89PNG\r\n\x1a\nfake png data").unwrap();
    let dir_entry = folder_path.join(FOLDER_DIRECTORY_ENTRY);
    std::fs::write(&dir_entry, b"[Desktop Entry]\nIcon=./.icon.png\n").unwrap();

    // Verify detection
    assert_eq!(
        resolve_custom_folder_icon(&folder_path),
        Some(icon_path.clone())
    );

    // Remove custom icon
    assert!(remove_custom_folder_icon(&folder_path));
    assert!(!icon_path.exists());
    assert!(!dir_entry.exists());
    assert!(resolve_custom_folder_icon(&folder_path).is_none());
}

#[test]
fn test_inspector_icon_multi_selection_aggregation_metrics() {
    let dir = tempdir().unwrap();
    let mut paths = Vec::new();

    // Create 4 files with known sizes
    for i in 0..4 {
        let p = dir.path().join(format!("file_{i}.dat"));
        let mut f = std::fs::File::create(&p).unwrap();
        let payload = vec![b'A' + i as u8; 100 * (i + 1)];
        f.write_all(&payload).unwrap();
        f.flush().unwrap();
        paths.push(p);
    }

    // Create 2 directories
    for i in 0..2 {
        let sub = dir.path().join(format!("sub_{i}"));
        std::fs::create_dir(&sub).unwrap();
        paths.push(sub);
    }

    let summary = aggregate_metadata(&paths).unwrap();
    assert_eq!(summary.total_items, 6);
    assert_eq!(summary.total_files, 4);
    assert_eq!(summary.total_directories, 2);
    let expected_file_bytes: u64 = 100 + 200 + 300 + 400;
    assert!(summary.total_logical_bytes >= expected_file_bytes);

    // Files only aggregation has exact byte count
    let files_summary = aggregate_metadata(&paths[..4]).unwrap();
    assert_eq!(files_summary.total_items, 4);
    assert_eq!(files_summary.total_files, 4);
    assert_eq!(files_summary.total_directories, 0);
    assert_eq!(files_summary.total_logical_bytes, expected_file_bytes);
}

#[test]
fn test_inspector_general_format_dual_size() {
    let formatted = format_dual_size(14_188_544, 14_888_960);
    assert!(formatted.contains("on disk"));
    assert!(formatted.contains("14,188,544 bytes"));
}

#[test]
fn test_inspector_general_format_time() {
    let now = std::time::SystemTime::now();
    let text = format_inspector_time(Some(now));
    assert!(!text.is_empty());
    assert_ne!(text, "—");

    assert_eq!(format_inspector_time(None), "—");
}

#[test]
fn test_inspector_general_locked_toggle() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"locked file test").unwrap();
    file.flush().unwrap();
    let path = file.path().to_path_buf();

    // Set locked (removes write bits)
    set_file_locked(&path, true).unwrap();
    let meta = std::fs::symlink_metadata(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(meta.permissions().mode() & 0o222, 0);
    }

    // Set unlocked (restores write bit)
    set_file_locked(&path, false).unwrap();
    let meta2 = std::fs::symlink_metadata(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(meta2.permissions().mode() & 0o200, 0);
    }
}

#[test]
fn test_inspector_general_atomic_rename() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("old_name.txt");
    std::fs::write(&file_path, b"content").unwrap();

    let new_path = rename_target(&file_path, "new_name.txt").unwrap();
    assert_eq!(new_path, dir.path().join("new_name.txt"));
    assert!(!file_path.exists());
    assert!(new_path.exists());

    // Invalid rename with slash fails
    assert!(rename_target(&new_path, "invalid/name.txt").is_err());
    // Empty rename fails
    assert!(rename_target(&new_path, "   ").is_err());
}

#[test]
fn test_inspector_hide_extension_xattr() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(b"hide extension test").unwrap();
    file.flush().unwrap();
    let path = file.path();

    // Default false
    let initial = ExtendedAttributesService::get_hide_extension(path).unwrap();
    assert!(!initial);

    // Set true
    ExtendedAttributesService::set_hide_extension(path, true).unwrap();
    let hidden = ExtendedAttributesService::get_hide_extension(path).unwrap();
    assert!(hidden);

    // Set false
    ExtendedAttributesService::set_hide_extension(path, false).unwrap();
    let unhidden = ExtendedAttributesService::get_hide_extension(path).unwrap();
    assert!(!unhidden);
}

#[test]
fn test_inspector_more_info_format_duration() {
    assert_eq!(format_duration(0.0), "00:00");
    assert_eq!(format_duration(225.0), "03:45");
    assert_eq!(format_duration(3600.0 + 120.0 + 5.0), "01:02:05");
}

#[test]
fn test_inspector_permissions_privilege_conversions() {
    // 0: Read & Write -> 0o6 for file, 0o7 for dir
    assert_eq!(privilege_to_bits(0, false), 0o6);
    assert_eq!(privilege_to_bits(0, true), 0o7);

    // 1: Read only -> 0o4 for file, 0o5 for dir
    assert_eq!(privilege_to_bits(1, false), 0o4);
    assert_eq!(privilege_to_bits(1, true), 0o5);

    // 2: Write only -> 0o2 for file, 0o3 for dir
    assert_eq!(privilege_to_bits(2, false), 0o2);
    assert_eq!(privilege_to_bits(2, true), 0o3);

    // 3: No access -> 0
    assert_eq!(privilege_to_bits(3, false), 0);
    assert_eq!(privilege_to_bits(3, true), 0);

    // bits_to_privilege
    assert_eq!(bits_to_privilege(0o7), 0);
    assert_eq!(bits_to_privilege(0o6), 0);
    assert_eq!(bits_to_privilege(0o5), 1);
    assert_eq!(bits_to_privilege(0o4), 1);
    assert_eq!(bits_to_privilege(0o2), 2);
    assert_eq!(bits_to_privilege(0o0), 3);
}

#[test]
fn test_inspector_gtk_window_instantiation() {
    if gtk4::init().is_err() {
        return;
    }
    libadwaita::init().unwrap();

    let dir = tempdir().unwrap();
    let file_path = dir.path().join("sample.txt");
    std::fs::write(&file_path, b"GTK inspector test content").unwrap();

    let dialog =
        GetInfoDialog::open_for_paths(None as Option<&gtk4::Window>, vec![file_path.clone()]);
    assert_eq!(dialog.mode, InspectorMode::Modal);
    assert_eq!(dialog.window.title().as_deref(), Some("sample.txt Info"));

    // Multi-selection rebind
    let file_path2 = dir.path().join("sample2.txt");
    std::fs::write(&file_path2, b"second item").unwrap();
    dialog.rebind_target(&[file_path, file_path2]);
    assert_eq!(dialog.window.title().as_deref(), Some("2 Items Info"));
}
