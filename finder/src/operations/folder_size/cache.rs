//! Two-tier caching architecture for folder sizes (in-memory LRU + persistent SQLite).

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, SystemTime};

use lru::LruCache;
use parking_lot::{Mutex, RwLock};
use rusqlite::Connection;

use super::db::{
    self, delete_folder_size_record, open_cache_db, open_in_memory_db, upsert_folder_size_record,
};
use super::models::{FolderSizeError, FolderSizeResult};

/// Default capacity for the L1 in-memory LRU cache.
pub const DEFAULT_LRU_CAPACITY: usize = 10_000;

/// Background database operations dispatched to the SQLite persistence worker.
enum DbOp {
    Upsert {
        path: PathBuf,
        dev_id: u64,
        inode: u64,
        result: FolderSizeResult,
        seq: u64,
    },
    Invalidate(Vec<PathBuf>),
    Flush(std_mpsc::Sender<()>),
}

/// Two-tier folder size cache providing nanosecond-precision mtime validation,
/// thread-safe in-memory LRU, and asynchronous SQLite persistence.
pub struct FolderSizeCache {
    l1_cache: Arc<RwLock<LruCache<PathBuf, FolderSizeResult>>>,
    db_conn: Arc<Mutex<Connection>>,
    db_tx: async_channel::Sender<DbOp>,
    path_epochs: Arc<RwLock<HashMap<PathBuf, u64>>>,
    inserted_epochs: Arc<RwLock<HashMap<PathBuf, u64>>>,
    seq_counter: Arc<AtomicU64>,
}

impl FolderSizeCache {
    /// Creates a new [`FolderSizeCache`] backed by the default persistent SQLite cache at
    /// `~/.cache/findx/folder_sizes.sqlite` and an in-memory LRU of 10,000 entries.
    ///
    /// # Errors
    ///
    /// Returns [`FolderSizeError`] if SQLite connection initialization fails.
    pub fn new() -> Result<Self, FolderSizeError> {
        Self::with_db_path(None)
    }

    /// Creates a new [`FolderSizeCache`] backed by an in-memory SQLite database,
    /// primarily for testing and isolated sessions.
    ///
    /// # Errors
    ///
    /// Returns [`FolderSizeError`] if in-memory database creation fails.
    pub fn new_in_memory() -> Result<Self, FolderSizeError> {
        let conn = open_in_memory_db()?;
        Self::from_connection(conn)
    }

    /// Creates a new [`FolderSizeCache`] with a specified custom SQLite database file path.
    ///
    /// # Errors
    ///
    /// Returns [`FolderSizeError`] if opening the database at the specified path fails.
    pub fn with_db_path(path: Option<&Path>) -> Result<Self, FolderSizeError> {
        let conn = open_cache_db(path)?;
        Self::from_connection(conn)
    }

    /// Constructs the two-tier cache from an existing open [`Connection`].
    fn from_connection(conn: Connection) -> Result<Self, FolderSizeError> {
        let capacity = NonZeroUsize::new(DEFAULT_LRU_CAPACITY).unwrap_or(NonZeroUsize::MIN);
        let l1_cache = Arc::new(RwLock::new(LruCache::new(capacity)));
        let db_conn = Arc::new(Mutex::new(conn));
        let path_epochs = Arc::new(RwLock::new(HashMap::new()));
        let inserted_epochs = Arc::new(RwLock::new(HashMap::new()));
        let seq_counter = Arc::new(AtomicU64::new(1));

        let (db_tx, db_rx) = async_channel::unbounded::<DbOp>();
        let worker_conn = Arc::clone(&db_conn);
        let worker_epochs = Arc::clone(&path_epochs);

        // Spawn dedicated background worker thread for asynchronous SQLite persistence
        std::thread::Builder::new()
            .name("findx-folder-size-db".to_string())
            .spawn(move || {
                while let Ok(op) = db_rx.recv_blocking() {
                    match op {
                        DbOp::Upsert {
                            path,
                            dev_id,
                            inode,
                            result,
                            seq,
                        } => {
                            // Discard upsert if path was invalidated after insertion was scheduled
                            if let Some(&inval_seq) = worker_epochs.read().get(&path)
                                && seq <= inval_seq
                            {
                                continue;
                            }

                            let guard = worker_conn.lock();
                            if let Err(err) =
                                upsert_folder_size_record(&guard, &path, dev_id, inode, &result)
                            {
                                tracing::warn!(
                                    path = %path.display(),
                                    error = %err,
                                    "failed writing folder size record to sqlite cache"
                                );
                            }
                        }
                        DbOp::Invalidate(paths) => {
                            let guard = worker_conn.lock();
                            for p in &paths {
                                let _ = delete_folder_size_record(&guard, p);
                            }
                        }
                        DbOp::Flush(ack) => {
                            let _ = ack.send(());
                        }
                    }
                }
            })
            .map_err(|e| FolderSizeError::Io {
                path: PathBuf::from("db_worker_thread"),
                source: e,
            })?;

        Ok(Self {
            l1_cache,
            db_conn,
            db_tx,
            path_epochs,
            inserted_epochs,
            seq_counter,
        })
    }

