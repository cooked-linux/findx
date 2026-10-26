//! Low-level file transfer engine supporting `copy_file_range` reflink,
//! chunked buffered streaming fallback, atomic moves, metadata preservation,
//! and page-cache protection.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;

use shared::models::operation::{OperationKind, TransferItem};

use super::error::{OperationError, Result};
use super::types::TaskControl;

/// Chunk buffer size for streaming fallback (1 MB).
pub const CHUNK_BUFFER_SIZE: usize = 1024 * 1024;

/// File size threshold above which page cache advise is issued (10 MB).
pub const PAGE_CACHE_FADVISE_THRESHOLD: u64 = 10 * 1024 * 1024;

/// Maximum slice size for a single `copy_file_range` invocation (16 MB).
pub const MAX_COPY_FILE_RANGE_SLICE: usize = 16 * 1024 * 1024;

/// Best-effort extended attributes copying between paths.
fn preserve_xattrs(src: &Path, dst: &Path) {
    if let Ok(attrs) = xattr::list(src) {
        for attr in attrs {
            if let Ok(Some(val)) = xattr::get(src, &attr) {
                let _ = xattr::set(dst, &attr, &val);
            }
        }
    }
}

/// Safely removes a partially transferred file upon failure or cancellation.
fn cleanup_partial_destination(dst: &Path, dst_file: Option<File>) {
    drop(dst_file);
    let _ = std::fs::remove_file(dst);
}

/// Executes an atomic same-filesystem move using `std::fs::rename`.
///
/// # Errors
/// Returns an [`OperationError`] if the move fails or crosses filesystem boundaries (`EXDEV`).
pub fn atomic_move(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|err| OperationError::Io {
            path: parent.to_path_buf(),
            source: err,
        })?;
    }

    std::fs::rename(src, dst).map_err(|err| {
        if err.raw_os_error() == Some(libc::EXDEV) {
            OperationError::Invalid("Cross-device link not supported for atomic rename".into())
        } else {
            OperationError::Io {
                path: src.to_path_buf(),
                source: err,
            }
        }
    })
}

/// Creates a symbolic link pointing to the same target as `src`.
///
/// # Errors
/// Returns an [`OperationError`] if reading the symlink target or creating the symlink fails.
pub fn copy_symlink(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|err| OperationError::Io {
            path: parent.to_path_buf(),
            source: err,
        })?;
    }

    let target = std::fs::read_link(src).map_err(|err| OperationError::Io {
        path: src.to_path_buf(),
        source: err,
    })?;

    std::os::unix::fs::symlink(&target, dst).map_err(|err| OperationError::Io {
        path: dst.to_path_buf(),
        source: err,
    })?;

    if let Ok(meta) = std::fs::symlink_metadata(src) {
        let atime = filetime::FileTime::from_last_access_time(&meta);
        let mtime = filetime::FileTime::from_last_modification_time(&meta);
        let _ = filetime::set_symlink_file_times(dst, atime, mtime);
    }

    Ok(())
}

/// Creates a destination directory copying permissions and timestamps from `src`.
///
/// # Errors
/// Returns an [`OperationError`] if creating the directory fails.
pub fn create_directory_with_metadata(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|err| OperationError::Io {
        path: dst.to_path_buf(),
        source: err,
    })?;

    if let Ok(meta) = std::fs::symlink_metadata(src) {
        let _ = std::fs::set_permissions(dst, meta.permissions());
        let atime = filetime::FileTime::from_last_access_time(&meta);
        let mtime = filetime::FileTime::from_last_modification_time(&meta);
        let _ = filetime::set_file_times(dst, atime, mtime);
    }
    preserve_xattrs(src, dst);

    Ok(())
}

