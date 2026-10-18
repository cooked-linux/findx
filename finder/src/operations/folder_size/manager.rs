//! Background calculation manager, priority scheduler, and UI dispatcher for folder sizes.

use std::cmp::Ordering as CmpOrdering;
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use dashmap::{DashMap, DashSet};

use super::cache::FolderSizeCache;
use super::models::{FolderSizeResult, Priority};
use super::scanner::calculate_folder_size_sync;

/// Default number of concurrent background directory traversal workers.
pub const DEFAULT_SCANNER_CONCURRENCY: usize = 4;

/// Work item tracked inside the priority scheduler heap.
#[derive(Debug, Eq, PartialEq)]
struct PrioritizedJob {
    priority: Priority,
    sequence: u64,
    path: PathBuf,
}

impl Ord for PrioritizedJob {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for PrioritizedJob {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

/// Spawns a background task running the future on either the ambient Tokio runtime
/// or a dedicated fallback runtime thread.
fn spawn_manager_task<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
    } else {
        let _ = std::thread::Builder::new()
            .name("findx-folder-size-scheduler".to_string())
            .spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    rt.block_on(future);
                }
            });
    }
}

/// Thread-safe manager coordinating asynchronous recursive folder size calculations,
/// priority scheduling, duplicate request throttling, and UI result dispatching.
pub struct FolderSizeManager {
    cache: Arc<FolderSizeCache>,
    work_queue: async_channel::Sender<(PathBuf, Priority)>,
    results_receiver: async_channel::Receiver<(PathBuf, FolderSizeResult)>,
    in_flight: Arc<DashSet<PathBuf>>,
    cancel_tokens: Arc<DashMap<PathBuf, Arc<AtomicBool>>>,
}

