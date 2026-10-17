//! Asynchronous directory reader and inotify watcher for Miller Columns.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use shared::indexer::{FileWatcher, FsEvent};
use shared::models::{FileItem, FileMetadata};
use tokio::sync::mpsc;

use super::models::ColumnEntry;
use crate::error::{FinderError, Result};
use crate::sorting::comparator::{SortCriteria, sort_file_items};

/// Reads directory entries asynchronously, constructing [`FileItem`]s and sorting them.
///
/// # Errors
/// Returns [`FinderError`] if opening or reading the directory fails (e.g. permission denied).
pub async fn read_directory(
    path: &Path,
    criteria: Option<&SortCriteria>,
) -> Result<Vec<ColumnEntry>> {
    let mut read_dir = match tokio::fs::read_dir(path).await {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(FinderError::Fs(shared::errors::FsError::permission_denied(
                path,
            )));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(FinderError::Fs(shared::errors::FsError::not_found(path)));
        }
        Err(err) => return Err(FinderError::Io(err)),
    };

    let mut items = Vec::new();
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if let Ok(item) = stat_file_item(path, &file_name).await {
            items.push(item);
        }
    }

    let default_criteria = SortCriteria::default();
    let active_criteria = criteria.unwrap_or(&default_criteria);
    sort_file_items(&mut items, active_criteria);

    Ok(items
        .into_iter()
        .map(|item| ColumnEntry::new(item, false))
        .collect())
}

/// Stats an individual entry within `dir_path`, handling symlinks and broken links gracefully.
///
/// # Errors
/// Returns [`std::io::Error`] if reading filesystem metadata fails.
pub async fn stat_file_item(dir_path: &Path, entry_name: &str) -> std::io::Result<FileItem> {
    let full_path = dir_path.join(entry_name);
    let symlink_meta = tokio::fs::symlink_metadata(&full_path).await?;
    let is_symlink = symlink_meta.is_symlink();

    let (
        is_dir,
        symlink_target,
        size_bytes,
        permissions,
        inode,
        device_id,
        modified,
        accessed,
        created,
        mime_type,
    ) = if is_symlink {
        let target = tokio::fs::read_link(&full_path).await.ok();
        let target_meta = tokio::fs::metadata(&full_path).await.ok();

        if let Some(meta) = target_meta {
            let mime = if meta.is_dir() {
                "inode/directory".to_string()
            } else {
                shared::mime::detect_mime_type(&full_path)
                    .unwrap_or_else(|_| "application/octet-stream".to_string())
            };
            (
                meta.is_dir(),
                target,
                meta.len(),
                meta.permissions().mode(),
                meta.ino(),
                meta.dev(),
                meta.modified().ok(),
                meta.accessed().ok(),
                meta.created().ok(),
                mime,
            )
        } else {
            // Broken symlink
            (
                false,
                target,
                symlink_meta.len(),
                symlink_meta.permissions().mode(),
                symlink_meta.ino(),
                symlink_meta.dev(),
                symlink_meta.modified().ok(),
                symlink_meta.accessed().ok(),
                symlink_meta.created().ok(),
                "application/x-symlink".to_string(),
            )
        }
    } else {
        let is_dir = symlink_meta.is_dir();
        let mime = if is_dir {
            "inode/directory".to_string()
        } else {
            shared::mime::detect_mime_type(&full_path)
                .unwrap_or_else(|_| "application/octet-stream".to_string())
        };
        (
            is_dir,
            None,
            symlink_meta.len(),
            symlink_meta.permissions().mode(),
            symlink_meta.ino(),
            symlink_meta.dev(),
            symlink_meta.modified().ok(),
            symlink_meta.accessed().ok(),
            symlink_meta.created().ok(),
            mime,
        )
    };

    let is_hidden = entry_name.starts_with('.');
    let is_executable = (permissions & 0o111) != 0;
    let tags = shared::fs::get_tags(&full_path).unwrap_or_default();

    let metadata = FileMetadata {
        size_bytes,
        modified,
        accessed,
        created,
        permissions,
        inode,
        device_id,
        mime_type,
        is_hidden,
        is_executable,
    };

    Ok(FileItem::new(
        full_path,
        entry_name,
        is_dir,
        is_symlink,
        symlink_target,
        metadata,
        tags,
    ))
}

/// Event emitted by a running column inotify directory watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnWatcherEvent {
    /// Directory entries were reloaded due to filesystem mutation.
    Reloaded {
        /// Updated entries for the column.
        entries: Vec<ColumnEntry>,
        /// Index of the previously selected item if it still exists in the refreshed entries.
        preserved_selected_index: Option<usize>,
    },
    /// The watched parent directory was deleted or moved (`DeleteSelf`).
    DirectoryDeleted(PathBuf),
}

