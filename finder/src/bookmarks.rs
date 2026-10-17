//! Freedesktop Bookmarks persistence engine (`~/.config/gtk-3.0/bookmarks`).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use shared::ipc::{file_uri_to_path, path_to_file_uri};

/// Entry in the user's Freedesktop bookmarks file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BookmarkEntry {
    /// URI representing the resource (e.g. `file:///home/user/Documents` or `trash:///`).
    pub uri: String,
    /// Optional custom label/display name for the bookmark.
    pub label: Option<String>,
    /// Local filesystem path decoded from the URI.
    pub path: PathBuf,
}

impl BookmarkEntry {
    /// Constructs a new [`BookmarkEntry`] for a given filesystem path with an optional label.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>, label: Option<String>) -> Self {
        let path = path.into();
        let uri = path_to_file_uri(&path).unwrap_or_else(|_| format!("file://{}", path.display()));
        Self { uri, label, path }
    }

    /// Constructs a [`BookmarkEntry`] for a given URI, path, and optional label.
    #[must_use]
    pub fn new(uri: impl Into<String>, path: impl Into<PathBuf>, label: Option<String>) -> Self {
        Self {
            uri: uri.into(),
            label,
            path: path.into(),
        }
    }

    /// Returns the effective display name for this bookmark.
    #[must_use]
    pub fn display_name(&self) -> String {
        if let Some(lbl) = &self.label
            && !lbl.trim().is_empty()
        {
            return lbl.clone();
        }
        if self.uri.starts_with("trash://") {
            return "Trash".to_string();
        }
        self.path.file_name().and_then(|n| n.to_str()).map_or_else(
            || self.path.to_string_lossy().into_owned(),
            ToString::to_string,
        )
    }

    /// Returns the recommended symbolic icon name for this bookmark.
    #[must_use]
    pub fn icon_name(&self) -> &'static str {
        if self.uri.starts_with("trash://") {
            return "user-trash-symbolic";
        }
        let name_lower = self.display_name().to_ascii_lowercase();
        match name_lower.as_str() {
            "home" => "user-home-symbolic",
            "desktop" => "user-desktop-symbolic",
            "documents" => "folder-documents-symbolic",
            "downloads" => "folder-download-symbolic",
            "music" => "folder-music-symbolic",
            "pictures" => "folder-pictures-symbolic",
            "videos" => "folder-videos-symbolic",
            "trash" => "user-trash-symbolic",
            _ => "folder-symbolic",
        }
    }
}

/// Resolves the path to the GTK bookmarks file (`~/.config/gtk-3.0/bookmarks`).
#[must_use]
pub fn bookmarks_file_path() -> PathBuf {
    if let Ok(cfg) = std::env::var("XDG_CONFIG_HOME")
        && !cfg.trim().is_empty()
    {
        return PathBuf::from(cfg).join("gtk-3.0").join("bookmarks");
    }
    let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
    home.join(".config").join("gtk-3.0").join("bookmarks")
}

/// Parses raw text content conforming to Freedesktop bookmarks format into entries.
#[must_use]
pub fn parse_bookmarks(content: &str) -> Vec<BookmarkEntry> {
    let mut entries = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let (uri_part, label_part) = match trimmed.split_once(' ') {
            Some((u, l)) => (u.trim(), Some(l.trim().to_string())),
            None => (trimmed, None),
        };

        if uri_part.is_empty() {
            continue;
        }

        let entry = parse_single_bookmark(uri_part, label_part);
        entries.push(entry);
    }

    entries
}

fn parse_single_bookmark(uri_part: &str, label: Option<String>) -> BookmarkEntry {
    if uri_part.starts_with("file://") {
        let path = file_uri_to_path(uri_part).unwrap_or_else(|_| {
            let raw = uri_part.strip_prefix("file://").unwrap_or(uri_part);
            PathBuf::from(raw)
        });
        BookmarkEntry {
            uri: uri_part.to_string(),
            label,
            path,
        }
    } else if uri_part.starts_with("trash://") {
        let trash_dir = resolve_trash_dir();
        BookmarkEntry {
            uri: uri_part.to_string(),
            label,
            path: trash_dir,
        }
    } else if uri_part.starts_with('/') {
        // Line was a plain file path instead of a URI
        let path = PathBuf::from(uri_part);
        let uri = path_to_file_uri(&path).unwrap_or_else(|_| format!("file://{uri_part}"));
        BookmarkEntry { uri, label, path }
    } else {
        // Other custom URI scheme (e.g. smb://, sftp://)
        BookmarkEntry {
            uri: uri_part.to_string(),
            label,
            path: PathBuf::from(uri_part),
        }
    }
}

/// Serializes bookmark entries to the Freedesktop GTK 3 bookmarks format string.
#[must_use]
pub fn serialize_bookmarks(bookmarks: &[BookmarkEntry]) -> String {
    let mut out = String::new();
    for b in bookmarks {
        match &b.label {
            Some(lbl) if !lbl.trim().is_empty() => {
                out.push_str(&b.uri);
                out.push(' ');
                out.push_str(lbl.trim());
                out.push('\n');
            }
            _ => {
                out.push_str(&b.uri);
                out.push('\n');
            }
        }
    }
    out
}

