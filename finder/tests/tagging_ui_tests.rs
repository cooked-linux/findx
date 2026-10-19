#![allow(
    clippy::unwrap_used,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::redundant_clone,
    clippy::cloned_ref_to_slice_refs,
    clippy::assert_is_empty,
    clippy::useless_vec
)]
//! Integration and UI tests for file tagging, color swatches, tag pill editor, and badges.

use std::sync::Mutex;
use std::time::SystemTime;

use finder::ui::tagging::{
    ColorSwatchBar, MultiSelectionSummary, SwatchState, TagBadgeView, TagPillEditor,
    apply_tag_to_paths, create_tags_context_menu_widget, is_tag_virtual_path,
    make_tag_virtual_path, parse_tag_virtual_path, query_tagged_files, remove_tag_from_paths,
    toggle_color_on_paths,
};
use gtk4::prelude::*;
use shared::models::{FileItem, FileMetadata, FileTag, TagColor};
use tempfile::NamedTempFile;

static GTK_TEST_MUTEX: Mutex<()> = Mutex::new(());

fn mock_file_item(path: std::path::PathBuf, name: &str, tags: Vec<FileTag>) -> FileItem {
    FileItem::new(
        path,
        name.to_string(),
        false,
        false,
        None,
        FileMetadata {
            size_bytes: 1024,
            modified: Some(SystemTime::now()),
            accessed: Some(SystemTime::now()),
            created: Some(SystemTime::now()),
            permissions: 0o644,
            inode: 1,
            device_id: 1,
            mime_type: "text/plain".to_string(),
            is_hidden: false,
            is_executable: false,
        },
        tags,
    )
}

#[test]
fn test_tagging_public_api_virtual_paths() {
    let p = make_tag_virtual_path("WorkDocs");
    assert!(is_tag_virtual_path(&p));
    assert_eq!(parse_tag_virtual_path(&p).as_deref(), Some("WorkDocs"));
}

#[test]
fn test_multi_selection_summary_and_batch_actions() {
    let f1 = NamedTempFile::new().unwrap();
    let f2 = NamedTempFile::new().unwrap();
    let f3 = NamedTempFile::new().unwrap();

    let p1 = f1.path().to_path_buf();
    let p2 = f2.path().to_path_buf();
    let p3 = f3.path().to_path_buf();

    let tag_shared = FileTag::new("SharedProject", Some(TagColor::Green));
    let tag_subset = FileTag::new("Urgent", Some(TagColor::Red));

    // Apply shared tag to all 3 files
    apply_tag_to_paths(&[p1.clone(), p2.clone(), p3.clone()], &tag_shared);
    // Apply subset tag to only file 1
    apply_tag_to_paths(&[p1.clone()], &tag_subset);

    let items = vec![
        mock_file_item(
            p1.clone(),
            "f1",
            vec![tag_shared.clone(), tag_subset.clone()],
        ),
        mock_file_item(p2.clone(), "f2", vec![tag_shared.clone()]),
        mock_file_item(p3.clone(), "f3", vec![tag_shared.clone()]),
    ];

    let tags_slice: Vec<Vec<FileTag>> = items.iter().map(|it| it.tags.clone()).collect();
    let summary = MultiSelectionSummary::compute(&tags_slice);
    assert!(
        summary
            .shared_tags
            .iter()
            .any(|t| t.name == "SharedProject")
    );
    assert!(summary.partial_tags.iter().any(|t| t.name == "Urgent"));
    assert!(summary.shared_colors.contains(&TagColor::Green));
    assert!(summary.partial_colors.contains(&TagColor::Red));
    assert!(!summary.shared_colors.contains(&TagColor::Blue));
    assert!(!summary.partial_colors.contains(&TagColor::Blue));

    // Toggle color on all 3 paths
    toggle_color_on_paths(&[p1.clone(), p2.clone(), p3.clone()], TagColor::Blue, true);

    // Remove shared tag from paths
    remove_tag_from_paths(&[p1, p2, p3], "SharedProject");
}

#[test]
fn test_tagging_ui_widgets_instantiation() {
    let _lock = GTK_TEST_MUTEX.lock().unwrap();
    if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err()) {
        return;
    }

    // 1. ColorSwatchBar
    let swatch_bar = ColorSwatchBar::new();
    swatch_bar.set_color_state(TagColor::Red, SwatchState::Active);
    swatch_bar.set_color_state(TagColor::Blue, SwatchState::Partial);
    assert_eq!(
        swatch_bar.get_color_state(TagColor::Red),
        SwatchState::Active
    );
    assert_eq!(
        swatch_bar.get_color_state(TagColor::Blue),
        SwatchState::Partial
    );

    // 2. TagPillEditor
    let pill_editor = TagPillEditor::new();
    pill_editor.set_tags(&[FileTag::new("Important", Some(TagColor::Yellow))], &[]);
    let pills = pill_editor.get_tags();
    assert_eq!(pills.len(), 1);
    assert_eq!(pills[0].name, "Important");

    // 3. TagBadgeView
    let badge_view = TagBadgeView::new_column_cluster(&[
        FileTag::new("Alpha", Some(TagColor::Red)),
        FileTag::new("Beta", Some(TagColor::Green)),
    ]);
    badge_view.update(&[FileTag::new("Gamma", Some(TagColor::Blue))]);

    // 4. Context Menu Widget
    let f1 = NamedTempFile::new().unwrap();
    let f2 = NamedTempFile::new().unwrap();
    let anchor = gtk4::Button::new();
    let ctx_widget = create_tags_context_menu_widget(
        &[f1.path().to_path_buf(), f2.path().to_path_buf()],
        &anchor,
    );
    assert!(ctx_widget.first_child().is_some());
}

#[test]
fn test_query_tagged_files_empty_safe() {
    let non_existent = query_tagged_files("NonExistentTagXYZ999");
    assert!(non_existent.is_empty());
}
