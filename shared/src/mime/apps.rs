//! Freedesktop `mimeapps.list` configuration file parser and atomic writer.
//!
//! Conforms to the Freedesktop XDG MIME Applications Specification:
//! - Priority hierarchy: user custom defaults -> desktop-specific -> system admin -> distro defaults.
//! - Parses standard INI sections: `[Default Applications]`, `[Added Associations]`, `[Removed Associations]`.
//! - Provides atomic updater `set_default_app` that preserves comments and unmodified sections.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::MimeError;

/// An individual line in a `mimeapps.list` configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedLine {
    /// Comment line starting with `#` or `;`.
    Comment(String),
    /// Blank or whitespace-only line.
    Blank(String),
    /// Section header, e.g. `[Default Applications]`.
    Section(String),
    /// Key-value entry associated with an INI section.
    Entry {
        section: String,
        mime_type: String,
        value: String,
        raw_key: String,
    },
}

/// In-memory representation of a `mimeapps.list` configuration file preserving formatting.
#[derive(Debug, Clone, Default)]
pub struct MimeAppsConfig {
    lines: Vec<ParsedLine>,
}

impl MimeAppsConfig {
    /// Creates a new empty configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Parses configuration content from an INI string.
    ///
    /// # Errors
    ///
    /// Returns [`MimeError::InvalidConfig`] if a line has invalid syntax.
    pub fn parse(content: &str) -> Result<Self, MimeError> {
        let mut lines = Vec::new();
        let mut current_section = String::new();

        for (idx, line) in content.lines().enumerate() {
            let line_num = idx + 1;
            let trimmed = line.trim();

            if trimmed.is_empty() {
                lines.push(ParsedLine::Blank(line.to_string()));
            } else if trimmed.starts_with('#') || trimmed.starts_with(';') {
                lines.push(ParsedLine::Comment(line.to_string()));
            } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                let sec_name = trimmed[1..trimmed.len() - 1].trim().to_string();
                current_section.clone_from(&sec_name);
                lines.push(ParsedLine::Section(sec_name));
            } else if let Some((key, val)) = line.split_once('=') {
                let trimmed_key = key.trim();
                if trimmed_key.is_empty() {
                    return Err(MimeError::InvalidConfig(line_num, line.to_string()));
                }
                lines.push(ParsedLine::Entry {
                    section: current_section.clone(),
                    mime_type: trimmed_key.to_string(),
                    value: val.trim().to_string(),
                    raw_key: key.to_string(),
                });
            } else {
                return Err(MimeError::InvalidConfig(line_num, line.to_string()));
            }
        }

