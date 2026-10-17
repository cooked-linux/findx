//! LRU inotify watch pool for hierarchical list view tree expansion.
//!
//! Enforces a strict budget ceiling (at most 500 active inotify directory watches)
//! across expanded directory nodes. Automatically evicts collapsed subtrees and
//! least-recently-used watches when the pool reaches capacity.
//! Dispatches deletion events to prune removed nodes atomically from the tree.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use shared::indexer::{FileWatcher, FsEvent};
use tokio::sync::mpsc;

/// Maximum number of simultaneous active inotify directory watchers.
pub const MAX_WATCH_BUDGET: usize = 500;

/// Event emitted by directory watchers in the watch pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListWatcherEvent {
    /// An item inside a watched directory was deleted.
    ItemDeleted(PathBuf),
    /// A watched directory itself was deleted or moved (`IN_DELETE_SELF`).
    DirectoryDeleted(PathBuf),
}

/// Internal handle to a running directory watcher.
#[derive(Debug)]
struct WatchHandle {
    is_active: Arc<AtomicBool>,
}

impl WatchHandle {
    fn stop(&self) {
        self.is_active.store(false, Ordering::SeqCst);
    }
}

/// LRU pool managing bounded filesystem watches for expanded directory nodes.
pub struct LruWatchPool {
    max_capacity: usize,
    active_watches: HashMap<PathBuf, WatchHandle>,
    lru_order: VecDeque<PathBuf>,
    event_tx: mpsc::Sender<ListWatcherEvent>,
}

impl LruWatchPool {
    /// Constructs a new [`LruWatchPool`] with a bounded maximum capacity.
    #[must_use]
    pub fn new(event_tx: mpsc::Sender<ListWatcherEvent>, max_capacity: usize) -> Self {
        Self {
            max_capacity,
            active_watches: HashMap::new(),
            lru_order: VecDeque::new(),
            event_tx,
        }
    }

    /// Adds or refreshes a watch for `dir_path`.
    ///
    /// If capacity is exceeded, the least-recently-used watch is evicted.
    pub fn add_watch(&mut self, dir_path: PathBuf) {
        // If already monitored, move to most-recently-used position (back of queue)
        if self.active_watches.contains_key(&dir_path) {
            self.lru_order.retain(|p| p != &dir_path);
            self.lru_order.push_back(dir_path);
            return;
        }

        // Evict LRU entries if at budget ceiling
        while self.active_watches.len() >= self.max_capacity {
            if let Some(oldest) = self.lru_order.pop_front() {
                if let Some(handle) = self.active_watches.remove(&oldest) {
                    handle.stop();
                }
            } else {
                break;
            }
        }

        let is_active = Arc::new(AtomicBool::new(true));
        let handle = WatchHandle {
            is_active: Arc::clone(&is_active),
        };

        let watch_path = dir_path.clone();
        let tx = self.event_tx.clone();
        let active_flag = Arc::clone(&is_active);

        tokio::spawn(async move {
            let Ok(mut watcher) = FileWatcher::with_debounce(
                std::slice::from_ref(&watch_path),
                Duration::from_millis(150),
            ) else {
                return;
            };

            while active_flag.load(Ordering::SeqCst) {
                tokio::select! {
                    maybe_event = watcher.recv() => {
                        let Some(event) = maybe_event else {
                            break;
                        };

                        if !active_flag.load(Ordering::SeqCst) {
                            break;
                        }

                        match event {
                            FsEvent::Deleted(p) => {
                                if p == watch_path {
                                    let _ = tx.send(ListWatcherEvent::DirectoryDeleted(watch_path.clone())).await;
                                    break;
                                }
                                let _ = tx.send(ListWatcherEvent::ItemDeleted(p)).await;
                            }
                            FsEvent::Moved { from, .. } => {
                                if from == watch_path {
                                    let _ = tx.send(ListWatcherEvent::DirectoryDeleted(watch_path.clone())).await;
                                    break;
                                }
                                let _ = tx.send(ListWatcherEvent::ItemDeleted(from)).await;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        self.active_watches.insert(dir_path.clone(), handle);
        self.lru_order.push_back(dir_path);
    }

    /// Evicts watches for `subtree_root` and all descendant directory paths immediately.
    pub fn evict_subtree(&mut self, subtree_root: &Path) {
        let keys_to_remove: Vec<PathBuf> = self
            .active_watches
            .keys()
            .filter(|p| p.starts_with(subtree_root))
            .cloned()
            .collect();

        for path in keys_to_remove {
            if let Some(handle) = self.active_watches.remove(&path) {
                handle.stop();
            }
            self.lru_order.retain(|p| p != &path);
        }
    }

    /// Evicts an individual directory watch.
    pub fn remove_watch(&mut self, dir_path: &Path) {
        if let Some(handle) = self.active_watches.remove(dir_path) {
            handle.stop();
        }
        self.lru_order.retain(|p| p != dir_path);
    }

    /// Stops all running watchers and empties the pool.
    pub fn clear(&mut self) {
        for handle in self.active_watches.values() {
            handle.stop();
        }
        self.active_watches.clear();
        self.lru_order.clear();
    }

    /// Returns the number of currently active directory watches.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active_watches.len()
    }

    /// Returns `true` if `dir_path` is currently being monitored.
    #[must_use]
    pub fn is_watching(&self, dir_path: &Path) -> bool {
        self.active_watches.contains_key(dir_path)
    }
}

impl Drop for LruWatchPool {
    fn drop(&mut self) {
        self.clear();
    }
}
