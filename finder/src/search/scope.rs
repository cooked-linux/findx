//! Scope bar state machine and search preferences / history persistence.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::error::{Result, SearchError};
use super::types::{SearchMode, SearchScope};

const MAX_HISTORY_ENTRIES: usize = 20;

/// State model managing scope bar settings and active directory context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeBarState {
    pub scope: SearchScope,
    pub mode: SearchMode,
    pub include_system_files: bool,
    pub folder_name: String,
    pub folder_path: PathBuf,
    pub query_text: String,
}

impl ScopeBarState {
    /// Constructs a new [`ScopeBarState`] initialized for the given folder.
    #[must_use]
    pub fn new(current_folder: PathBuf) -> Self {
        let folder_name = current_folder
            .file_name()
            .map_or_else(|| "Folder".to_string(), |n| n.to_string_lossy().to_string());
        Self {
            scope: SearchScope::CurrentFolder,
            mode: SearchMode::NameMatches,
            include_system_files: false,
            folder_name,
            folder_path: current_folder,
            query_text: String::new(),
        }
    }

    /// Updates the current directory and synchronizes folder name.
    pub fn set_folder(&mut self, folder: PathBuf) {
        let name = folder
            .file_name()
            .map_or_else(|| "Folder".to_string(), |n| n.to_string_lossy().to_string());
        self.folder_name = name;
        self.folder_path = folder;
    }

    /// Toggles scope between `CurrentFolder` and `ThisPc`.
    pub const fn toggle_scope(&mut self) {
        self.scope = match self.scope {
            SearchScope::CurrentFolder => SearchScope::ThisPc,
            SearchScope::ThisPc => SearchScope::CurrentFolder,
        };
    }

    /// Toggles search mode between `NameMatches` and `ContentContains`.
    pub const fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            SearchMode::NameMatches => SearchMode::ContentContains,
            SearchMode::ContentContains => SearchMode::NameMatches,
        };
    }
}

/// A recorded entry in search history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHistoryEntry {
    pub query: String,
    pub timestamp: i64,
    pub use_count: u32,
}

/// Persistent container for search query history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHistory {
    pub entries: Vec<SearchHistoryEntry>,
}

impl SearchHistory {
    /// Constructs an empty [`SearchHistory`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Loads history from disk at the default config path.
    #[must_use]
    pub fn load() -> Self {
        let Ok(path) = history_file_path() else {
            return Self::default();
        };
        Self::load_from_path(&path).unwrap_or_default()
    }

    /// Loads history from a specific file path.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError::Io`] or [`SearchError::History`] if reading or parsing fails.
    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path).map_err(|e| SearchError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;

        let history: Self = serde_json::from_str(&content)
            .map_err(|e| SearchError::History(format!("Failed to deserialize history: {e}")))?;

        Ok(history)
    }

    /// Atomically saves history to disk at the default config path.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError`] if saving fails.
    pub fn save(&self) -> Result<()> {
        let path = history_file_path()?;
        self.save_to_path(&path)
    }

    /// Atomically saves history to a specific file path using a temporary file.
    ///
    /// # Errors
    ///
    /// Returns [`SearchError`] if saving fails.
    #[allow(clippy::cast_possible_truncation)]
    pub fn save_to_path(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| SearchError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| SearchError::History(format!("Failed to serialize history: {e}")))?;

        let rand_suffix: u64 = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let tmp_path = path.with_extension(format!("tmp.{rand_suffix}"));

        fs::write(&tmp_path, json).map_err(|e| SearchError::Io {
            path: tmp_path.clone(),
            source: e,
        })?;

        fs::rename(&tmp_path, path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            SearchError::Io {
                path: path.to_path_buf(),
                source: e,
            }
        })?;

        Ok(())
    }

    /// Records a search query, deduplicating and maintaining MRU order up to 20 entries.
    #[allow(clippy::cast_possible_wrap, clippy::useless_let_if_seq)]
    pub fn record_query(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return;
        }

        let now_epoch = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Check if query exists (case-insensitive deduplication)
        let mut existing_count = 1;
        if let Some(pos) = self
            .entries
            .iter()
            .position(|e| e.query.eq_ignore_ascii_case(trimmed))
        {
            let removed = self.entries.remove(pos);
            existing_count = removed.use_count + 1;
        }

        self.entries.insert(
            0,
            SearchHistoryEntry {
                query: trimmed.to_string(),
                timestamp: now_epoch,
                use_count: existing_count,
            },
        );

        if self.entries.len() > MAX_HISTORY_ENTRIES {
            self.entries.truncate(MAX_HISTORY_ENTRIES);
        }
    }

    /// Clears all stored search history entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns the most recent search query strings in order.
    #[must_use]
    pub fn recent_queries(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.query.clone()).collect()
    }
}