        Ok(Self { lines })
    }

    /// Returns desktop application IDs registered in `[Default Applications]` for a MIME type.
    #[must_use]
    pub fn get_default_applications(&self, mime_type: &str) -> Vec<&str> {
        self.get_entries_for_section("Default Applications", mime_type)
    }

    /// Returns desktop application IDs registered in `[Added Associations]` for a MIME type.
    #[must_use]
    pub fn get_added_associations(&self, mime_type: &str) -> Vec<&str> {
        self.get_entries_for_section("Added Associations", mime_type)
    }

    /// Returns desktop application IDs registered in `[Removed Associations]` for a MIME type.
    #[must_use]
    pub fn get_removed_associations(&self, mime_type: &str) -> Vec<&str> {
        self.get_entries_for_section("Removed Associations", mime_type)
    }

    fn get_entries_for_section(&self, target_section: &str, mime_type: &str) -> Vec<&str> {
        for line in &self.lines {
            if let ParsedLine::Entry {
                section,
                mime_type: entry_mime,
                value,
                ..
            } = line
                && section.eq_ignore_ascii_case(target_section)
                && entry_mime.eq_ignore_ascii_case(mime_type)
            {
                return value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        Vec::new()
    }

    /// Updates or inserts a default application and updates added/removed associations.
    pub fn set_default(&mut self, mime_type: &str, desktop_id: &str) {
        self.update_default_applications(mime_type, desktop_id);
        self.prepend_added_association(mime_type, desktop_id);
        self.remove_from_removed_associations(mime_type, desktop_id);
    }

    fn update_default_applications(&mut self, mime_type: &str, desktop_id: &str) {
        let mut entry_index = None;
        for (i, line) in self.lines.iter().enumerate() {
            if let ParsedLine::Entry {
                section,
                mime_type: entry_mime,
                ..
            } = line
                && section.eq_ignore_ascii_case("Default Applications")
                && entry_mime.eq_ignore_ascii_case(mime_type)
            {
                entry_index = Some(i);
                break;
            }
        }

        if let Some(idx) = entry_index {
            if let Some(ParsedLine::Entry { value, .. }) = self.lines.get_mut(idx) {
                *value = desktop_id.to_string();
            }
        } else {
            let insert_pos = self.find_insertion_point_for_section("Default Applications");
            self.lines.insert(
                insert_pos,
                ParsedLine::Entry {
                    section: "Default Applications".to_string(),
                    mime_type: mime_type.to_string(),
                    value: desktop_id.to_string(),
                    raw_key: mime_type.to_string(),
                },
            );
        }
    }

    fn prepend_added_association(&mut self, mime_type: &str, desktop_id: &str) {
        let mut entry_index = None;
        for (i, line) in self.lines.iter().enumerate() {
            if let ParsedLine::Entry {
                section,
                mime_type: entry_mime,
                ..
            } = line
                && section.eq_ignore_ascii_case("Added Associations")
                && entry_mime.eq_ignore_ascii_case(mime_type)
            {
                entry_index = Some(i);
                break;
            }
        }

        if let Some(idx) = entry_index {
            if let Some(ParsedLine::Entry { value, .. }) = self.lines.get_mut(idx) {
                let remaining: Vec<&str> = value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != desktop_id)
                    .collect();

                if remaining.is_empty() {
                    *value = format!("{desktop_id};");
                } else {
                    *value = format!("{desktop_id};{};", remaining.join(";"));
                }
            }
        } else {
            let insert_pos = self.find_insertion_point_for_section("Added Associations");
            self.lines.insert(
                insert_pos,
                ParsedLine::Entry {
                    section: "Added Associations".to_string(),
                    mime_type: mime_type.to_string(),
                    value: format!("{desktop_id};"),
                    raw_key: mime_type.to_string(),
                },
            );
        }
    }

    fn remove_from_removed_associations(&mut self, mime_type: &str, desktop_id: &str) {
        for line in &mut self.lines {
            if let ParsedLine::Entry {
                section,
                mime_type: entry_mime,
                value,
                ..
            } = line
                && section.eq_ignore_ascii_case("Removed Associations")
                && entry_mime.eq_ignore_ascii_case(mime_type)
            {
                let remaining: Vec<&str> = value
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != desktop_id)
                    .collect();
                if remaining.is_empty() {
                    value.clear();
                } else {
                    *value = format!("{};", remaining.join(";"));
                }
            }
        }
    }

    fn find_insertion_point_for_section(&mut self, target_section: &str) -> usize {
        let mut section_start = None;
        let mut next_section_start = None;

        for (i, line) in self.lines.iter().enumerate() {
            if let ParsedLine::Section(name) = line {
                if name.eq_ignore_ascii_case(target_section) {
                    section_start = Some(i);
                } else if section_start.is_some() {
                    next_section_start = Some(i);
                    break;
                }
            }
        }

        if let Some(next_idx) = next_section_start {
            next_idx
        } else if section_start.is_some() {
            self.lines.len()
        } else {
            // Section doesn't exist: append header first, then return end of file
            if !self.lines.is_empty() && !matches!(self.lines.last(), Some(ParsedLine::Blank(_))) {
                self.lines.push(ParsedLine::Blank(String::new()));
            }
            self.lines
                .push(ParsedLine::Section(target_section.to_string()));
            self.lines.len()
        }
    }

    /// Serializes configuration back to INI string format.
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                ParsedLine::Comment(s) | ParsedLine::Blank(s) => {
                    let _ = writeln!(out, "{s}");
                }
                ParsedLine::Section(name) => {
                    let _ = writeln!(out, "[{name}]");
                }
                ParsedLine::Entry { raw_key, value, .. } => {
                    let _ = writeln!(out, "{raw_key}={value}");
                }
            }
        }
        out
    }
}

/// Resolves standard path to the current user's `mimeapps.list`.
#[must_use]
pub fn user_mimeapps_path() -> PathBuf {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME")
        && !config_home.trim().is_empty()
    {
        return PathBuf::from(config_home).join("mimeapps.list");
    }

    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config").join("mimeapps.list");
    }

    PathBuf::from(".config/mimeapps.list")
}