/// Internal helper to wait while task is in paused state.
fn check_pause_and_cancel(
    control: &TaskControl,
    dst: &Path,
    dst_file: &mut Option<File>,
) -> Result<()> {
    if control.is_cancelled() {
        cleanup_partial_destination(dst, dst_file.take());
        return Err(OperationError::Cancelled);
    }

    while control.is_paused() {
        if control.is_cancelled() {
            cleanup_partial_destination(dst, dst_file.take());
            return Err(OperationError::Cancelled);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    Ok(())
}

/// Transfers data between opened files handling `copy_file_range` and fallback streaming chunks.
fn copy_loop_internal<F>(
    src: &Path,
    dst: &Path,
    src_file: &mut File,
    dst_file_opt: &mut Option<File>,
    total_bytes: u64,
    control: &TaskControl,
    mut on_progress: F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    let mut copied: u64 = 0;
    let mut use_copy_file_range = true;
    let mut buffer = vec![0u8; CHUNK_BUFFER_SIZE];

    while copied < total_bytes {
        check_pause_and_cancel(control, dst, dst_file_opt)?;

        let Some(ref mut dst_file) = *dst_file_opt else {
            return Err(OperationError::Cancelled);
        };

        let remaining = total_bytes.saturating_sub(copied);

        if use_copy_file_range {
            let chunk_slice = usize::try_from(remaining)
                .unwrap_or(usize::MAX)
                .min(CHUNK_BUFFER_SIZE);

            match nix::fcntl::copy_file_range(&*src_file, None, &mut *dst_file, None, chunk_slice) {
                Ok(0) => break,
                Ok(bytes) => {
                    copied = copied.saturating_add(bytes as u64);
                    on_progress(copied, total_bytes);
                    check_pause_and_cancel(control, dst, dst_file_opt)?;
                }
                Err(_nix_err) => {
                    use_copy_file_range = false;

                    if let Err(e) = src_file.seek(SeekFrom::Start(copied)) {
                        cleanup_partial_destination(dst, dst_file_opt.take());
                        return Err(OperationError::Io {
                            path: src.to_path_buf(),
                            source: e,
                        });
                    }
                    if let Err(e) = dst_file.seek(SeekFrom::Start(copied)) {
                        cleanup_partial_destination(dst, dst_file_opt.take());
                        return Err(OperationError::Io {
                            path: dst.to_path_buf(),
                            source: e,
                        });
                    }
                }
            }
        } else {
            let to_read = usize::try_from(remaining)
                .unwrap_or(usize::MAX)
                .min(CHUNK_BUFFER_SIZE);

            let bytes_read = match src_file.read(&mut buffer[..to_read]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) => {
                    cleanup_partial_destination(dst, dst_file_opt.take());
                    return Err(OperationError::Io {
                        path: src.to_path_buf(),
                        source: err,
                    });
                }
            };

            if let Err(err) = dst_file.write_all(&buffer[..bytes_read]) {
                cleanup_partial_destination(dst, dst_file_opt.take());
                return Err(OperationError::Io {
                    path: dst.to_path_buf(),
                    source: err,
                });
            }

            copied = copied.saturating_add(bytes_read as u64);
            on_progress(copied, total_bytes);
            check_pause_and_cancel(control, dst, dst_file_opt)?;
        }
    }

    Ok(copied)
}

/// Applies page cache advise on completed large file copies.
fn apply_page_cache_advice(src_file: &File, dst_file: &File, total_bytes: u64) {
    if total_bytes > PAGE_CACHE_FADVISE_THRESHOLD {
        let _ = nix::fcntl::posix_fadvise(
            src_file.as_raw_fd(),
            0,
            0,
            nix::fcntl::PosixFadviseAdvice::POSIX_FADV_DONTNEED,
        );
        let _ = nix::fcntl::posix_fadvise(
            dst_file.as_raw_fd(),
            0,
            0,
            nix::fcntl::PosixFadviseAdvice::POSIX_FADV_DONTNEED,
        );
    }
}

/// Preserves permissions, timestamps, and extended attributes from source to destination.
fn preserve_file_metadata(src: &Path, dst: &Path, meta: &std::fs::Metadata) {
    let _ = std::fs::set_permissions(dst, meta.permissions());
    let atime = filetime::FileTime::from_last_access_time(meta);
    let mtime = filetime::FileTime::from_last_modification_time(meta);
    let _ = filetime::set_file_times(dst, atime, mtime);
    preserve_xattrs(src, dst);
}

/// Transfers a regular file using `copy_file_range` reflink cloning with streaming chunked fallback.
///
/// # Errors
/// Returns an [`OperationError`] if reading, writing, seeking, or metadata preservation fails,
/// or if cancellation was requested.
pub fn copy_file_with_control<F>(
    src: &Path,
    dst: &Path,
    control: &TaskControl,
    mut on_progress: F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    if control.is_cancelled() {
        return Err(OperationError::Cancelled);
    }

    let mut src_file = File::open(src).map_err(|err| OperationError::Io {
        path: src.to_path_buf(),
        source: err,
    })?;

    let meta = src_file.metadata().map_err(|err| OperationError::Io {
        path: src.to_path_buf(),
        source: err,
    })?;
    let total_bytes = meta.len();

    if let Some(parent) = dst.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|err| OperationError::Io {
            path: parent.to_path_buf(),
            source: err,
        })?;
    }

    let mut dst_file_opt = Some(File::create(dst).map_err(|err| OperationError::Io {
        path: dst.to_path_buf(),
        source: err,
    })?);

    if total_bytes == 0 {
        if let Some(mut dst_file) = dst_file_opt.take() {
            let _ = dst_file.flush();
        }
        preserve_file_metadata(src, dst, &meta);
        on_progress(0, 0);
        return Ok(0);
    }

    let copied = copy_loop_internal(
        src,
        dst,
        &mut src_file,
        &mut dst_file_opt,
        total_bytes,
        control,
        &mut on_progress,
    )?;

    if control.is_cancelled() {
        cleanup_partial_destination(dst, dst_file_opt.take());
        return Err(OperationError::Cancelled);
    }

    let Some(mut dst_file) = dst_file_opt.take() else {
        return Err(OperationError::Cancelled);
    };

    if let Err(err) = dst_file.flush() {
        cleanup_partial_destination(dst, Some(dst_file));
        return Err(OperationError::Io {
            path: dst.to_path_buf(),
            source: err,
        });
    }

    apply_page_cache_advice(&src_file, &dst_file, total_bytes);

    drop(dst_file);
    drop(src_file);

    preserve_file_metadata(src, dst, &meta);

    Ok(copied)
}

