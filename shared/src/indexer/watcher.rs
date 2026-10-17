//! Real-time inotify file watcher with debouncing and ignored pattern filtering.
//!
//! Monitors directory trees recursively using [`notify::RecommendedWatcher`],
//! debounces rapid burst file modifications over a 200ms window, and filters out
//! temporary files, editor swap files, and blacklisted paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::indexer::IndexerError;

/// Default debouncing window duration (200 milliseconds).
pub const DEFAULT_DEBOUNCE_DURATION: Duration = Duration::from_millis(200);

/// Coalesced filesystem mutation event emitted by the debouncer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEvent {
    /// A new regular file or directory was created.
    Created(PathBuf),
    /// An existing file's contents or attributes were modified.
    Modified(PathBuf),
    /// A file or directory was deleted or unlinked.
    Deleted(PathBuf),
    /// A file or directory was renamed or moved.
    Moved {
        /// Source path before move.
        from: PathBuf,
        /// Target path after move.
        to: PathBuf,
    },
}

impl FsEvent {
    /// Returns the primary affected path of this event.
    #[must_use]
    pub fn target_path(&self) -> &Path {
        match self {
            Self::Created(p) | Self::Modified(p) | Self::Deleted(p) => p,
            Self::Moved { to, .. } => to,
        }
    }
}

/// Checks whether a given path matches temporary/ignored patterns (`.*.swp`, `*~`, `.DS_Store`).
#[must_use]
pub fn is_ignored_fs_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };

    if file_name == ".DS_Store" {
        return true;
    }

    if file_name.ends_with('~') {
        return true;
    }

    let ext = Path::new(file_name).extension().and_then(|e| e.to_str());

    if file_name.starts_with('.')
        && ext.is_some_and(|e| {
            e.eq_ignore_ascii_case("swp")
                || e.eq_ignore_ascii_case("swpx")
                || e.eq_ignore_ascii_case("swo")
        })
    {
        return true;
    }

    if ext.is_some_and(|e| e.eq_ignore_ascii_case("tmp")) {
        return true;
    }

    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        if name == ".git" || name == "node_modules" || name == "target" || name == ".cache" {
            return true;
        }
    }

    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug)]
enum RawNotification {
    Create(PathBuf),
    Modify(PathBuf),
    Delete(PathBuf),
    RenameBoth { from: PathBuf, to: PathBuf },
    RenameFrom(PathBuf),
    RenameTo(PathBuf),
}

/// Recursive inotify filesystem watcher with integrated 200ms debouncing.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    event_rx: mpsc::Receiver<FsEvent>,
}

impl FileWatcher {
    /// Creates a new [`FileWatcher`] monitoring the specified roots.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if the underlying inotify watcher fails to initialize.
    pub fn new(roots: &[PathBuf]) -> Result<Self, IndexerError> {
        Self::with_debounce(roots, DEFAULT_DEBOUNCE_DURATION)
    }

    /// Creates a new [`FileWatcher`] with a custom debouncing window.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if inotify initialization or root registration fails.
    pub fn with_debounce(roots: &[PathBuf], debounce: Duration) -> Result<Self, IndexerError> {
        let (raw_tx, raw_rx) = mpsc::unbounded_channel::<RawNotification>();
        let (event_tx, event_rx) = mpsc::channel::<FsEvent>(512);

        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    handle_raw_event(event, &raw_tx);
                }
            },
            Config::default(),
        )
        .map_err(|e| IndexerError::Watcher(e.to_string()))?;

        for root in roots {
            if root.exists() {
                watcher
                    .watch(root, RecursiveMode::Recursive)
                    .map_err(|e| IndexerError::Watcher(e.to_string()))?;
            }
        }

        tokio::spawn(run_debouncer_loop(raw_rx, event_tx, debounce));

        Ok(Self {
            _watcher: watcher,
            event_rx,
        })
    }

    /// Asynchronously waits for the next debounced filesystem event.
    pub async fn recv(&mut self) -> Option<FsEvent> {
        self.event_rx.recv().await
    }

    /// Attempts to read the next debounced event without waiting.
    ///
    /// # Errors
    ///
    /// Returns `TryRecvError` if the channel is empty or disconnected.
    pub fn try_recv(&mut self) -> Result<FsEvent, mpsc::error::TryRecvError> {
        self.event_rx.try_recv()
    }
}

