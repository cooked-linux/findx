//! Hybrid smart folder query engine with FTS5 acceleration, filesystem fallback
//! crawling, and monotonic cancellation tokens.

use std::collections::HashSet;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rayon::prelude::*;
use rusqlite::Connection;
use shared::indexer::{is_ignored_fs_path, search_indexed_files};
use shared::models::{FileItem, FileMetadata};
use walkdir::WalkDir;

use super::compiler::{CompiledQuery, compile_definition};
use super::error::{Result, SmartFolderError};
use super::types::SmartFolderDefinition;

/// Maximum number of FTS5 candidates pulled before in-memory filtering.
const FTS_CANDIDATE_LIMIT: usize = 5_000;

/// Maximum recursion depth for fallback filesystem scans.
const SCAN_MAX_DEPTH: usize = 64;

/// Query identifier used to discard stale live evaluations.
pub type QueryId = u64;

/// Result bundle produced by a smart folder query execution.
#[derive(Debug, Clone)]
pub struct SmartFolderResults {
    /// Monotonic query identifier that produced this result set.
    pub query_id: QueryId,
    /// Matching filesystem items sorted by natural file name order.
    pub items: Vec<FileItem>,
    /// Non-fatal warnings encountered while compiling criteria.
    pub warnings: Vec<String>,
    /// Readable scope roots actually evaluated.
    pub active_scopes: Vec<PathBuf>,
}

/// Slot holding an optional SQLite connection for hybrid FTS5 acceleration.
#[derive(Clone, Debug)]
enum DatabaseSlot {
    /// FTS5 acceleration is available.
    Ready(Arc<Mutex<Connection>>),
    /// Filesystem-only degraded mode.
    Unavailable,
}

/// Hybrid smart folder execution engine backed by SQLite FTS5.
#[derive(Clone, Debug)]
pub struct SmartFolderEngine {
    db_conn: DatabaseSlot,
    pub(crate) current_query_id: Arc<AtomicU64>,
}