    /// Queries the cache for a directory's size results.
    ///
    /// 1. Checks Layer 1 (LRU memory cache): if present and `dir_mtime == current_mtime`, returns entry.
    /// 2. Checks Layer 2 (SQLite database): if present and `dir_mtime == current_mtime`, promotes to Layer 1.
    /// 3. If present but stale (`dir_mtime != current_mtime`), invalidates the entry and returns `None`.
    #[must_use]
    pub fn get(&self, path: &Path, current_mtime: SystemTime) -> Option<FolderSizeResult> {
        // If this path was invalidated in this session and not re-inserted after, return None
        if let Some(&inval_seq) = self.path_epochs.read().get(path) {
            let ins_seq = self.inserted_epochs.read().get(path).copied().unwrap_or(0);
            if ins_seq <= inval_seq {
                return None;
            }
        }

        // 1. Layer 1 LRU check
        {
            let mut l1 = self.l1_cache.write();
            if let Some(entry) = l1.get(path) {
                if entry.dir_mtime == current_mtime {
                    return Some(*entry);
                }
                // Stale entry in L1
                l1.pop(path);
                drop(l1);
                self.invalidate(path);
                return None;
            }
        }

        // 2. Layer 2 SQLite check
        let record_opt = {
            let guard = self.db_conn.lock();
            db::get_folder_size_record(&guard, path).ok().flatten()
        };

        if let Some((_dev, _ino, record)) = record_opt {
            if record.dir_mtime == current_mtime {
                // Promote to Layer 1 LRU
                self.l1_cache.write().put(path.to_path_buf(), record);
                return Some(record);
            }
            // Stale entry in L2
            self.invalidate(path);
        }

        None
    }

    /// Inserts a newly calculated result into Layer 1 LRU and enqueues an asynchronous write to Layer 2 SQLite.
    pub fn insert(&self, path: PathBuf, result: FolderSizeResult) {
        let seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        self.inserted_epochs.write().insert(path.clone(), seq);

        // Store in L1 LRU
        self.l1_cache.write().put(path.clone(), result);

        // Determine device and inode if available
        let (dev_id, inode) =
            std::fs::symlink_metadata(&path).map_or((0, 0), |m| (m.dev(), m.ino()));

        // Asynchronously persist to SQLite
        let _ = self.db_tx.try_send(DbOp::Upsert {
            path,
            dev_id,
            inode,
            result,
            seq,
        });
    }

    /// Invalidates a directory path and all its ancestor directories from Layer 1 LRU
    /// and marks them dirty/deleted in Layer 2 SQLite.
    pub fn invalidate(&self, path: &Path) {
        let inval_seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);

        let mut paths_to_evict = Vec::new();
        paths_to_evict.push(path.to_path_buf());

        let mut curr = path.parent();
        while let Some(parent) = curr {
            paths_to_evict.push(parent.to_path_buf());
            curr = parent.parent();
        }

        // Record invalidation sequence for path and all ancestors
        {
            let mut epochs = self.path_epochs.write();
            for p in &paths_to_evict {
                epochs.insert(p.clone(), inval_seq);
            }
        }

        // Evict from L1 LRU
        {
            let mut l1 = self.l1_cache.write();
            for p in &paths_to_evict {
                l1.pop(p);
            }
        }

