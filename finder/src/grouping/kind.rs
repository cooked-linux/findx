//! MIME and filesystem classification into categorical kind groups.

use shared::models::FileItem;

use super::models::GroupKey;

/// Priority and identifier constants for Kind categories.
pub const PRIORITY_FOLDERS: u32 = 0;
pub const PRIORITY_APPLICATIONS: u32 = 1;
pub const PRIORITY_IMAGES: u32 = 2;
pub const PRIORITY_MOVIES: u32 = 3;
pub const PRIORITY_AUDIO: u32 = 4;
pub const PRIORITY_PDF: u32 = 5;
pub const PRIORITY_PLAIN_TEXT: u32 = 6;
pub const PRIORITY_DEVELOPER_SOURCE: u32 = 7;
pub const PRIORITY_ARCHIVES: u32 = 8;
pub const PRIORITY_OTHER_DOCUMENTS: u32 = 9;

pub const ID_FOLDERS: &str = "folders";
pub const ID_APPLICATIONS: &str = "applications";
pub const ID_IMAGES: &str = "images";
pub const ID_MOVIES: &str = "movies";
pub const ID_AUDIO: &str = "audio";
pub const ID_PDF: &str = "pdf";
pub const ID_PLAIN_TEXT: &str = "plain_text";
pub const ID_DEVELOPER_SOURCE: &str = "developer_source";
pub const ID_ARCHIVES: &str = "archives";
pub const ID_OTHER_DOCUMENTS: &str = "other_documents";

pub const TITLE_FOLDERS: &str = "Folders";
pub const TITLE_APPLICATIONS: &str = "Applications & Binaries";
pub const TITLE_IMAGES: &str = "Images";
pub const TITLE_MOVIES: &str = "Movies & Video";
pub const TITLE_AUDIO: &str = "Audio & Music";
pub const TITLE_PDF: &str = "PDF Documents";
pub const TITLE_PLAIN_TEXT: &str = "Plain Text";
pub const TITLE_DEVELOPER_SOURCE: &str = "Developer Source";
pub const TITLE_ARCHIVES: &str = "Archives";
pub const TITLE_OTHER_DOCUMENTS: &str = "Other Documents";

/// Returns all possible Kind category [`GroupKey`]s in priority order.
#[must_use]
pub fn all_kind_group_keys() -> Vec<GroupKey> {
    vec![
        GroupKey::new(PRIORITY_FOLDERS, ID_FOLDERS, TITLE_FOLDERS),
        GroupKey::new(PRIORITY_APPLICATIONS, ID_APPLICATIONS, TITLE_APPLICATIONS),
        GroupKey::new(PRIORITY_IMAGES, ID_IMAGES, TITLE_IMAGES),
        GroupKey::new(PRIORITY_MOVIES, ID_MOVIES, TITLE_MOVIES),
        GroupKey::new(PRIORITY_AUDIO, ID_AUDIO, TITLE_AUDIO),
        GroupKey::new(PRIORITY_PDF, ID_PDF, TITLE_PDF),
        GroupKey::new(PRIORITY_PLAIN_TEXT, ID_PLAIN_TEXT, TITLE_PLAIN_TEXT),
        GroupKey::new(
            PRIORITY_DEVELOPER_SOURCE,
            ID_DEVELOPER_SOURCE,
            TITLE_DEVELOPER_SOURCE,
        ),
        GroupKey::new(PRIORITY_ARCHIVES, ID_ARCHIVES, TITLE_ARCHIVES),
        GroupKey::new(
            PRIORITY_OTHER_DOCUMENTS,
            ID_OTHER_DOCUMENTS,
            TITLE_OTHER_DOCUMENTS,
        ),
    ]
}