impl SmartFolderEngine {
    /// Constructs an engine from an existing shared SQLite connection.
    #[must_use]
    pub fn new(db_conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db_conn: DatabaseSlot::Ready(db_conn),
            current_query_id: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Opens the default findx database, falling back to an in-memory database.
    #[must_use]
    pub fn open_default() -> Self {
        match shared::Database::open_default() {
            Ok(db) => Self::new(Arc::new(Mutex::new(db.into_inner()))),
            Err(error) => {
                tracing::warn!("Falling back to in-memory smart folder database: {error}");
                Self::in_memory()
            }
        }
    }

    /// Creates an in-memory migrated database engine, primarily for tests.
    #[must_use]
    pub fn in_memory() -> Self {
        let Ok(mut conn) = Connection::open_in_memory() else {
            tracing::error!("Unable to open in-memory smart folder database");
            return Self::without_database();
        };
        if let Err(error) = shared::db::apply_migrations(&mut conn) {
            tracing::warn!("Unable to migrate in-memory smart folder database: {error}");
        }
        Self::new(Arc::new(Mutex::new(conn)))
    }

    /// Creates an engine that degrades gracefully to filesystem-only scanning.
    #[must_use]
    pub fn without_database() -> Self {
        Self {
            db_conn: DatabaseSlot::Unavailable,
            current_query_id: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Runs `callback` against the shared SQLite connection when FTS is available.
    pub fn with_connection<T>(&self, callback: impl FnOnce(&Connection) -> T) -> Option<T> {
        match &self.db_conn {
            DatabaseSlot::Ready(shared) => {
                let guard = shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Some(callback(&guard))
            }
            DatabaseSlot::Unavailable => None,
        }
    }

    /// Atomically allocates the next monotonic query identifier.
    #[must_use]
    pub fn next_query_id(&self) -> QueryId {
        self.current_query_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Returns the identifier of the most recently dispatched query.
    #[must_use]
    pub fn latest_query_id(&self) -> QueryId {
        self.current_query_id.load(Ordering::Relaxed)
    }

    /// Returns `true` when `query_id` is still the freshest dispatched query.
    #[must_use]
    pub fn is_current(&self, query_id: QueryId) -> bool {
        self.latest_query_id() == query_id
    }

    /// Invalidates all in-flight evaluations by advancing the query counter.
    pub fn cancel_in_flight(&self) {
        self.current_query_id.fetch_add(1, Ordering::Relaxed);
    }

    /// Compiles a smart folder definition without executing it.
    #[must_use]
    pub fn compile(&self, definition: &SmartFolderDefinition) -> CompiledQuery {
        compile_definition(definition)
    }

    /// Allocates a new query identifier and executes the supplied definition.
    ///
    /// # Errors
    ///
    /// Returns [`SmartFolderError::Cancelled`] when a newer query supersedes this evaluation.
    pub fn execute(&self, definition: &SmartFolderDefinition) -> Result<SmartFolderResults> {
        let query_id = self.next_query_id();
        self.execute_with_id(definition, query_id)
    }

    /// Executes a definition under an existing query identifier.
    ///
    /// # Errors
    ///
    /// Returns [`SmartFolderError::Cancelled`] when the identifier is stale.
    pub fn execute_with_id(
        &self,
        definition: &SmartFolderDefinition,
        query_id: QueryId,
    ) -> Result<SmartFolderResults> {
        let compiled = self.compile(definition);
        self.execute_compiled(&compiled, query_id)
    }

    /// Executes an already-compiled query, checking cancellation at boundaries.
    ///
    /// # Errors
    ///
    /// Returns [`SmartFolderError::Cancelled`] when `query_id` is no longer current.
    #[allow(clippy::unnecessary_wraps)]
    pub fn execute_compiled(
        &self,
        compiled: &CompiledQuery,
        query_id: QueryId,
    ) -> Result<SmartFolderResults> {
        if !self.is_current(query_id) {
            return Err(SmartFolderError::Cancelled);
        }

        let mut warnings = compiled.warnings.clone();
        let mut candidate_paths = HashSet::new();

        if let Some(fts_query) = compiled.fts5_query.as_deref() {
            self.collect_fts_candidates(
                fts_query,
                &compiled.active_scopes,
                &mut candidate_paths,
                &mut warnings,
            );
        }

        self.collect_filesystem_candidates(
            &compiled.active_scopes,
            &mut candidate_paths,
            query_id,
        )?;

        if !self.is_current(query_id) {
            return Err(SmartFolderError::Cancelled);
        }

        let paths: Vec<PathBuf> = candidate_paths.into_iter().collect();
        let filter = &compiled.filter_fn;
        let mut items: Vec<FileItem> = paths
            .par_iter()
            .filter_map(|path| file_item_from_path(path))
            .filter(|item| filter(item))
            .collect();

        items.sort_by(|a, b| shared::fs::natural_cmp(a.file_name(), b.file_name()));

        Ok(SmartFolderResults {
            query_id,
            items,
            warnings,
            active_scopes: compiled.active_scopes.clone(),
        })
    }

    fn collect_fts_candidates(
        &self,
        fts_query: &str,
        active_scopes: &[PathBuf],
        candidates: &mut HashSet<PathBuf>,
        warnings: &mut Vec<String>,
    ) {
        if active_scopes.is_empty() {
            return;
        }

        let hits =
            self.with_connection(|conn| search_indexed_files(conn, fts_query, FTS_CANDIDATE_LIMIT));
        match hits {
            Some(Ok(hits)) => {
                for hit in hits {
                    if active_scopes
                        .iter()
                        .any(|scope| hit.path.starts_with(scope))
                        && hit.path.exists()
                    {
                        candidates.insert(hit.path);
                    }
                }
            }
            Some(Err(error)) => {
                warnings.push(format!("FTS5 query failed, using filesystem scan: {error}"));
            }
            None => {}
        }
    }

    fn collect_filesystem_candidates(
        &self,
        active_scopes: &[PathBuf],
        candidates: &mut HashSet<PathBuf>,
        query_id: QueryId,
    ) -> Result<()> {
        for scope in active_scopes {
            if !self.is_current(query_id) {
                return Err(SmartFolderError::Cancelled);
            }

            let walker = WalkDir::new(scope)
                .max_depth(SCAN_MAX_DEPTH)
                .follow_links(false)
                .into_iter()
                .filter_entry(|entry| !is_ignored_fs_path(entry.path()));

            for entry in walker {
                if !self.is_current(query_id) {
                    return Err(SmartFolderError::Cancelled);
                }

                let Ok(entry) = entry else {
                    continue;
                };
                let path = entry.path();
                if path == scope || !(entry.file_type().is_file() || entry.file_type().is_dir()) {
                    continue;
                }
                candidates.insert(path.to_path_buf());
            }
        }
        Ok(())
    }
}

/// Builds a full [`FileItem`] from filesystem metadata, tags, and MIME sniffing.
#[must_use]
pub fn file_item_from_path(path: &Path) -> Option<FileItem> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    let file_name = path.file_name().map_or_else(
        || path.to_string_lossy().to_string(),
        |name| name.to_string_lossy().to_string(),
    );
    let is_dir = metadata.file_type().is_dir();
    let is_symlink = metadata.file_type().is_symlink();

    let mime_type = if is_dir {
        "inode/directory".to_string()
    } else {
        shared::mime::detect_mime_type(path)
            .unwrap_or_else(|_| "application/octet-stream".to_string())
    };

    let tags = shared::fs::get_tags(path).unwrap_or_default();

    Some(FileItem::new(
        path.to_path_buf(),
        file_name.clone(),
        is_dir,
        is_symlink,
        if is_symlink {
            std::fs::read_link(path).ok()
        } else {
            None
        },
        FileMetadata {
            size_bytes: metadata.len(),
            modified: metadata.modified().ok(),
            accessed: metadata.accessed().ok(),
            created: metadata.created().ok(),
            permissions: metadata.permissions().mode(),
            inode: metadata.ino(),
            device_id: metadata.dev(),
            mime_type,
            is_hidden: file_name.starts_with('.'),
            is_executable: (metadata.permissions().mode() & 0o111) != 0,
        },
        tags,
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::smart_folders::types::{
        BooleanMode, CriteriaAttribute, CriteriaGroup, CriteriaOperator, CriteriaSlice,
    };

    fn test_engine() -> SmartFolderEngine {
        SmartFolderEngine::in_memory()
    }

    #[test]
    fn test_engine_scans_filesystem_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("report.pdf");
        let txt = dir.path().join("notes.txt");
        std::fs::write(&pdf, b"%PDF-1.4 test").unwrap();
        std::fs::write(&txt, b"hello").unwrap();

        let mut def = SmartFolderDefinition::new(
            "PDFs",
            CriteriaGroup::all(vec![CriteriaSlice::new(
                CriteriaAttribute::Kind,
                CriteriaOperator::Equals,
                "PDF",
            )]),
        );
        def.scopes = vec![dir.path().to_path_buf()];

        let engine = test_engine();
        let results = engine.execute(&def).unwrap();
        assert_eq!(results.items.len(), 1);
        assert_eq!(results.items[0].file_name, "report.pdf");
    }

    #[test]
    fn test_engine_combines_fts_and_filesystem_results() {
        let dir = tempfile::tempdir().unwrap();
        let indexed = dir.path().join("report-indexed.txt");
        let unindexed = dir.path().join("report-fallback.txt");
        std::fs::write(&indexed, b"report contents").unwrap();
        std::fs::write(&unindexed, b"report contents").unwrap();

        let engine = test_engine();
        engine
            .with_connection(|conn| {
                shared::db::fts::index_document(
                    conn,
                    &shared::db::fts::FtsDocument::new(
                        indexed.to_string_lossy(),
                        "report-indexed.txt",
                        "quarterly report",
                        "text/plain",
                        "",
                    ),
                )
            })
            .unwrap()
            .unwrap();

        let mut def = SmartFolderDefinition::new(
            "Reports",
            CriteriaGroup::all(vec![CriteriaSlice::new(
                CriteriaAttribute::Name,
                CriteriaOperator::Contains,
                "report",
            )]),
        );
        def.scopes = vec![dir.path().to_path_buf()];

        let results = engine.execute(&def).unwrap();
        let names: Vec<&str> = results.items.iter().map(FileItem::file_name).collect();
        assert_eq!(names, vec!["report-fallback.txt", "report-indexed.txt"]);
    }

    #[test]
    fn test_stale_query_is_cancelled() {
        let engine = test_engine();
        let def = SmartFolderDefinition::new("Empty", CriteriaGroup::new(BooleanMode::All));
        let id = engine.next_query_id();
        engine.cancel_in_flight();
        assert!(matches!(
            engine.execute_with_id(&def, id),
            Err(SmartFolderError::Cancelled)
        ));
    }

    #[test]
    fn test_file_item_from_path_reads_metadata() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"payload").unwrap();
        let item = file_item_from_path(file.path()).unwrap();
        assert_eq!(item.metadata.size_bytes, 7);
        assert!(!item.is_dir);
    }
}
