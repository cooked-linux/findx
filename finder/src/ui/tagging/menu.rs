//! Context menu integration and multi-selection batch tagging mechanics.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gtk4::prelude::*;
use shared::db::Database;
use shared::models::{FileTag, TagColor};

use super::badges::TagBadgeCache;
use super::pill_editor::TagPillEditor;
use super::swatches::ColorSwatchBar;

/// Summary of tags and colors across multiple selected items.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MultiSelectionSummary {
    /// Tags present on ALL selected items.
    pub shared_tags: Vec<FileTag>,
    /// Tags present on only a strict subset (at least 1, but not all) of selected items.
    pub partial_tags: Vec<FileTag>,
    /// Canonical colors present on ALL selected items.
    pub shared_colors: Vec<TagColor>,
    /// Canonical colors present on only a strict subset of selected items.
    pub partial_colors: Vec<TagColor>,
}

impl MultiSelectionSummary {
    /// Computes multi-selection tag and color distributions from a slice of tag collections.
    #[must_use]
    pub fn compute(items_tags: &[Vec<FileTag>]) -> Self {
        if items_tags.is_empty() {
            return Self::default();
        }
        let total_items = items_tags.len();

        let mut tag_counts: HashMap<String, (usize, FileTag)> = HashMap::new();
        let mut color_counts: HashMap<TagColor, usize> = HashMap::new();

        for file_tags in items_tags {
            let mut seen_tags_in_file = HashSet::new();
            let mut seen_colors_in_file = HashSet::new();

            for tag in file_tags {
                let lower_name = tag.name.to_lowercase();
                if seen_tags_in_file.insert(lower_name.clone()) {
                    let entry = tag_counts
                        .entry(lower_name)
                        .or_insert_with(|| (0, tag.clone()));
                    entry.0 += 1;
                }
                if let Some(color) = tag.color
                    && seen_colors_in_file.insert(color)
                {
                    *color_counts.entry(color).or_insert(0) += 1;
                }
            }
        }

        let mut shared_tags = Vec::new();
        let mut partial_tags = Vec::new();

        for (_key, (count, tag)) in tag_counts {
            if count == total_items {
                shared_tags.push(tag);
            } else {
                partial_tags.push(tag);
            }
        }

        let mut shared_colors = Vec::new();
        let mut partial_colors = Vec::new();

        for (color, count) in color_counts {
            if count == total_items {
                shared_colors.push(color);
            } else {
                partial_colors.push(color);
            }
        }

        shared_tags.sort_by(|a, b| a.name.cmp(&b.name));
        partial_tags.sort_by(|a, b| a.name.cmp(&b.name));
        shared_colors.sort_by_key(TagColor::color_index);
        partial_colors.sort_by_key(TagColor::color_index);

        Self {
            shared_tags,
            partial_tags,
            shared_colors,
            partial_colors,
        }
    }

    /// Computes summary by reading tags for `paths` from extended attributes.
    #[must_use]
    pub fn for_paths(paths: &[PathBuf]) -> Self {
        let items_tags: Vec<Vec<FileTag>> = paths
            .iter()
            .map(|p| crate::metadata::ExtendedAttributesService::get_tags(p).unwrap_or_default())
            .collect();
        Self::compute(&items_tags)
    }
}

/// Applies a tag to all specified file paths, syncing xattr, central database, and badge cache.
pub fn apply_tag_to_paths(paths: &[PathBuf], tag: &FileTag) {
    let pool_opt = Database::open_default().ok();

    for path in paths {
        let mut tags =
            crate::metadata::ExtendedAttributesService::get_tags(path).unwrap_or_default();
        if let Some(existing) = tags
            .iter_mut()
            .find(|t| t.name.eq_ignore_ascii_case(&tag.name))
        {
            if tag.color.is_some() {
                existing.color = tag.color;
            }
        } else {
            tags.push(tag.clone());
        }

        let _ = crate::metadata::ExtendedAttributesService::set_tags(path, &tags);
        if let Some(ref pool) = pool_opt {
            let _ = crate::metadata::sync_file_tags(pool, path, &tags);
        }
        TagBadgeCache::global().invalidate(path);
    }
}

