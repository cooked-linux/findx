//! Column view models and state architecture for Miller Columns.

use std::path::{Path, PathBuf};

use shared::models::FileItem;

/// Default column width in pixels.
pub const DEFAULT_COLUMN_WIDTH: i32 = 220;
/// Minimum allowed column width in pixels.
pub const MIN_COLUMN_WIDTH: i32 = 160;
/// Maximum allowed column width in pixels.
pub const MAX_COLUMN_WIDTH: i32 = 600;

/// Represents an item entry displayed inside a directory column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnEntry {
    /// Associated filesystem item.
    pub item: FileItem,
    /// Indicates whether this item is a currently expanded directory in the child column.
    pub is_expanded_directory: bool,
}

impl ColumnEntry {
    /// Creates a new [`ColumnEntry`].
    #[must_use]
    pub const fn new(item: FileItem, is_expanded_directory: bool) -> Self {
        Self {
            item,
            is_expanded_directory,
        }
    }

    /// Returns a reference to the item's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.item.path()
    }

    /// Returns the file name of the entry.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.item.file_name()
    }

    /// Returns `true` if the item is a directory.
    #[must_use]
    pub const fn is_dir(&self) -> bool {
        self.item.is_dir()
    }

    /// Returns `true` if the item is a symbolic link.
    #[must_use]
    pub const fn is_symlink(&self) -> bool {
        self.item.is_symlink()
    }
}

/// Discriminant of column content: either a directory list or a terminal file preview.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnKind {
    /// Directory listing column.
    Directory {
        /// Absolute directory path.
        path: PathBuf,
        /// Listed directory entries.
        entries: Vec<ColumnEntry>,
        /// Index of the currently selected entry, if any.
        selected_index: Option<usize>,
        /// Vertical scroll offset.
        scroll_pos_y: f64,
    },
    /// Terminal leaf file preview column.
    FilePreview {
        /// Inspected file item.
        item: FileItem,
        /// Resolved thumbnail image path on disk, if available.
        thumbnail_path: Option<PathBuf>,
    },
}

impl ColumnKind {
    /// Returns `true` if this is a directory column.
    #[must_use]
    pub const fn is_directory(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }

    /// Returns `true` if this is a file preview column.
    #[must_use]
    pub const fn is_file_preview(&self) -> bool {
        matches!(self, Self::FilePreview { .. })
    }

    /// Returns the directory path or file path associated with this column.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Directory { path, .. } => path,
            Self::FilePreview { item, .. } => item.path(),
        }
    }

    /// Returns the entries if this is a directory column.
    #[must_use]
    pub fn entries(&self) -> Option<&[ColumnEntry]> {
        match self {
            Self::Directory { entries, .. } => Some(entries),
            Self::FilePreview { .. } => None,
        }
    }

    /// Returns mutable reference to entries if this is a directory column.
    pub const fn entries_mut(&mut self) -> Option<&mut Vec<ColumnEntry>> {
        match self {
            Self::Directory { entries, .. } => Some(entries),
            Self::FilePreview { .. } => None,
        }
    }

    /// Returns the selected index if this is a directory column.
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        match self {
            Self::Directory { selected_index, .. } => *selected_index,
            Self::FilePreview { .. } => None,
        }
    }

    /// Returns the selected entry if this is a directory column and an item is selected.
    #[must_use]
    pub fn selected_entry(&self) -> Option<&ColumnEntry> {
        match self {
            Self::Directory {
                entries,
                selected_index,
                ..
            } => (*selected_index).and_then(|idx| entries.get(idx)),
            Self::FilePreview { .. } => None,
        }
    }
}

/// View model representation of a single column in the cascading stack.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnViewModel {
    /// Zero-based horizontal column index in the stack.
    pub index: usize,
    /// Width of the column in pixels.
    pub width_px: i32,
    /// Kind and contents of the column.
    pub kind: ColumnKind,
    /// Optional error message (e.g. permission denied).
    pub error: Option<String>,
}

