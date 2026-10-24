//! Core domain types for in-app search and scope switching.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use shared::models::file::{FileItem, FileMetadata};
use shared::models::mime::MimeCategory;
use std::path::PathBuf;

/// Search scope boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SearchScope {
    /// Restricting results to descendants of active directory.
    #[default]
    CurrentFolder,
    /// Querying system-wide SQLite FTS5 database.
    ThisPc,
}

/// Matching depth mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SearchMode {
    /// Matching file/folder basenames.
    #[default]
    NameMatches,
    /// Inspecting document bodies and text content.
    ContentContains,
}

/// Relational comparison operators for size filtering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeOperator {
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Equal,
    Between(u64, u64),
}

/// Filter constraint for file size in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeFilter {
    pub op: SizeOperator,
    pub bytes: u64,
}

impl SizeFilter {
    /// Evaluates if the candidate size satisfies the filter.
    #[must_use]
    pub const fn matches(&self, candidate_bytes: u64) -> bool {
        match self.op {
            SizeOperator::GreaterThan => candidate_bytes > self.bytes,
            SizeOperator::GreaterThanOrEqual => candidate_bytes >= self.bytes,
            SizeOperator::LessThan => candidate_bytes < self.bytes,
            SizeOperator::LessThanOrEqual => candidate_bytes <= self.bytes,
            SizeOperator::Equal => candidate_bytes == self.bytes,
            SizeOperator::Between(min, max) => candidate_bytes >= min && candidate_bytes <= max,
        }
    }
}

/// Temporal constraints for file modification filtering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateConstraint {
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
    ThisYear,
    Before(DateTime<Utc>),
    After(DateTime<Utc>),
}

/// Specialized attribute constraints for queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeFilter {
    Kind(MimeCategory),
    Size(SizeFilter),
    Tag(String),
    Date(DateConstraint),
}

/// Token variants extracted from search input strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchToken {
    Word(String),
    ExactPhrase(String),
    Wildcard(String),
    Attribute(AttributeFilter),
    Negated(String),
    And,
    Or,
}

/// Aggregated query structure parsed from user input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedQuery {
    pub raw: String,
    pub terms: Vec<String>,
    pub exact_phrases: Vec<String>,
    pub wildcards: Vec<String>,
    pub attributes: Vec<AttributeFilter>,
    pub negated_terms: Vec<String>,
    pub mode: SearchMode,
    pub include_system_files: bool,
}

/// Single search result hit with metadata, relevance score, and optional snippet.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResultItem {
    pub path: PathBuf,
    pub display_name: String,
    pub is_dir: bool,
    pub mime_type: String,
    pub size_bytes: u64,
    pub modified_time: i64,
    pub match_snippet: Option<String>,
    pub relevance_score: f64,
    pub inode: u64,
    pub device_id: u64,
}

impl SearchResultItem {
    /// Converts a search result item into a [`FileItem`] for display in Finder views.
    #[must_use]
    pub fn to_file_item(&self) -> FileItem {
        let meta = FileMetadata {
            size_bytes: self.size_bytes,
            modified: std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(
                u64::try_from(self.modified_time.max(0)).unwrap_or(0),
            )),
            accessed: None,
            created: None,
            permissions: if self.is_dir { 0o755 } else { 0o644 },
            inode: self.inode,
            device_id: self.device_id,
            mime_type: self.mime_type.clone(),
            is_hidden: self.display_name.starts_with('.'),
            is_executable: false,
        };
        FileItem::new(
            self.path.clone(),
            self.display_name.clone(),
            self.is_dir,
            false,
            None,
            meta,
            Vec::new(),
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_search_scope_and_mode_defaults() {
        assert_eq!(SearchScope::default(), SearchScope::CurrentFolder);
        assert_eq!(SearchMode::default(), SearchMode::NameMatches);
    }

    #[test]
    fn test_search_scope_serialization() {
        let serialized = serde_json::to_string(&SearchScope::ThisPc).unwrap();
        let deserialized: SearchScope = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, SearchScope::ThisPc);
    }

    #[test]
    fn test_search_mode_serialization() {
        let serialized = serde_json::to_string(&SearchMode::ContentContains).unwrap();
        let deserialized: SearchMode = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, SearchMode::ContentContains);
    }

    #[test]
    fn test_size_filter_matches() {
        let filter_gt = SizeFilter {
            op: SizeOperator::GreaterThan,
            bytes: 1024,
        };
        assert!(filter_gt.matches(2048));
        assert!(!filter_gt.matches(1024));
        assert!(!filter_gt.matches(512));

        let filter_between = SizeFilter {
            op: SizeOperator::Between(100, 200),
            bytes: 0,
        };
        assert!(filter_between.matches(100));
        assert!(filter_between.matches(150));
        assert!(filter_between.matches(200));
        assert!(!filter_between.matches(99));
        assert!(!filter_between.matches(201));
    }

    #[test]
    fn test_search_result_item_to_file_item() {
        let item = SearchResultItem {
            path: PathBuf::from("/home/user/test.rs"),
            display_name: "test.rs".to_string(),
            is_dir: false,
            mime_type: "text/rust".to_string(),
            size_bytes: 4096,
            modified_time: 1_700_000_000,
            match_snippet: Some("fn main()".to_string()),
            relevance_score: 500.0,
            inode: 12345,
            device_id: 67890,
        };

        let file_item = item.to_file_item();
        assert_eq!(file_item.path, PathBuf::from("/home/user/test.rs"));
        assert_eq!(file_item.file_name, "test.rs");
        assert!(!file_item.is_dir);
        assert!(!file_item.metadata.is_hidden);
        assert_eq!(file_item.metadata.size_bytes, 4096);
        assert_eq!(file_item.metadata.inode, 12345);
        assert_eq!(file_item.metadata.device_id, 67890);
        assert_eq!(file_item.metadata.mime_type, "text/rust");
        assert_eq!(file_item.metadata.permissions, 0o644);
    }

    #[test]
    fn test_search_result_item_dir_to_file_item() {
        let item = SearchResultItem {
            path: PathBuf::from("/home/user/.hidden_dir"),
            display_name: ".hidden_dir".to_string(),
            is_dir: true,
            mime_type: "inode/directory".to_string(),
            size_bytes: 4096,
            modified_time: 1_700_000_000,
            match_snippet: None,
            relevance_score: 1000.0,
            inode: 111,
            device_id: 222,
        };

        let file_item = item.to_file_item();
        assert!(file_item.is_dir);
        assert!(file_item.metadata.is_hidden);
        assert_eq!(file_item.metadata.permissions, 0o755);
    }
}
