//! Persistent cache for expanded tree nodes (`~/.cache/findx/expanded_nodes.json`).
//!
//! Preserves expanded subdirectories across folder navigations, automatically
//! restoring open subtrees when returning to previously inspected workspaces.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Cache mapping parent container directories to sets of expanded descendant subdirectories.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExpandedNodesCache {
    /// Map of container root path to list of expanded directory paths.
    pub entries: HashMap<PathBuf, Vec<PathBuf>>,
}

impl ExpandedNodesCache {
    /// Constructs a new empty [`ExpandedNodesCache`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Resolves the filesystem path for the cache file (`~/.cache/findx/expanded_nodes.json`).
    #[must_use]
    pub fn cache_path() -> PathBuf {
        if let Ok(cache) = std::env::var("XDG_CACHE_HOME")
            && !cache.trim().is_empty()
        {
            return PathBuf::from(cache)
                .join("findx")
                .join("expanded_nodes.json");
        }
        let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
        home.join(".cache")
            .join("findx")
            .join("expanded_nodes.json")
    }

    /// Loads the expansion state cache from disk, returning defaults on any failure.
    #[must_use]
    pub fn load() -> Self {
        let path = Self::cache_path();
        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(cache) = serde_json::from_str::<Self>(&content)
        {
            return cache;
        }
        Self::default()
    }

    /// Saves the expansion state cache to disk atomically.
    ///
    /// # Errors
    /// Returns an error if the directory cannot be created or the file cannot be written.
    pub fn save(&self) -> Result<()> {
        let path = Self::cache_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create cache directory: {}",
                    parent.to_string_lossy()
                )
            })?;
        }

        let json = serde_json::to_string_pretty(self)
            .context("failed to serialize expanded nodes cache")?;
        fs::write(&path, json)
            .with_context(|| format!("failed to write cache file: {}", path.to_string_lossy()))?;
        Ok(())
    }

    /// Returns the set of expanded directory paths recorded for `container`.
    #[must_use]
    pub fn get_expanded(&self, container: &Path) -> HashSet<PathBuf> {
        self.entries
            .get(container)
            .map_or_else(HashSet::new, |vec| vec.iter().cloned().collect())
    }

    /// Sets the list of expanded directory paths for `container`.
    pub fn set_expanded(&mut self, container: &Path, paths: HashSet<PathBuf>) {
        let mut sorted: Vec<PathBuf> = paths.into_iter().collect();
        sorted.sort();
        self.entries.insert(container.to_path_buf(), sorted);
    }

    /// Records an expanded path for `container`.
    pub fn add_expanded(&mut self, container: &Path, path: PathBuf) {
        let list = self.entries.entry(container.to_path_buf()).or_default();
        if !list.contains(&path) {
            list.push(path);
            list.sort();
        }
    }

    /// Removes an expanded path for `container`.
    pub fn remove_expanded(&mut self, container: &Path, path: &Path) {
        if let Some(list) = self.entries.get_mut(container) {
            list.retain(|p| p != path);
        }
    }
}
