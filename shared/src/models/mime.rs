//! MIME category classification models.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use crate::errors::ModelError;

/// High-level MIME category classification for grouping files in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MimeCategory {
    /// Directory or folder.
    Folder,
    /// Human-readable document (PDF, Office, Plaintext, Markdown).
    Document,
    /// Software source code or markup.
    SourceCode,
    /// Image formats (PNG, JPEG, SVG, `WebM`, etc.).
    Image,
    /// Audio files (MP3, FLAC, WAV, AAC, etc.).
    Audio,
    /// Video formats (MP4, MKV, `WebM`, AVI, etc.).
    Video,
    /// Compressed archives (ZIP, TAR, GZ, 7z, etc.).
    Archive,
    /// Executable binaries or scripts.
    Executable,
    /// Typography and font files (TTF, OTF, WOFF, etc.).
    Font,
    /// Unclassified or unrecognized MIME type.
    Unknown,
}

impl MimeCategory {
    /// Maps an explicit MIME type string (e.g. `"text/rust"`, `"application/pdf"`)
    /// into a high-level [`MimeCategory`].
    #[must_use]
    pub fn from_mime_str(mime: &str) -> Self {
        let trimmed = mime.trim().to_ascii_lowercase();
        let mime_clean = trimmed.split(';').next().unwrap_or(&trimmed).trim();

        if mime_clean.is_empty() {
            return Self::Unknown;
        }

        // 1. Folders / Directories
        if mime_clean == "inode/directory" || mime_clean == "inode/folder" {
            return Self::Folder;
        }

        // 2. High-level media families
        if mime_clean.starts_with("image/") {
            return Self::Image;
        }
        if mime_clean.starts_with("audio/") {
            return Self::Audio;
        }
        if mime_clean.starts_with("video/") {
            return Self::Video;
        }
        if mime_clean.starts_with("font/") {
            return Self::Font;
        }

        // 3. Source code & scripts
        if Self::is_source_code_mime(mime_clean) {
            return Self::SourceCode;
        }

        // 4. Archives
        if Self::is_archive_mime(mime_clean) {
            return Self::Archive;
        }

        // 5. Executables & system binaries
        if Self::is_executable_mime(mime_clean) {
            return Self::Executable;
        }

        // 6. Fonts (vendor/application prefixes)
        if Self::is_font_mime(mime_clean) {
            return Self::Font;
        }

        // 7. Documents
        if Self::is_document_mime(mime_clean) {
            return Self::Document;
        }

        Self::Unknown
    }

    /// Determines [`MimeCategory`] from a filesystem path, consulting file extension
    /// and standard MIME sniffing database.
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        // Fast path for explicit extension heuristics
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            match ext_lower.as_str() {
                "rs" | "go" | "py" | "c" | "cpp" | "h" | "hpp" | "js" | "ts" | "jsx" | "tsx"
                | "java" | "kt" | "swift" | "rb" | "php" | "sh" | "bash" | "zsh" | "fish"
                | "lua" | "zig" | "nim" | "toml" | "yaml" | "yml" | "json" | "xml" | "sql"
                | "css" | "scss" | "sass" | "html" | "htm" => return Self::SourceCode,
                "md" | "markdown" | "txt" | "pdf" | "doc" | "docx" | "odt" | "xls" | "xlsx"
                | "ods" | "ppt" | "pptx" | "odp" | "rtf" | "csv" | "tsv" | "epub" => {
                    return Self::Document;
                }
                "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "tgz" => {
                    return Self::Archive;
                }
                "exe" | "bin" | "elf" | "appimage" => return Self::Executable,
                "ttf" | "otf" | "woff" | "woff2" => return Self::Font,
                _ => {}
            }
        }

        // Fallback to mime_guess
        mime_guess::from_path(path)
            .first_raw()
            .map_or(Self::Unknown, Self::from_mime_str)
    }

    /// Returns a static string representation of the category.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::Document => "Document",
            Self::SourceCode => "Source Code",
            Self::Image => "Image",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Archive => "Archive",
            Self::Executable => "Executable",
            Self::Font => "Font",
            Self::Unknown => "Unknown",
        }
    }

    /// Returns the standard Freedesktop icon theme name corresponding to this category.
    #[must_use]
    pub const fn icon_name(&self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Document => "x-office-document",
            Self::SourceCode => "text-x-script",
            Self::Image => "image-x-generic",
            Self::Audio => "audio-x-generic",
            Self::Video => "video-x-generic",
            Self::Archive => "package-x-generic",
            Self::Executable => "application-x-executable",
            Self::Font => "font-x-generic",
            Self::Unknown => "unknown",
        }
    }

    fn is_source_code_mime(mime: &str) -> bool {
        if mime.starts_with("text/x-") {
            return true;
        }
        matches!(
            mime,
            "text/rust"
                | "text/javascript"
                | "application/javascript"
                | "application/x-javascript"
                | "application/typescript"
                | "application/json"
                | "application/xml"
                | "text/xml"
                | "text/css"
                | "application/toml"
                | "application/x-toml"
                | "application/yaml"
                | "application/x-yaml"
                | "text/yaml"
                | "application/sql"
                | "application/x-shellscript"
                | "application/x-python-code"
        )
    }

    fn is_archive_mime(mime: &str) -> bool {
        matches!(
            mime,
            "application/zip"
                | "application/x-tar"
                | "application/gzip"
                | "application/x-gzip"
                | "application/x-bzip"
                | "application/x-bzip2"
                | "application/x-xz"
                | "application/x-7z-compressed"
                | "application/x-rar"
                | "application/vnd.rar"
                | "application/x-rar-compressed"
                | "application/zstd"
                | "application/x-cpio"
                | "application/x-iso9660-image"
                | "application/x-archive"
                | "application/x-debian-package"
                | "application/x-redhat-package-manager"
        )
    }

    fn is_executable_mime(mime: &str) -> bool {
        matches!(
            mime,
            "application/x-executable"
                | "application/x-sharedlib"
                | "application/x-pie-executable"
                | "application/x-mach-binary"
                | "application/vnd.microsoft.portable-executable"
                | "application/x-msdownload"
                | "application/x-msdos-program"
                | "application/x-elf"
                | "application/x-object"
        )
    }

    fn is_font_mime(mime: &str) -> bool {
        matches!(
            mime,
            "application/vnd.ms-opentype"
                | "application/x-font-ttf"
                | "application/x-font-otf"
                | "application/font-woff"
                | "application/font-woff2"
        )
    }

    fn is_document_mime(mime: &str) -> bool {
        if mime.starts_with("text/") {
            return true;
        }
        if mime.starts_with("application/vnd.openxmlformats-")
            || mime.starts_with("application/vnd.oasis.opendocument.")
            || mime.starts_with("application/vnd.ms-")
        {
            return true;
        }
        matches!(
            mime,
            "application/pdf"
                | "application/postscript"
                | "application/epub+zip"
                | "application/msword"
                | "application/rtf"
        )
    }
}