/// Loads bookmark entries from `~/.config/gtk-3.0/bookmarks`.
///
/// If the file does not exist, cannot be read, or is empty, returns [`default_places`].
#[must_use]
pub fn load_user_bookmarks() -> Vec<BookmarkEntry> {
    let path = bookmarks_file_path();
    if let Ok(content) = fs::read_to_string(&path) {
        let entries = parse_bookmarks(&content);
        if !entries.is_empty() {
            return entries;
        }
    }
    default_places()
}

/// Saves bookmark entries to `~/.config/gtk-3.0/bookmarks`.
///
/// # Errors
///
/// Returns an error if the directory cannot be created or the file cannot be written.
pub fn save_user_bookmarks(bookmarks: &[BookmarkEntry]) -> Result<()> {
    let path = bookmarks_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory for {}", path.display()))?;
    }
    let serialized = serialize_bookmarks(bookmarks);
    fs::write(&path, serialized)
        .with_context(|| format!("Failed to write bookmarks to {}", path.display()))?;
    Ok(())
}

/// Generates standard default places (Home, Desktop, Documents, Downloads, Trash).
#[must_use]
pub fn default_places() -> Vec<BookmarkEntry> {
    let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);

    let desktop = resolve_xdg_dir("XDG_DESKTOP_DIR", &home, "Desktop");
    let documents = resolve_xdg_dir("XDG_DOCUMENTS_DIR", &home, "Documents");
    let downloads = resolve_xdg_dir("XDG_DOWNLOAD_DIR", &home, "Downloads");
    let trash_dir = resolve_trash_dir();

    vec![
        BookmarkEntry::from_path(home, Some("Home".to_string())),
        BookmarkEntry::from_path(desktop, Some("Desktop".to_string())),
        BookmarkEntry::from_path(documents, Some("Documents".to_string())),
        BookmarkEntry::from_path(downloads, Some("Downloads".to_string())),
        BookmarkEntry::new("trash:///", trash_dir, Some("Trash".to_string())),
    ]
}

fn resolve_trash_dir() -> PathBuf {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME")
        && !data_home.trim().is_empty()
    {
        return PathBuf::from(data_home).join("Trash");
    }
    let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
    home.join(".local").join("share").join("Trash")
}

fn resolve_xdg_dir(var_name: &str, home: &Path, fallback: &str) -> PathBuf {
    if let Ok(val) = std::env::var(var_name)
        && !val.trim().is_empty()
    {
        let p = PathBuf::from(val.trim());
        if p.is_absolute() {
            return p;
        }
    }

    // Try parsing ~/.config/user-dirs.dirs
    let config_dir =
        std::env::var("XDG_CONFIG_HOME").map_or_else(|_| home.join(".config"), PathBuf::from);
    let user_dirs_file = config_dir.join("user-dirs.dirs");

    if let Ok(content) = fs::read_to_string(user_dirs_file) {
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix(var_name) {
                let rest = rest.trim_start();
                if let Some(val_part) = rest.strip_prefix('=') {
                    let val_clean = val_part.trim().trim_matches('"');
                    let expanded = val_clean.strip_prefix("$HOME/").map_or_else(
                        || {
                            if val_clean == "$HOME" {
                                home.to_path_buf()
                            } else {
                                PathBuf::from(val_clean)
                            }
                        },
                        |sub| home.join(sub),
                    );
                    return expanded;
                }
            }
        }
    }

    home.join(fallback)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bookmarks_basic() {
        let data = "file:///home/user/Documents Documents\nfile:///home/user/Downloads\ntrash:/// Trash\n# A comment\n\n";
        let parsed = parse_bookmarks(data);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].uri, "file:///home/user/Documents");
        assert_eq!(parsed[0].label.as_deref(), Some("Documents"));
        assert_eq!(parsed[0].path, PathBuf::from("/home/user/Documents"));

        assert_eq!(parsed[1].uri, "file:///home/user/Downloads");
        assert_eq!(parsed[1].label, None);
        assert_eq!(parsed[1].path, PathBuf::from("/home/user/Downloads"));

        assert_eq!(parsed[2].uri, "trash:///");
        assert_eq!(parsed[2].label.as_deref(), Some("Trash"));
    }

    #[test]
    fn test_parse_bookmarks_percent_encoded() {
        let data = "file:///home/user/My%20Photos My Photos\n";
        let parsed = parse_bookmarks(data);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, PathBuf::from("/home/user/My Photos"));
        assert_eq!(parsed[0].label.as_deref(), Some("My Photos"));
    }

    #[test]
    fn test_roundtrip_serialization() {
        let entries = vec![
            BookmarkEntry {
                uri: "file:///home/user/Documents".to_string(),
                label: Some("Docs".to_string()),
                path: PathBuf::from("/home/user/Documents"),
            },
            BookmarkEntry {
                uri: "file:///home/user/Downloads".to_string(),
                label: None,
                path: PathBuf::from("/home/user/Downloads"),
            },
        ];

        let serialized = serialize_bookmarks(&entries);
        let parsed = parse_bookmarks(&serialized);
        assert_eq!(entries, parsed);
    }

    #[test]
    fn test_default_places_non_empty() {
        let defaults = default_places();
        assert_eq!(defaults.len(), 5);
        assert_eq!(defaults[0].label.as_deref(), Some("Home"));
        assert_eq!(defaults[4].uri, "trash:///");
    }
}