/// Returns the Freedesktop `mimeapps.list` files in strict priority order:
/// 1. User custom defaults: `~/.config/mimeapps.list`
/// 2. Desktop-specific defaults: `~/.config/<desktop>-mimeapps.list`
/// 3. System administrator: `/etc/xdg/mimeapps.list`
/// 4. Distribution defaults: `/usr/share/applications/mimeapps.list`
#[must_use]
pub fn default_mimeapps_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let current_desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let desktop_tokens: Vec<String> = current_desktop
        .as_deref()
        .map(|d| {
            d.split(':')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // 1. User custom defaults: ~/.config/mimeapps.list
    let user_path = user_mimeapps_path();
    let config_dir = user_path
        .parent()
        .map_or_else(|| PathBuf::from(".config"), Path::to_path_buf);
    paths.push(user_path);

    // 2. Desktop-specific defaults: ~/.config/<desktop>-mimeapps.list
    for token in &desktop_tokens {
        paths.push(config_dir.join(format!("{token}-mimeapps.list")));
    }

    // 3. System administrator: /etc/xdg/mimeapps.list
    let config_dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".to_string());
    for dir_str in config_dirs.split(':') {
        let dir = Path::new(dir_str);
        paths.push(dir.join("mimeapps.list"));
        for token in &desktop_tokens {
            paths.push(dir.join(format!("{token}-mimeapps.list")));
        }
    }

    // 4. Distribution defaults: /usr/share/applications/mimeapps.list
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for dir_str in data_dirs.split(':') {
        let app_dir = Path::new(dir_str).join("applications");
        paths.push(app_dir.join("mimeapps.list"));
        for token in &desktop_tokens {
            paths.push(app_dir.join(format!("{token}-mimeapps.list")));
        }
    }

    paths
}

/// Atomically writes content to a target file via temporary file replacement.
///
/// # Errors
///
/// Returns [`MimeError::Io`] if creating, writing, syncing, or renaming fails.
pub fn atomic_write_file(path: &Path, content: &str) -> Result<(), MimeError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let mut temp_file = tempfile::NamedTempFile::new_in(parent)?;
    temp_file.write_all(content.as_bytes())?;
    temp_file.as_file().sync_all()?;
    temp_file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// Resolves the default application desktop ID for a given MIME type across configuration paths.
///
/// # Errors
///
/// Returns [`MimeError::Io`] if an existing file cannot be read, or
/// [`MimeError::InvalidConfig`] if configuration syntax is malformed.
pub fn get_default_app_for_mime_with_paths(
    mime_type: &str,
    paths: &[PathBuf],
) -> Result<Option<String>, MimeError> {
    let mut removed = HashSet::new();

    // Pass 1: Check [Default Applications] in priority order
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(path)?;
        let config = MimeAppsConfig::parse(&content)?;

        for app in config.get_removed_associations(mime_type) {
            removed.insert(app.to_string());
        }

        for app in config.get_default_applications(mime_type) {
            if !removed.contains(app) {
                return Ok(Some(app.to_string()));
            }
        }
    }

    // Pass 2: Fallback to [Added Associations] in priority order
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let content = std::fs::read_to_string(path)?;
        let config = MimeAppsConfig::parse(&content)?;

        for app in config.get_added_associations(mime_type) {
            if !removed.contains(app) {
                return Ok(Some(app.to_string()));
            }
        }
    }

    Ok(None)
}

/// Resolves the default application desktop ID for a MIME type using standard system paths.
///
/// # Errors
///
/// Returns [`MimeError::Io`] if reading fails or [`MimeError::InvalidConfig`] on syntax errors.
pub fn get_default_app_for_mime(mime_type: &str) -> Result<Option<String>, MimeError> {
    let paths = default_mimeapps_paths();
    get_default_app_for_mime_with_paths(mime_type, &paths)
}

/// Updates default application associations in a specific file atomically.
///
/// # Errors
///
/// Returns [`MimeError::Io`] or [`MimeError::InvalidConfig`] if read, parse, or atomic write fails.
pub fn set_default_app_in_file(
    path: &Path,
    mime_type: &str,
    desktop_id: &str,
) -> Result<(), MimeError> {
    let mut config = if path.is_file() {
        let content = std::fs::read_to_string(path)?;
        MimeAppsConfig::parse(&content)?
    } else {
        MimeAppsConfig::new()
    };

    config.set_default(mime_type, desktop_id);
    let serialized = config.serialize();
    atomic_write_file(path, &serialized)
}

