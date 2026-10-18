//! Reactive directory monitor with inotify event debouncing and cache invalidation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use shared::indexer::{FileWatcher, FsEvent, IndexerError};

use super::cache::FolderSizeCache;

/// Default cooldown duration for directory recalculations (2.0 seconds).
pub const DEFAULT_COOLDOWN_DURATION: Duration = Duration::from_secs(2);

/// Extracts the affected directory paths from a filesystem event.
#[must_use]
pub fn extract_affected_directories(event: &FsEvent) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match event {
        FsEvent::Created(p) | FsEvent::Modified(p) | FsEvent::Deleted(p) => {
            if let Some(parent) = p.parent() {
                dirs.push(parent.to_path_buf());
            } else {
                dirs.push(p.clone());
            }
        }
        FsEvent::Moved { from, to } => {
            if let Some(parent) = from.parent() {
                dirs.push(parent.to_path_buf());
            }
            if let Some(parent) = to.parent() {
                let parent_buf = parent.to_path_buf();
                if !dirs.contains(&parent_buf) {
                    dirs.push(parent_buf);
                }
            }
        }
    }
    dirs
}

/// Spawns a background task running the future on either the ambient Tokio runtime
/// or a dedicated fallback runtime thread.
fn spawn_future<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
    } else {
        let _ = std::thread::Builder::new()
            .name("findx-folder-size-watcher".to_string())
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

/// Reactive directory watcher coordinating real-time inotify invalidation
/// and a 2-second cooldown debouncer for folder recalculations.
pub struct FolderSizeWatcher {
    cache: Arc<FolderSizeCache>,
    cooldown: Duration,
    event_tx: async_channel::Sender<FsEvent>,
    recalc_rx: async_channel::Receiver<PathBuf>,
}

impl FolderSizeWatcher {
    /// Creates a new [`FolderSizeWatcher`] with custom cooldown debouncing duration.
    #[must_use]
    pub fn new(cache: Arc<FolderSizeCache>, cooldown: Duration) -> Self {
        let (event_tx, event_rx) = async_channel::unbounded::<FsEvent>();
        let (recalc_tx, recalc_rx) = async_channel::unbounded::<PathBuf>();

        let cache_clone = Arc::clone(&cache);

        // Run debouncer loop
        spawn_future(async move {
            let mut pending: HashMap<PathBuf, tokio::time::Instant> = HashMap::new();

            loop {
                let now = tokio::time::Instant::now();
                let next_deadline = pending.values().copied().min();
                let sleep_duration = match next_deadline {
                    Some(dl) if dl > now => dl - now,
                    Some(_) => Duration::ZERO,
                    None => Duration::from_secs(3600),
                };

                tokio::select! {
                    event_res = event_rx.recv() => {
                        match event_res {
                            Ok(event) => {
                                let affected = extract_affected_directories(&event);
                                for dir in affected {
                                    cache_clone.invalidate(&dir);
                                    let deadline = tokio::time::Instant::now() + cooldown;
                                    pending.insert(dir, deadline);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    () = tokio::time::sleep(sleep_duration), if !pending.is_empty() => {
                        let cur_now = tokio::time::Instant::now();
                        let mut expired = Vec::new();
                        pending.retain(|path, &mut deadline| {
                            if cur_now >= deadline {
                                expired.push(path.clone());
                                false
                            } else {
                                true
                            }
                        });

                        for path in expired {
                            let _ = recalc_tx.send(path).await;
                        }
                    }
                }
            }
        });

        Self {
            cache,
            cooldown,
            event_tx,
            recalc_rx,
        }
    }

    /// Creates a new [`FolderSizeWatcher`] configured with the default 2-second cooldown window.
    #[must_use]
    pub fn default_watcher(cache: Arc<FolderSizeCache>) -> Self {
        Self::new(cache, DEFAULT_COOLDOWN_DURATION)
    }

    /// Submits a filesystem mutation event into the watcher debouncer pipeline.
    ///
    /// Synchronously invalidates direct parent and ancestor directory cache records,
    /// and enqueues the event for debounced recalculation scheduling.
    ///
    /// # Errors
    ///
    /// Returns [`async_channel::SendError`] if the event channel is closed.
    pub fn handle_event(&self, event: FsEvent) -> Result<(), async_channel::SendError<FsEvent>> {
        let affected = extract_affected_directories(&event);
        for dir in &affected {
            self.cache.invalidate(dir);
        }
        self.event_tx.send_blocking(event)
    }

    /// Subscribes to the stream of directories ready for size recalculation
    /// after their cooldown timers have expired.
    #[must_use]
    pub fn subscribe_recalculations(&self) -> async_channel::Receiver<PathBuf> {
        self.recalc_rx.clone()
    }

    /// Returns a reference to the underlying [`FolderSizeCache`].
    #[must_use]
    pub const fn cache(&self) -> &Arc<FolderSizeCache> {
        &self.cache
    }

    /// Attaches an existing [`FileWatcher`] to stream filesystem notifications
    /// into this debouncer.
    pub fn attach_file_watcher(&self, mut watcher: FileWatcher) {
        let event_tx = self.event_tx.clone();
        let cache = Arc::clone(&self.cache);
        spawn_future(async move {
            while let Some(event) = watcher.recv().await {
                let affected = extract_affected_directories(&event);
                for dir in &affected {
                    cache.invalidate(dir);
                }
                if event_tx.send(event).await.is_err() {
                    break;
                }
            }
        });
    }

    /// Creates and attaches a recursive inotify [`FileWatcher`] for the given roots.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if inotify registration fails.
    pub fn watch_roots(&self, roots: &[PathBuf]) -> Result<(), IndexerError> {
        let watcher = FileWatcher::new(roots)?;
        self.attach_file_watcher(watcher);
        Ok(())
    }

    /// Returns the configured cooldown duration.
    #[must_use]
    pub const fn cooldown(&self) -> Duration {
        self.cooldown
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
    use crate::operations::folder_size::models::FolderSizeResult;
    use std::time::SystemTime;

    #[tokio::test]
    async fn test_watcher_rapid_sequential_writes_trigger_single_invalidation() {
        let cache = Arc::new(FolderSizeCache::new_in_memory().unwrap());
        // Use 60ms cooldown in unit test for rapid test execution
        let cooldown = Duration::from_millis(60);
        let watcher = FolderSizeWatcher::new(Arc::clone(&cache), cooldown);
        let rx = watcher.subscribe_recalculations();

        let parent_dir = PathBuf::from("/home/user/workspace");
        let now = SystemTime::now();

        // Populate cache for parent directory
        let initial_res = FolderSizeResult {
            total_bytes: 50_000,
            file_count: 10,
            folder_count: 2,
            dir_mtime: now,
            calculated_at: now,
        };
        cache.insert(parent_dir.clone(), initial_res);
        assert_eq!(cache.get(&parent_dir, now), Some(initial_res));

        // Simulate rapid sequential file creations inside parent_dir over 100ms
        for i in 0..10 {
            let child_file = parent_dir.join(format!("build_artifact_{i}.o"));
            watcher.handle_event(FsEvent::Created(child_file)).unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // Cache must have been invalidated immediately on event
        assert_eq!(cache.get(&parent_dir, now), None);

        // Before cooldown expires (within 40ms of last event), no recalculation should have fired yet
        assert!(rx.try_recv().is_err());

        // Wait for cooldown to expire (60ms + margin)
        let ready_dir = tokio::time::timeout(Duration::from_millis(250), rx.recv())
            .await
            .expect("should receive debounced event")
            .expect("channel active");

        assert_eq!(ready_dir, parent_dir);

        // Verify that no further recalculation events were enqueued (single debounced event)
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_watcher_move_invalidates_both_source_and_destination() {
        let cache = Arc::new(FolderSizeCache::new_in_memory().unwrap());
        let watcher = FolderSizeWatcher::new(Arc::clone(&cache), Duration::from_millis(60));
        let rx = watcher.subscribe_recalculations();

        let src_dir = PathBuf::from("/home/user/downloads");
        let dst_dir = PathBuf::from("/home/user/documents");
        let now = SystemTime::now();

        let dummy_res = FolderSizeResult {
            total_bytes: 1024,
            file_count: 1,
            folder_count: 0,
            dir_mtime: now,
            calculated_at: now,
        };

        cache.insert(src_dir.clone(), dummy_res);
        cache.insert(dst_dir.clone(), dummy_res);
        cache.flush();

        watcher
            .handle_event(FsEvent::Moved {
                from: src_dir.join("report.pdf"),
                to: dst_dir.join("report.pdf"),
            })
            .unwrap();

        // Both folders must be invalidated immediately
        assert_eq!(cache.get(&src_dir, now), None);
        assert_eq!(cache.get(&dst_dir, now), None);

        // Wait for both to be notified after cooldown
        let mut ready = Vec::new();
        for _ in 0..2 {
            let dir = tokio::time::timeout(Duration::from_millis(300), rx.recv())
                .await
                .expect("recv dir")
                .expect("channel");
            ready.push(dir);
        }

        assert!(ready.contains(&src_dir));
        assert!(ready.contains(&dst_dir));
    }
}
