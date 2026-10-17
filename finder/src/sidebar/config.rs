//! Persistent configuration for the Finder sidebar (`~/.config/findx/sidebar.json`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Sidebar persistent configuration model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarConfig {
    /// Width of the sidebar in pixels (default 210).
    pub width: i32,
    /// Visibility state of the sidebar.
    pub is_visible: bool,
    /// Names of sections currently collapsed (e.g. `["Favorites", "Locations", "Tags"]`).
    pub collapsed_sections: Vec<String>,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            width: 210,
            is_visible: true,
            collapsed_sections: Vec::new(),
        }
    }
}

impl SidebarConfig {
    /// Resolves the file path for the sidebar configuration (`~/.config/findx/sidebar.json`).
    #[must_use]
    pub fn config_path() -> PathBuf {
        if let Ok(cfg) = std::env::var("XDG_CONFIG_HOME")
            && !cfg.trim().is_empty()
        {
            return PathBuf::from(cfg).join("findx").join("sidebar.json");
        }
        let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
        home.join(".config").join("findx").join("sidebar.json")
    }

    /// Loads the sidebar configuration from disk, returning defaults on any failure.
    #[must_use]
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(cfg) = serde_json::from_str::<Self>(&content)
        {
            return cfg;
        }
        Self::default()
    }

    /// Saves the sidebar configuration to disk.
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
            .context("Failed to serialize sidebar configuration")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write configuration to {}", path.display()))?;
        Ok(())
    }

    /// Checks if a named section is recorded as collapsed.
    #[must_use]
    pub fn is_section_collapsed(&self, section: &str) -> bool {
        self.collapsed_sections
            .iter()
            .any(|s| s.eq_ignore_ascii_case(section))
    }

    /// Toggles or updates the collapsed state of a section.
    pub fn set_section_collapsed(&mut self, section: &str, collapsed: bool) {
        let exists = self.is_section_collapsed(section);
        if collapsed && !exists {
            self.collapsed_sections.push(section.to_string());
        } else if !collapsed && exists {
            self.collapsed_sections
                .retain(|s| !s.eq_ignore_ascii_case(section));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_sidebar_config_default() {
        let cfg = SidebarConfig::default();
        assert_eq!(cfg.width, 210);
        assert!(cfg.is_visible);
        assert_eq!(cfg.collapsed_sections, [] as [std::string::String; 0]);
    }

    #[test]
    fn test_set_section_collapsed() {
        let mut cfg = SidebarConfig::default();
        cfg.set_section_collapsed("Favorites", true);
        assert!(cfg.is_section_collapsed("Favorites"));
        assert!(cfg.is_section_collapsed("favorites"));

        cfg.set_section_collapsed("Favorites", false);
        assert!(!cfg.is_section_collapsed("Favorites"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut cfg = SidebarConfig {
            width: 240,
            is_visible: false,
            ..Default::default()
        };
        cfg.collapsed_sections.push("Tags".to_string());

        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: SidebarConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, deserialized);
    }
}
