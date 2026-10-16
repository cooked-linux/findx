//! Filesystem items and metadata models.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::models::tag::{FileTag, TagColor};

/// Represents a single file or directory entity in the filesystem.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileItem {
    /// Canonical or absolute path of the file item.
    pub path: PathBuf,
    /// UTF-8 file or directory name.
    pub file_name: String,
    /// Indicates if the item is a directory.
    pub is_dir: bool,
    /// Indicates if the item is a symbolic link.
    pub is_symlink: bool,
    /// Target path if the item is a symbolic link.
    pub symlink_target: Option<PathBuf>,
    /// Detailed file metadata (size, timestamps, inode, etc.).
    pub metadata: FileMetadata,
    /// User tags and color labels applied to the item.
    pub tags: Vec<FileTag>,
}

impl FileItem {
    /// Creates a new [`FileItem`] instance.
    #[must_use]
    pub fn new(
        path: impl Into<PathBuf>,
        file_name: impl Into<String>,
        is_dir: bool,
        is_symlink: bool,
        symlink_target: Option<PathBuf>,
        metadata: FileMetadata,
        tags: Vec<FileTag>,
    ) -> Self {
        Self {
            path: path.into(),
            file_name: file_name.into(),
            is_dir,
            is_symlink,
            symlink_target,
            metadata,
            tags,
        }
    }

    /// Returns a borrowed reference to the item's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a borrowed reference to the file name.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Returns the file extension as a string slice, if present.
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.path.extension().and_then(|ext| ext.to_str())
    }

    /// Returns `true` if this item represents a directory.
    #[must_use]
    pub const fn is_dir(&self) -> bool {
        self.is_dir
    }

    /// Returns `true` if this item represents a symlink.
    #[must_use]
    pub const fn is_symlink(&self) -> bool {
        self.is_symlink
    }

    /// Checks if a tag with the specified name exists on this item.
    #[must_use]
    pub fn has_tag(&self, tag_name: &str) -> bool {
        self.tags
            .iter()
            .any(|t| t.name.eq_ignore_ascii_case(tag_name))
    }

    /// Returns all color labels associated with this item's tags.
    #[must_use]
    pub fn tag_colors(&self) -> Vec<TagColor> {
        self.tags.iter().filter_map(|t| t.color).collect()
    }
}

/// Detailed filesystem metadata for a [`FileItem`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileMetadata {
    /// File size in bytes.
    pub size_bytes: u64,
    /// Last modification timestamp.
    pub modified: Option<SystemTime>,
    /// Last access timestamp.
    pub accessed: Option<SystemTime>,
    /// Creation timestamp (derived from statx `stx_btime` if supported).
    pub created: Option<SystemTime>,
    /// POSIX file permission bits (mode).
    pub permissions: u32,
    /// Filesystem inode number.
    pub inode: u64,
    /// Device ID holding the inode.
    pub device_id: u64,
    /// MIME type string (e.g. `"text/plain"`, `"inode/directory"`).
    pub mime_type: String,
    /// Whether the file is hidden (starts with `.` on Unix).
    pub is_hidden: bool,
    /// Whether the file has executable permissions.
    pub is_executable: bool,
}

impl FileMetadata {
    /// Constructs a new [`FileMetadata`] struct.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "comprehensive metadata initialization"
    )]
    pub fn new(
        size_bytes: u64,
        modified: Option<SystemTime>,
        accessed: Option<SystemTime>,
        created: Option<SystemTime>,
        permissions: u32,
        inode: u64,
        device_id: u64,
        mime_type: impl Into<String>,
        is_hidden: bool,
        is_executable: bool,
    ) -> Self {
        Self {
            size_bytes,
            modified,
            accessed,
            created,
            permissions,
            inode,
            device_id,
            mime_type: mime_type.into(),
            is_hidden,
            is_executable,
        }
    }

    /// Formats POSIX permission mode bits as a standard `rwxrwxrwx` string.
    #[must_use]
    pub fn permissions_display(&self) -> String {
        let mode = self.permissions;
        let mut res = String::with_capacity(9);
        let flags = [
            (0o400, 'r'),
            (0o200, 'w'),
            (0o100, 'x'),
            (0o040, 'r'),
            (0o020, 'w'),
            (0o010, 'x'),
            (0o004, 'r'),
            (0o002, 'w'),
            (0o001, 'x'),
        ];
        for (bit, ch) in flags {
            if (mode & bit) != 0 {
                res.push(ch);
            } else {
                res.push('-');
            }
        }
        res
    }
}

impl Default for FileMetadata {
    fn default() -> Self {
        Self {
            size_bytes: 0,
            modified: None,
            accessed: None,
            created: None,
            permissions: 0o644,
            inode: 0,
            device_id: 0,
            mime_type: "application/octet-stream".to_string(),
            is_hidden: false,
            is_executable: false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_file_item_construction_and_getters() {
        let meta = FileMetadata::new(
            1024,
            None,
            None,
            None,
            0o755,
            12345,
            67890,
            "text/plain",
            false,
            true,
        );

        let item = FileItem::new(
            PathBuf::from("/home/user/test.txt"),
            "test.txt",
            false,
            false,
            None,
            meta,
            vec![FileTag {
                name: "Work".to_string(),
                color: Some(TagColor::Blue),
            }],
        );

        assert_eq!(item.path(), Path::new("/home/user/test.txt"));
        assert_eq!(item.file_name(), "test.txt");
        assert_eq!(item.extension(), Some("txt"));
        assert!(!item.is_dir());
        assert!(!item.is_symlink());
        assert!(item.has_tag("work"));
        assert!(!item.has_tag("personal"));
        assert_eq!(item.tag_colors(), vec![TagColor::Blue]);
        assert_eq!(item.metadata.permissions_display(), "rwxr-xr-x");
    }

    #[test]
    fn test_permissions_display() {
        let meta = FileMetadata {
            permissions: 0o644,
            ..Default::default()
        };
        assert_eq!(meta.permissions_display(), "rw-r--r--");

        let meta = FileMetadata {
            permissions: 0o777,
            ..Default::default()
        };
        assert_eq!(meta.permissions_display(), "rwxrwxrwx");

        let meta = FileMetadata {
            permissions: 0o000,
            ..Default::default()
        };
        assert_eq!(meta.permissions_display(), "---------");
    }

    #[test]
    fn test_file_item_serde() {
        let meta = FileMetadata::new(
            2048,
            Some(SystemTime::UNIX_EPOCH),
            None,
            None,
            0o644,
            999,
            1,
            "application/json",
            true,
            false,
        );

        let item = FileItem::new(
            PathBuf::from("/tmp/.config.json"),
            ".config.json",
            false,
            false,
            None,
            meta,
            vec![],
        );

        let json = serde_json::to_string(&item).unwrap();
        let deserialized: FileItem = serde_json::from_str(&json).unwrap();
        assert_eq!(item, deserialized);
    }
}