impl fmt::Display for MimeCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MimeCategory {
    type Err = ModelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "folder" => Ok(Self::Folder),
            "document" => Ok(Self::Document),
            "sourcecode" | "source code" | "source" => Ok(Self::SourceCode),
            "image" => Ok(Self::Image),
            "audio" => Ok(Self::Audio),
            "video" => Ok(Self::Video),
            "archive" => Ok(Self::Archive),
            "executable" => Ok(Self::Executable),
            "font" => Ok(Self::Font),
            "unknown" => Ok(Self::Unknown),
            _ => {
                let inferred = Self::from_mime_str(trimmed);
                if inferred == Self::Unknown {
                    Err(ModelError::invalid_mime_category(s))
                } else {
                    Ok(inferred)
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_from_mime_str_mappings() {
        assert_eq!(MimeCategory::from_mime_str("text/rust"), MimeCategory::SourceCode);
        assert_eq!(MimeCategory::from_mime_str("text/x-rust"), MimeCategory::SourceCode);
        assert_eq!(MimeCategory::from_mime_str("application/javascript"), MimeCategory::SourceCode);
        assert_eq!(MimeCategory::from_mime_str("application/pdf"), MimeCategory::Document);
        assert_eq!(MimeCategory::from_mime_str("text/plain"), MimeCategory::Document);
        assert_eq!(MimeCategory::from_mime_str("image/png"), MimeCategory::Image);
        assert_eq!(MimeCategory::from_mime_str("audio/mpeg"), MimeCategory::Audio);
        assert_eq!(MimeCategory::from_mime_str("video/mp4"), MimeCategory::Video);
        assert_eq!(MimeCategory::from_mime_str("application/zip"), MimeCategory::Archive);
        assert_eq!(MimeCategory::from_mime_str("application/x-executable"), MimeCategory::Executable);
        assert_eq!(MimeCategory::from_mime_str("font/ttf"), MimeCategory::Font);
        assert_eq!(MimeCategory::from_mime_str("inode/directory"), MimeCategory::Folder);
        assert_eq!(MimeCategory::from_mime_str("application/octet-stream"), MimeCategory::Unknown);
    }

    #[test]
    fn test_from_path() {
        assert_eq!(MimeCategory::from_path(Path::new("main.rs")), MimeCategory::SourceCode);
        assert_eq!(MimeCategory::from_path(Path::new("/tmp/test.py")), MimeCategory::SourceCode);
        assert_eq!(MimeCategory::from_path(Path::new("report.pdf")), MimeCategory::Document);
        assert_eq!(MimeCategory::from_path(Path::new("photo.jpg")), MimeCategory::Image);
        assert_eq!(MimeCategory::from_path(Path::new("song.mp3")), MimeCategory::Audio);
        assert_eq!(MimeCategory::from_path(Path::new("clip.mp4")), MimeCategory::Video);
        assert_eq!(MimeCategory::from_path(Path::new("backup.tar.gz")), MimeCategory::Archive);
        assert_eq!(MimeCategory::from_path(Path::new("font.woff2")), MimeCategory::Font);
    }

    #[test]
    fn test_as_str_and_icon() {
        assert_eq!(MimeCategory::Folder.as_str(), "Folder");
        assert_eq!(MimeCategory::Folder.icon_name(), "folder");
        assert_eq!(MimeCategory::SourceCode.icon_name(), "text-x-script");
    }

    #[test]
    fn test_mime_category_from_str() {
        assert_eq!("Document".parse::<MimeCategory>().unwrap(), MimeCategory::Document);
        assert_eq!("source code".parse::<MimeCategory>().unwrap(), MimeCategory::SourceCode);
        assert_eq!("text/rust".parse::<MimeCategory>().unwrap(), MimeCategory::SourceCode);
        assert!("totally-invalid/xyz".parse::<MimeCategory>().is_err());
    }

    #[test]
    fn test_mime_category_serde() {
        let cat = MimeCategory::SourceCode;
        let json = serde_json::to_string(&cat).unwrap();
        let deserialized: MimeCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, deserialized);
    }
}
