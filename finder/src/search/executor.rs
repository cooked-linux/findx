//! High-performance search executor with cycle prevention, cancellation, and hybrid ranking.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use shared::db::connection::Database;
use shared::fs::InodeKey;
use shared::indexer::query::search_indexed_files;
use shared::models::mime::MimeCategory;

use super::error::{Result, SearchError};
use super::parser::parse_query;
use super::types::{ParsedQuery, SearchMode, SearchResultItem, SearchScope};

const MAX_CONTENT_SEARCH_BYTES: u64 = 2 * 1024 * 1024; // 2MB
const SNIPPET_CONTEXT_CHARS: usize = 50;

/// Monotonic query cancellation controller.
#[derive(Debug, Clone, Default)]
pub struct SearchCancellation {
    current_id: Arc<AtomicU64>,
}

impl SearchCancellation {
    /// Constructs a new cancellation tracker with initial query ID 0.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_id: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Advances to the next monotonic query ID.
    #[must_use]
    pub fn next_query_id(&self) -> u64 {
        self.current_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Returns the currently active query ID.
    #[must_use]
    pub fn current_query_id(&self) -> u64 {
        self.current_id.load(Ordering::SeqCst)
    }

    /// Validates whether the given query ID matches the currently active query.
    #[must_use]
    pub fn is_active(&self, query_id: u64) -> bool {
        self.current_id.load(Ordering::SeqCst) == query_id
    }

    /// Explicitly cancels the active search query.
    pub fn cancel(&self) {
        self.current_id.fetch_add(1, Ordering::SeqCst);
    }
}

/// Parameters configuring a search execution run.
#[derive(Debug, Clone)]
pub struct SearchExecutionParams {
    pub query: String,
    pub scope: SearchScope,
    pub mode: SearchMode,
    pub root_path: PathBuf,
    pub include_system_files: bool,
    pub query_id: u64,
}

/// Executes a search based on the provided parameters and cancellation token.
///
/// # Errors
///
/// Returns [`SearchError::Cancelled`] if a newer query has arrived or if traversal fails.
pub fn execute_search(
    params: &SearchExecutionParams,
    cancellation: &SearchCancellation,
) -> Result<Vec<SearchResultItem>> {
    let mut parsed = parse_query(&params.query);
    parsed.mode = params.mode;
    parsed.include_system_files = params.include_system_files;

    if parsed.raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    if !cancellation.is_active(params.query_id) {
        return Err(SearchError::Cancelled);
    }

    match params.scope {
        SearchScope::ThisPc => {
            // Attempt system-wide indexed search, falling back to filesystem traversal if unavailable
            match execute_this_pc_search(&parsed, params.query_id, cancellation) {
                Ok(items) => Ok(items),
                Err(e) => {
                    tracing::warn!(
                        "ThisPc index search failed ({e}), falling back to filesystem traversal"
                    );
                    let home =
                        std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
                    execute_local_folder_search(&home, &parsed, params.query_id, cancellation)
                }
            }
        }
        SearchScope::CurrentFolder => {
            execute_local_folder_search(&params.root_path, &parsed, params.query_id, cancellation)
        }
    }
}

/// Executes system-wide search against the SQLite FTS5 database.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
fn execute_this_pc_search(
    query: &ParsedQuery,
    query_id: u64,
    cancellation: &SearchCancellation,
) -> Result<Vec<SearchResultItem>> {
    let db = Database::open_default()?;
    let hits = search_indexed_files(&db, &query.raw, 100)?;

    if !cancellation.is_active(query_id) {
        return Err(SearchError::Cancelled);
    }

    let now_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let mut results = Vec::new();
    for hit in hits {
        if !cancellation.is_active(query_id) {
            return Err(SearchError::Cancelled);
        }

        let is_dir = hit.mime_type == "inode/directory";

        // Filter out system/hidden files if not requested
        if !query.include_system_files && hit.filename.starts_with('.') {
            continue;
        }

        // Apply attribute filters
        if !query.matches_attributes(hit.size_bytes, hit.modified_time, &hit.mime_type, &[]) {
            continue;
        }

        let (inode, device_id) = get_file_identifiers(&hit.path);

        let mut item = SearchResultItem {
            path: hit.path,
            display_name: hit.filename,
            is_dir,
            mime_type: hit.mime_type,
            size_bytes: hit.size_bytes,
            modified_time: hit.modified_time,
            match_snippet: hit.snippet,
            relevance_score: hit.rank_score,
            inode,
            device_id,
        };

        // Recalculate hybrid ranking score
        item.relevance_score = calculate_score(
            &item.display_name,
            query,
            item.match_snippet.is_some(),
            item.modified_time,
            now_epoch,
        );

        results.push(item);
    }

    results.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results)
}

