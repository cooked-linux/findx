//! Standardized byte size binning engine for filesystem items.

use shared::models::FileItem;

use super::models::GroupKey;

pub const BYTES_1_MB: u64 = 1024 * 1024;
pub const BYTES_100_MB: u64 = 100 * 1024 * 1024;
pub const BYTES_1_GB: u64 = 1024 * 1024 * 1024;
pub const BYTES_10_GB: u64 = 10 * 1024 * 1024 * 1024;

pub const PRIORITY_FOLDERS: u32 = 0;
pub const PRIORITY_OVER_10_GB: u32 = 1;
pub const PRIORITY_1_TO_10_GB: u32 = 2;
pub const PRIORITY_100_MB_TO_1_GB: u32 = 3;
pub const PRIORITY_1_TO_100_MB: u32 = 4;
pub const PRIORITY_UNDER_1_MB: u32 = 5;
pub const PRIORITY_ZERO_BYTES: u32 = 6;

pub const ID_FOLDERS: &str = "folders";
pub const ID_OVER_10_GB: &str = "over_10gb";
pub const ID_1_TO_10_GB: &str = "1gb_to_10gb";
pub const ID_100_MB_TO_1_GB: &str = "100mb_to_1gb";
pub const ID_1_TO_100_MB: &str = "1mb_to_100mb";
pub const ID_UNDER_1_MB: &str = "under_1mb";
pub const ID_ZERO_BYTES: &str = "zero_bytes";

pub const TITLE_FOLDERS: &str = "Folders";
pub const TITLE_OVER_10_GB: &str = "Over 10 GB";
pub const TITLE_1_TO_10_GB: &str = "1 GB to 10 GB";
pub const TITLE_100_MB_TO_1_GB: &str = "100 MB to 1 GB";
pub const TITLE_1_TO_100_MB: &str = "1 MB to 100 MB";
pub const TITLE_UNDER_1_MB: &str = "Under 1 MB";
pub const TITLE_ZERO_BYTES: &str = "Zero Bytes";

/// Returns all possible size bin [`GroupKey`]s in priority order.
#[must_use]
pub fn all_size_group_keys() -> Vec<GroupKey> {
    vec![
        GroupKey::new(PRIORITY_FOLDERS, ID_FOLDERS, TITLE_FOLDERS),
        GroupKey::new(PRIORITY_OVER_10_GB, ID_OVER_10_GB, TITLE_OVER_10_GB),
        GroupKey::new(PRIORITY_1_TO_10_GB, ID_1_TO_10_GB, TITLE_1_TO_10_GB),
        GroupKey::new(
            PRIORITY_100_MB_TO_1_GB,
            ID_100_MB_TO_1_GB,
            TITLE_100_MB_TO_1_GB,
        ),
        GroupKey::new(PRIORITY_1_TO_100_MB, ID_1_TO_100_MB, TITLE_1_TO_100_MB),
        GroupKey::new(PRIORITY_UNDER_1_MB, ID_UNDER_1_MB, TITLE_UNDER_1_MB),
        GroupKey::new(PRIORITY_ZERO_BYTES, ID_ZERO_BYTES, TITLE_ZERO_BYTES),
    ]
}

/// Classifies a [`FileItem`] into its corresponding byte size [`GroupKey`].
///
/// Directories are always bucketed into the `Folders` category.
#[must_use]
pub fn classify_size(item: &FileItem) -> GroupKey {
    if item.is_dir || item.metadata.mime_type == "inode/directory" {
        return GroupKey::new(PRIORITY_FOLDERS, ID_FOLDERS, TITLE_FOLDERS);
    }

    classify_byte_size(item.metadata.size_bytes)
}

/// Classifies a raw byte count into a size bin [`GroupKey`].
#[must_use]
pub fn classify_byte_size(size_bytes: u64) -> GroupKey {
    if size_bytes >= BYTES_10_GB {
        GroupKey::new(PRIORITY_OVER_10_GB, ID_OVER_10_GB, TITLE_OVER_10_GB)
    } else if size_bytes >= BYTES_1_GB {
        GroupKey::new(PRIORITY_1_TO_10_GB, ID_1_TO_10_GB, TITLE_1_TO_10_GB)
    } else if size_bytes >= BYTES_100_MB {
        GroupKey::new(
            PRIORITY_100_MB_TO_1_GB,
            ID_100_MB_TO_1_GB,
            TITLE_100_MB_TO_1_GB,
        )
    } else if size_bytes >= BYTES_1_MB {
        GroupKey::new(PRIORITY_1_TO_100_MB, ID_1_TO_100_MB, TITLE_1_TO_100_MB)
    } else if size_bytes > 0 {
        GroupKey::new(PRIORITY_UNDER_1_MB, ID_UNDER_1_MB, TITLE_UNDER_1_MB)
    } else {
        GroupKey::new(PRIORITY_ZERO_BYTES, ID_ZERO_BYTES, TITLE_ZERO_BYTES)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;

    use shared::models::FileMetadata;

    use super::*;

    fn create_item_with_size(size: u64, is_dir: bool) -> FileItem {
        let meta = FileMetadata {
            size_bytes: size,
            modified: None,
            accessed: None,
            created: None,
            permissions: 0o644,
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
        FileItem::new(
            PathBuf::from("/test"),
            "test",
            is_dir,
            false,
            None,
            meta,
            Vec::new(),
        )
    }

    #[test]
    fn test_size_binning() {
        let dir = create_item_with_size(10 * BYTES_1_GB, true);
        assert_eq!(classify_size(&dir).id, ID_FOLDERS);

        let zero = create_item_with_size(0, false);
        assert_eq!(classify_size(&zero).id, ID_ZERO_BYTES);

        let small = create_item_with_size(500 * 1024, false);
        assert_eq!(classify_size(&small).id, ID_UNDER_1_MB);

        let medium = create_item_with_size(50 * BYTES_1_MB, false);
        assert_eq!(classify_size(&medium).id, ID_1_TO_100_MB);

        let large = create_item_with_size(500 * BYTES_1_MB, false);
        assert_eq!(classify_size(&large).id, ID_100_MB_TO_1_GB);

        let very_large = create_item_with_size(4 * BYTES_1_GB, false);
        assert_eq!(classify_size(&very_large).id, ID_1_TO_10_GB);

        let massive = create_item_with_size(15 * BYTES_1_GB, false);
        assert_eq!(classify_size(&massive).id, ID_OVER_10_GB);
    }
}