/// Classifies a [`FileItem`] into its corresponding Kind [`GroupKey`].
#[must_use]
pub fn classify_kind(item: &FileItem) -> GroupKey {
    // 1. Folders / Directories
    if item.is_dir || item.metadata.mime_type == "inode/directory" {
        return GroupKey::new(PRIORITY_FOLDERS, ID_FOLDERS, TITLE_FOLDERS);
    }

    let mime = item.metadata.mime_type.to_ascii_lowercase();
    let ext = item
        .extension()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let file_name = item.file_name.to_ascii_lowercase();

    // 2. Applications & Binaries
    if is_application_or_binary(&mime, &ext, item.metadata.is_executable) {
        return GroupKey::new(PRIORITY_APPLICATIONS, ID_APPLICATIONS, TITLE_APPLICATIONS);
    }

    // 3. Images
    if mime.starts_with("image/") || is_image_extension(&ext) {
        return GroupKey::new(PRIORITY_IMAGES, ID_IMAGES, TITLE_IMAGES);
    }

    // 4. Movies & Video
    if mime.starts_with("video/") || is_video_extension(&ext) {
        return GroupKey::new(PRIORITY_MOVIES, ID_MOVIES, TITLE_MOVIES);
    }

    // 5. Audio & Music
    if mime.starts_with("audio/") || is_audio_extension(&ext) {
        return GroupKey::new(PRIORITY_AUDIO, ID_AUDIO, TITLE_AUDIO);
    }

    // 6. PDF Documents
    if mime == "application/pdf" || ext == "pdf" {
        return GroupKey::new(PRIORITY_PDF, ID_PDF, TITLE_PDF);
    }

    // 7. Plain Text
    if is_plain_text(&mime, &ext) {
        return GroupKey::new(PRIORITY_PLAIN_TEXT, ID_PLAIN_TEXT, TITLE_PLAIN_TEXT);
    }

    // 8. Developer Source
    if is_developer_source(&mime, &ext) {
        return GroupKey::new(
            PRIORITY_DEVELOPER_SOURCE,
            ID_DEVELOPER_SOURCE,
            TITLE_DEVELOPER_SOURCE,
        );
    }

    // 9. Archives
    if is_archive(&mime, &ext, &file_name) {
        return GroupKey::new(PRIORITY_ARCHIVES, ID_ARCHIVES, TITLE_ARCHIVES);
    }

    // 10. Other Documents (Fallback)
    GroupKey::new(
        PRIORITY_OTHER_DOCUMENTS,
        ID_OTHER_DOCUMENTS,
        TITLE_OTHER_DOCUMENTS,
    )
}

fn is_application_or_binary(mime: &str, ext: &str, is_executable: bool) -> bool {
    if matches!(
        mime,
        "application/x-executable"
            | "application/x-pie-executable"
            | "application/x-sharedlib"
            | "application/x-desktop"
            | "application/vnd.debian.binary-package"
            | "application/x-rpm"
            | "application/x-appimage"
    ) {
        return true;
    }

    if matches!(
        ext,
        "desktop" | "appimage" | "exe" | "bin" | "so" | "dll" | "dylib" | "deb" | "rpm"
    ) {
        return true;
    }

    // Mark as binary application only if explicitly executable and not a source script/text
    if is_executable && !matches!(ext, "sh" | "bash" | "zsh" | "py" | "pl" | "rb" | "txt") {
        return mime.contains("application/") && !mime.contains("json") && !mime.contains("xml");
    }

    false
}

fn is_image_extension(ext: &str) -> bool {
    matches!(
        ext,
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "bmp"
            | "ico"
            | "tiff"
            | "tif"
            | "avif"
            | "heic"
            | "heif"
            | "raw"
            | "cr2"
            | "nef"
    )
}

fn is_video_extension(ext: &str) -> bool {
    matches!(
        ext,
        "mp4"
            | "mkv"
            | "avi"
            | "mov"
            | "wmv"
            | "flv"
            | "webm"
            | "m4v"
            | "3gp"
            | "ts"
            | "mpg"
            | "mpeg"
    )
}

fn is_audio_extension(ext: &str) -> bool {
    matches!(
        ext,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "wma" | "opus" | "aiff" | "mid" | "midi"
    )
}

fn is_plain_text(mime: &str, ext: &str) -> bool {
    if matches!(mime, "text/plain" | "text/markdown") {
        // Exclude specific code extensions that might be flagged text/plain by mime_guess
        if is_code_extension(ext) {
            return false;
        }
        return true;
    }

    matches!(ext, "txt" | "text" | "md" | "markdown" | "rst" | "log")
}

fn is_developer_source(mime: &str, ext: &str) -> bool {
    if mime.starts_with("text/x-") {
        return true;
    }

    if matches!(
        mime,
        "application/json"
            | "application/javascript"
            | "application/typescript"
            | "application/xml"
            | "application/x-sh"
            | "application/x-shellscript"
            | "text/javascript"
            | "text/css"
            | "text/html"
            | "text/xml"
            | "application/sql"
            | "application/toml"
            | "application/yaml"
    ) {
        return true;
    }

    is_code_extension(ext)
}

fn is_code_extension(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "c"
            | "cpp"
            | "cc"
            | "cxx"
            | "h"
            | "hpp"
            | "py"
            | "pyw"
            | "js"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "jsx"
            | "go"
            | "java"
            | "kt"
            | "kts"
            | "scala"
            | "swift"
            | "rb"
            | "php"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "sql"
            | "graphql"
            | "gql"
            | "proto"
            | "diff"
            | "patch"
            | "vue"
            | "svelte"
            | "lua"
            | "zig"
            | "nim"
            | "dart"
            | "r"
            | "jl"
    )
}

