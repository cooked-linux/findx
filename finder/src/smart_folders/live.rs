//! Live smart folder subscriptions backed by inotify and periodic mount recovery.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_channel::Sender;
use shared::indexer::{FileWatcher, FsEvent, is_ignored_fs_path};
use shared::models::FileItem;

use super::compiler::{CompiledQuery, compile_definition, resolve_active_scopes};
use super::engine::{SmartFolderEngine, file_item_from_path};
use super::error::{Result, SmartFolderError};
use super::types::SmartFolderDefinition;

/// Remount polling cadence for disconnected search scopes.
const MOUNT_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Handle owning a background live smart folder subscription.
pub struct SmartFolderSubscription {
    stop: Arc<AtomicBool>,
    _worker: std::thread::JoinHandle<()>,
}

impl std::fmt::Debug for SmartFolderSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartFolderSubscription")
            .field("stopped", &self.stop.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Drop for SmartFolderSubscription {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl SmartFolderEngine {
    /// Starts a live subscription streaming updated result vectors to `sender`.
    ///
    /// The initial result set is emitted immediately, followed by debounced inotify
    /// re-evaluations. Disconnected scopes are polled every five seconds so results
    /// repopulate automatically when a removable volume reappears.
    ///
    /// # Errors
    ///
    /// Returns [`SmartFolderError::Database`] if the worker thread cannot be spawned.
    pub fn subscribe(
        &self,
        definition: &SmartFolderDefinition,
        sender: Sender<Vec<FileItem>>,
    ) -> Result<SmartFolderSubscription> {
        let query_id = self.next_query_id();
        let engine = self.clone();
        let subscription_definition = definition.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);

        let worker = std::thread::Builder::new()
            .name("smart-folder-live".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => {
                        if let Err(error) = runtime.block_on(run_subscription(
                            engine,
                            subscription_definition,
                            query_id,
                            sender,
                            thread_stop,
                        )) {
                            tracing::debug!("Smart folder live subscription ended: {error}");
                        }
                    }
                    Err(error) => {
                        tracing::warn!("Unable to start smart folder runtime: {error}");
                    }
                }
            })
            .map_err(|error| SmartFolderError::Database {
                message: format!("unable to spawn smart folder worker: {error}"),
            })?;

        Ok(SmartFolderSubscription {
            stop,
            _worker: worker,
        })
    }
}

async fn run_subscription(
    engine: SmartFolderEngine,
    definition: SmartFolderDefinition,
    query_id: u64,
    sender: Sender<Vec<FileItem>>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    loop {
        if stop.load(Ordering::Relaxed) || !engine.is_current(query_id) {
            return Ok(());
        }

        let compiled = compile_definition(&definition);
        let mut items = match engine.execute_compiled(&compiled, query_id) {
            Ok(results) => results.items,
            Err(SmartFolderError::Cancelled) => return Ok(()),
            Err(error) => return Err(error),
        };

        if sender.send(items.clone()).await.is_err() {
            return Ok(());
        }

        let mut watcher = FileWatcher::new(&compiled.active_scopes).ok();
        let mut ticker = tokio::time::interval(MOUNT_POLL_INTERVAL);

        loop {
            if stop.load(Ordering::Relaxed) || !engine.is_current(query_id) {
                return Ok(());
            }

            if scopes_changed(&definition, &compiled.active_scopes) {
                break;
            }

            let Some(active_watcher) = watcher.as_mut() else {
                ticker.tick().await;
                continue;
            };

            tokio::select! {
                _ = ticker.tick() => {}
                event = active_watcher.recv() => {
                    let Some(event) = event else {
                        watcher = None;
                        continue;
                    };
                    apply_event(event, &compiled, &mut items);
                    if sender.send(items.clone()).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

fn scopes_changed(
    definition: &SmartFolderDefinition,
    active_scopes: &[std::path::PathBuf],
) -> bool {
    let expected: HashSet<_> = resolve_active_scopes(&definition.scopes)
        .into_iter()
        .collect();
    let active: HashSet<_> = active_scopes.iter().cloned().collect();
    expected != active
}

fn apply_event(event: FsEvent, compiled: &CompiledQuery, items: &mut Vec<FileItem>) {
    match event {
        FsEvent::Deleted(path) => {
            items.retain(|item| item.path != path);
        }
        FsEvent::Moved { from, to } => {
            items.retain(|item| item.path != from);
            reinsert_path(&to, compiled, items);
        }
        FsEvent::Created(path) | FsEvent::Modified(path) => {
            if is_ignored_fs_path(&path) {
                return;
            }
            reinsert_path(&path, compiled, items);
        }
    }

    items.sort_by(|left, right| shared::fs::natural_cmp(left.file_name(), right.file_name()));
}

fn reinsert_path(path: &std::path::Path, compiled: &CompiledQuery, items: &mut Vec<FileItem>) {
    items.retain(|item| item.path != path);
    let Some(item) = file_item_from_path(path) else {
        return;
    };
    if (compiled.filter_fn)(&item) {
        items.push(item);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use crate::smart_folders::types::{
        CriteriaAttribute, CriteriaGroup, CriteriaOperator, CriteriaSlice,
    };

    #[test]
    fn test_apply_event_removes_deleted_paths() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("report.txt");
        std::fs::write(&file, b"report").unwrap();
        let item = file_item_from_path(&file).unwrap();

        let definition = SmartFolderDefinition::new(
            "Reports",
            CriteriaGroup::all(vec![CriteriaSlice::new(
                CriteriaAttribute::Name,
                CriteriaOperator::Contains,
                "report",
            )]),
        );
        let compiled = compile_definition(&definition);

        let mut items = vec![item];
        apply_event(FsEvent::Deleted(file), &compiled, &mut items);
        assert!(items.is_empty());
    }
}