/// Executes directory traversal search starting from `root_path`.
///
/// # Errors
///
/// Returns [`SearchError::Cancelled`] if the search query is cancelled during traversal.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
pub fn execute_local_folder_search(
    root_path: &Path,
    query: &ParsedQuery,
    query_id: u64,
    cancellation: &SearchCancellation,
) -> Result<Vec<SearchResultItem>> {
    let mut results = Vec::new();
    let mut visited_inodes = HashSet::new();

    let now_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    traverse_dir(
        root_path,
        query,
        query_id,
        cancellation,
        &mut visited_inodes,
        &mut results,
        now_epoch,
    )?;

    results.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results)
}

#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::manual_let_else,
    clippy::collapsible_if
)]
fn traverse_dir(
    dir_path: &Path,
    query: &ParsedQuery,
    query_id: u64,
    cancellation: &SearchCancellation,
    visited_inodes: &mut HashSet<InodeKey>,
    results: &mut Vec<SearchResultItem>,
    now_epoch: i64,
) -> Result<()> {
    if !cancellation.is_active(query_id) {
        return Err(SearchError::Cancelled);
    }

    // Blacklist check on directory path
    if is_blacklisted_path(dir_path) {
        return Ok(());
    }

    let meta = match fs::metadata(dir_path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    let inode_key = InodeKey::from_metadata(&meta);
    if !visited_inodes.insert(inode_key) {
        // Already visited this inode/device: loop or bind mount prevented
        return Ok(());
    }

    let read_dir = match fs::read_dir(dir_path) {
        Ok(rd) => rd,
        Err(_) => return Ok(()),
    };

    let mut subdirs = Vec::new();

    for entry_res in read_dir {
        if !cancellation.is_active(query_id) {
            return Err(SearchError::Cancelled);
        }

        let entry = match entry_res {
            Ok(e) => e,
            Err(_) => continue,
        };

        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        if is_blacklisted_name(&name_str) {
            continue;
        }

        if !query.include_system_files && name_str.starts_with('.') {
            continue;
        }

        let child_path = entry.path();
        let child_meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let is_dir = child_meta.is_dir();
        let size_bytes = child_meta.len();
        let mtime = child_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs() as i64);

        let (inode, device_id) = (inode_key.ino, inode_key.dev);

        let detected_mime = if is_dir {
            "inode/directory".to_string()
        } else {
            shared::mime::detect_mime_type(&child_path)
                .unwrap_or_else(|_| "application/octet-stream".to_string())
        };

        if !query.matches_attributes(size_bytes, mtime, &detected_mime, &[]) {
            if is_dir {
                subdirs.push(child_path);
            }
            continue;
        }

        let mut matched = query.matches_filename(&name_str);
        let mut snippet = None;

        // Content search if name does not match or if ContentContains mode is active
        if query.mode == SearchMode::ContentContains
            && !is_dir
            && size_bytes <= MAX_CONTENT_SEARCH_BYTES
        {
            if is_text_mime(&detected_mime) {
                if let Ok(content) = fs::read_to_string(&child_path) {
                    if let Some(snip) = extract_content_snippet(&content, query) {
                        matched = true;
                        snippet = Some(snip);
                    }
                }
            }
        }

        if matched {
            let relevance_score =
                calculate_score(&name_str, query, snippet.is_some(), mtime, now_epoch);

            results.push(SearchResultItem {
                path: child_path.clone(),
                display_name: name_str.to_string(),
                is_dir,
                mime_type: detected_mime,
                size_bytes,
                modified_time: mtime,
                match_snippet: snippet,
                relevance_score,
                inode,
                device_id,
            });
        }

        if is_dir {
            subdirs.push(child_path);
        }
    }

    for subdir in subdirs {
        traverse_dir(
            &subdir,
            query,
            query_id,
            cancellation,
            visited_inodes,
            results,
            now_epoch,
        )?;
    }

    Ok(())
}

#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::if_same_then_else
)]
fn calculate_score(
    display_name: &str,
    query: &ParsedQuery,
    is_content_match: bool,
    mtime: i64,
    now_epoch: i64,
) -> f64 {
    let name_lower = display_name.to_ascii_lowercase();
    let raw_lower = query.raw.trim().to_ascii_lowercase();

    let base_score = if name_lower == raw_lower {
        1000.0
    } else if name_lower.starts_with(&raw_lower) {
        750.0
    } else if query
        .terms
        .iter()
        .any(|t| name_lower.starts_with(&t.to_ascii_lowercase()))
    {
        750.0
    } else if query
        .terms
        .iter()
        .any(|t| name_lower.contains(&t.to_ascii_lowercase()))
    {
        500.0
    } else if query.matches_filename(display_name) {
        500.0
    } else if is_content_match {
        250.0
    } else {
        100.0
    };

    // Recency bonus: up to +100.0 decaying with age
    let age_secs = (now_epoch - mtime).max(0) as f64;
    let recency_bonus = 100.0 / (1.0 + age_secs / 604_800.0);

    base_score + recency_bonus
}

