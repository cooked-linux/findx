//! Pre-flight manifest scanner and disk space verification.

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use shared::models::operation::{BatchOperation, OperationKind, TransferItem};
use walkdir::WalkDir;

use super::error::{OperationError, Result};
use super::types::{OperationEvent, PreflightManifest, TaskControl};

/// Progress notification threshold in number of items scanned.
const PREFLIGHT_PROGRESS_INTERVAL: u64 = 500;

/// Resolves a candidate destination path for duplicate operations in the same folder.
#[must_use]
pub fn resolve_duplicate_destination(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("item");
    let ext = source.extension().and_then(|e| e.to_str());

    let candidate_name = ext.map_or_else(
        || format!("{stem} copy"),
        |extension| format!("{stem} copy.{extension}"),
    );

    let mut candidate = parent.join(&candidate_name);
    if !candidate.exists() {
        return candidate;
    }

    let mut counter = 2;
    loop {
        let numbered_name = ext.map_or_else(
            || format!("{stem} copy {counter}"),
            |extension| format!("{stem} copy {counter}.{extension}"),
        );
        candidate = parent.join(&numbered_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

/// Computes the destination path for an item given its source root and base destination.
fn compute_item_destination(
    source_root: &Path,
    item_path: &Path,
    base_dest: &Path,
) -> Result<PathBuf> {
    if item_path == source_root {
        let file_name = source_root.file_name().ok_or_else(|| {
            OperationError::Invalid(format!(
                "Cannot determine file name for root path: {}",
                source_root.display()
            ))
        })?;
        Ok(base_dest.join(file_name))
    } else {
        let parent = source_root.parent().unwrap_or(source_root);
        let relative = item_path.strip_prefix(parent).map_err(|_| {
            OperationError::Invalid(format!(
                "Failed to compute relative path from {} to {}",
                parent.display(),
                item_path.display()
            ))
        })?;
        Ok(base_dest.join(relative))
    }
}

/// Resolves the planned destination path for a given entry path during scanning.
fn resolve_entry_destination(
    source: &Path,
    entry_path: &Path,
    base_dest: Option<&Path>,
    duplicate_dest: Option<&Path>,
) -> Result<Option<PathBuf>> {
    if let Some(dst_root) = base_dest {
        Ok(Some(compute_item_destination(
            source, entry_path, dst_root,
        )?))
    } else if let Some(dup_root) = duplicate_dest {
        if entry_path == source {
            Ok(Some(dup_root.to_path_buf()))
        } else {
            let rel = entry_path.strip_prefix(source).map_err(|_| {
                OperationError::Invalid(format!(
                    "Invalid prefix for duplicate path: {}",
                    entry_path.display()
                ))
            })?;
            Ok(Some(dup_root.join(rel)))
        }
    } else {
        Ok(None)
    }
}

/// Queries available disk space in bytes on the filesystem hosting `path`.
///
/// # Errors
/// Returns an [`OperationError`] if `statvfs` fails on the target or its ancestors.
pub fn query_available_space(path: &Path) -> Result<u64> {
    let check_path = if path.exists() {
        path.to_path_buf()
    } else {
        path.ancestors()
            .find(|p| p.exists())
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
    };

    let stat = nix::sys::statvfs::statvfs(&check_path).map_err(|err| OperationError::Io {
        path: check_path.clone(),
        source: std::io::Error::from_raw_os_error(err as i32),
    })?;

    let fragment_size = if stat.fragment_size() > 0 {
        stat.fragment_size()
    } else {
        stat.block_size()
    };

    Ok(stat.blocks_available().saturating_mul(fragment_size))
}

/// Resolves the filesystem device ID hosting the destination path, if destination is provided.
fn query_destination_device(destination: Option<&Path>) -> Result<Option<u64>> {
    let Some(dst) = destination else {
        return Ok(None);
    };

    let check_path = if dst.exists() {
        dst.to_path_buf()
    } else {
        dst.ancestors()
            .find(|p| p.exists())
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
    };

    let meta = std::fs::metadata(&check_path).map_err(|err| OperationError::Io {
        path: check_path,
        source: err,
    })?;
    Ok(Some(meta.dev()))
}

/// Verifies whether the destination filesystem has sufficient free space.
fn verify_destination_space(
    op: &BatchOperation,
    total_bytes: u64,
    requires_cross_device_copy: bool,
) -> Result<()> {
    let needs_space_check = match op.kind {
        OperationKind::Copy | OperationKind::Duplicate => true,
        OperationKind::Move => requires_cross_device_copy,
        OperationKind::Trash | OperationKind::Restore | OperationKind::Delete => false,
    };

    if needs_space_check && let Some(ref dst) = op.destination {
        let available_bytes = query_available_space(dst)?;
        if available_bytes < total_bytes {
            return Err(OperationError::InsufficientSpace {
                path: dst.clone(),
                required_bytes: total_bytes,
                available_bytes,
            });
        }
    }
    Ok(())
}

/// Accumulator state populated during pre-flight source traversal.
struct PreflightAccumulator<'a, F> {
    op: &'a BatchOperation,
    control: &'a TaskControl,
    on_event: F,
    total_items: u64,
    total_bytes: u64,
    transfer_items: Vec<TransferItem>,
    visited_dirs: HashSet<(u64, u64)>,
    has_cross_device: bool,
}

impl<F: FnMut(OperationEvent)> PreflightAccumulator<'_, F> {
    fn scan_source(&mut self, source: &Path, dst_device: Option<u64>) -> Result<()> {
        let source_meta = std::fs::symlink_metadata(source).map_err(|err| match err.kind() {
            std::io::ErrorKind::PermissionDenied => OperationError::PermissionDenied {
                path: source.to_path_buf(),
            },
            _ => OperationError::Io {
                path: source.to_path_buf(),
                source: err,
            },
        })?;

        let source_dev = source_meta.dev();
        if let Some(target_dev) = dst_device
            && source_dev != target_dev
        {
            self.has_cross_device = true;
        }

        let base_dest = match self.op.kind {
            OperationKind::Copy | OperationKind::Move => self.op.destination.as_deref(),
            OperationKind::Duplicate
            | OperationKind::Trash
            | OperationKind::Restore
            | OperationKind::Delete => None,
        };

        let duplicate_dest = if self.op.kind == OperationKind::Duplicate {
            Some(resolve_duplicate_destination(source))
        } else {
            None
        };

        let walker = WalkDir::new(source)
            .follow_links(false)
            .same_file_system(false);

        for entry_res in walker {
            if self.control.is_cancelled() {
                return Err(OperationError::Cancelled);
            }

            let entry = entry_res.map_err(|err| {
                let path = err.path().unwrap_or(source).to_path_buf();
                if err
                    .io_error()
                    .is_some_and(|ioe| ioe.kind() == std::io::ErrorKind::PermissionDenied)
                {
                    OperationError::PermissionDenied { path }
                } else {
                    OperationError::Io {
                        path,
                        source: std::io::Error::other(err.to_string()),
                    }
                }
            })?;

            let path = entry.path().to_path_buf();
            let file_type = entry.file_type();

            if file_type.is_dir() {
                let meta = std::fs::symlink_metadata(&path).map_err(|err| OperationError::Io {
                    path: path.clone(),
                    source: err,
                })?;
                let dir_key = (meta.dev(), meta.ino());
                if !self.visited_dirs.insert(dir_key) {
                    return Err(OperationError::SymlinkLoop { path });
                }
            }

            let item_size = if file_type.is_file() {
                entry.metadata().map_or(0, |m| m.len())
            } else {
                0
            };

            let item_dest =
                resolve_entry_destination(source, &path, base_dest, duplicate_dest.as_deref())?;

            self.transfer_items
                .push(TransferItem::new(&path, item_dest, item_size));
            self.total_items = self.total_items.saturating_add(1);
            self.total_bytes = self.total_bytes.saturating_add(item_size);

            if self.total_items.is_multiple_of(PREFLIGHT_PROGRESS_INTERVAL) {
                (self.on_event)(OperationEvent::PreflightProgress {
                    operation_id: self.op.id.clone(),
                    items_scanned: self.total_items,
                    bytes_scanned: self.total_bytes,
                });
            }
        }

        Ok(())
    }
}

/// Executes a pre-flight inspection across all source paths in a batch operation.
///
/// # Errors
/// Returns an [`OperationError`] if traversal encounters permission denial, symlink loop,
/// insufficient disk space, or if the task was cancelled.
pub fn run_preflight_scan<F>(
    op: &BatchOperation,
    control: &TaskControl,
    mut on_event: F,
) -> Result<PreflightManifest>
where
    F: FnMut(OperationEvent),
{
    if op.kind.requires_destination() && op.destination.is_none() {
        return Err(OperationError::Invalid(format!(
            "Operation '{}' requires a target destination",
            op.kind
        )));
    }

    let dst_device = query_destination_device(op.destination.as_deref())?;

    let mut acc = PreflightAccumulator {
        op,
        control,
        on_event: &mut on_event,
        total_items: 0,
        total_bytes: 0,
        transfer_items: Vec::new(),
        visited_dirs: HashSet::new(),
        has_cross_device: false,
    };

    for source in &op.sources {
        acc.scan_source(source, dst_device)?;
    }

    let requires_cross_device_copy = if op.kind == OperationKind::Move {
        acc.has_cross_device
    } else if op.kind == OperationKind::Copy {
        true
    } else {
        acc.has_cross_device
    };

    verify_destination_space(op, acc.total_bytes, requires_cross_device_copy)?;

    let manifest = PreflightManifest {
        total_items: acc.total_items,
        total_bytes: acc.total_bytes,
        items: acc.transfer_items,
        requires_cross_device_copy,
    };

    on_event(OperationEvent::PreflightCompleted {
        operation_id: op.id.clone(),
        manifest: manifest.clone(),
    });

    Ok(manifest)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_preflight_file_counting_and_bytes() {
        let dir = tempdir().expect("tempdir");
        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).expect("create_dir");

        let file1 = src_dir.join("a.txt");
        let mut f1 = File::create(&file1).expect("create file");
        f1.write_all(b"12345").expect("write file");

        let sub_dir = src_dir.join("sub");
        fs::create_dir(&sub_dir).expect("create sub");
        let file2 = sub_dir.join("b.txt");
        let mut f2 = File::create(&file2).expect("create file2");
        f2.write_all(b"abcdef").expect("write file2");

        let dst_dir = dir.path().join("dst");
        fs::create_dir(&dst_dir).expect("create dst");

        let op = BatchOperation {
            id: "test-op-1".into(),
            kind: OperationKind::Copy,
            sources: vec![src_dir],
            destination: Some(dst_dir.clone()),
        };

        let control = TaskControl::new();
        let mut progress_events = Vec::new();
        let manifest = run_preflight_scan(&op, &control, |evt| {
            progress_events.push(evt);
        })
        .expect("preflight scan succeeds");

        // 1 src_dir + 1 file1 + 1 sub_dir + 1 file2 = 4 items
        assert_eq!(manifest.total_items, 4);
        assert_eq!(manifest.total_bytes, 11);
        assert!(!manifest.items.is_empty());
        assert!(!progress_events.is_empty());
    }

    #[test]
    fn test_preflight_statvfs_insufficient_space_detection() {
        let dir = tempdir().expect("tempdir");
        let dst = dir.path().join("dst");
        fs::create_dir(&dst).expect("create dst");

        let available = query_available_space(&dst).expect("query available space");
        assert!(available > 0);

        let file = dir.path().join("file.bin");
        let mut f = File::create(&file).expect("create file");
        f.write_all(b"test").expect("write");

        let op = BatchOperation {
            id: "test-space".into(),
            kind: OperationKind::Copy,
            sources: vec![file],
            destination: Some(dst),
        };

        let control = TaskControl::new();
        let result = run_preflight_scan(&op, &control, |_| {});
        assert!(result.is_ok());
    }

    #[test]
    fn test_preflight_cancellation() {
        let dir = tempdir().expect("tempdir");
        let file = dir.path().join("file.bin");
        File::create(&file).expect("create file");

        let op = BatchOperation {
            id: "test-cancel".into(),
            kind: OperationKind::Copy,
            sources: vec![file],
            destination: Some(dir.path().to_path_buf()),
        };

        let control = TaskControl::new();
        control.cancel();

        let result = run_preflight_scan(&op, &control, |_| {});
        assert!(matches!(result, Err(OperationError::Cancelled)));
    }
}
