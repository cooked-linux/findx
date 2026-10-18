//! Multi-selection metadata aggregation service.
//!
//! Computes summary statistics across multiple selected filesystem entities:
//! - Total item, directory, and file counts
//! - Cumulative logical and allocated physical byte sizes
//! - Tag intersection (`shared_tags`) vs union-difference (`partial_tags`)
//! - Uniform permission detection and locked item presence

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rayon::prelude::*;
use shared::models::FileTag;

use super::error::Result;
use super::extract::extract_deep_metadata;

/// Aggregated metadata summary for multiple selected filesystem items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiSelectionSummary {
    /// Total number of items in the selection.
    pub total_items: usize,
    /// Total count of directories in the selection.
    pub total_directories: usize,
    /// Total count of files and non-directory entities.
    pub total_files: usize,
    /// Total logical byte size across all items.
    pub total_logical_bytes: u64,
    /// Total physical allocated byte size on disk (`st_blocks * 512`).
    pub total_physical_bytes: u64,
    /// Tags common to all items in the selection (intersection).
    pub shared_tags: Vec<FileTag>,
    /// Tags present on some, but not all, items in the selection.
    pub partial_tags: Vec<FileTag>,
    /// Octal permission mask if all items share identical permissions, or `None`.
    pub uniform_permissions: Option<u32>,
    /// Whether any item in the selection is marked locked / read-only (`(mode & 0o222) == 0`).
    pub has_locked_items: bool,
}

/// Aggregates metadata across a slice of filesystem paths in parallel using Rayon.
///
/// # Errors
///
/// Returns [`MetadataError`](crate::metadata::MetadataError) if inspection fails for any path.
pub fn aggregate_metadata(paths: &[PathBuf]) -> Result<MultiSelectionSummary> {
    if paths.is_empty() {
        return Ok(MultiSelectionSummary {
            total_items: 0,
            total_directories: 0,
            total_files: 0,
            total_logical_bytes: 0,
            total_physical_bytes: 0,
            shared_tags: Vec::new(),
            partial_tags: Vec::new(),
            uniform_permissions: None,
            has_locked_items: false,
        });
    }

    let items = paths
        .par_iter()
        .map(|path| extract_deep_metadata(path))
        .collect::<Result<Vec<_>>>()?;

    let total_items = items.len();
    let mut total_directories = 0;
    let mut total_files = 0;
    let mut total_logical_bytes = 0u64;
    let mut total_physical_bytes = 0u64;
    let mut has_locked_items = false;

    let mut first_permission = None;
    let mut permissions_uniform = true;

    // Track tag occurrences across items
    let mut tag_counts: HashMap<FileTag, usize> = HashMap::new();

    for item in &items {
        if item.is_directory {
            total_directories += 1;
        } else {
            total_files += 1;
        }

        total_logical_bytes = total_logical_bytes.saturating_add(item.size_logical);
        total_physical_bytes = total_physical_bytes.saturating_add(item.size_physical);

        if item.permissions.is_locked {
            has_locked_items = true;
        }

        let mode_norm = item.permissions.mode & 0o7777;
        match first_permission {
            None => first_permission = Some(mode_norm),
            Some(m) if m != mode_norm => permissions_uniform = false,
            _ => {}
        }

        // Deduplicate tags per file item
        let unique_item_tags: HashSet<&FileTag> = item.extended.tags.iter().collect();
        for tag in unique_item_tags {
            *tag_counts.entry(tag.clone()).or_insert(0) += 1;
        }
    }

    let mut shared_tags = Vec::new();
    let mut partial_tags = Vec::new();

    for (tag, count) in tag_counts {
        if count == total_items {
            shared_tags.push(tag);
        } else {
            partial_tags.push(tag);
        }
    }

    shared_tags.sort_by(|a, b| a.name.cmp(&b.name));
    partial_tags.sort_by(|a, b| a.name.cmp(&b.name));

    let uniform_permissions = if permissions_uniform {
        first_permission
    } else {
        None
    };

    Ok(MultiSelectionSummary {
        total_items,
        total_directories,
        total_files,
        total_logical_bytes,
        total_physical_bytes,
        shared_tags,
        partial_tags,
        uniform_permissions,
        has_locked_items,
    })
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
    use shared::models::TagColor;
    use std::io::Write;
    use tempfile::tempdir;

    use crate::metadata::xattr::ExtendedAttributesService;

    #[test]
    fn test_aggregate_empty_selection() {
        let summary = aggregate_metadata(&[]).unwrap();
        assert_eq!(summary.total_items, 0);
        assert_eq!(summary.total_directories, 0);
        assert_eq!(summary.total_files, 0);
        assert_eq!(summary.total_logical_bytes, 0);
        assert_eq!(summary.total_physical_bytes, 0);
        assert!(summary.shared_tags.is_empty());
        assert!(summary.partial_tags.is_empty());
        assert_eq!(summary.uniform_permissions, None);
        assert!(!summary.has_locked_items);
    }

    #[test]
    fn test_aggregate_mixed_batch_10_items() {
        let dir = tempdir().unwrap();
        let mut paths = Vec::new();

        // 1. Create 6 files
        for i in 0..6 {
            let file_path = dir.path().join(format!("file_{i}.txt"));
            let mut file = std::fs::File::create(&file_path).unwrap();
            let content = format!("File payload number {i} with some content bytes");
            file.write_all(content.as_bytes()).unwrap();
            file.flush().unwrap();

            // Set tags: all items have "CommonTag"
            let mut tags = vec![FileTag::new("CommonTag", Some(TagColor::Blue))];
            if i % 2 == 0 {
                tags.push(FileTag::new("EvenOnly", Some(TagColor::Green)));
            }
            ExtendedAttributesService::set_tags(&file_path, &tags).unwrap();

            paths.push(file_path);
        }

        // 2. Create 4 directories
        for i in 0..4 {
            let subdir_path = dir.path().join(format!("subdir_{i}"));
            std::fs::create_dir(&subdir_path).unwrap();

            let tags = vec![FileTag::new("CommonTag", Some(TagColor::Blue))];
            ExtendedAttributesService::set_tags(&subdir_path, &tags).unwrap();

            paths.push(subdir_path);
        }

        assert_eq!(paths.len(), 10);

        let summary = aggregate_metadata(&paths).unwrap();
        assert_eq!(summary.total_items, 10);
        assert_eq!(summary.total_files, 6);
        assert_eq!(summary.total_directories, 4);
        assert!(summary.total_logical_bytes > 0);

        // CommonTag must be shared across all 10 items
        assert_eq!(summary.shared_tags.len(), 1);
        assert_eq!(summary.shared_tags[0].name, "CommonTag");
        assert_eq!(summary.shared_tags[0].color, Some(TagColor::Blue));

        // EvenOnly was only set on 3 files (0, 2, 4) -> must be in partial_tags
        assert_eq!(summary.partial_tags.len(), 1);
        assert_eq!(summary.partial_tags[0].name, "EvenOnly");
        assert_eq!(summary.partial_tags[0].color, Some(TagColor::Green));
    }
}
