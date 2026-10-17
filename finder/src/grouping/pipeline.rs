//! Concurrent grouping pipeline and background worker for virtualized views.

use std::collections::{BTreeMap, HashSet};

use rayon::prelude::*;
use shared::models::FileItem;

use super::date::classify_date;
use super::kind::classify_kind;
use super::models::{GroupDimension, GroupKey, SectionGroup};
use super::size::classify_size;
use crate::sorting::comparator::{SortCriteria, compare_file_items};

/// Resolves the sort priority for a tag label, placing canonical macOS colors first.
fn tag_priority(tag: &str) -> u32 {
    match tag.to_ascii_lowercase().as_str() {
        "red" => 0,
        "orange" => 1,
        "yellow" => 2,
        "green" => 3,
        "blue" => 4,
        "purple" => 5,
        "gray" | "grey" => 6,
        _ => 100,
    }
}

/// Synchronously partitions and sorts filesystem items into categorized section groups.
///
/// If `dimension == GroupDimension::None`, returns a single unsectioned group.
/// Groups containing 0 items are omitted.
/// Items within each group are sorted in parallel using Rayon.
#[must_use]
pub fn group_and_sort<S: ::std::hash::BuildHasher + Sync>(
    items: &[FileItem],
    dimension: GroupDimension,
    criteria: SortCriteria,
    collapsed_keys: &HashSet<String, S>,
) -> Vec<SectionGroup<FileItem>> {
    if items.is_empty() {
        return Vec::new();
    }

    if dimension == GroupDimension::None {
        let mut sorted = items.to_vec();
        sorted.par_sort_by(|a, b| compare_file_items(a, b, &criteria));
        return vec![SectionGroup {
            key: GroupKey::new(0, "all", "All Items"),
            items: sorted,
            is_collapsed: false,
        }];
    }

    let now = chrono::Local::now();
    let mut map: BTreeMap<GroupKey, Vec<FileItem>> = BTreeMap::new();

    for item in items {
        match dimension {
            GroupDimension::None => unreachable!(),
            GroupDimension::Kind => {
                let key = classify_kind(item);
                map.entry(key).or_default().push(item.clone());
            }
            GroupDimension::DateModified | GroupDimension::DateAdded => {
                let key = classify_date(item, dimension, now);
                map.entry(key).or_default().push(item.clone());
            }
            GroupDimension::Size => {
                let key = classify_size(item);
                map.entry(key).or_default().push(item.clone());
            }
            GroupDimension::Tags => {
                if item.tags.is_empty() {
                    let key = GroupKey::new(999, "no_tags", "No Tags");
                    map.entry(key).or_default().push(item.clone());
                } else {
                    for tag in &item.tags {
                        let prio = tag_priority(&tag.name);
                        let key = GroupKey::new(
                            prio,
                            format!("tag:{}", tag.name.to_ascii_lowercase()),
                            tag.name.clone(),
                        );
                        map.entry(key).or_default().push(item.clone());
                    }
                }
            }
        }
    }

    let mut sections: Vec<SectionGroup<FileItem>> = map
        .into_iter()
        .filter(|(_, group_items)| !group_items.is_empty())
        .map(|(key, group_items)| {
            let is_collapsed =
                collapsed_keys.contains(&key.id) || collapsed_keys.contains(&key.title);
            SectionGroup {
                key,
                items: group_items,
                is_collapsed,
            }
        })
        .collect();

    // Concurrently sort items in each section group using Rayon
    sections.par_iter_mut().for_each(|group| {
        group
            .items
            .par_sort_by(|a, b| compare_file_items(a, b, &criteria));
    });

    sections
}

/// Asynchronously runs the partitioning and sorting pipeline on a blocking worker thread,
/// dispatching the resulting immutable [`Vec<SectionGroup<FileItem>>`] to the UI thread via [`glib::MainContext`].
pub fn group_and_sort_async<F, S>(
    items: Vec<FileItem>,
    dimension: GroupDimension,
    criteria: SortCriteria,
    collapsed_keys: HashSet<String, S>,
    callback: F,
) where
    F: FnOnce(Vec<SectionGroup<FileItem>>) + 'static,
    S: ::std::hash::BuildHasher + Send + Sync + 'static,
{
    glib::MainContext::default().spawn_local(async move {
        let result = tokio::task::spawn_blocking(move || {
            group_and_sort(&items, dimension, criteria, &collapsed_keys)
        })
        .await;

        if let Ok(sections) = result {
            callback(sections);
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use shared::models::{FileMetadata, FileTag, TagColor};

    use super::*;
    use crate::sorting::comparator::{SortColumn, SortDirection};

    fn make_item(name: &str, is_dir: bool, size: u64, mime: &str, tags: Vec<&str>) -> FileItem {
        let meta = FileMetadata {
            size_bytes: size,
            modified: Some(SystemTime::UNIX_EPOCH),
            accessed: None,
            created: None,
            permissions: if is_dir { 0o755 } else { 0o644 },
            inode: 1,
            device_id: 1,
            mime_type: mime.to_string(),
            is_hidden: false,
            is_executable: false,
        };
        let file_tags = tags
            .into_iter()
            .map(|t| FileTag {
                name: t.to_string(),
                color: Some(TagColor::Blue),
            })
            .collect();
        FileItem::new(
            PathBuf::from(format!("/tmp/{name}")),
            name,
            is_dir,
            false,
            None,
            meta,
            file_tags,
        )
    }

    #[test]
    fn test_group_and_sort_by_kind() {
        let items = vec![
            make_item("photo.jpg", false, 500, "image/jpeg", vec![]),
            make_item("docs", true, 4096, "inode/directory", vec![]),
            make_item("main.rs", false, 1200, "text/x-rust", vec![]),
            make_item("video.mp4", false, 100_000, "video/mp4", vec![]),
        ];

        let criteria = SortCriteria::new(SortColumn::Name, SortDirection::Ascending, true);
        let collapsed = HashSet::new();

        let sections = group_and_sort(&items, GroupDimension::Kind, criteria, &collapsed);
        assert_eq!(sections.len(), 4);
        // Priority order: Folders (0) -> Images (2) -> Movies (3) -> Developer Source (7)
        assert_eq!(sections[0].key.title, "Folders");
        assert_eq!(sections[1].key.title, "Images");
        assert_eq!(sections[2].key.title, "Movies & Video");
        assert_eq!(sections[3].key.title, "Developer Source");
    }

    #[test]
    fn test_omit_empty_groups() {
        let items = vec![make_item("song.mp3", false, 4000, "audio/mpeg", vec![])];

        let criteria = SortCriteria::default();
        let collapsed = HashSet::new();
        let sections = group_and_sort(&items, GroupDimension::Kind, criteria, &collapsed);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].key.title, "Audio & Music");
    }

    #[test]
    fn test_collapsed_group_flagging() {
        let items = vec![
            make_item("folder1", true, 4096, "inode/directory", vec![]),
            make_item("photo.png", false, 2048, "image/png", vec![]),
        ];

        let mut collapsed = HashSet::new();
        collapsed.insert("folders".to_string());

        let sections = group_and_sort(
            &items,
            GroupDimension::Kind,
            SortCriteria::default(),
            &collapsed,
        );
        assert_eq!(sections[0].key.id, "folders");
        assert!(sections[0].is_collapsed);
        assert_eq!(sections[1].key.id, "images");
        assert!(!sections[1].is_collapsed);
    }
}
