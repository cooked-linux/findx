//! Domain models representing files, metadata, tagging, MIME categories, search, and batch operations.

pub mod file;
pub mod mime;
pub mod operation;
pub mod search;
pub mod tag;

pub use file::{FileItem, FileMetadata};
pub use mime::MimeCategory;
pub use operation::{BatchOperation, OperationKind, OperationState, ProgressReport, TransferItem};
pub use search::{
    AggregatedSearchResults, CategorySection, SearchCategory, SearchPayload, SearchRecord,
};
pub use tag::{FileTag, TagColor};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn test_all_models_serde_roundtrip() {
        // 1. FileItem & FileMetadata
        let metadata = FileMetadata::new(
            4096,
            Some(SystemTime::UNIX_EPOCH),
            Some(SystemTime::UNIX_EPOCH),
            Some(SystemTime::UNIX_EPOCH),
            0o755,
            42,
            1,
            "text/rust",
            false,
            true,
        );
        let tag = FileTag::new("Core", Some(TagColor::Red));
        let file_item = FileItem::new(
            PathBuf::from("/home/user/main.rs"),
            "main.rs",
            false,
            false,
            None,
            metadata,
            vec![tag.clone()],
        );

        let json = serde_json::to_string(&file_item).unwrap();
        let deserialized_file: FileItem = serde_json::from_str(&json).unwrap();
        assert_eq!(file_item, deserialized_file);

        // 2. TagColor & FileTag
        for color in &TagColor::ALL {
            let color_json = serde_json::to_string(color).unwrap();
            let deserialized_color: TagColor = serde_json::from_str(&color_json).unwrap();
            assert_eq!(*color, deserialized_color);
        }
        let tag_json = serde_json::to_string(&tag).unwrap();
        let deserialized_tag: FileTag = serde_json::from_str(&tag_json).unwrap();
        assert_eq!(tag, deserialized_tag);

        // 3. MimeCategory
        let categories = [
            MimeCategory::Folder,
            MimeCategory::Document,
            MimeCategory::SourceCode,
            MimeCategory::Image,
            MimeCategory::Audio,
            MimeCategory::Video,
            MimeCategory::Archive,
            MimeCategory::Executable,
            MimeCategory::Font,
            MimeCategory::Unknown,
        ];
        for cat in &categories {
            let cat_json = serde_json::to_string(cat).unwrap();
            let deserialized_cat: MimeCategory = serde_json::from_str(&cat_json).unwrap();
            assert_eq!(*cat, deserialized_cat);
        }

        // 4. SearchRecord & SearchPayload
        let search_record = SearchRecord::new(
            "app:firefox",
            "Firefox Web Browser",
            Some("Browse the World Wide Web".to_string()),
            SearchCategory::Applications,
            Some("firefox".to_string()),
            950.0,
            SearchPayload::Application {
                desktop_id: "firefox.desktop".to_string(),
                exec: "firefox %u".to_string(),
            },
        );
        let search_json = serde_json::to_string(&search_record).unwrap();
        let deserialized_search: SearchRecord = serde_json::from_str(&search_json).unwrap();
        assert_eq!(search_record, deserialized_search);

        // 5. BatchOperation & ProgressReport
        let batch_op = BatchOperation::new(
            "op-101",
            OperationKind::Copy,
            vec![PathBuf::from("/a/b"), PathBuf::from("/a/c")],
            Some(PathBuf::from("/target")),
        );
        let op_json = serde_json::to_string(&batch_op).unwrap();
        let deserialized_op: BatchOperation = serde_json::from_str(&op_json).unwrap();
        assert_eq!(batch_op, deserialized_op);

        let progress = ProgressReport::new(
            "op-101",
            2,
            5,
            2048,
            4096,
            1024.0,
            Some(2.0),
            Some(PathBuf::from("/a/b")),
        );
        let progress_json = serde_json::to_string(&progress).unwrap();
        let deserialized_prog: ProgressReport = serde_json::from_str(&progress_json).unwrap();
        assert_eq!(progress, deserialized_prog);
    }

    #[test]
    fn test_tag_color_parsing_from_xattr_strings() {
        // macOS format: "TagName\nColorIndex"
        let macos_raw = "Work\n6\nImportant\n1\nPersonal\n3";
        let tags = FileTag::parse_xdg_string(macos_raw);
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], FileTag::new("Work", Some(TagColor::Red)));
        assert_eq!(tags[1], FileTag::new("Important", Some(TagColor::Gray)));
        assert_eq!(tags[2], FileTag::new("Personal", Some(TagColor::Purple)));

        // XDG format: comma separated
        let xdg_comma = "Work, Green, Project";
        let tags2 = FileTag::parse_xdg_string(xdg_comma);
        assert_eq!(tags2.len(), 3);
        assert_eq!(tags2[0], FileTag::new("Work", None));
        assert_eq!(tags2[1], FileTag::new("Green", Some(TagColor::Green)));
        assert_eq!(tags2[2], FileTag::new("Project", None));

        // Color string direct parsing (names, indices, hex codes)
        assert_eq!("Red".parse::<TagColor>().unwrap(), TagColor::Red);
        assert_eq!("orange".parse::<TagColor>().unwrap(), TagColor::Orange);
        assert_eq!("YELLOW".parse::<TagColor>().unwrap(), TagColor::Yellow);
        assert_eq!("green".parse::<TagColor>().unwrap(), TagColor::Green);
        assert_eq!("blue".parse::<TagColor>().unwrap(), TagColor::Blue);
        assert_eq!("purple".parse::<TagColor>().unwrap(), TagColor::Purple);
        assert_eq!("gray".parse::<TagColor>().unwrap(), TagColor::Gray);
        assert_eq!("grey".parse::<TagColor>().unwrap(), TagColor::Gray);

        // Numeric index
        assert_eq!("1".parse::<TagColor>().unwrap(), TagColor::Gray);
        assert_eq!("2".parse::<TagColor>().unwrap(), TagColor::Green);
        assert_eq!("3".parse::<TagColor>().unwrap(), TagColor::Purple);
        assert_eq!("4".parse::<TagColor>().unwrap(), TagColor::Blue);
        assert_eq!("5".parse::<TagColor>().unwrap(), TagColor::Yellow);
        assert_eq!("6".parse::<TagColor>().unwrap(), TagColor::Red);
        assert_eq!("7".parse::<TagColor>().unwrap(), TagColor::Orange);

        // Hex codes
        assert_eq!("#8e8e93".parse::<TagColor>().unwrap(), TagColor::Gray);
        assert_eq!("#34c759".parse::<TagColor>().unwrap(), TagColor::Green);
        assert_eq!("#af52de".parse::<TagColor>().unwrap(), TagColor::Purple);
        assert_eq!("#007aff".parse::<TagColor>().unwrap(), TagColor::Blue);
        assert_eq!("#ffcc00".parse::<TagColor>().unwrap(), TagColor::Yellow);
        assert_eq!("#ff3b30".parse::<TagColor>().unwrap(), TagColor::Red);
        assert_eq!("#ff9500".parse::<TagColor>().unwrap(), TagColor::Orange);
    }

    #[test]
    fn test_mime_category_heuristics() {
        assert_eq!(
            MimeCategory::from_mime_str("inode/directory"),
            MimeCategory::Folder
        );
        assert_eq!(
            MimeCategory::from_mime_str("application/pdf"),
            MimeCategory::Document
        );
        assert_eq!(
            MimeCategory::from_mime_str("text/markdown"),
            MimeCategory::Document
        );
        assert_eq!(
            MimeCategory::from_mime_str("text/x-rust"),
            MimeCategory::SourceCode
        );
        assert_eq!(
            MimeCategory::from_mime_str("text/rust"),
            MimeCategory::SourceCode
        );
        assert_eq!(
            MimeCategory::from_mime_str("application/json"),
            MimeCategory::SourceCode
        );
        assert_eq!(
            MimeCategory::from_mime_str("image/svg+xml"),
            MimeCategory::Image
        );
        assert_eq!(
            MimeCategory::from_mime_str("audio/flac"),
            MimeCategory::Audio
        );
        assert_eq!(
            MimeCategory::from_mime_str("video/webm"),
            MimeCategory::Video
        );
        assert_eq!(
            MimeCategory::from_mime_str("application/zip"),
            MimeCategory::Archive
        );
        assert_eq!(
            MimeCategory::from_mime_str("application/x-executable"),
            MimeCategory::Executable
        );
        assert_eq!(
            MimeCategory::from_mime_str("font/woff2"),
            MimeCategory::Font
        );
        assert_eq!(
            MimeCategory::from_mime_str("unknown/binary"),
            MimeCategory::Unknown
        );

        // Path-based tests
        assert_eq!(
            MimeCategory::from_path(Path::new("Cargo.toml")),
            MimeCategory::SourceCode
        );
        assert_eq!(
            MimeCategory::from_path(Path::new("build.rs")),
            MimeCategory::SourceCode
        );
        assert_eq!(
            MimeCategory::from_path(Path::new("document.odt")),
            MimeCategory::Document
        );
        assert_eq!(
            MimeCategory::from_path(Path::new("archive.tar.gz")),
            MimeCategory::Archive
        );
        assert_eq!(
            MimeCategory::from_path(Path::new("app.AppImage")),
            MimeCategory::Executable
        );
        assert_eq!(
            MimeCategory::from_path(Path::new("sample.wav")),
            MimeCategory::Audio
        );
    }

    #[test]
    fn test_batch_operation_validation() {
        let op_ok = BatchOperation::new(
            "op-1",
            OperationKind::Move,
            vec![PathBuf::from("/src/file.txt")],
            Some(PathBuf::from("/dst")),
        );
        assert!(op_ok.validate().is_ok());

        let op_no_dest = BatchOperation::new(
            "op-2",
            OperationKind::Move,
            vec![PathBuf::from("/src/file.txt")],
            None,
        );
        assert!(op_no_dest.validate().is_err());

        let op_trash = BatchOperation::new(
            "op-3",
            OperationKind::Trash,
            vec![PathBuf::from("/src/file.txt")],
            None,
        );
        assert!(op_trash.validate().is_ok());

        let op_empty = BatchOperation::new("op-4", OperationKind::Trash, vec![], None);
        assert!(op_empty.validate().is_err());
    }
}
