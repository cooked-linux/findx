//! Fast file cloning using Linux reflink / `copy_file_range` syscall with fallback.
//!
//! Utilizes `copy_file_range` for server-side / filesystem-level copy-on-write
//! reflink operations on supported filesystems (Btrfs, XFS, ZFS), seamlessly falling back
//! to chunked buffered copying across filesystem boundaries (`EXDEV`) or on unsupported systems (`ENOSYS`).

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::errors::{FsError, Result};

/// Default buffer size for fallback chunked file copying (64 KB).
pub const DEFAULT_COPY_BUFFER_SIZE: usize = 65_536;

/// Maximum slice size passed to a single `copy_file_range` syscall (1 GB).
const MAX_COPY_FILE_RANGE_SLICE: usize = 1_073_741_824;

/// Clones or copies a file from `src` to `dst` using `copy_file_range` with fallback.
///
/// # Errors
/// Returns an error if reading from `src` or writing to `dst` fails.
pub fn clone_file(src: &Path, dst: &Path) -> Result<u64> {
    clone_file_with_progress(src, dst, |_, _| {}, || false)
}

/// Fallback helper performing chunked buffered copying when `copy_file_range` is unavailable.
fn copy_buffered_chunk(
    src_file: &mut File,
    dst_file: &mut File,
    buffer: &mut [u8],
    remaining: u64,
    src_path: &Path,
    dst_path: &Path,
) -> Result<usize> {
    let limit = usize::try_from(remaining).unwrap_or(usize::MAX);
    let bytes_to_read = std::cmp::min(buffer.len(), limit);

    let bytes_read = match src_file.read(&mut buffer[..bytes_to_read]) {
        Ok(0) => return Ok(0),
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(0),
        Err(e) => return Err(FsError::io(src_path, e).into()),
    };

    dst_file
        .write_all(&buffer[..bytes_read])
        .map_err(|e| FsError::io(dst_path, e))?;

    Ok(bytes_read)
}

/// Opens source and destination files for cloning, handling 0-byte file edge cases.
fn open_clone_pair(
    src: &Path,
    dst: &Path,
) -> Result<Option<(File, File, u64, std::fs::Permissions)>> {
    let src_file = File::open(src).map_err(|e| FsError::io(src, e))?;
    let metadata = src_file.metadata().map_err(|e| FsError::io(src, e))?;

    if metadata.is_dir() {
        return Err(FsError::io(
            src,
            std::io::Error::new(std::io::ErrorKind::IsADirectory, "cannot clone a directory"),
        )
        .into());
    }

    let total_bytes = metadata.len();
    if total_bytes == 0 {
        File::create(dst).map_err(|e| FsError::io(dst, e))?;
        let _ = std::fs::set_permissions(dst, metadata.permissions());
        return Ok(None);
    }

    let dst_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dst)
        .map_err(|e| FsError::io(dst, e))?;

    Ok(Some((
        src_file,
        dst_file,
        total_bytes,
        metadata.permissions(),
    )))
}

/// Clones or copies a file from `src` to `dst` with real-time progress callbacks
/// and cancellation support.
///
/// On cancellation, the partial destination file is cleaned up and removed.
///
/// # Errors
/// Returns an error if I/O fails or if the operation is cancelled.
pub fn clone_file_with_progress<F, C>(
    src: &Path,
    dst: &Path,
    mut on_progress: F,
    mut is_cancelled: C,
) -> Result<u64>
where
    F: FnMut(u64, u64),
    C: FnMut() -> bool,
{
    if is_cancelled() {
        return Err(FsError::io(
            dst,
            std::io::Error::new(std::io::ErrorKind::Interrupted, "copy operation cancelled"),
        )
        .into());
    }

    let Some((mut src_file, mut dst_file, total_bytes, permissions)) = open_clone_pair(src, dst)?
    else {
        on_progress(0, 0);
        return Ok(0);
    };

    let mut copied: u64 = 0;
    let mut use_copy_file_range = true;
    let mut buffer = vec![0u8; DEFAULT_COPY_BUFFER_SIZE];

    while copied < total_bytes {
        if is_cancelled() {
            drop(dst_file);
            drop(src_file);
            let _ = std::fs::remove_file(dst);
            return Err(FsError::io(
                dst,
                std::io::Error::new(std::io::ErrorKind::Interrupted, "copy operation cancelled"),
            )
            .into());
        }

        if use_copy_file_range {
            let remaining = usize::try_from(total_bytes - copied).unwrap_or(usize::MAX);
            let chunk_to_copy = std::cmp::min(remaining, MAX_COPY_FILE_RANGE_SLICE);

            match nix::fcntl::copy_file_range(&src_file, None, &dst_file, None, chunk_to_copy) {
                Ok(0) => break,
                Ok(n) => {
                    copied += n as u64;
                    on_progress(copied, total_bytes);
                }
                Err(nix_err) => {
                    use_copy_file_range = false;

                    if let Err(e) = src_file.seek(SeekFrom::Start(copied)) {
                        drop(dst_file);
                        drop(src_file);
                        let _ = std::fs::remove_file(dst);
                        return Err(FsError::io(src, e).into());
                    }
                    if let Err(e) = dst_file.seek(SeekFrom::Start(copied)) {
                        drop(dst_file);
                        drop(src_file);
                        let _ = std::fs::remove_file(dst);
                        return Err(FsError::io(dst, e).into());
                    }

                    if nix_err == nix::errno::Errno::EACCES {
                        drop(dst_file);
                        drop(src_file);
                        let _ = std::fs::remove_file(dst);
                        return Err(FsError::permission_denied(src).into());
                    }
                }
            }
        } else {
            let res = copy_buffered_chunk(
                &mut src_file,
                &mut dst_file,
                &mut buffer,
                total_bytes - copied,
                src,
                dst,
            );

            match res {
                Ok(0) => break,
                Ok(n) => {
                    copied += n as u64;
                    on_progress(copied, total_bytes);
                }
                Err(err) => {
                    drop(dst_file);
                    drop(src_file);
                    let _ = std::fs::remove_file(dst);
                    return Err(err);
                }
            }
        }
    }

    let _ = dst_file.flush();
    drop(dst_file);
    drop(src_file);

    let _ = std::fs::set_permissions(dst, permissions);
    preserve_xattrs(src, dst);

    Ok(copied)
}