/// Removes a tag by name from all specified file paths, syncing xattr, central database, and cache.
pub fn remove_tag_from_paths(paths: &[PathBuf], tag_name: &str) {
    let pool_opt = Database::open_default().ok();

    for path in paths {
        let mut tags =
            crate::metadata::ExtendedAttributesService::get_tags(path).unwrap_or_default();
        tags.retain(|t| !t.name.eq_ignore_ascii_case(tag_name));

        let _ = crate::metadata::ExtendedAttributesService::set_tags(path, &tags);
        if let Some(ref pool) = pool_opt {
            let _ = crate::metadata::sync_file_tags(pool, path, &tags);
        }
        TagBadgeCache::global().invalidate(path);
    }
}

/// Toggles a canonical color label across all specified file paths.
pub fn toggle_color_on_paths(paths: &[PathBuf], color: TagColor, active: bool) {
    let pool_opt = Database::open_default().ok();

    for path in paths {
        let mut tags =
            crate::metadata::ExtendedAttributesService::get_tags(path).unwrap_or_default();

        if active {
            if let Some(existing) = tags
                .iter_mut()
                .find(|t| t.name.eq_ignore_ascii_case(color.display_name()))
            {
                existing.color = Some(color);
            } else if let Some(first) = tags.first_mut() {
                first.color = Some(color);
            } else {
                tags.push(FileTag::new(color.display_name(), Some(color)));
            }
        } else {
            tags.retain(|t| {
                !t.name.eq_ignore_ascii_case(color.display_name()) && t.color != Some(color)
            });
        }

        let _ = crate::metadata::ExtendedAttributesService::set_tags(path, &tags);
        if let Some(ref pool) = pool_opt {
            let _ = crate::metadata::sync_file_tags(pool, path, &tags);
        }
        TagBadgeCache::global().invalidate(path);
    }
}

