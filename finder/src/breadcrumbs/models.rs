//! Breadcrumb path segment and lineage models.

use std::path::{Component, Path, PathBuf};

/// Represents an individual interactive breadcrumb segment button in the path bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSegment {
    /// Display label for the segment (e.g. "Computer", "Home", "src", "main.rs").
    pub name: String,
    /// Logical filesystem path represented by this segment.
    pub path: PathBuf,
    /// Freedesktop symbolic icon name for the segment.
    pub icon_name: String,
    /// Whether this segment is a directory (supporting child navigation & drop ingestion).
    pub is_directory: bool,
    /// Whether this segment is the root / Computer / volume anchor.
    pub is_root: bool,
    /// Whether this segment represents an active leaf item (e.g. a selected file).
    pub is_leaf: bool,
}

impl PathSegment {
    /// Creates a new [`PathSegment`].
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        icon_name: impl Into<String>,
        is_directory: bool,
        is_root: bool,
        is_leaf: bool,
    ) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            icon_name: icon_name.into(),
            is_directory,
            is_root,
            is_leaf,
        }
    }
}

/// Represents the complete lineage of path segments from root to the active location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreadcrumbLineage {
    /// Ordered list of path segments from left (root/volume) to right (active directory or leaf).
    pub segments: Vec<PathSegment>,
    /// Index of the currently active segment (usually the last segment).
    pub active_index: usize,
}

impl BreadcrumbLineage {
    /// Creates a new [`BreadcrumbLineage`] with the specified segments and active index.
    #[must_use]
    pub const fn new(segments: Vec<PathSegment>, active_index: usize) -> Self {
        Self {
            segments,
            active_index,
        }
    }

    /// Resolves breadcrumb lineage from a path without a leaf selection.
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        Self::resolve(path, None)
    }

    /// Resolves breadcrumb lineage from a current directory path and optional selected leaf item.
    ///
    /// Preserves logical path traversal without forcing canonical symlink dereferencing.
    #[must_use]
    pub fn resolve(current_path: &Path, selected_leaf: Option<&Path>) -> Self {
        let (dir_path, leaf_path) = match selected_leaf {
            Some(leaf) if leaf != current_path => (current_path, Some(leaf)),
            _ => {
                // If no selection is provided, check if current_path itself is a leaf file
                if (current_path.extension().is_some() || current_path.is_file())
                    && !current_path.is_dir()
                {
                    current_path
                        .parent()
                        .map_or((current_path, None), |parent| (parent, Some(current_path)))
                } else {
                    (current_path, None)
                }
            }
        };

        let mut segments = Vec::new();
        let mut accumulated = PathBuf::new();

        for component in dir_path.components() {
            match component {
                Component::RootDir => {
                    accumulated = PathBuf::from("/");
                    segments.push(PathSegment {
                        name: "Computer".to_string(),
                        path: PathBuf::from("/"),
                        icon_name: "drive-harddisk-symbolic".to_string(),
                        is_directory: true,
                        is_root: true,
                        is_leaf: false,
                    });
                }
                Component::Normal(os_str) => {
                    let name = os_str.to_string_lossy().to_string();
                    accumulated.push(&name);
                    let seg_path = accumulated.clone();
                    let icon_name = resolve_folder_icon(&seg_path, &name);

                    segments.push(PathSegment {
                        name,
                        path: seg_path,
                        icon_name,
                        is_directory: true,
                        is_root: false,
                        is_leaf: false,
                    });
                }
                Component::CurDir | Component::Prefix(_) | Component::ParentDir => {
                    // Ignore relative components for logical ancestry
                }
            }
        }

        // Ensure at least root Computer segment is present if path was empty or root
        if segments.is_empty() {
            segments.push(PathSegment {
                name: "Computer".to_string(),
                path: PathBuf::from("/"),
                icon_name: "drive-harddisk-symbolic".to_string(),
                is_directory: true,
                is_root: true,
                is_leaf: false,
            });
        }

        // Append leaf item if present
        if let Some(leaf) = leaf_path {
            let leaf_name = leaf.file_name().map_or_else(
                || leaf.to_string_lossy().to_string(),
                |n| n.to_string_lossy().to_string(),
            );
            let is_directory = leaf.is_dir();
            let icon_name = if is_directory {
                resolve_folder_icon(leaf, &leaf_name)
            } else {
                resolve_file_icon(leaf)
            };

            segments.push(PathSegment {
                name: leaf_name,
                path: leaf.to_path_buf(),
                icon_name,
                is_directory,
                is_root: false,
                is_leaf: true,
            });
        }

        let active_index = if segments.is_empty() {
            0
        } else {
            segments.len() - 1
        };

        Self {
            segments,
            active_index,
        }
    }

    /// Returns the active segment, if any.
    #[must_use]
    pub fn active_segment(&self) -> Option<&PathSegment> {
        self.segments.get(self.active_index)
    }

    /// Returns the number of segments in this lineage.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.segments.len()
    }

    /// Returns `true` if the lineage has no segments.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// Resolves the appropriate icon name for a directory segment based on name and path.
