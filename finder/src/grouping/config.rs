//! Persistent per-directory grouping and sorting settings (`~/.config/findx/group_settings.json`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::models::GroupDimension;
use crate::sorting::comparator::{SortColumn, SortDirection};

/// View settings configuration for a single directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryGroupSettings {
    /// Dimension by which items in this directory are grouped.
    #[serde(default)]
    pub group_by: GroupDimension,
    /// Primary column used to sort items.
    #[serde(default)]
    pub sort_by: SortColumn,
    /// Sorting direction.
    #[serde(default)]
    pub sort_direction: SortDirection,
    /// IDs or titles of category sections currently collapsed.
    #[serde(default)]
    pub collapsed_groups: Vec<String>,
}

impl Default for DirectoryGroupSettings {
    fn default() -> Self {
        Self {
            group_by: GroupDimension::None,
            sort_by: SortColumn::Name,
            sort_direction: SortDirection::Ascending,
            collapsed_groups: Vec::new(),
        }
    }
}

impl DirectoryGroupSettings {
    /// Checks if a group is marked as collapsed.
    #[must_use]
    pub fn is_group_collapsed(&self, group: &str) -> bool {
        self.collapsed_groups
            .iter()
            .any(|g| g.eq_ignore_ascii_case(group))
    }

    /// Toggles or sets the collapsed state of a group.
    pub fn set_group_collapsed(&mut self, group: &str, collapsed: bool) {
        let exists = self.is_group_collapsed(group);
        if collapsed && !exists {
            self.collapsed_groups.push(group.to_string());
        } else if !collapsed && exists {
            self.collapsed_groups
                .retain(|g| !g.eq_ignore_ascii_case(group));
        }
    }
}

/// Global map of persistent per-directory settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSettingsStore {
    /// Mapping of absolute directory paths to their respective view settings.
    #[serde(flatten)]
    pub directories: HashMap<PathBuf, DirectoryGroupSettings>,
}

impl GroupSettingsStore {
    /// Resolves the file path for the grouping settings (`~/.config/findx/group_settings.json`).
    #[must_use]
    pub fn config_path() -> PathBuf {
        if let Ok(cfg) = std::env::var("XDG_CONFIG_HOME")
            && !cfg.trim().is_empty()
        {
            return PathBuf::from(cfg).join("findx").join("group_settings.json");
        }
        let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
        home.join(".config")
            .join("findx")
            .join("group_settings.json")
    }

    /// Loads the settings store from disk, returning defaults on any failure.
    #[must_use]
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(store) = serde_json::from_str::<Self>(&content)
        {
            return store;
        }
        Self::default()
    }

    /// Saves the settings store to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created or the file cannot be written.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize group settings store")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write configuration to {}", path.display()))?;
        Ok(())
    }

    /// Returns the settings for a directory, or the default if not configured.
    #[must_use]
    pub fn get_for_dir(&self, path: &Path) -> DirectoryGroupSettings {
        self.directories.get(path).cloned().unwrap_or_default()
    }

    /// Updates the settings for a directory.
    pub fn set_for_dir(&mut self, path: PathBuf, settings: DirectoryGroupSettings) {
        self.directories.insert(path, settings);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_directory_group_settings_default() {
        let settings = DirectoryGroupSettings::default();
        assert_eq!(settings.group_by, GroupDimension::None);
        assert_eq!(settings.sort_by, SortColumn::Name);
        assert_eq!(settings.sort_direction, SortDirection::Ascending);
        assert_eq!(settings.collapsed_groups, [] as [String; 0]);
    }

    #[test]
    fn test_collapsed_group_toggle() {
        let mut settings = DirectoryGroupSettings::default();
        settings.set_group_collapsed("Earlier", true);
        assert!(settings.is_group_collapsed("Earlier"));
        assert!(settings.is_group_collapsed("earlier"));

        settings.set_group_collapsed("Earlier", false);
        assert!(!settings.is_group_collapsed("Earlier"));
    }

    #[test]
    fn test_json_roundtrip_spec_format() {
        let mut store = GroupSettingsStore::default();
        let dl_path = PathBuf::from("/home/user/Downloads");
        let settings = DirectoryGroupSettings {
            group_by: GroupDimension::DateModified,
            sort_by: SortColumn::Name,
            sort_direction: SortDirection::Ascending,
            collapsed_groups: vec!["Earlier".to_string()],
        };
        store.set_for_dir(dl_path.clone(), settings.clone());

        let json = serde_json::to_string_pretty(&store).unwrap();
        assert!(json.contains("/home/user/Downloads"));
        assert!(json.contains("DateModified"));
        assert!(json.contains("Ascending"));
        assert!(json.contains("Earlier"));

        let deserialized: GroupSettingsStore = serde_json::from_str(&json).unwrap();
        assert_eq!(store, deserialized);
        assert_eq!(deserialized.get_for_dir(&dl_path), settings);
    }
}