fn handle_raw_event(event: Event, raw_tx: &mpsc::UnboundedSender<RawNotification>) {
    match event.kind {
        EventKind::Create(_) => {
            for path in event.paths {
                if !is_ignored_fs_path(&path) {
                    let _ = raw_tx.send(RawNotification::Create(path));
                }
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            if event.paths.len() >= 2 {
                let from = event.paths[0].clone();
                let to = event.paths[1].clone();
                let _ = raw_tx.send(RawNotification::RenameBoth { from, to });
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            for path in event.paths {
                let _ = raw_tx.send(RawNotification::RenameFrom(path));
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            for path in event.paths {
                let _ = raw_tx.send(RawNotification::RenameTo(path));
            }
        }
        EventKind::Modify(_) => {
            for path in event.paths {
                if !is_ignored_fs_path(&path) {
                    let _ = raw_tx.send(RawNotification::Modify(path));
                }
            }
        }
        EventKind::Remove(_) => {
            for path in event.paths {
                if !is_ignored_fs_path(&path) {
                    let _ = raw_tx.send(RawNotification::Delete(path));
                }
            }
        }
        _ => {}
    }
}

async fn run_debouncer_loop(
    mut raw_rx: mpsc::UnboundedReceiver<RawNotification>,
    event_tx: mpsc::Sender<FsEvent>,
    debounce: Duration,
) {
    let mut pending: HashMap<PathBuf, (PendingAction, Instant)> = HashMap::new();
    let mut pending_rename_from: Option<(PathBuf, Instant)> = None;

    loop {
        let earliest_deadline = pending
            .values()
            .map(|(_, instant)| *instant)
            .chain(pending_rename_from.as_ref().map(|(_, instant)| *instant))
            .min();

        tokio::select! {
            raw = raw_rx.recv() => {
                let Some(notification) = raw else {
                    break;
                };
                process_raw_notification(notification, debounce, &mut pending, &mut pending_rename_from, &event_tx).await;
            }
            () = async {
                if let Some(deadline) = earliest_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    futures_util_pending().await;
                }
            } => {
                flush_expired(&mut pending, &mut pending_rename_from, &event_tx).await;
            }
        }
    }

    // Flush any remaining pending events on shutdown
    flush_expired(&mut pending, &mut pending_rename_from, &event_tx).await;
}

async fn futures_util_pending() {
    std::future::pending::<()>().await;
}

async fn process_raw_notification(
    notification: RawNotification,
    debounce: Duration,
    pending: &mut HashMap<PathBuf, (PendingAction, Instant)>,
    pending_rename_from: &mut Option<(PathBuf, Instant)>,
    event_tx: &mpsc::Sender<FsEvent>,
) {
    let now = Instant::now();
    let deadline = now + debounce;

    match notification {
        RawNotification::Create(path) => {
            if pending.get(&path).map(|(a, _)| *a) == Some(PendingAction::Deleted) {
                // Deleted then Created => Modified
                pending.insert(path, (PendingAction::Modified, deadline));
            } else {
                pending.insert(path, (PendingAction::Created, deadline));
            }
        }
        RawNotification::Modify(path) => {
            if let Some(&(action, _)) = pending.get(&path)
                && action == PendingAction::Created
            {
                pending.insert(path, (PendingAction::Created, deadline));
                return;
            }
            pending.insert(path, (PendingAction::Modified, deadline));
        }
        RawNotification::Delete(path) => {
            if let Some(&(PendingAction::Created, _)) = pending.get(&path) {
                // Created then deleted within debounce window => cancel out completely
                pending.remove(&path);
                return;
            }
            pending.insert(path, (PendingAction::Deleted, deadline));
        }
        RawNotification::RenameBoth { from, to } => {
            handle_rename_pair(from, to, pending, event_tx, deadline).await;
        }
        RawNotification::RenameFrom(from) => {
            *pending_rename_from = Some((from, deadline));
        }
        RawNotification::RenameTo(to) => {
            if let Some((from, _)) = pending_rename_from.take() {
                handle_rename_pair(from, to, pending, event_tx, deadline).await;
            } else if !is_ignored_fs_path(&to) {
                pending.insert(to, (PendingAction::Created, deadline));
            }
        }
    }
}

async fn handle_rename_pair(
    from: PathBuf,
    to: PathBuf,
    pending: &mut HashMap<PathBuf, (PendingAction, Instant)>,
    event_tx: &mpsc::Sender<FsEvent>,
    deadline: Instant,
) {
    let from_ignored = is_ignored_fs_path(&from);
    let to_ignored = is_ignored_fs_path(&to);

    if from_ignored && to_ignored {
        return;
    }

    if from_ignored && !to_ignored {
        // e.g. editor saving file.tmp -> file.txt
        pending.insert(to, (PendingAction::Modified, deadline));
        return;
    }

    if !from_ignored && to_ignored {
        // moved to ignored location or temporary file
        pending.insert(from, (PendingAction::Deleted, deadline));
        return;
    }

    // Both are valid non-ignored paths => genuine Move
    pending.remove(&from);
    pending.remove(&to);
    let _ = event_tx.send(FsEvent::Moved { from, to }).await;
}

async fn flush_expired(
    pending: &mut HashMap<PathBuf, (PendingAction, Instant)>,
    pending_rename_from: &mut Option<(PathBuf, Instant)>,
    event_tx: &mpsc::Sender<FsEvent>,
) {
    let now = Instant::now();

    // Check orphaned RenameFrom
    if let Some((from, deadline)) = pending_rename_from.as_ref()
        && *deadline <= now
    {
        let from_path = from.clone();
        *pending_rename_from = None;
        if !is_ignored_fs_path(&from_path) {
            let _ = event_tx.send(FsEvent::Deleted(from_path)).await;
        }
    }

    let mut expired = Vec::new();
    for (path, (action, deadline)) in pending.iter() {
        if *deadline <= now {
            expired.push((path.clone(), *action));
        }
    }

    for (path, action) in expired {
        pending.remove(&path);
        let event = match action {
            PendingAction::Created => FsEvent::Created(path),
            PendingAction::Modified => FsEvent::Modified(path),
            PendingAction::Deleted => FsEvent::Deleted(path),
        };
        let _ = event_tx.send(event).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_ignored_patterns() {
        assert!(is_ignored_fs_path(Path::new("/home/user/.DS_Store")));
        assert!(is_ignored_fs_path(Path::new("/home/user/document.txt~")));
        assert!(is_ignored_fs_path(Path::new("/home/user/.doc.txt.swp")));
        assert!(is_ignored_fs_path(Path::new("/home/user/.file.swpx")));
        assert!(is_ignored_fs_path(Path::new("/home/user/data.tmp")));
        assert!(is_ignored_fs_path(Path::new("/home/user/.git/config")));
        assert!(is_ignored_fs_path(Path::new(
            "/home/user/node_modules/express/index.js"
        )));

        assert!(!is_ignored_fs_path(Path::new("/home/user/main.rs")));
        assert!(!is_ignored_fs_path(Path::new("/home/user/notes.txt")));
        assert!(!is_ignored_fs_path(Path::new("/home/user/README.md")));
    }

    #[tokio::test]
    async fn test_watcher_detects_real_filesystem_events() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path().to_path_buf();

        let mut watcher =
            FileWatcher::with_debounce(std::slice::from_ref(&root), Duration::from_millis(50))
                .unwrap();

        // Create a file
        let file_path = root.join("test_real.txt");
        {
            let mut f = File::create(&file_path).unwrap();
            writeln!(f, "Initial content").unwrap();
        }

        // Wait for created event
        let event = tokio::time::timeout(Duration::from_millis(600), watcher.recv())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(event.target_path(), file_path);

        // Delete the file
        std::fs::remove_file(&file_path).unwrap();

        let event = tokio::time::timeout(Duration::from_millis(600), watcher.recv())
            .await
            .unwrap()
            .unwrap();

        assert!(matches!(event, FsEvent::Deleted(_)));
        assert_eq!(event.target_path(), file_path);
    }
}