/// Resolves the default XDG search history file path.
///
/// # Errors
///
/// Returns [`SearchError::History`] if neither `XDG_CONFIG_HOME` nor `HOME` is set.
pub fn history_file_path() -> Result<PathBuf> {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var("HOME").map_err(|_| {
                SearchError::History("Neither XDG_CONFIG_HOME nor HOME set".to_string())
            })?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("filemanager").join("search_history.json"))
}

/// Convenience helper to load search history from disk.
#[must_use]
pub fn load_history() -> SearchHistory {
    SearchHistory::load()
}

/// Convenience helper to record a query in persistent search history.
///
/// # Errors
///
/// Returns [`SearchError`] if saving history fails.
pub fn record_query(query: &str) -> Result<()> {
    let mut history = SearchHistory::load();
    history.record_query(query);
    history.save()
}

/// Convenience helper to clear persistent search history.
///
/// # Errors
///
/// Returns [`SearchError`] if saving cleared history fails.
pub fn clear_history() -> Result<()> {
    let mut history = SearchHistory::load();
    history.clear();
    history.save()
}

/// Convenience helper returning list of recent query strings.
#[must_use]
pub fn recent_queries() -> Vec<String> {
    SearchHistory::load().recent_queries()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_scope_bar_state_defaults_and_toggles() {
        let folder = PathBuf::from("/home/user/Documents");
        let mut state = ScopeBarState::new(folder.clone());

        assert_eq!(state.folder_name, "Documents");
        assert_eq!(state.folder_path, folder);
        assert_eq!(state.scope, SearchScope::CurrentFolder);
        assert_eq!(state.mode, SearchMode::NameMatches);
        assert!(!state.include_system_files);

        state.toggle_scope();
        assert_eq!(state.scope, SearchScope::ThisPc);
        state.toggle_scope();
        assert_eq!(state.scope, SearchScope::CurrentFolder);

        state.toggle_mode();
        assert_eq!(state.mode, SearchMode::ContentContains);
        state.toggle_mode();
        assert_eq!(state.mode, SearchMode::NameMatches);
    }

    #[test]
    fn test_history_record_deduplication_and_mru() {
        let mut history = SearchHistory::new();
        history.record_query("query 1");
        history.record_query("query 2");
        history.record_query("query 1"); // Re-recording moves to front

        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.entries[0].query, "query 1");
        assert_eq!(history.entries[0].use_count, 2);
        assert_eq!(history.entries[1].query, "query 2");
        assert_eq!(history.entries[1].use_count, 1);
    }

    #[test]
    fn test_history_max_20_entries() {
        let mut history = SearchHistory::new();
        for i in 0..25 {
            history.record_query(&format!("term {i}"));
        }
        assert_eq!(history.entries.len(), 20);
        assert_eq!(history.entries[0].query, "term 24");
    }

    #[test]
    fn test_history_atomic_save_and_load() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("sub").join("search_history.json");

        let mut history = SearchHistory::new();
        history.record_query("foo");
        history.record_query("bar");

        history.save_to_path(&file_path).unwrap();
        assert!(file_path.exists());

        let loaded = SearchHistory::load_from_path(&file_path).unwrap();
        assert_eq!(loaded.recent_queries(), vec!["bar", "foo"]);
    }

    #[test]
    fn test_history_clear() {
        let mut history = SearchHistory::new();
        history.record_query("to be cleared");
        assert_eq!(history.entries.len(), 1);

        history.clear();
        assert!(history.entries.is_empty());
    }
}
