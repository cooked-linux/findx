//! Quick Look window geometry and user preference persistence (`~/.config/findx/quicklook.json`).

use std::path::{Path, PathBuf};

/// Quick Look window geometry and playback preferences.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuickLookConfig {
    /// Window width in pixels.
    pub window_width: i32,
    /// Window height in pixels.
    pub window_height: i32,
    /// Audio playback volume (0.0 to 1.0).
    pub volume_level: f64,
    /// Whether fullscreen presentation was active.
    pub is_fullscreen: bool,
}

impl Default for QuickLookConfig {
    fn default() -> Self {
        Self {
            window_width: 800,
            window_height: 600,
            volume_level: 0.85,
            is_fullscreen: false,
        }
    }
}

impl QuickLookConfig {
    pub const MIN_WIDTH: i32 = 400;
    pub const MIN_HEIGHT: i32 = 300;

    /// Resolves the canonical configuration file path (`~/.config/findx/quicklook.json`).
    #[must_use]
    pub fn config_path() -> PathBuf {
        if let Ok(cfg) = std::env::var("XDG_CONFIG_HOME") {
            return PathBuf::from(cfg).join("findx").join("quicklook.json");
        }
        let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from);
        home.join(".config").join("findx").join("quicklook.json")
    }

    /// Clamps requested dimensions to minimum thresholds and available display boundary.
    #[must_use]
    pub fn clamp_dimensions(
        width: i32,
        height: i32,
        max_width: i32,
        max_height: i32,
    ) -> (i32, i32) {
        let max_w = max_width.max(Self::MIN_WIDTH);
        let max_h = max_height.max(Self::MIN_HEIGHT);
        (
            width.clamp(Self::MIN_WIDTH, max_w),
            height.clamp(Self::MIN_HEIGHT, max_h),
        )
    }

    /// Loads configuration from the specified path, falling back to defaults on error.
    #[must_use]
    pub fn load_from_path(path: &Path) -> Self {
        let Ok(data) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&data).unwrap_or_default()
    }

    /// Loads configuration from the standard `~/.config/findx/quicklook.json` path.
    #[must_use]
    pub fn load() -> Self {
        Self::load_from_path(&Self::config_path())
    }

    /// Serializes configuration to the target path atomically.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::Error`] if file creation or writing fails.
    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, data)
    }

    /// Serializes configuration to the standard path.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::Error`] if directory creation or file writing fails.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to_path(&Self::config_path())
    }

    /// Asynchronously writes preferences to disk on a background thread.
    pub fn save_async(&self) {
        let config = self.clone();
        std::thread::spawn(move || {
            let _ = config.save();
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_serde_roundtrip() {
        let original = QuickLookConfig {
            window_width: 960,
            window_height: 640,
            volume_level: 0.72,
            is_fullscreen: true,
        };

        let temp = NamedTempFile::new().unwrap();
        original.save_to_path(temp.path()).unwrap();

        let loaded = QuickLookConfig::load_from_path(temp.path());
        assert_eq!(original, loaded);
    }

    #[test]
    fn test_config_dimension_clamping() {
        // Below minimums
        let (w1, h1) = QuickLookConfig::clamp_dimensions(100, 50, 1920, 1080);
        assert_eq!(w1, QuickLookConfig::MIN_WIDTH);
        assert_eq!(h1, QuickLookConfig::MIN_HEIGHT);

        // Within bounds
        let (w2, h2) = QuickLookConfig::clamp_dimensions(800, 600, 1920, 1080);
        assert_eq!(w2, 800);
        assert_eq!(h2, 600);

        // Exceeding display workarea
        let (w3, h3) = QuickLookConfig::clamp_dimensions(2560, 1440, 1920, 1080);
        assert_eq!(w3, 1920);
        assert_eq!(h3, 1080);
    }
}
