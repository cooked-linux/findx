//! Real-time file and document content search provider for Spotlight.
//!
//! Connecting directly to [`shared::indexer::search_indexed_files`], this provider
//! executes sub-50ms hybrid FTS5 queries matching both filenames and document bodies,
//! mapping [`SearchHit`] results into canonical [`SearchRecord`] items under
//! [`SearchCategory::Documents`] and [`SearchCategory::Folders`].

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gdk4::prelude::*;
use rusqlite::Connection;

use shared::indexer::{SearchHit, search_indexed_files};
use shared::ipc::{FileManagerClient, path_to_file_uri};
use shared::models::{SearchCategory, SearchPayload, SearchRecord};

use crate::arbiter::dispatcher::QueryScope;
use crate::arbiter::provider::{ProviderError, SearchProvider};

/// Default limit for file search result candidates.
pub const DEFAULT_FILE_LIMIT: usize = 20;

/// Search provider executing real-time hybrid SQLite FTS5 queries against indexed files.
pub struct FileSearchProvider {
    conn: Arc<Mutex<Connection>>,
    limit: usize,
}

impl Clone for FileSearchProvider {
    fn clone(&self) -> Self {
        Self {
            conn: Arc::clone(&self.conn),
            limit: self.limit,
        }
    }
}

impl fmt::Debug for FileSearchProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileSearchProvider")
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

impl FileSearchProvider {
    /// Constructs a new [`FileSearchProvider`] with the given database connection and result limit.
    #[must_use]
    pub const fn new(conn: Arc<Mutex<Connection>>, limit: usize) -> Self {
        Self { conn, limit }
    }

    /// Opens a read-only SQLite database connection at `path` configured with WAL pragmas.
    ///
    /// # Errors
    ///
    /// Returns [`rusqlite::Error`] if opening the database file fails.
    pub fn open_readonly(path: impl AsRef<Path>, limit: usize) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        let _ = conn.execute_batch("PRAGMA busy_timeout = 5000;");
        Ok(Self::new(Arc::new(Mutex::new(conn)), limit))
    }

    /// Opens the default system database at `~/.local/share/findx/findx.db` in read-only mode.
    ///
    /// # Errors
    ///
    /// Returns an error message if resolving the XDG directory or opening the database fails.
    pub fn open_default(limit: usize) -> Result<Self, String> {
        let path = shared::db::default_db_path().map_err(|e| e.to_string())?;
        Self::open_readonly(&path, limit).map_err(|e| e.to_string())
    }

    /// Returns the maximum number of file results retrieved per query.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Sets the maximum number of file results retrieved per query.
    pub const fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
    }
}

/// Converts a [`SearchHit`] into a canonical Spotlight [`SearchRecord`].
#[must_use]
pub fn hit_to_search_record(hit: SearchHit) -> SearchRecord {
    let is_dir = hit.path.is_dir() || hit.mime_type == "inode/directory";
    let category = if is_dir {
        SearchCategory::Folders
    } else {
        SearchCategory::Documents
    };

    let icon_name = if is_dir {
        "folder".to_string()
    } else {
        "x-office-document".to_string()
    };

    let subtitle = hit
        .snippet
        .clone()
        .or_else(|| hit.path.parent().map(|p| p.to_string_lossy().into_owned()));

    let payload = SearchPayload::File {
        path: hit.path.clone(),
    };

    SearchRecord::new(
        format!("file:{}", hit.path.to_string_lossy()),
        hit.filename,
        subtitle,
        category,
        Some(icon_name),
        hit.rank_score,
        payload,
    )
}

#[async_trait::async_trait]
impl SearchProvider for FileSearchProvider {
    fn id(&self) -> &'static str {
        "files"
    }

    fn display_name(&self) -> &'static str {
        "Files"
    }

    fn is_fast_path(&self) -> bool {
        false
    }

    fn handles_scope(&self, scope: QueryScope) -> bool {
        matches!(scope, QueryScope::All | QueryScope::Files)
    }

    async fn search(
        &self,
        query: &str,
        _filter: Option<&str>,
        deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError> {
        if Instant::now() >= deadline {
            return Err(ProviderError::Timeout(Duration::from_millis(0)));
        }

        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn.lock().map_err(|e| {
            ProviderError::Failure(format!("Failed to lock sqlite connection: {e}"))
        })?;

        let hits = search_indexed_files(&conn, trimmed_query, self.limit)
            .map_err(|e| ProviderError::Failure(e.to_string()))?;
        drop(conn);

        if Instant::now() >= deadline {
            return Err(ProviderError::Timeout(deadline.elapsed()));
        }

        let records = hits.into_iter().map(hit_to_search_record).collect();
        Ok(records)
    }
}

/// Opens a file or folder using the default system desktop handler via `xdg-open`.
///
/// # Errors
///
/// Returns an [`std::io::Error`] if spawning the child process fails.
pub fn open_file(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("xdg-open").arg(path).spawn()?;
    Ok(())
}

/// Reveals a file or folder in the default desktop file manager via D-Bus IPC.
///
/// # Errors
///
/// Returns an error if URI conversion or file manager communication fails.
pub async fn show_in_file_manager(path: &Path) -> anyhow::Result<()> {
    let uri = path_to_file_uri(path)?;
    let client = FileManagerClient::new();
    client.show_items(&[uri], "").await?;
    Ok(())
}

/// Copies the absolute canonical filesystem path to the system clipboard.
///
/// Returns `true` if a clipboard surface was available and the text was dispatched.
#[must_use]
pub fn copy_file_path_to_clipboard(path: &Path) -> bool {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = canonical.to_string_lossy();
    gdk4::Display::default().is_some_and(|display| {
        display.clipboard().set_text(&path_str);
        true
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_hit_to_search_record_mapping() {
        let hit = SearchHit {
            path: PathBuf::from("/tmp/test_report.pdf"),
            filename: "test_report.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            size_bytes: 1024,
            modified_time: 1_700_000_000,
            snippet: Some("Content match <b>report</b> text".to_string()),
            rank_score: 95.5,
        };

        let record = hit_to_search_record(hit);
        assert_eq!(record.id, "file:/tmp/test_report.pdf");
        assert_eq!(record.title, "test_report.pdf");
        assert_eq!(
            record.subtitle,
            Some("Content match <b>report</b> text".to_string())
        );
        assert_eq!(record.category, SearchCategory::Documents);
        assert_eq!(record.icon_name, Some("x-office-document".to_string()));
        assert!((record.score - 95.5).abs() < f64::EPSILON);
        assert_eq!(
            record.payload,
            SearchPayload::File {
                path: PathBuf::from("/tmp/test_report.pdf")
            }
        );
    }

    #[test]
    fn test_hit_to_search_record_folder_mapping() {
        let hit = SearchHit {
            path: PathBuf::from("/tmp/projects"),
            filename: "projects".to_string(),
            mime_type: "inode/directory".to_string(),
            size_bytes: 4096,
            modified_time: 1_700_000_000,
            snippet: None,
            rank_score: 110.0,
        };

        let record = hit_to_search_record(hit);
        assert_eq!(record.id, "file:/tmp/projects");
        assert_eq!(record.title, "projects");
        assert_eq!(record.subtitle, Some("/tmp".to_string()));
        assert_eq!(record.category, SearchCategory::Folders);
        assert_eq!(record.icon_name, Some("folder".to_string()));
    }
}