/// Constructs the context menu tagging section containing the 7-color swatch bar
/// and a "Tags..." menu item that opens `TagPillEditor` in a floating popover.
#[must_use]
pub fn create_tags_context_menu_widget<W: IsA<gtk4::Widget>>(
    paths: &[PathBuf],
    parent_anchor: &W,
) -> gtk4::Box {
    let container = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(2)
        .build();

    let summary = MultiSelectionSummary::for_paths(paths);

    // 1. Color Swatch Bar
    let swatches = ColorSwatchBar::new();
    swatches.set_active_colors(&summary.shared_colors, &summary.partial_colors);

    let paths_for_swatches = paths.to_vec();
    swatches.connect_color_toggled(move |color, active| {
        toggle_color_on_paths(&paths_for_swatches, color, active);
    });
    container.append(&swatches.container);

    // 2. "Tags..." Button
    let tags_btn = gtk4::Button::builder()
        .has_frame(false)
        .css_classes(["flat", "context-menu-item"])
        .build();

    let btn_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let title_label = gtk4::Label::builder()
        .label("Tags...")
        .halign(gtk4::Align::Start)
        .hexpand(true)
        .build();
    btn_box.append(&title_label);

    tags_btn.set_child(Some(&btn_box));

    // Wire popover opening on click
    let parent_weak = parent_anchor.as_ref().downgrade();
    let paths_for_editor = paths.to_vec();

    tags_btn.connect_clicked(move |_| {
        let Some(parent) = parent_weak.upgrade() else {
            return;
        };

        let floating_popover = gtk4::Popover::builder()
            .has_arrow(true)
            .autohide(true)
            .css_classes(["context-menu-popover"])
            .build();
        floating_popover.set_parent(&parent);

        let editor = TagPillEditor::new();
        let current_summary = MultiSelectionSummary::for_paths(&paths_for_editor);
        editor.set_tags(&current_summary.shared_tags, &current_summary.partial_tags);

        let paths_add = paths_for_editor.clone();
        editor.connect_tag_added(move |tag| {
            apply_tag_to_paths(&paths_add, &tag);
        });

        let paths_rem = paths_for_editor.clone();
        editor.connect_tag_removed(move |tag_name| {
            remove_tag_from_paths(&paths_rem, &tag_name);
        });

        let popover_close = floating_popover.clone();
        editor.connect_escape(move || {
            popover_close.popdown();
        });

        floating_popover.set_child(Some(&editor.container));
        floating_popover.connect_closed(|p| {
            p.unparent();
        });
        floating_popover.popup();
    });

    container.append(&tags_btn);
    container
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use crate::ui::tagging::GTK_TEST_MUTEX;
    use tempfile::NamedTempFile;

    fn ensure_gtk() -> bool {
        if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err())
        {
            return false;
        }
        true
    }

    #[test]
    fn test_multi_selection_summary_computation() {
        let item1 = vec![
            FileTag::new("Work", Some(TagColor::Red)),
            FileTag::new("Shared", Some(TagColor::Blue)),
        ];
        let item2 = vec![
            FileTag::new("Work", Some(TagColor::Red)),
            FileTag::new("Personal", None),
        ];
        let item3 = vec![
            FileTag::new("Work", Some(TagColor::Red)),
            FileTag::new("Shared", Some(TagColor::Blue)),
        ];

        let summary = MultiSelectionSummary::compute(&[item1, item2, item3]);

        // "Work" is on all 3 items
        assert_eq!(summary.shared_tags.len(), 1);
        assert_eq!(summary.shared_tags[0].name, "Work");

        // "Shared" and "Personal" are on subsets
        assert_eq!(summary.partial_tags.len(), 2);
        assert!(summary.partial_tags.iter().any(|t| t.name == "Shared"));
        assert!(summary.partial_tags.iter().any(|t| t.name == "Personal"));

        // Red is on all 3 items
        assert_eq!(summary.shared_colors, vec![TagColor::Red]);
        // Blue is on 2 items
        assert_eq!(summary.partial_colors, vec![TagColor::Blue]);
    }

    #[test]
    fn test_batch_tagging_menu_5_files() {
        let files: Vec<NamedTempFile> = (0..5).map(|_| NamedTempFile::new().unwrap()).collect();
        let paths: Vec<PathBuf> = files.iter().map(|f| f.path().to_path_buf()).collect();

        // 1. Initial state: no tags
        let initial_summary = MultiSelectionSummary::for_paths(&paths);
        assert!(initial_summary.shared_tags.is_empty());
        assert!(initial_summary.partial_tags.is_empty());

        // 2. Apply tag "Project-Omega" (Green) to all 5 files
        let green_tag = FileTag::new("Project-Omega", Some(TagColor::Green));
        apply_tag_to_paths(&paths, &green_tag);

        for path in &paths {
            let tags = crate::metadata::ExtendedAttributesService::get_tags(path).unwrap();
            assert_eq!(tags.len(), 1);
            assert_eq!(tags[0].name, "Project-Omega");
            assert_eq!(tags[0].color, Some(TagColor::Green));
        }

        let summary_after_apply = MultiSelectionSummary::for_paths(&paths);
        assert_eq!(summary_after_apply.shared_tags.len(), 1);
        assert_eq!(summary_after_apply.shared_tags[0].name, "Project-Omega");
        assert_eq!(summary_after_apply.shared_colors, vec![TagColor::Green]);
        assert!(summary_after_apply.partial_tags.is_empty());

        // 3. Apply tag "Special" to only 2 files (subset / partial)
        let special_tag = FileTag::new("Special", None);
        apply_tag_to_paths(&paths[0..2], &special_tag);

        let summary_partial = MultiSelectionSummary::for_paths(&paths);
        assert_eq!(summary_partial.shared_tags.len(), 1);
        assert_eq!(summary_partial.partial_tags.len(), 1);
        assert_eq!(summary_partial.partial_tags[0].name, "Special");

        // 4. Toggle color label Blue across all 5 files
        toggle_color_on_paths(&paths, TagColor::Blue, true);
        let summary_blue = MultiSelectionSummary::for_paths(&paths);
        assert!(summary_blue.shared_colors.contains(&TagColor::Blue));

        // 5. Remove "Project-Omega" from all files
        remove_tag_from_paths(&paths, "Project-Omega");
        for path in &paths {
            let tags = crate::metadata::ExtendedAttributesService::get_tags(path).unwrap();
            assert!(!tags.iter().any(|t| t.name == "Project-Omega"));
        }
    }

    #[test]
    fn test_context_menu_widget_creation() {
        let _lock = GTK_TEST_MUTEX.lock().unwrap();
        if !ensure_gtk() {
            return;
        }

        let file = NamedTempFile::new().unwrap();
        let paths = vec![file.path().to_path_buf()];
        let dummy_row = gtk4::ListBoxRow::new();

        let widget = create_tags_context_menu_widget(&paths, &dummy_row);
        assert!(widget.first_child().is_some());
    }
}