/// Executes a transfer for a single manifest item across various operation kinds.
///
/// # Errors
/// Returns an [`OperationError`] if I/O fails or cancellation is signaled.
pub fn execute_item_transfer<F>(
    item: &TransferItem,
    kind: OperationKind,
    control: &TaskControl,
    on_progress: F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    match kind {
        OperationKind::Trash => {
            shared::fs::trash::move_to_trash(&item.source)?;
            Ok(0)
        }
        OperationKind::Delete => {
            let meta = std::fs::symlink_metadata(&item.source).map_err(|e| OperationError::Io {
                path: item.source.clone(),
                source: e,
            })?;
            if meta.is_dir() {
                std::fs::remove_dir_all(&item.source).map_err(|e| OperationError::Io {
                    path: item.source.clone(),
                    source: e,
                })?;
            } else {
                std::fs::remove_file(&item.source).map_err(|e| OperationError::Io {
                    path: item.source.clone(),
                    source: e,
                })?;
            }
            Ok(0)
        }
        OperationKind::Copy | OperationKind::Duplicate => {
            let dst = item.destination.as_deref().ok_or_else(|| {
                OperationError::Invalid(format!(
                    "Missing destination path for {}",
                    item.source.display()
                ))
            })?;

            let meta = std::fs::symlink_metadata(&item.source).map_err(|e| OperationError::Io {
                path: item.source.clone(),
                source: e,
            })?;

            if meta.is_symlink() {
                copy_symlink(&item.source, dst)?;
                Ok(0)
            } else if meta.is_dir() {
                create_directory_with_metadata(&item.source, dst)?;
                Ok(0)
            } else {
                copy_file_with_control(&item.source, dst, control, on_progress)
            }
        }
        OperationKind::Move => {
            let dst = item.destination.as_deref().ok_or_else(|| {
                OperationError::Invalid(format!(
                    "Missing destination path for {}",
                    item.source.display()
                ))
            })?;

            let meta = std::fs::symlink_metadata(&item.source).map_err(|e| OperationError::Io {
                path: item.source.clone(),
                source: e,
            })?;

            // Try fast-path atomic move first
            if atomic_move(&item.source, dst).is_ok() {
                return Ok(item.size_bytes);
            }

            // Fallback for cross-device move
            if meta.is_symlink() {
                copy_symlink(&item.source, dst)?;
                let _ = std::fs::remove_file(&item.source);
                Ok(0)
            } else if meta.is_dir() {
                create_directory_with_metadata(&item.source, dst)?;
                Ok(0)
            } else {
                let bytes = copy_file_with_control(&item.source, dst, control, on_progress)?;
                let _ = std::fs::remove_file(&item.source);
                Ok(bytes)
            }
        }
        OperationKind::Restore => {
            // Restore handled via trash recovery
            Ok(0)
        }
        OperationKind::Compress => Err(OperationError::Invalid(
            "Compression operations must be dispatched through the archive engine".into(),
        )),
    }
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
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_copy_file_5mb_with_progress() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src_5mb.bin");
        let dst = dir.path().join("dst_5mb.bin");

        let size: usize = 5 * 1024 * 1024; // 5 MB
        let payload = vec![0xABu8; size];
        {
            let mut f = File::create(&src).expect("create src");
            f.write_all(&payload).expect("write src");
        }

        let control = TaskControl::new();
        let mut progress_count = 0;
        let mut last_copied = 0;

        let result = copy_file_with_control(&src, &dst, &control, |copied, total| {
            progress_count += 1;
            last_copied = copied;
            assert_eq!(total, size as u64);
        });

        assert!(result.is_ok());
        assert_eq!(last_copied, size as u64);
        assert!(progress_count >= 1);
        assert!(dst.exists());
        assert_eq!(
            std::fs::metadata(&dst).expect("metadata").len(),
            size as u64
        );
    }

    #[test]
    fn test_copy_file_cancellation_cleanup() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src_cancel.bin");
        let dst = dir.path().join("dst_cancel.bin");

        let size: usize = 5 * 1024 * 1024; // 5 MB
        let payload = vec![0xEFu8; size];
        {
            let mut f = File::create(&src).expect("create src");
            f.write_all(&payload).expect("write src");
        }

        let control = TaskControl::new();
        let control_clone = control.clone();

        let result = copy_file_with_control(&src, &dst, &control, |copied, _total| {
            if copied > 0 {
                control_clone.cancel();
            }
        });

        assert!(matches!(result, Err(OperationError::Cancelled)));
        assert!(
            !dst.exists(),
            "destination file must be cleaned up on cancel"
        );
    }

    #[test]
    fn test_atomic_move_success() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("move_src.txt");
        let dst = dir.path().join("move_dst.txt");

        std::fs::write(&src, b"move payload").expect("write src");
        assert!(src.exists());

        let res = atomic_move(&src, &dst);
        assert!(res.is_ok());
        assert!(!src.exists());
        assert!(dst.exists());
        assert_eq!(std::fs::read(&dst).expect("read"), b"move payload");
    }
}