/// Handle allowing caller to stop the background watcher loop.
#[derive(Debug, Clone)]
pub struct ColumnWatcherHandle {
    is_active: Arc<AtomicBool>,
}

impl ColumnWatcherHandle {
    /// Signals the watcher loop to terminate.
    pub fn stop(&self) {
        self.is_active.store(false, Ordering::SeqCst);
    }

    /// Returns `true` if the watcher is still active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }
}

/// Starts an inotify watcher for a directory column, emitting events across a Tokio channel.
///
/// On `Create` / `Delete` / `Modify`, re-reads the directory and maintains existing selection.
/// On `DeleteSelf` or directory deletion, triggers teardown notification.
#[must_use]
pub fn start_column_watcher(
    dir_path: PathBuf,
    selected_name: Option<String>,
    sort_criteria: Option<SortCriteria>,
) -> (ColumnWatcherHandle, mpsc::Receiver<ColumnWatcherEvent>) {
    let (tx, rx) = mpsc::channel(32);
    let is_active = Arc::new(AtomicBool::new(true));
    let handle = ColumnWatcherHandle {
        is_active: Arc::clone(&is_active),
    };

    tokio::spawn(async move {
        let Ok(mut watcher) =
            FileWatcher::with_debounce(std::slice::from_ref(&dir_path), Duration::from_millis(150))
        else {
            return;
        };

        let mut current_selected_name = selected_name;

        while is_active.load(Ordering::SeqCst) {
            tokio::select! {
                maybe_event = watcher.recv() => {
                    let Some(event) = maybe_event else {
                        break;
                    };

                    if !is_active.load(Ordering::SeqCst) {
                        break;
                    }

                    // Check for parent directory deletion
                    let is_delete_self = match &event {
                        FsEvent::Deleted(p) if p == &dir_path => true,
                        FsEvent::Moved { from, .. } if from == &dir_path => true,
                        _ => !dir_path.exists(),
                    };

                    if is_delete_self {
                        let _ = tx.send(ColumnWatcherEvent::DirectoryDeleted(dir_path)).await;
                        break;
                    }

                    // Reload directory entries
                    if let Ok(entries) = read_directory(&dir_path, sort_criteria.as_ref()).await {
                        let preserved_idx = current_selected_name
                            .as_ref()
                            .and_then(|sel_name| entries.iter().position(|e| e.file_name() == sel_name));

                        // If the item was not found, clear selection
                        if preserved_idx.is_none() {
                            current_selected_name = None;
                        }

                        if tx.send(ColumnWatcherEvent::Reloaded {
                            entries,
                            preserved_selected_index: preserved_idx,
                        }).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });

    (handle, rx)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use super::*;

    #[tokio::test]
    async fn test_read_directory_sorting_and_kinds() {
        let temp_dir =
            std::env::temp_dir().join(format!("findx_test_read_dir_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Create folders and files
        fs::create_dir(temp_dir.join("folder_b")).unwrap();
        fs::create_dir(temp_dir.join("folder_a")).unwrap();
        fs::write(temp_dir.join("file_2.txt"), b"hello 2").unwrap();
        fs::write(temp_dir.join("file_10.txt"), b"hello 10").unwrap();
        fs::write(temp_dir.join("file_1.txt"), b"hello 1").unwrap();

        let entries = read_directory(&temp_dir, None).await.unwrap();

        // Folders first, sorted naturally: folder_a, folder_b
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].file_name(), "folder_a");
        assert!(entries[0].is_dir());
        assert_eq!(entries[1].file_name(), "folder_b");
        assert!(entries[1].is_dir());

        // Files sorted naturally: file_1.txt, file_2.txt, file_10.txt
        assert_eq!(entries[2].file_name(), "file_1.txt");
        assert!(!entries[2].is_dir());
        assert_eq!(entries[3].file_name(), "file_2.txt");
        assert!(!entries[3].is_dir());
        assert_eq!(entries[4].file_name(), "file_10.txt");
        assert!(!entries[4].is_dir());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_read_directory_permission_denied() {
        // Test non-existent path
        let non_existent = PathBuf::from("/non/existent/path/for/finder/test");
        let res = read_directory(&non_existent, None).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_column_watcher_cancellation() {
        let temp_dir =
            std::env::temp_dir().join(format!("findx_test_watcher_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let (handle, _rx) = start_column_watcher(temp_dir.clone(), None, None);
        assert!(handle.is_active());
        handle.stop();
        assert!(!handle.is_active());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