fn is_archive(mime: &str, ext: &str, file_name: &str) -> bool {
    if matches!(
        mime,
        "application/zip"
            | "application/x-tar"
            | "application/gzip"
            | "application/x-gzip"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/x-7z-compressed"
            | "application/zstd"
            | "application/x-rar-compressed"
            | "application/x-archive"
            | "application/vnd.rar"
    ) {
        return true;
    }

    if file_name.ends_with(".tar.gz")
        || file_name.ends_with(".tar.xz")
        || file_name.ends_with(".tar.bz2")
        || file_name.ends_with(".tar.zst")
        || file_name.ends_with(".tar.zstd")
    {
        return true;
    }

    matches!(
        ext,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "tbz2" | "xz" | "txz" | "7z" | "zst" | "rar" | "iso"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use shared::models::FileMetadata;

    use super::*;

    fn create_test_file(
        path: &str,
        is_dir: bool,
        mime: &str,
        size: u64,
        is_exec: bool,
    ) -> FileItem {
        let meta = FileMetadata {
            size_bytes: size,
            modified: Some(SystemTime::UNIX_EPOCH),
            accessed: None,
            created: None,
            permissions: if is_exec { 0o755 } else { 0o644 },
            inode: 1,
            device_id: 1,
            mime_type: mime.to_string(),
            is_hidden: false,
            is_executable: is_exec,
        };

        let p = PathBuf::from(path);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        FileItem::new(p, name, is_dir, false, None, meta, Vec::new())
    }

    #[test]
    fn test_classify_folders() {
        let dir = create_test_file("/home/user/Documents", true, "inode/directory", 4096, true);
        let key = classify_kind(&dir);
        assert_eq!(key.priority, PRIORITY_FOLDERS);
        assert_eq!(key.id, ID_FOLDERS);
    }

    #[test]
    fn test_classify_media() {
        let img = create_test_file("/tmp/pic.png", false, "image/png", 1024, false);
        assert_eq!(classify_kind(&img).id, ID_IMAGES);

        let vid = create_test_file("/tmp/movie.mp4", false, "video/mp4", 1024 * 1024, false);
        assert_eq!(classify_kind(&vid).id, ID_MOVIES);

        let aud = create_test_file("/tmp/song.flac", false, "audio/flac", 5000, false);
        assert_eq!(classify_kind(&aud).id, ID_AUDIO);
    }

    #[test]
    fn test_classify_documents() {
        let pdf = create_test_file("/tmp/doc.pdf", false, "application/pdf", 2048, false);
        assert_eq!(classify_kind(&pdf).id, ID_PDF);

        let txt = create_test_file("/tmp/notes.txt", false, "text/plain", 100, false);
        assert_eq!(classify_kind(&txt).id, ID_PLAIN_TEXT);

        let md = create_test_file("/tmp/README.md", false, "text/markdown", 200, false);
        assert_eq!(classify_kind(&md).id, ID_PLAIN_TEXT);
    }

    #[test]
    fn test_classify_developer_source() {
        let rs = create_test_file("/tmp/main.rs", false, "text/x-rust", 500, false);
        assert_eq!(classify_kind(&rs).id, ID_DEVELOPER_SOURCE);

        let py = create_test_file("/tmp/app.py", false, "text/plain", 300, false);
        assert_eq!(classify_kind(&py).id, ID_DEVELOPER_SOURCE);

        let json = create_test_file("/tmp/conf.json", false, "application/json", 400, false);
        assert_eq!(classify_kind(&json).id, ID_DEVELOPER_SOURCE);
    }

    #[test]
    fn test_classify_archives() {
        let zip = create_test_file("/tmp/bundle.zip", false, "application/zip", 8000, false);
        assert_eq!(classify_kind(&zip).id, ID_ARCHIVES);

        let targz = create_test_file("/tmp/dist.tar.gz", false, "application/gzip", 12000, false);
        assert_eq!(classify_kind(&targz).id, ID_ARCHIVES);
    }

    #[test]
    fn test_classify_applications() {
        let app = create_test_file(
            "/usr/bin/findx",
            false,
            "application/x-executable",
            20000,
            true,
        );
        assert_eq!(classify_kind(&app).id, ID_APPLICATIONS);

        let dtop = create_test_file(
            "/usr/share/applications/findx.desktop",
            false,
            "application/x-desktop",
            500,
            false,
        );
        assert_eq!(classify_kind(&dtop).id, ID_APPLICATIONS);
    }

    #[test]
    fn test_classify_other() {
        let epub = create_test_file(
            "/tmp/book.epub",
            false,
            "application/epub+zip",
            50000,
            false,
        );
        assert_eq!(classify_kind(&epub).id, ID_OTHER_DOCUMENTS);
    }
}
