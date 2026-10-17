//! Preferences persistence for Finder Path Bar and general settings (`~/.config/findx/preferences.json`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Global Finder user preferences model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinderPreferences {
    /// Visibility state of the bottom breadcrumbs path bar (`Ctrl+Alt+P`).
    #[serde(default = "default_true")]
    pub pathbar_visible: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for FinderPreferences {
    fn default() -> Self {
        Self {
            pathbar_visible: true,
        }
    }
}

impl FinderPreferences {
    /// Resolves the configuration path (`~/.config/findx/preferences.json`).
    #[must_use]
    pub fn config_path() -> PathBuf {
        if let Ok(cfg) = std::env::var("XDG_CONFIG_HOME")
            && !cfg.trim().is_empty()
        {
            return PathBuf::from(cfg).join("findx").join("preferences.json");
        }
        let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
        home.join(".config").join("findx").join("preferences.json")
    }

    /// Loads preferences from disk, returning defaults on any failure.
    #[must_use]
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(prefs) = serde_json::from_str::<Self>(&content)
        {
            return prefs;
        }
        Self::default()
    }

    /// Saves preferences to disk.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be created or file cannot be written.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize preferences")?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write preferences to {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_finder_preferences_default() {
        let prefs = FinderPreferences::default();
        assert!(prefs.pathbar_visible);
    }

    #[test]
    fn test_finder_preferences_roundtrip() {
        let prefs = FinderPreferences {
            pathbar_visible: false,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let parsed: FinderPreferences = serde_json::from_str(&json).unwrap();
        assert_eq!(prefs, parsed);
        assert!(!parsed.pathbar_visible);
    }
}