/// Checks if path is a blacklisted system pseudo-filesystem.
fn is_blacklisted_path(path: &Path) -> bool {
    let p = path.to_string_lossy();
    p.starts_with("/proc")
        || p.starts_with("/sys")
        || p.starts_with("/dev")
        || p.starts_with("/run")
}

/// Checks if name is an ignored development artifact directory.
fn is_blacklisted_name(name: &str) -> bool {
    matches!(name, "node_modules" | ".git" | "target" | ".cache")
}

fn is_text_mime(mime: &str) -> bool {
    let cat = MimeCategory::from_mime_str(mime);
    cat == MimeCategory::Document
        || cat == MimeCategory::SourceCode
        || mime.starts_with("text/")
        || mime.contains("json")
        || mime.contains("xml")
        || mime.contains("yaml")
        || mime.contains("toml")
}

/// Extracts a context snippet around the first matched term or phrase in text content.
#[must_use]
pub fn extract_content_snippet(content: &str, query: &ParsedQuery) -> Option<String> {
    let content_lower = content.to_ascii_lowercase();

    // Look for exact phrases first, then terms
    let needle = query
        .exact_phrases
        .iter()
        .find(|p| content_lower.contains(&p.to_ascii_lowercase()))
        .or_else(|| {
            query
                .terms
                .iter()
                .find(|t| content_lower.contains(&t.to_ascii_lowercase()))
        })?;

    let needle_lower = needle.to_ascii_lowercase();
    let match_start = content_lower.find(&needle_lower)?;
    let match_end = match_start + needle.len();

    let mut start_char_idx = match_start.saturating_sub(SNIPPET_CONTEXT_CHARS);
    while start_char_idx > 0 && !content.is_char_boundary(start_char_idx) {
        start_char_idx -= 1;
    }

    let mut end_char_idx = (match_end + SNIPPET_CONTEXT_CHARS).min(content.len());
    while end_char_idx < content.len() && !content.is_char_boundary(end_char_idx) {
        end_char_idx += 1;
    }

    let prefix = if start_char_idx > 0 { "..." } else { "" };
    let suffix = if end_char_idx < content.len() {
        "..."
    } else {
        ""
    };

    let snippet_body = &content[start_char_idx..end_char_idx];
    Some(format!("{prefix}{snippet_body}{suffix}"))
}

fn get_file_identifiers(path: &Path) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = fs::metadata(path) {
            return (meta.ino(), meta.dev());
        }
    }
    (0, 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cancellation_active_state() {
        let cancel = SearchCancellation::new();
        let q1 = cancel.next_query_id();
        assert_eq!(q1, 1);
        assert!(cancel.is_active(q1));

        let q2 = cancel.next_query_id();
        assert_eq!(q2, 2);
        assert!(!cancel.is_active(q1));
        assert!(cancel.is_active(q2));

        cancel.cancel();
        assert!(!cancel.is_active(q2));
    }

    #[test]
    fn test_inode_cycle_prevention() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let dir_a = root.join("dir_a");
        fs::create_dir(&dir_a).unwrap();

        let file = dir_a.join("test_file.txt");
        fs::write(&file, "contents").unwrap();

        // Create symlink cycle: dir_a/loop -> dir_a
        #[cfg(unix)]
        {
            let loop_link = dir_a.join("loop");
            let _ = std::os::unix::fs::symlink(&dir_a, &loop_link);
        }

        let cancel = SearchCancellation::new();
        let query_id = cancel.next_query_id();
        let parsed = parse_query("test_file");

        let results = execute_local_folder_search(root, &parsed, query_id, &cancel).unwrap();
        // Should find exactly 1 result without infinite loop
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].display_name, "test_file.txt");
    }

    #[test]
    fn test_five_rapid_queries_cancellation() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for i in 0..10 {
            fs::write(root.join(format!("file_{i}.txt")), "test data").unwrap();
        }

        let cancel = SearchCancellation::new();
        let mut last_id = 0;
        for _ in 0..5 {
            last_id = cancel.next_query_id();
        }

        // Running queries 1 to 4 should be cancelled immediately
        for id in 1..last_id {
            let parsed = parse_query("file");
            let res = execute_local_folder_search(root, &parsed, id, &cancel);
            assert!(matches!(res, Err(SearchError::Cancelled)));
        }

        // Last query should succeed
        let parsed = parse_query("file");
        let res = execute_local_folder_search(root, &parsed, last_id, &cancel).unwrap();
        assert_eq!(res.len(), 10);
    }

    #[test]
    fn test_content_search_snippet_extraction() {
        let text = "Here is some introductory context before the confidential passcode and then trailing details.";
        let query = parse_query("confidential passcode");
        let snippet = extract_content_snippet(text, &query);
        assert!(snippet.is_some());
        let s = snippet.unwrap();
        assert!(s.contains("confidential passcode"));
    }
}