impl ColumnViewModel {
    /// Constructs a directory column view model.
    #[must_use]
    pub fn new_directory(
        index: usize,
        width_px: i32,
        path: PathBuf,
        entries: Vec<ColumnEntry>,
    ) -> Self {
        Self {
            index,
            width_px: width_px.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH),
            kind: ColumnKind::Directory {
                path,
                entries,
                selected_index: None,
                scroll_pos_y: 0.0,
            },
            error: None,
        }
    }

    /// Constructs a file preview column view model.
    #[must_use]
    pub fn new_preview(
        index: usize,
        width_px: i32,
        item: FileItem,
        thumbnail_path: Option<PathBuf>,
    ) -> Self {
        Self {
            index,
            width_px: width_px.clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH),
            kind: ColumnKind::FilePreview {
                item,
                thumbnail_path,
            },
            error: None,
        }
    }

    /// Returns the selected entry if this is a directory column.
    #[must_use]
    pub fn selected_entry(&self) -> Option<&ColumnEntry> {
        self.kind.selected_entry()
    }

    /// Selects the entry at `index`, updating `is_expanded_directory` flags.
    pub fn set_selected_index(&mut self, index: Option<usize>) {
        if let ColumnKind::Directory {
            entries,
            selected_index,
            ..
        } = &mut self.kind
        {
            *selected_index = index;
            for (i, entry) in entries.iter_mut().enumerate() {
                entry.is_expanded_directory = index == Some(i) && entry.is_dir();
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use shared::models::FileMetadata;

    use super::*;

    fn dummy_file_item(name: &str, is_dir: bool) -> FileItem {
        FileItem::new(
            PathBuf::from("/test").join(name),
            name,
            is_dir,
            false,
            None,
            FileMetadata {
                size_bytes: 1024,
                modified: None,
                accessed: None,
                created: None,
                permissions: 0o755,
                inode: 1,
                device_id: 1,
                mime_type: if is_dir {
                    "inode/directory".to_string()
                } else {
                    "text/plain".to_string()
                },
                is_hidden: false,
                is_executable: is_dir,
            },
            vec![],
        )
    }

    #[test]
    fn test_column_view_model_clamping() {
        let col = ColumnViewModel::new_directory(0, 100, PathBuf::from("/test"), vec![]);
        assert_eq!(col.width_px, MIN_COLUMN_WIDTH);

        let col_max = ColumnViewModel::new_directory(0, 1000, PathBuf::from("/test"), vec![]);
        assert_eq!(col_max.width_px, MAX_COLUMN_WIDTH);

        let col_normal = ColumnViewModel::new_directory(1, 240, PathBuf::from("/test"), vec![]);
        assert_eq!(col_normal.width_px, 240);
        assert_eq!(col_normal.index, 1);
    }

    #[test]
    fn test_selection_updates_expanded_flag() {
        let entries = vec![
            ColumnEntry::new(dummy_file_item("folder1", true), false),
            ColumnEntry::new(dummy_file_item("file1.txt", false), false),
        ];
        let mut col = ColumnViewModel::new_directory(0, 220, PathBuf::from("/test"), entries);

        col.set_selected_index(Some(0));
        assert_eq!(col.kind.selected_index(), Some(0));
        assert!(col.kind.entries().unwrap()[0].is_expanded_directory);
        assert!(!col.kind.entries().unwrap()[1].is_expanded_directory);

        col.set_selected_index(Some(1));
        assert_eq!(col.kind.selected_index(), Some(1));
        // Selected index 1 is a file, so is_expanded_directory must be false
        assert!(!col.kind.entries().unwrap()[0].is_expanded_directory);
        assert!(!col.kind.entries().unwrap()[1].is_expanded_directory);

        col.set_selected_index(None);
        assert_eq!(col.kind.selected_index(), None);
    }

    #[test]
    fn test_preview_column() {
        let item = dummy_file_item("test.png", false);
        let preview = ColumnViewModel::new_preview(2, 220, item, Some(PathBuf::from("/thumb.png")));
        assert!(preview.kind.is_file_preview());
        assert!(!preview.kind.is_directory());
        assert_eq!(preview.kind.path(), Path::new("/test/test.png"));
    }
}