        // Synchronously remove from L2 SQLite to prevent stale reads
        {
            let guard = self.db_conn.lock();
            for p in &paths_to_evict {
                let _ = delete_folder_size_record(&guard, p);
            }
        }

        // Enqueue deletion in L2 SQLite worker queue to sequence behind pending upserts
        let _ = self.db_tx.try_send(DbOp::Invalidate(paths_to_evict));
    }

    /// Flushes all pending asynchronous SQLite operations, blocking until the queue is drained.
    pub fn flush(&self) {
        let (tx, rx) = std_mpsc::channel();
        if self.db_tx.send_blocking(DbOp::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::large_stack_arrays
)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn test_cache_l1_hit_and_miss() {
        let cache = FolderSizeCache::new_in_memory().unwrap();
        let path = PathBuf::from("/test/folder1");
        let now = SystemTime::now();

        let result = FolderSizeResult {
            total_bytes: 4096,
            file_count: 5,
            folder_count: 2,
            dir_mtime: now,
            calculated_at: now,
        };

        // Cache miss initially
        assert_eq!(cache.get(&path, now), None);

        // Insert and query
        cache.insert(path.clone(), result);
        let cached = cache.get(&path, now);
        assert_eq!(cached, Some(result));
    }

    #[test]
    fn test_cache_stale_invalidation() {
        let cache = FolderSizeCache::new_in_memory().unwrap();
        let path = PathBuf::from("/test/stale_folder");
        let t1 = SystemTime::now() - Duration::from_secs(60);
        let t2 = SystemTime::now();

        let result = FolderSizeResult {
            total_bytes: 8192,
            file_count: 10,
            folder_count: 1,
            dir_mtime: t1,
            calculated_at: t1,
        };

        cache.insert(path.clone(), result);

        // Query with newer mtime t2 -> triggers stale invalidation and returns None
        assert_eq!(cache.get(&path, t2), None);

        // Subsequent query with old t1 should also return None because it was evicted
        assert_eq!(cache.get(&path, t1), None);
    }

    #[test]
    fn test_cache_ancestor_invalidation() {
        let cache = FolderSizeCache::new_in_memory().unwrap();
        let parent = PathBuf::from("/home/user/docs");
        let child = PathBuf::from("/home/user/docs/project");
        let now = SystemTime::now();

        let res_parent = FolderSizeResult {
            total_bytes: 100_000,
            file_count: 20,
            folder_count: 3,
            dir_mtime: now,
            calculated_at: now,
        };
        let res_child = FolderSizeResult {
            total_bytes: 40_000,
            file_count: 8,
            folder_count: 1,
            dir_mtime: now,
            calculated_at: now,
        };

        cache.insert(parent.clone(), res_parent);
        cache.insert(child.clone(), res_child);

        assert_eq!(cache.get(&parent, now), Some(res_parent));
        assert_eq!(cache.get(&child, now), Some(res_child));

        // Invalidating child must also invalidate parent
        cache.invalidate(&child);

        assert_eq!(cache.get(&child, now), None);
        assert_eq!(cache.get(&parent, now), None);
    }

    #[test]
    fn test_cache_persistent_sqlite_reload() {
        let tmp = tempdir().unwrap();
        let db_file = tmp.path().join("sizes_test.sqlite");
        let path = PathBuf::from("/test/persistent");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);

        let result = FolderSizeResult {
            total_bytes: 65536,
            file_count: 42,
            folder_count: 4,
            dir_mtime: now,
            calculated_at: now,
        };

        {
            let cache1 = FolderSizeCache::with_db_path(Some(&db_file)).unwrap();
            cache1.insert(path.clone(), result);
            cache1.flush();
        }

        // Open brand new cache pointing to the same SQLite file
        let cache2 = FolderSizeCache::with_db_path(Some(&db_file)).unwrap();

        // L1 in cache2 is empty, but SQLite has the record -> promotes to L1
        let loaded = cache2.get(&path, now);
        assert_eq!(loaded, Some(result));

        // Second call should hit L1 directly
        assert_eq!(cache2.get(&path, now), Some(result));
    }
}
