//! Parallel filesystem directory traversal and physical byte size scanner.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use dashmap::DashSet;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::models::{FolderSizeError, FolderSizeResult};

/// Traversal accumulator tracking bytes and counts for a subtree.
#[derive(Debug, Default, Clone, Copy)]
struct SubtreeStats {
    total_bytes: u64,
    file_count: u64,
    folder_count: u64,
}

impl SubtreeStats {
    const fn add(&mut self, other: Self) {
        self.total_bytes = self.total_bytes.saturating_add(other.total_bytes);
        self.file_count = self.file_count.saturating_add(other.file_count);
        self.folder_count = self.folder_count.saturating_add(other.folder_count);
    }
}

/// Synchronously calculates the recursive size and item count of a folder tree
/// using Rayon parallelism and inode deduplication for hard links.
///
/// Physical disk block allocation (`st_blocks * 512`) is computed to accurately
/// account for sparse files and filesystem block granularity.
///
/// # Errors
///
/// Returns [`FolderSizeError::NotFound`] if the root path does not exist.
/// Returns [`FolderSizeError::PermissionDenied`] if the root path cannot be read.
/// Returns [`FolderSizeError::Cancelled`] if `cancel_token` is flagged.
pub fn calculate_folder_size_sync(
    folder_path: &Path,
    cancel_token: Option<&AtomicBool>,
) -> Result<FolderSizeResult, FolderSizeError> {
    if cancel_token.is_some_and(|token| token.load(Ordering::Relaxed)) {
        return Err(FolderSizeError::Cancelled);
    }

    let root_meta = match fs::symlink_metadata(folder_path) {
        Ok(meta) => meta,
        Err(err) => match err.kind() {
            std::io::ErrorKind::NotFound => {
                return Err(FolderSizeError::NotFound(folder_path.to_path_buf()));
            }
            std::io::ErrorKind::PermissionDenied => {
                return Err(FolderSizeError::PermissionDenied(folder_path.to_path_buf()));
            }
            _ => {
                return Err(FolderSizeError::Io {
                    path: folder_path.to_path_buf(),
                    source: err,
                });
            }
        },
    };

    if !root_meta.is_dir() {
        return Err(FolderSizeError::NotFound(folder_path.to_path_buf()));
    }

    let root_dev = root_meta.dev();
    let root_ino = root_meta.ino();
    let root_mtime = root_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    let visited_inodes = Arc::new(DashSet::new());
    visited_inodes.insert((root_dev, root_ino));

    let stats = walk_dir_parallel(folder_path, &visited_inodes, cancel_token)?;

    let calculated_at = SystemTime::now();

    Ok(FolderSizeResult {
        total_bytes: stats.total_bytes,
        file_count: stats.file_count,
        folder_count: stats.folder_count,
        dir_mtime: root_mtime,
        calculated_at,
    })
}

