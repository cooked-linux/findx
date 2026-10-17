//! SQLite ingestion coordinator and batch queue processor.
//!
//! Buffers scanned and modified files in memory, periodically committing
//! them in bulk SQLite transactions every 500ms or 1,000 files to minimize
//! I/O overhead and database lock contention.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{Connection, params};
use std::os::unix::fs::MetadataExt;
use tokio::time::Instant;

use super::crawler::{DirectoryCrawler, DiscoveredFile};
use super::extractor::extract_file_content_sync;
use super::watcher::FsEvent;
use crate::indexer::IndexerError;

/// Default commit batch size threshold (1,000 files).
pub const DEFAULT_BATCH_THRESHOLD: usize = 1000;

/// Default commit interval threshold (500 milliseconds).
pub const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Action queued for database ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineOp {
    /// Insert or update the file record and FTS virtual table.
    Upsert(PathBuf),
    /// Delete the file from both tables.
    Delete(PathBuf),
}

/// Batch ingestion pipeline coordinating crawled files and live watcher events.
pub struct IndexPipeline {
    conn: Arc<Mutex<Connection>>,
    batch_queue: Vec<PipelineOp>,
    batch_threshold: usize,
    flush_interval: Duration,
    last_flush: Instant,
}

impl IndexPipeline {
    /// Creates a new [`IndexPipeline`] instance with default thresholds.
    #[must_use]
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self::with_config(conn, DEFAULT_BATCH_THRESHOLD, DEFAULT_FLUSH_INTERVAL)
    }

    /// Creates an [`IndexPipeline`] with custom batch size and flush interval thresholds.
    #[must_use]
    pub fn with_config(
        conn: Arc<Mutex<Connection>>,
        batch_threshold: usize,
        flush_interval: Duration,
    ) -> Self {
        Self {
            conn,
            batch_queue: Vec::with_capacity(batch_threshold),
            batch_threshold,
            flush_interval,
            last_flush: Instant::now(),
        }
    }

    /// Enqueues a discovered file for indexing, flushing if batch threshold is reached.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if transaction commit fails.
    pub fn enqueue_discovered(&mut self, file: DiscoveredFile) -> Result<(), IndexerError> {
        self.enqueue_upsert(file.path)
    }

    /// Enqueues a path for upsert into the index.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if threshold flush fails.
    pub fn enqueue_upsert(&mut self, path: PathBuf) -> Result<(), IndexerError> {
        self.batch_queue.push(PipelineOp::Upsert(path));
        self.flush_if_needed()?;
        Ok(())
    }

    /// Enqueues a path deletion from the index.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if threshold flush fails.
    pub fn enqueue_delete(&mut self, path: PathBuf) -> Result<(), IndexerError> {
        self.batch_queue.push(PipelineOp::Delete(path));
        self.flush_if_needed()?;
        Ok(())
    }

    /// Enqueues a filesystem event emitted by the watcher.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if threshold flush fails.
    pub fn enqueue_event(&mut self, event: FsEvent) -> Result<(), IndexerError> {
        match event {
            FsEvent::Created(p) | FsEvent::Modified(p) => self.enqueue_upsert(p),
            FsEvent::Deleted(p) => self.enqueue_delete(p),
            FsEvent::Moved { from, to } => {
                self.enqueue_delete(from)?;
                self.enqueue_upsert(to)
            }
        }
    }

    /// Flushes queued operations if batch size or time interval thresholds are exceeded.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if transaction execution fails.
    pub fn flush_if_needed(&mut self) -> Result<usize, IndexerError> {
        if self.batch_queue.len() >= self.batch_threshold
            || self.last_flush.elapsed() >= self.flush_interval
        {
            self.flush()
        } else {
            Ok(0)
        }
    }

    /// Flushes all queued operations into SQLite within a single transaction.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if acquiring the database lock or committing fails.
    #[allow(clippy::cast_possible_wrap, clippy::significant_drop_tightening)]
    pub fn flush(&mut self) -> Result<usize, IndexerError> {
        if self.batch_queue.is_empty() {
            return Ok(0);
        }

        let operations = std::mem::take(&mut self.batch_queue);
        let count = operations.len();

        let mut conn = self
            .conn
            .lock()
            .map_err(|e| IndexerError::Watcher(e.to_string()))?;
        let tx = conn.transaction()?;

        {
            let mut del_fts_stmt = tx.prepare_cached(
                "DELETE FROM files_fts WHERE rowid = (SELECT id FROM file_records WHERE path = ?1);",
            )?;
            let mut del_rec_stmt =
                tx.prepare_cached("DELETE FROM file_records WHERE path = ?1;")?;
            let mut ins_rec_stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO file_records (device_id, inode, path, filename, mime_type, size_bytes, modified_time, indexed_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
            )?;
            let mut ins_fts_stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO files_fts (rowid, path, filename, content, mime_type, tags)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            )?;

            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            for op in operations {
                match op {
                    PipelineOp::Delete(path) => {
                        let path_str = path.to_string_lossy();
                        del_fts_stmt.execute(params![path_str])?;
                        del_rec_stmt.execute(params![path_str])?;
                    }
                    PipelineOp::Upsert(path) => {
                        let path_str = path.to_string_lossy();
                        let Ok(meta) = std::fs::metadata(&path) else {
                            // File was deleted or unlinked before processing
                            del_fts_stmt.execute(params![path_str])?;
                            del_rec_stmt.execute(params![path_str])?;
                            continue;
                        };

                        let filename = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();

                        let mime_type = mime_guess::from_path(&path)
                            .first_or_octet_stream()
                            .to_string();

                        let content = extract_file_content_sync(&path).unwrap_or_default();
                        let tags = crate::fs::get_tags(&path)
                            .map(|tags| {
                                tags.into_iter()
                                    .map(|t| t.name)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_default();

                        // Delete existing FTS record associated with this path if it already exists
                        del_fts_stmt.execute(params![path_str])?;

                        ins_rec_stmt.execute(params![
                            meta.dev() as i64,
                            meta.ino() as i64,
                            path_str,
                            filename,
                            mime_type,
                            meta.len() as i64,
                            meta.mtime(),
                            now_epoch,
                        ])?;

                        let row_id = tx.last_insert_rowid();

                        ins_fts_stmt.execute(params![
                            row_id, path_str, filename, content, mime_type, tags,
                        ])?;
                    }
                }
            }
        }

        tx.commit()?;
        self.last_flush = Instant::now();
        Ok(count)
    }

    /// Convenience helper executing a directory crawler and ingesting all results.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if crawling or database flushing fails.
    pub fn ingest_crawler(&mut self, crawler: &DirectoryCrawler) -> Result<usize, IndexerError> {
        let mut total = 0;
        for file in crawler.crawl() {
            let item = file?;
            self.enqueue_discovered(item)?;
            total += 1;
        }
        self.flush()?;
        Ok(total)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::db::migrations::apply_migrations;
    use std::fs::File;
    use std::io::Write;

    fn setup_test_db() -> Arc<Mutex<Connection>> {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_migrations(&mut conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn test_pipeline_upsert_and_deletion() {
        let db = setup_test_db();
        let mut pipeline = IndexPipeline::new(Arc::clone(&db));

        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("doc.md");

        // 1. Create file and enqueue upsert
        {
            let mut f = File::create(&file_path).unwrap();
            writeln!(f, "# Title\nFindX high performance search engine.").unwrap();
        }

        pipeline.enqueue_upsert(file_path.clone()).unwrap();
        pipeline.flush().unwrap();

        // Verify inserted into both tables
        {
            let conn = db.lock().unwrap();
            let count_records: i64 = conn
                .query_row("SELECT COUNT(1) FROM file_records;", [], |r| r.get(0))
                .unwrap();
            let count_fts: i64 = conn
                .query_row("SELECT COUNT(1) FROM files_fts;", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count_records, 1);
            assert_eq!(count_fts, 1);

            let rowid: i64 = conn
                .query_row(
                    "SELECT rowid FROM files_fts WHERE files_fts MATCH 'performance';",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let rec_id: i64 = conn
                .query_row("SELECT id FROM file_records;", [], |r| r.get(0))
                .unwrap();
            assert_eq!(rowid, rec_id);
        }

        // 2. Modify file and upsert again
        {
            let mut f = File::create(&file_path).unwrap();
            writeln!(f, "# Updated\nQuantum distributed indexing.").unwrap();
        }

        pipeline.enqueue_upsert(file_path.clone()).unwrap();
        pipeline.flush().unwrap();

        // Ensure no duplicate rows were generated
        {
            let conn = db.lock().unwrap();
            let count_records: i64 = conn
                .query_row("SELECT COUNT(1) FROM file_records;", [], |r| r.get(0))
                .unwrap();
            let count_fts: i64 = conn
                .query_row("SELECT COUNT(1) FROM files_fts;", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count_records, 1);
            assert_eq!(count_fts, 1);
        }

        // 3. Delete file and enqueue delete
        pipeline.enqueue_delete(file_path).unwrap();
        pipeline.flush().unwrap();

        {
            let conn = db.lock().unwrap();
            let count_records: i64 = conn
                .query_row("SELECT COUNT(1) FROM file_records;", [], |r| r.get(0))
                .unwrap();
            let count_fts: i64 = conn
                .query_row("SELECT COUNT(1) FROM files_fts;", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count_records, 0);
            assert_eq!(count_fts, 0);
        }
    }
}
