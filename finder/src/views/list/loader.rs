//! Asynchronous and recursive directory loading for hierarchical list view.
//!
//! Provides non-blocking subtree directory reading via `tokio::task::spawn_blocking`,
//! recursive expansion with cycle detection (`seen_inodes`) and 20-level depth limits,
//! and batch row insertion (50 items per frame) via `glib::idle_add_local`.

use std::cell::Cell;
use std::collections::HashSet;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use shared::models::FileItem;

use super::tree_model::TreeNode;
use crate::error::{FinderError, Result};
use crate::sorting::comparator::{SortCriteria, compare_file_items};

/// Maximum recursion depth allowed during recursive tree expansion to prevent stack overflows.
pub const MAX_RECURSION_DEPTH: u32 = 20;

/// Number of items inserted per frame during batch list realization.
pub const BATCH_CHUNK_SIZE: usize = 50;

fn parse_dir_entry(entry: &std::fs::DirEntry, depth: u32) -> Option<TreeNode> {
    let full_path = entry.path();
    let file_name = entry.file_name().to_string_lossy().to_string();

    let symlink_meta = std::fs::symlink_metadata(&full_path).ok()?;
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
        let target = std::fs::read_link(&full_path).ok();
        let target_meta = std::fs::metadata(&full_path).ok();
        if let Some(meta) = target_meta {
            let is_directory = meta.is_dir();
            let mime = if is_directory {
                "inode/directory".to_string()
            } else {
                shared::mime::detect_mime_type(&full_path)
                    .unwrap_or_else(|_| "application/octet-stream".to_string())
            };
            (
                is_directory,
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
                0,
                symlink_meta.permissions().mode(),
                symlink_meta.ino(),
                symlink_meta.dev(),
                symlink_meta.modified().ok(),
                symlink_meta.accessed().ok(),
                symlink_meta.created().ok(),
                "application/x-broken-symlink".to_string(),
            )
        }
    } else {
        let is_directory = symlink_meta.is_dir();
        let mime = if is_directory {
            "inode/directory".to_string()
        } else {
            shared::mime::detect_mime_type(&full_path)
                .unwrap_or_else(|_| "application/octet-stream".to_string())
        };
        (
            is_directory,
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

    let is_hidden = file_name.starts_with('.');
    let is_executable = (permissions & 0o111) != 0;
    let tags = shared::fs::get_tags(&full_path).unwrap_or_default();

    let metadata = shared::models::FileMetadata {
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

    let item = FileItem::new(
        full_path,
        file_name,
        is_dir,
        is_symlink,
        symlink_target,
        metadata,
        tags,
    );
    Some(TreeNode::new(item, depth))
}

/// Synchronously reads entries within `dir_path`, constructing [`TreeNode`]s at `depth`.
///
/// # Errors
/// Returns [`FinderError`] if directory reading fails due to permissions or missing path.
pub fn read_dir_sync(
    dir_path: &Path,
    depth: u32,
    criteria: &SortCriteria,
) -> Result<Vec<TreeNode>> {
    let read_dir = match std::fs::read_dir(dir_path) {
        Ok(rd) => rd,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(FinderError::Fs(shared::errors::FsError::permission_denied(
                dir_path,
            )));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(FinderError::Fs(shared::errors::FsError::not_found(
                dir_path,
            )));
        }
        Err(err) => return Err(FinderError::Io(err)),
    };

    let mut nodes = Vec::new();
    for entry_res in read_dir {
        if let Ok(entry) = entry_res
            && let Some(node) = parse_dir_entry(&entry, depth)
        {
            nodes.push(node);
        }
    }

    nodes.sort_by(|a, b| compare_file_items(&a.item, &b.item, criteria));
    Ok(nodes)
}

/// Asynchronously reads entries within `dir_path` using `tokio::task::spawn_blocking`.
///
/// # Errors
/// Returns [`FinderError`] if directory reading fails.
pub async fn read_dir_async(
    dir_path: PathBuf,
    depth: u32,
    criteria: SortCriteria,
) -> Result<Vec<TreeNode>> {
    tokio::task::spawn_blocking(move || read_dir_sync(&dir_path, depth, &criteria))
        .await
        .map_err(|e| FinderError::Io(std::io::Error::other(e.to_string())))?
}

/// Recursively reads `dir_path` and all descendant directories up to [`MAX_RECURSION_DEPTH`].
///
/// Detects filesystem cycles and symlink loops using `seen_inodes`.
///
/// # Errors
/// Returns [`FinderError`] if reading the root directory fails.
pub fn read_subtree_recursive_sync<S: std::hash::BuildHasher>(
    dir_path: &Path,
    current_depth: u32,
    criteria: &SortCriteria,
    seen_inodes: &mut HashSet<(u64, u64), S>,
) -> Result<Vec<TreeNode>> {
    let mut children = read_dir_sync(dir_path, current_depth, criteria)?;
    if current_depth >= MAX_RECURSION_DEPTH {
        return Ok(children);
    }

    for child in &mut children {
        if child.is_directory {
            if seen_inodes.contains(&child.inode_key) {
                // Symlink loop or cycle detected - do not expand recursively
                continue;
            }
            seen_inodes.insert(child.inode_key);
            if let Ok(sub_children) =
                read_subtree_recursive_sync(&child.path, current_depth + 1, criteria, seen_inodes)
            {
                child.children = sub_children;
                child.is_expanded = true;
            }
        }
    }

    Ok(children)
}

/// Asynchronously reads a subtree recursively on a blocking worker thread.
///
/// # Errors
/// Returns [`FinderError`] if directory reading fails.
pub async fn read_subtree_recursive_async<S: std::hash::BuildHasher + Send + 'static>(
    dir_path: PathBuf,
    current_depth: u32,
    criteria: SortCriteria,
    mut seen_inodes: HashSet<(u64, u64), S>,
) -> Result<(Vec<TreeNode>, HashSet<(u64, u64), S>)> {
    tokio::task::spawn_blocking(move || {
        let children =
            read_subtree_recursive_sync(&dir_path, current_depth, &criteria, &mut seen_inodes)?;
        Ok((children, seen_inodes))
    })
    .await
    .map_err(|e| FinderError::Io(std::io::Error::other(e.to_string())))?
}

/// Dispatches batch row processing in chunks of `BATCH_CHUNK_SIZE` (50 items/frame) via `glib::idle_add_local`.
pub fn batch_process_rows<F>(
    rows: Vec<TreeNode>,
    chunk_size: usize,
    mut on_chunk: F,
    on_complete: Option<Box<dyn FnOnce() + 'static>>,
) where
    F: FnMut(&[TreeNode], bool) + 'static,
{
    if rows.is_empty() {
        on_chunk(&[], true);
        if let Some(comp) = on_complete {
            comp();
        }
        return;
    }

    if rows.len() <= chunk_size {
        on_chunk(&rows, true);
        if let Some(comp) = on_complete {
            comp();
        }
        return;
    }

    let rows_rc = Rc::new(rows);
    let cursor = Rc::new(Cell::new(0usize));
    let on_complete_cell = Rc::new(Cell::new(on_complete));

    glib::idle_add_local(move || {
        let start = cursor.get();
        let total = rows_rc.len();
        let end = (start + chunk_size).min(total);
        let is_last = end >= total;

        let slice = &rows_rc[start..end];
        on_chunk(slice, is_last);
        cursor.set(end);

        if is_last {
            if let Some(comp) = on_complete_cell.take() {
                comp();
            }
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}