/// Recursively traverses a directory, utilizing Rayon work-stealing parallelism
/// when discovering multiple subdirectories.
fn walk_dir_parallel(
    current_dir: &Path,
    visited_inodes: &Arc<DashSet<(u64, u64)>>,
    cancel_token: Option<&AtomicBool>,
) -> Result<SubtreeStats, FolderSizeError> {
    if cancel_token.is_some_and(|token| token.load(Ordering::Relaxed)) {
        return Err(FolderSizeError::Cancelled);
    }

    let entries = match fs::read_dir(current_dir) {
        Ok(rd) => rd,
        Err(err) => {
            tracing::warn!(
                path = %current_dir.display(),
                error = %err,
                "skipping unreadable subdirectory during folder size scan"
            );
            return Ok(SubtreeStats::default());
        }
    };

    let mut current_stats = SubtreeStats::default();
    let mut subdirs: Vec<PathBuf> = Vec::new();

    for entry_result in entries {
        if cancel_token.is_some_and(|token| token.load(Ordering::Relaxed)) {
            return Err(FolderSizeError::Cancelled);
        }

        let Ok(entry) = entry_result else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        let entry_path = entry.path();

        if file_type.is_symlink() {
            // Do not follow symlinks to prevent directory traversal loops and external device walks
            if let Ok(meta) = fs::symlink_metadata(&entry_path) {
                let dev = meta.dev();
                let ino = meta.ino();
                if visited_inodes.insert((dev, ino)) {
                    let blocks_bytes = meta.blocks().saturating_mul(512);
                    current_stats.total_bytes =
                        current_stats.total_bytes.saturating_add(blocks_bytes);
                }
                current_stats.file_count = current_stats.file_count.saturating_add(1);
            }
        } else if file_type.is_dir() {
            if let Ok(meta) = fs::symlink_metadata(&entry_path) {
                let dev = meta.dev();
                let ino = meta.ino();
                if visited_inodes.insert((dev, ino)) {
                    current_stats.folder_count = current_stats.folder_count.saturating_add(1);
                    subdirs.push(entry_path);
                }
            }
        } else {
            // Regular files, block/character devices, fifos, sockets
            if let Ok(meta) = fs::symlink_metadata(&entry_path) {
                let dev = meta.dev();
                let ino = meta.ino();
                if visited_inodes.insert((dev, ino)) {
                    let blocks_bytes = meta.blocks().saturating_mul(512);
                    current_stats.total_bytes =
                        current_stats.total_bytes.saturating_add(blocks_bytes);
                }
                current_stats.file_count = current_stats.file_count.saturating_add(1);
            }
        }
    }

    if subdirs.is_empty() {
        return Ok(current_stats);
    }

    let sub_stats: SubtreeStats = if subdirs.len() > 1 {
        let visited = Arc::clone(visited_inodes);
        subdirs
            .into_par_iter()
            .map(|subdir| walk_dir_parallel(&subdir, &visited, cancel_token))
            .try_reduce(SubtreeStats::default, |mut acc, s| {
                acc.add(s);
                Ok(acc)
            })?
    } else {
        let only_dir = &subdirs[0];
        walk_dir_parallel(only_dir, visited_inodes, cancel_token)?
    };

    current_stats.add(sub_stats);
    Ok(current_stats)
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
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::fs as unix_fs;
    use tempfile::tempdir;

    #[test]
    fn test_calculate_folder_size_with_hardlinks_and_deduplication() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test_root");
        fs::create_dir_all(root.join("subdir")).unwrap();

        let orig_file = root.join("original.dat");
        {
            let mut f = File::create(&orig_file).unwrap();
            f.write_all(&[0x42u8; 8192]).unwrap();
            f.sync_all().unwrap();
        }

        // Create a hard link to original.dat in subdir
        let hardlink_file = root.join("subdir/hardlinked.dat");
        fs::hard_link(&orig_file, &hardlink_file).unwrap();

        // Create an additional small file
        let small_file = root.join("subdir/small.txt");
        {
            let mut f = File::create(&small_file).unwrap();
            f.write_all(b"Hello FindX").unwrap();
            f.sync_all().unwrap();
        }

        let result = calculate_folder_size_sync(&root, None).unwrap();

        // Files: original.dat, hardlinked.dat, small.txt = 3 files
        assert_eq!(result.file_count, 3);
        // Folders: subdir = 1 folder
        assert_eq!(result.folder_count, 1);
        assert_eq!(result.total_items(), 4);

        // Verify that original.dat and hardlinked.dat are only counted once for byte size
        let orig_meta = fs::symlink_metadata(&orig_file).unwrap();
        let small_meta = fs::symlink_metadata(&small_file).unwrap();
        let expected_bytes = (orig_meta.blocks() + small_meta.blocks()) * 512;
        assert_eq!(result.total_bytes, expected_bytes);
    }

    #[test]
    fn test_calculate_folder_size_symlink_does_not_recurse() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("root_dir");
        let external = tmp.path().join("external_dir");

        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&external).unwrap();

        // Put a 1MB file in external directory
        let ext_file = external.join("large.bin");
        {
            let mut f = File::create(&ext_file).unwrap();
            f.write_all(&vec![0xAAu8; 1024 * 1024]).unwrap();
        }

        // Create symlink inside root pointing to external directory
        let link_to_external = root.join("link_to_ext");
        unix_fs::symlink(&external, &link_to_external).unwrap();

        let result = calculate_folder_size_sync(&root, None).unwrap();

        // Root should not follow symlink into external_dir: file_count is 1 (the symlink itself), folder_count is 0
        assert_eq!(result.folder_count, 0);
        assert_eq!(result.file_count, 1);
        assert!(result.total_bytes < 1024 * 1024);
    }

    #[test]
    fn test_calculate_folder_size_cancellation() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("cancel_test");
        fs::create_dir_all(&root).unwrap();

        let cancel_token = AtomicBool::new(true);
        let err = calculate_folder_size_sync(&root, Some(&cancel_token)).unwrap_err();
        assert!(matches!(err, FolderSizeError::Cancelled));
    }
}