impl FolderSizeManager {
    /// Creates a new [`FolderSizeManager`] with the specified background worker concurrency limit.
    #[must_use]
    pub fn new(cache: Arc<FolderSizeCache>, concurrency: usize) -> Self {
        let (work_tx, work_rx) = async_channel::unbounded::<(PathBuf, Priority)>();
        let (results_tx, results_rx) = async_channel::unbounded::<(PathBuf, FolderSizeResult)>();
        let (worker_tx, worker_rx) = async_channel::bounded::<PathBuf>(concurrency.max(1));

        let in_flight = Arc::new(DashSet::new());
        let cancel_tokens = Arc::new(DashMap::new());

        let scheduler_work_rx = work_rx;
        let scheduler_worker_tx = worker_tx;

        // Scheduler task: manages priority queue and dispatches to workers
        spawn_manager_task(async move {
            let mut heap = BinaryHeap::<PrioritizedJob>::new();
            let mut seq_counter: u64 = 0;

            loop {
                // If heap has items, try to dispatch without blocking; otherwise wait for next incoming item
                if heap.is_empty() {
                    match scheduler_work_rx.recv().await {
                        Ok((path, priority)) => {
                            seq_counter = seq_counter.wrapping_add(1);
                            heap.push(PrioritizedJob {
                                priority,
                                sequence: seq_counter,
                                path,
                            });
                        }
                        Err(_) => break,
                    }
                }

                // Drain any additional immediately available items from work_rx
                while let Ok((path, priority)) = scheduler_work_rx.try_recv() {
                    seq_counter = seq_counter.wrapping_add(1);
                    heap.push(PrioritizedJob {
                        priority,
                        sequence: seq_counter,
                        path,
                    });
                }

                // Dispatch top priority job to available worker
                if let Some(job) = heap.pop()
                    && scheduler_worker_tx.send(job.path).await.is_err()
                {
                    break;
                }
            }
        });

        // Worker threads: execute directory scanning jobs
        for i in 0..concurrency.max(1) {
            let rx = worker_rx.clone();
            let w_cache = Arc::clone(&cache);
            let w_results_tx = results_tx.clone();
            let w_in_flight = Arc::clone(&in_flight);
            let w_cancel_tokens = Arc::clone(&cancel_tokens);

            let thread_name = format!("findx-folder-size-worker-{i}");
            let _ = std::thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    while let Ok(path) = rx.recv_blocking() {
                        let cancel_token = w_cancel_tokens
                            .entry(path.clone())
                            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
                            .clone();

                        let res = calculate_folder_size_sync(&path, Some(&cancel_token));

                        w_in_flight.remove(&path);
                        w_cancel_tokens.remove(&path);

                        if let Ok(result) = res {
                            w_cache.insert(path.clone(), result);
                            let _ = w_results_tx.send_blocking((path, result));
                        }
                    }
                });
        }

        Self {
            cache,
            work_queue: work_tx,
            results_receiver: results_rx,
            in_flight,
            cancel_tokens,
        }
    }

    /// Constructs a manager using default concurrency (4 background workers).
    #[must_use]
    pub fn default_manager(cache: Arc<FolderSizeCache>) -> Self {
        Self::new(cache, DEFAULT_SCANNER_CONCURRENCY)
    }

    /// Requests calculation of recursive folder size and item count.
    ///
    /// - **Fast-path:** If a valid non-stale cached entry exists, returns `Some(result)` synchronously.
    /// - **Slow-path:** If not in cache, enqueues the path with the given [`Priority`]
    ///   (`Priority::High` for visible screen rows, `Priority::Low` for off-screen prefetch),
    ///   and returns `None` while computation proceeds in background.
    #[must_use]
    pub fn request_folder_size(&self, path: &Path, priority: Priority) -> Option<FolderSizeResult> {
        let current_mtime = std::fs::symlink_metadata(path)
            .map(|m| m.modified().unwrap_or(SystemTime::UNIX_EPOCH))
            .ok()?;

        if let Some(cached) = self.cache.get(path, current_mtime) {
            return Some(cached);
        }

        let path_buf = path.to_path_buf();
        if self.in_flight.insert(path_buf.clone()) {
            let _ = self.work_queue.try_send((path_buf, priority));
        }

        None
    }

    /// Cancels an in-flight folder size calculation if currently running.
    pub fn cancel_request(&self, path: &Path) {
        if let Some(token) = self.cancel_tokens.get(path) {
            token.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.in_flight.remove(path);
    }

    /// Returns a reference to the underlying cache.
    #[must_use]
    pub const fn cache(&self) -> &Arc<FolderSizeCache> {
        &self.cache
    }

    /// Returns the work submission channel sender.
    #[must_use]
    pub const fn work_queue(&self) -> &async_channel::Sender<(PathBuf, Priority)> {
        &self.work_queue
    }

    /// Returns the channel receiver delivering asynchronous calculation results.
    #[must_use]
    pub const fn results_receiver(&self) -> &async_channel::Receiver<(PathBuf, FolderSizeResult)> {
        &self.results_receiver
    }

    /// Attaches an idle callback to the GTK main loop dispatching completed calculations.
    ///
    /// Batches up to 30 completed calculations per tick, smoothly updating UI models
    /// without causing main-thread frame drops or stutter.
    pub fn attach_glib_dispatcher<F>(&self, callback: F) -> glib::SourceId
    where
        F: Fn(PathBuf, FolderSizeResult) + 'static,
    {
        let rx = self.results_receiver.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
            let mut count = 0;
            while count < 30 {
                match rx.try_recv() {
                    Ok((path, result)) => {
                        callback(path, result);
                        count += 1;
                    }
                    Err(async_channel::TryRecvError::Empty) => break,
                    Err(async_channel::TryRecvError::Closed) => {
                        return glib::ControlFlow::Break;
                    }
                }
            }
            glib::ControlFlow::Continue
        })
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
    use std::fs::{self, File};
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_manager_request_concurrent_directories() {
        let tmp = tempdir().unwrap();
        let cache = Arc::new(FolderSizeCache::new_in_memory().unwrap());
        let manager = FolderSizeManager::new(Arc::clone(&cache), 4);
        let rx = manager.results_receiver().clone();

        let mut expected_dirs = Vec::new();

        // Create 10 distinct directories with varying file content
        for i in 0..10 {
            let dir = tmp.path().join(format!("dir_{i}"));
            fs::create_dir_all(&dir).unwrap();

            let file_path = dir.join("data.bin");
            let mut f = File::create(&file_path).unwrap();
            f.write_all(&vec![0x55u8; (i + 1) * 1024]).unwrap();
            f.sync_all().unwrap();

            expected_dirs.push(dir);
        }

        // Request all 10 directories concurrently
        for dir in &expected_dirs {
            let immediate = manager.request_folder_size(dir, Priority::High);
            // Since they are not yet calculated, all initial requests must return None
            assert_eq!(immediate, None);
        }

        // Collect 10 completion notifications from results_receiver
        let mut completed = Vec::new();
        for _ in 0..10 {
            let (path, res) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("receive within timeout")
                .expect("channel active");

            assert_eq!(res.file_count, 1);
            assert_eq!(res.folder_count, 0);
            assert!(res.total_bytes > 0);
            completed.push(path);
        }

        assert_eq!(completed.len(), 10);
        for dir in &expected_dirs {
            assert!(completed.contains(dir));
        }

        // Subsequent requests must hit the fast-path and return Some(result) immediately
        for dir in &expected_dirs {
            let cached = manager.request_folder_size(dir, Priority::High);
            assert!(cached.is_some());
        }
    }

    #[tokio::test]
    async fn test_manager_cancellation() {
        let tmp = tempdir().unwrap();
        let cache = Arc::new(FolderSizeCache::new_in_memory().unwrap());
        let manager = FolderSizeManager::new(Arc::clone(&cache), 2);

        let dir = tmp.path().join("cancel_dir");
        fs::create_dir_all(&dir).unwrap();

        let immediate = manager.request_folder_size(&dir, Priority::Low);
        assert_eq!(immediate, None);

        // Cancel the request immediately
        manager.cancel_request(&dir);

        // Verify in_flight was cleared
        assert!(!manager.in_flight.contains(&dir));
    }
}