/// Copies extended attributes from `src` to `dst` best-effort without failing file copy.
fn preserve_xattrs(src: &Path, dst: &Path) {
    if let Ok(attrs) = xattr::list(src) {
        for attr in attrs {
            if let Ok(Some(val)) = xattr::get(src, &attr) {
                let _ = xattr::set(dst, &attr, &val);
            }
        }
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
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_clone_small_file() {
        let mut src = NamedTempFile::new().unwrap();
        let payload = b"Hello, findx reflink clone test!";
        src.write_all(payload).unwrap();
        src.flush().unwrap();

        let dst = NamedTempFile::new().unwrap();
        let dst_path = dst.path().to_path_buf();

        let copied = clone_file(src.path(), &dst_path).unwrap();
        assert_eq!(copied, payload.len() as u64);

        let content = std::fs::read(&dst_path).unwrap();
        assert_eq!(content, payload);
    }

    #[test]
    fn test_clone_empty_file() {
        let src = NamedTempFile::new().unwrap();
        let dst = NamedTempFile::new().unwrap();
        let dst_path = dst.path().to_path_buf();

        let copied = clone_file(src.path(), &dst_path).unwrap();
        assert_eq!(copied, 0);

        let content = std::fs::read(&dst_path).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_clone_multi_chunk_file_with_progress() {
        let mut src = NamedTempFile::new().unwrap();
        let chunk = vec![0xABu8; 100_000]; // 100 KB
        src.write_all(&chunk).unwrap();
        src.flush().unwrap();

        let dst = NamedTempFile::new().unwrap();
        let dst_path = dst.path().to_path_buf();

        let mut reports = Vec::new();
        let copied = clone_file_with_progress(
            src.path(),
            &dst_path,
            |curr, total| reports.push((curr, total)),
            || false,
        )
        .unwrap();

        assert_eq!(copied, 100_000);
        assert!(!reports.is_empty());
        assert_eq!(reports.last().unwrap(), &(100_000, 100_000));

        let content = std::fs::read(&dst_path).unwrap();
        assert_eq!(content, chunk);
    }

    #[test]
    fn test_clone_cancellation_cleans_up_destination() {
        let mut src = NamedTempFile::new().unwrap();
        let chunk = vec![0xCDu8; 200_000];
        src.write_all(&chunk).unwrap();
        src.flush().unwrap();

        let temp_dir = tempfile::tempdir().unwrap();
        let dst_path = temp_dir.path().join("cancelled_target.bin");

        let result = clone_file_with_progress(
            src.path(),
            &dst_path,
            |_, _| {},
            || true, // Cancel immediately
        );

        assert!(result.is_err());
        assert!(
            !dst_path.exists(),
            "destination file must be cleaned up on cancellation"
        );
    }

    #[test]
    fn test_clone_missing_source() {
        let missing = Path::new("/tmp/findx_nonexistent_clone_src_123.bin");
        let dst = NamedTempFile::new().unwrap();

        assert!(clone_file(missing, dst.path()).is_err());
    }
}