/// Updates `[Default Applications]` and prepends to `[Added Associations]` in `~/.config/mimeapps.list`.
///
/// Writes atomically to guarantee data safety and preserves all comments and unmodified sections.
///
/// # Errors
///
/// Returns [`MimeError::Io`] or [`MimeError::InvalidConfig`] if file operation fails.
pub fn set_default_app(mime_type: &str, desktop_id: &str) -> Result<(), MimeError> {
    let path = user_mimeapps_path();
    set_default_app_in_file(&path, mime_type, desktop_id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_mimeapps_list() {
        let content = r"# User associations
[Default Applications]
text/plain=org.gnome.TextEditor.desktop
image/png=org.gnome.Loupe.desktop

[Added Associations]
text/plain=org.gnome.TextEditor.desktop;code.desktop;

[Removed Associations]
image/png=gimp.desktop;
";
        let config = MimeAppsConfig::parse(content).unwrap();
        assert_eq!(
            config.get_default_applications("text/plain"),
            vec!["org.gnome.TextEditor.desktop"]
        );
        assert_eq!(
            config.get_added_associations("text/plain"),
            vec!["org.gnome.TextEditor.desktop", "code.desktop"]
        );
        assert_eq!(
            config.get_removed_associations("image/png"),
            vec!["gimp.desktop"]
        );
    }

    #[test]
    fn test_parse_invalid_syntax_reports_line_error() {
        let content = "[Default Applications]\nvalid/mime=app.desktop\nthis line is invalid\n";
        let err = MimeAppsConfig::parse(content).unwrap_err();
        match err {
            MimeError::InvalidConfig(line, text) => {
                assert_eq!(line, 3);
                assert_eq!(text, "this line is invalid");
            }
            other => panic!("Unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_set_default_app_in_file_preserves_comments_and_other_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mimeapps.list");

        let initial_content = r"# Initial comment
[Default Applications]
image/png=loupe.desktop

# Section comment
[Added Associations]
image/png=loupe.desktop;

[Custom Section]
CustomKey=CustomValue
";
        std::fs::write(&path, initial_content).unwrap();

        set_default_app_in_file(&path, "text/plain", "org.gnome.TextEditor.desktop").unwrap();

        let updated = std::fs::read_to_string(&path).unwrap();
        assert!(updated.contains("# Initial comment"));
        assert!(updated.contains("# Section comment"));
        assert!(updated.contains("[Custom Section]"));
        assert!(updated.contains("CustomKey=CustomValue"));
        assert!(updated.contains("text/plain=org.gnome.TextEditor.desktop"));
        assert!(updated.contains("text/plain=org.gnome.TextEditor.desktop;"));
        assert!(updated.contains("image/png=loupe.desktop"));

        // Now change default of text/plain to code.desktop
        set_default_app_in_file(&path, "text/plain", "code.desktop").unwrap();
        let second_update = std::fs::read_to_string(&path).unwrap();
        assert!(second_update.contains("text/plain=code.desktop"));
        assert!(second_update.contains("text/plain=code.desktop;org.gnome.TextEditor.desktop;"));
    }

    #[test]
    fn test_priority_hierarchy_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let user_file = dir.path().join("user_mimeapps.list");
        let sys_file = dir.path().join("sys_mimeapps.list");

        let user_content = r"[Removed Associations]
text/plain=blocked.desktop;
";
        let sys_content = r"[Default Applications]
text/plain=blocked.desktop
image/png=system-viewer.desktop
";
        std::fs::write(&user_file, user_content).unwrap();
        std::fs::write(&sys_file, sys_content).unwrap();

        let paths = vec![user_file, sys_file];

        // blocked.desktop was removed in user config, so it shouldn't be returned
        let default_text = get_default_app_for_mime_with_paths("text/plain", &paths).unwrap();
        assert_eq!(default_text, None);

        // image/png was not removed, system default is returned
        let default_png = get_default_app_for_mime_with_paths("image/png", &paths).unwrap();
        assert_eq!(default_png.as_deref(), Some("system-viewer.desktop"));
    }
}
