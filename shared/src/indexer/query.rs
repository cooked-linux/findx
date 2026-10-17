//! Sub-50ms hybrid search query and snippet engine.
//!
//! Formulates hybrid SQLite FTS5 queries combining BM25 relevance ranking,
//! filename prefix bonuses, and exponential recency decay, enforcing strict
//! query sanitization to prevent FTS5 syntax errors.

use std::path::PathBuf;
use std::time::SystemTime;

use rusqlite::{Connection, params};

use crate::indexer::IndexerError;

/// Individual search result hit produced by the hybrid indexing query engine.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Full path to the matched file.
    pub path: PathBuf,
    /// Filename of the file.
    pub filename: String,
    /// MIME type string (e.g. `text/plain`, `text/rust`).
    pub mime_type: String,
    /// Size of the file in bytes.
    pub size_bytes: u64,
    /// Modification epoch seconds.
    pub modified_time: i64,
    /// Highlighted snippet excerpt if document content matched.
    pub snippet: Option<String>,
    /// Calculated composite hybrid relevance rank score.
    pub rank_score: f64,
}

/// Sanitizes user queries for SQLite FTS5 matching, stripping syntax characters,
/// escaping reserved boolean operators, and applying prefix matching.
#[must_use]
pub fn sanitize_fts5_query(raw: &str) -> String {
    let tokens: Vec<String> = raw
        .split_whitespace()
        .filter_map(|word| {
            // Strip FTS5 operators and punctuation: ", *, :, ^, (, )
            let cleaned: String = word
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            let trimmed = cleaned.trim_matches(|c| c == '-' || c == '_');
            if trimmed.is_empty() {
                return None;
            }

            let upper = trimmed.to_ascii_uppercase();
            if upper == "AND" || upper == "OR" || upper == "NOT" || upper == "NEAR" {
                Some(format!("\"{trimmed}\"*"))
            } else {
                Some(format!("{trimmed}*"))
            }
        })
        .collect();

    tokens.join(" ")
}

/// Builder for constructing and executing hybrid search queries against SQLite FTS5.
#[derive(Debug, Clone)]
pub struct SearchQueryBuilder {
    query: String,
    limit: usize,
}

impl SearchQueryBuilder {
    /// Creates a new search query builder with default limit (50).
    #[must_use]
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: 50,
        }
    }

    /// Sets the maximum number of search results to return.
    #[must_use]
    pub const fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Executes the search query against the provided SQLite database connection.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError::Db`] if SQLite query compilation or execution fails.
    pub fn execute(&self, conn: &Connection) -> Result<Vec<SearchHit>, IndexerError> {
        search_indexed_files(conn, &self.query, self.limit)
    }
}

/// Executes a sub-50ms hybrid FTS5 search query combining BM25 ranking,
/// filename prefix bonus, and recency decay.
///
/// # Errors
///
/// Returns [`IndexerError::Db`] if SQLite query preparation or execution fails.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
pub fn search_indexed_files(
    conn: &Connection,
    raw_query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, IndexerError> {
    let sanitized = sanitize_fts5_query(raw_query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }

    let trimmed_prefix = raw_query.trim();
    let now_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let limit_i64 = limit as i64;

    let sql = "\
SELECT r.path, r.filename, r.mime_type, r.size_bytes, r.modified_time,
       snippet(files_fts, 2, '<b>', '</b>', '...', 12) as snippet_text,
       bm25(files_fts) as fts_rank,
       (
           (CASE WHEN r.filename LIKE ?2 || '%' THEN 100.0 ELSE 0.0 END) +
           (1.0 / (1.0 + bm25(files_fts))) * 50.0 +
           (1.0 / (1.0 + (?3 - r.modified_time) / 604800.0)) * 20.0
       ) as rank_score
FROM files_fts
JOIN file_records r ON files_fts.rowid = r.id
WHERE files_fts MATCH ?1
ORDER BY rank_score DESC
LIMIT ?4;";

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        params![sanitized, trimmed_prefix, now_epoch, limit_i64],
        |row| {
            let path_str: String = row.get(0)?;
            let filename: String = row.get(1)?;
            let mime_type: String = row.get(2)?;
            let size_bytes: i64 = row.get(3)?;
            let modified_time: i64 = row.get(4)?;
            let snippet_text: Option<String> = row.get(5)?;
            let rank_score: f64 = row.get(7)?;

            let snippet = match snippet_text {
                Some(s) if !s.is_empty() => Some(s),
                _ => None,
            };

            Ok(SearchHit {
                path: PathBuf::from(path_str),
                filename,
                mime_type,
                size_bytes: size_bytes as u64,
                modified_time,
                snippet,
                rank_score,
            })
        },
    )?;

    let mut hits = Vec::new();
    for row in rows {
        hits.push(row?);
    }
    Ok(hits)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::db::migrations::apply_migrations;
    use crate::indexer::pipeline::IndexPipeline;
    use std::fs::File;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_sanitization_removes_syntax_characters() {
        let clean = sanitize_fts5_query("\"hello\" *world* ^test:item AND OR NOT NEAR");
        assert!(clean.contains("hello*"));
        assert!(clean.contains("world*"));
        assert!(clean.contains("testitem*"));
        assert!(clean.contains("\"AND\"*"));
        assert!(clean.contains("\"OR\"*"));
    }

    #[test]
    fn test_hybrid_search_query_execution_and_ranking() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations(&mut conn).unwrap();
        let conn_arc = Arc::new(Mutex::new(conn));
        let mut pipeline = IndexPipeline::new(Arc::clone(&conn_arc));

        let temp_dir = tempfile::tempdir().unwrap();

        // 1. File starting with 'search' (gets filename prefix bonus)
        let file1 = temp_dir.path().join("search_engine.rs");
        {
            let mut f = File::create(&file1).unwrap();
            writeln!(
                f,
                "pub struct Engine; // Ultra fast text search indexing implementation"
            )
            .unwrap();
        }

        // 2. File with search inside content only
        let file2 = temp_dir.path().join("doc.txt");
        {
            let mut f = File::create(&file2).unwrap();
            writeln!(f, "This document contains search algorithms.").unwrap();
        }

        // 3. Unrelated file
        let file3 = temp_dir.path().join("cars.txt");
        {
            let mut f = File::create(&file3).unwrap();
            writeln!(f, "Automotive vehicles and wheels.").unwrap();
        }

        pipeline.enqueue_upsert(file1.clone()).unwrap();
        pipeline.enqueue_upsert(file2.clone()).unwrap();
        pipeline.enqueue_upsert(file3).unwrap();
        pipeline.flush().unwrap();

        let conn = conn_arc.lock().unwrap();

        let hits = search_indexed_files(&conn, "search", 10).unwrap();
        assert_eq!(hits.len(), 2);

        // search_engine.rs should have the highest rank score due to filename prefix bonus
        assert_eq!(hits[0].path, file1);
        assert!(hits[0].rank_score > hits[1].rank_score);
        assert!(hits[0].rank_score > 100.0);

        // Verify snippet extraction
        assert!(hits[0].snippet.is_some());
        let snip = hits[0].snippet.as_ref().unwrap();
        assert!(snip.contains("<b>search</b>") || snip.contains("search"));

        // Test builder
        let builder_hits = SearchQueryBuilder::new("search")
            .limit(5)
            .execute(&conn)
            .unwrap();
        assert_eq!(builder_hits.len(), 2);
    }
}