#[must_use]
pub fn resolve_folder_icon(path: &Path, name: &str) -> String {
    if let Ok(home) = std::env::var("HOME")
        && path == Path::new(&home)
    {
        return "user-home-symbolic".to_string();
    }
    match name.to_ascii_lowercase().as_str() {
        "home" => "user-home-symbolic".to_string(),
        "desktop" => "user-desktop-symbolic".to_string(),
        "documents" => "folder-documents-symbolic".to_string(),
        "downloads" => "folder-download-symbolic".to_string(),
        "music" => "folder-music-symbolic".to_string(),
        "pictures" => "folder-pictures-symbolic".to_string(),
        "videos" => "folder-videos-symbolic".to_string(),
        "trash" => "user-trash-symbolic".to_string(),
        _ => "folder-symbolic".to_string(),
    }
}

/// Resolves the appropriate icon name for a file segment based on MIME type or extension.
#[must_use]
pub fn resolve_file_icon(path: &Path) -> String {
    if let Ok(mime) = shared::mime::detect_mime_type(path) {
        if mime.starts_with("image/") {
            return "image-x-generic-symbolic".to_string();
        }
        if mime.starts_with("audio/") {
            return "audio-x-generic-symbolic".to_string();
        }
        if mime.starts_with("video/") {
            return "video-x-generic-symbolic".to_string();
        }
        if mime == "application/pdf" {
            return "application-pdf-symbolic".to_string();
        }
        if matches!(
            mime.as_str(),
            "application/zip" | "application/x-tar" | "application/gzip"
        ) {
            return "package-x-generic-symbolic".to_string();
        }
    }
    path.extension().and_then(|e| e.to_str()).map_or_else(
        || "text-x-generic-symbolic".to_string(),
        |ext| match ext.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "svg" | "webp" | "gif" => {
                "image-x-generic-symbolic".to_string()
            }
            "mp3" | "wav" | "flac" | "ogg" => "audio-x-generic-symbolic".to_string(),
            "mp4" | "mkv" | "webm" | "avi" => "video-x-generic-symbolic".to_string(),
            "pdf" => "application-pdf-symbolic".to_string(),
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" => "package-x-generic-symbolic".to_string(),
            _ => "text-x-generic-symbolic".to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lineage_root() {
        let lineage = BreadcrumbLineage::from_path(Path::new("/"));
        assert_eq!(lineage.segments.len(), 1);
        assert_eq!(lineage.active_index, 0);

        let root = &lineage.segments[0];
        assert_eq!(root.name, "Computer");
        assert_eq!(root.path, PathBuf::from("/"));
        assert!(root.is_root);
        assert!(root.is_directory);
        assert!(!root.is_leaf);
    }

    #[test]
    fn test_lineage_directory() {
        let lineage = BreadcrumbLineage::from_path(Path::new("/home/user"));
        assert_eq!(lineage.segments.len(), 3);
        assert_eq!(lineage.active_index, 2);

        assert_eq!(lineage.segments[0].name, "Computer");
        assert_eq!(lineage.segments[0].path, PathBuf::from("/"));
        assert!(lineage.segments[0].is_root);

        assert_eq!(lineage.segments[1].name, "home");
        assert_eq!(lineage.segments[1].path, PathBuf::from("/home"));
        assert!(!lineage.segments[1].is_root);
        assert!(lineage.segments[1].is_directory);
        assert!(!lineage.segments[1].is_leaf);

        assert_eq!(lineage.segments[2].name, "user");
        assert_eq!(lineage.segments[2].path, PathBuf::from("/home/user"));
        assert!(!lineage.segments[2].is_root);
        assert!(lineage.segments[2].is_directory);
        assert!(!lineage.segments[2].is_leaf);
    }

    #[test]
    fn test_lineage_file_leaf() {
        let lineage = BreadcrumbLineage::from_path(Path::new("/home/user/workspace/src/main.rs"));
        assert_eq!(lineage.segments.len(), 6);
        assert_eq!(lineage.active_index, 5);

        assert_eq!(lineage.segments[0].name, "Computer");
        assert_eq!(lineage.segments[1].name, "home");
        assert_eq!(lineage.segments[2].name, "user");
        assert_eq!(lineage.segments[3].name, "workspace");
        assert_eq!(lineage.segments[4].name, "src");

        let leaf = &lineage.segments[5];
        assert_eq!(leaf.name, "main.rs");
        assert_eq!(leaf.path, PathBuf::from("/home/user/workspace/src/main.rs"));
        assert!(!leaf.is_root);
        assert!(!leaf.is_directory);
        assert!(leaf.is_leaf);
        assert_eq!(leaf.icon_name, "text-x-generic-symbolic");
    }

    #[test]
    fn test_lineage_explicit_selection() {
        let lineage = BreadcrumbLineage::resolve(
            Path::new("/home/user"),
            Some(Path::new("/home/user/image.png")),
        );
        assert_eq!(lineage.segments.len(), 4);
        assert_eq!(lineage.active_index, 3);

        let leaf = &lineage.segments[3];
        assert_eq!(leaf.name, "image.png");
        assert_eq!(leaf.path, PathBuf::from("/home/user/image.png"));
        assert!(leaf.is_leaf);
        assert!(!leaf.is_directory);
        assert_eq!(leaf.icon_name, "image-x-generic-symbolic");
    }

    #[test]
    fn test_resolve_folder_icon() {
        assert_eq!(
            resolve_folder_icon(Path::new("/home/user/Documents"), "Documents"),
            "folder-documents-symbolic"
        );
        assert_eq!(
            resolve_folder_icon(Path::new("/home/user/Downloads"), "Downloads"),
            "folder-download-symbolic"
        );
        assert_eq!(
            resolve_folder_icon(Path::new("/home/user/custom"), "custom"),
            "folder-symbolic"
        );
    }
}
