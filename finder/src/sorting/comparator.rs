//! Multi-key natural numerical sorting comparator for file items.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use shared::fs::natural_cmp;
use shared::models::FileItem;

use crate::grouping::kind::classify_kind;

/// Metadata columns available for ordering items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SortColumn {
    /// Ordered alphabetically with natural numerical sequence collation (`file2` < `file10`).
    #[default]
    Name,
    /// Ordered by modification timestamp.
    DateModified,
    /// Ordered by byte size.
    Size,
    /// Ordered by file kind category or extension.
    Kind,
    /// Ordered by user tag labels and presence.
    Tags,
}

impl SortColumn {
    /// Returns the human-readable display label for the column.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::DateModified => "Date Modified",
            Self::Size => "Size",
            Self::Kind => "Kind",
            Self::Tags => "Tags",
        }
    }
}

/// Ordering direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SortDirection {
    /// Ascending order (A-Z, oldest first, smallest first).
    #[default]
    Ascending,
    /// Descending order (Z-A, newest first, largest first).
    Descending,
}

impl SortDirection {
    /// Toggles the sort direction.
    #[must_use]
    pub const fn toggle(&self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// Composite sorting criteria defining primary key, direction, and directory precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SortCriteria {
    /// Primary column used for sorting.
    pub primary: SortColumn,
    /// Sorting direction.
    pub direction: SortDirection,
    /// When `true`, directories always precede files regardless of primary sort key.
    pub folders_first: bool,
}

impl Default for SortCriteria {
    fn default() -> Self {
        Self {
            primary: SortColumn::Name,
            direction: SortDirection::Ascending,
            folders_first: true,
        }
    }
}

impl SortCriteria {
    /// Creates a new [`SortCriteria`].
    #[must_use]
    pub const fn new(primary: SortColumn, direction: SortDirection, folders_first: bool) -> Self {
        Self {
            primary,
            direction,
            folders_first,
        }
    }
}

/// Compares two [`FileItem`]s according to the specified [`SortCriteria`].
///
/// Directory precedence (`folders_first`) is always maintained at the top level.
/// Within the same item type (both dirs or both files), items are ordered by
/// `primary` column using `natural_cmp` for strings and natural ordering for numbers.
/// Ties are deterministically resolved by item name, then canonical path.
#[must_use]
pub fn compare_file_items(a: &FileItem, b: &FileItem, criteria: &SortCriteria) -> Ordering {
    // 1. Folders first check
    if criteria.folders_first {
        let a_is_dir = a.is_dir || a.metadata.mime_type == "inode/directory";
        let b_is_dir = b.is_dir || b.metadata.mime_type == "inode/directory";

        match (a_is_dir, b_is_dir) {
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            _ => {}
        }
    }

    // 2. Primary column comparison
    let primary_ord = match criteria.primary {
        SortColumn::Name => natural_cmp(a.file_name(), b.file_name()),
        SortColumn::DateModified => {
            let time_a = a.metadata.modified;
            let time_b = b.metadata.modified;
            time_a.cmp(&time_b)
        }
        SortColumn::Size => a.metadata.size_bytes.cmp(&b.metadata.size_bytes),
        SortColumn::Kind => {
            let key_a = classify_kind(a);
            let key_b = classify_kind(b);
            key_a.priority.cmp(&key_b.priority)
        }
        SortColumn::Tags => {
            let tag_a = a.tags.first().map(|t| t.name.as_str());
            let tag_b = b.tags.first().map(|t| t.name.as_str());

            match (tag_a, tag_b) {
                (Some(ta), Some(tb)) => natural_cmp(ta, tb),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
        }
    };

    let directed_ord = match criteria.direction {
        SortDirection::Ascending => primary_ord,
        SortDirection::Descending => primary_ord.reverse(),
    };

    // 3. Deterministic tie-breaking (Natural name collation, then path)
    directed_ord
        .then_with(|| natural_cmp(a.file_name(), b.file_name()))
        .then_with(|| a.path.cmp(&b.path))
}

/// Sorts a slice of [`FileItem`]s in place according to the given criteria.
pub fn sort_file_items(items: &mut [FileItem], criteria: &SortCriteria) {
    items.sort_by(|a, b| compare_file_items(a, b, criteria));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use shared::models::{FileMetadata, FileTag, TagColor};

    use super::*;

    fn create_test_item(
        name: &str,
        is_dir: bool,
        size: u64,
        modified: Option<SystemTime>,
        tags: Vec<&str>,
    ) -> FileItem {
        let meta = FileMetadata {
            size_bytes: size,
            modified,
            accessed: None,
            created: None,
            permissions: if is_dir { 0o755 } else { 0o644 },
            inode: 1,
            device_id: 1,
            mime_type: if is_dir {
                "inode/directory".to_string()
            } else {
                "application/octet-stream".to_string()
            },
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
    fn test_natural_sort_ordering() {
        let mut items = vec![
            create_test_item("File 20.txt", false, 100, None, vec![]),
            create_test_item("File 1.txt", false, 100, None, vec![]),
            create_test_item("File 10.txt", false, 100, None, vec![]),
            create_test_item("File 2.txt", false, 100, None, vec![]),
        ];

        let criteria = SortCriteria {
            primary: SortColumn::Name,
            direction: SortDirection::Ascending,
            folders_first: true,
        };

        sort_file_items(&mut items, &criteria);

        let names: Vec<&str> = items.iter().map(FileItem::file_name).collect();
        assert_eq!(
            names,
            vec!["File 1.txt", "File 2.txt", "File 10.txt", "File 20.txt"]
        );
    }

    #[test]
    fn test_folders_first_precedence() {
        let mut items = vec![
            create_test_item("zebra.txt", false, 100, None, vec![]),
            create_test_item("alpha_folder", true, 0, None, vec![]),
            create_test_item("apple.txt", false, 100, None, vec![]),
            create_test_item("beta_folder", true, 0, None, vec![]),
        ];

        let criteria = SortCriteria {
            primary: SortColumn::Name,
            direction: SortDirection::Ascending,
            folders_first: true,
        };

        sort_file_items(&mut items, &criteria);

        let names: Vec<&str> = items.iter().map(FileItem::file_name).collect();
        assert_eq!(
            names,
            vec!["alpha_folder", "beta_folder", "apple.txt", "zebra.txt"]
        );
    }

    #[test]
    fn test_sort_by_size_descending() {
        let mut items = vec![
            create_test_item("small.txt", false, 10, None, vec![]),
            create_test_item("huge.txt", false, 10_000, None, vec![]),
            create_test_item("medium.txt", false, 500, None, vec![]),
        ];

        let criteria = SortCriteria {
            primary: SortColumn::Size,
            direction: SortDirection::Descending,
            folders_first: false,
        };

        sort_file_items(&mut items, &criteria);

        let names: Vec<&str> = items.iter().map(FileItem::file_name).collect();
        assert_eq!(names, vec!["huge.txt", "medium.txt", "small.txt"]);
    }
}
