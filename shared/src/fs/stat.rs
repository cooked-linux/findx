//! Enhanced POSIX file status queries using Linux `statx`.
//!
//! Provides extended inode inspection including file birth time (`stx_btime`),
//! device major/minor components, inode deduplication keys ([`InodeKey`]),
//! and filesystem boundary detection.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::errors::{FsError, Result};

/// Inode identifier key composed of device ID and inode number for cycle/deduplication tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InodeKey {
    /// Device identifier.
    pub dev: u64,
    /// Inode number on the filesystem.
    pub ino: u64,
}

impl InodeKey {
    /// Constructs a new [`InodeKey`].
    #[must_use]
    pub const fn new(dev: u64, ino: u64) -> Self {
        Self { dev, ino }
    }

    /// Constructs an [`InodeKey`] directly from standard [`std::fs::Metadata`].
    #[must_use]
    pub fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
        }
    }
}

/// Enhanced filesystem metadata containing birth time and device identification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancedStat {
    /// Device major number.
    pub dev_major: u32,
    /// Device minor number.
    pub dev_minor: u32,
    /// Combined device ID.
    pub dev: u64,
    /// Inode number.
    pub ino: u64,
    /// Total file size in bytes.
    pub size: u64,
    /// File type and permissions mode mask.
    pub mode: u32,
    /// Number of hard links.
    pub nlink: u64,
    /// User ID of owner.
    pub uid: u32,
    /// Group ID of owner.
    pub gid: u32,
    /// Last access time.
    pub atime: Option<SystemTime>,
    /// Last modification time.
    pub mtime: Option<SystemTime>,
    /// Last status change time.
    pub ctime: Option<SystemTime>,
    /// File creation / birth time (if supported by filesystem).
    pub btime: Option<SystemTime>,
}

impl EnhancedStat {
    /// Returns the [`InodeKey`] identifying this file's inode and device.
    #[must_use]
    pub const fn inode_key(&self) -> InodeKey {
        InodeKey::new(self.dev, self.ino)
    }

    /// Returns `true` if this item is a regular file.
    #[must_use]
    pub const fn is_file(&self) -> bool {
        (self.mode & libc::S_IFMT) == libc::S_IFREG
    }

    /// Returns `true` if this item is a directory.
    #[must_use]
    pub const fn is_dir(&self) -> bool {
        (self.mode & libc::S_IFMT) == libc::S_IFDIR
    }

    /// Returns `true` if this item is a symbolic link.
    #[must_use]
    pub const fn is_symlink(&self) -> bool {
        (self.mode & libc::S_IFMT) == libc::S_IFLNK
    }
}

/// Converts a Linux `statx_timestamp` into [`SystemTime`].
fn timestamp_to_system_time(ts: &libc::statx_timestamp) -> Option<SystemTime> {
    if ts.tv_sec < 0 {
        let dur = Duration::new(ts.tv_sec.unsigned_abs(), ts.tv_nsec);
        SystemTime::UNIX_EPOCH.checked_sub(dur)
    } else {
        let dur = Duration::new(ts.tv_sec.cast_unsigned(), ts.tv_nsec);
        SystemTime::UNIX_EPOCH.checked_add(dur)
    }
}

/// Safe wrapper around Linux `statx(2)` syscall.
///
/// # Safety
/// Safely initializes zeroed buffer and delegates to libc.
#[allow(unsafe_code)]
fn raw_statx(
    path: &Path,
    flags: libc::c_int,
    mask: libc::c_uint,
) -> std::io::Result<libc::statx> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    let mut buf = std::mem::MaybeUninit::<libc::statx>::zeroed();

    let ret = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            c_path.as_ptr(),
            flags,
            mask,
            buf.as_mut_ptr(),
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(unsafe { buf.assume_init() })
}

/// Queries enhanced file status using `statx(2)`, falling back to `std::fs::metadata`.
fn query_stat(path: &Path, follow_symlinks: bool) -> Result<EnhancedStat> {
    let flags = if follow_symlinks {
        libc::AT_STATX_SYNC_AS_STAT
    } else {
        libc::AT_SYMLINK_NOFOLLOW | libc::AT_STATX_SYNC_AS_STAT
    };

    let mask = libc::STATX_BASIC_STATS | libc::STATX_BTIME;

    match raw_statx(path, flags, mask) {
        Ok(buf) => {
            let dev_major = buf.stx_dev_major;
            let dev_minor = buf.stx_dev_minor;
            let dev = nix::sys::stat::makedev(u64::from(dev_major), u64::from(dev_minor));
            let ino = buf.stx_ino;
            let size = buf.stx_size;
            let mode = u32::from(buf.stx_mode);
            let nlink = u64::from(buf.stx_nlink);
            let uid = buf.stx_uid;
            let gid = buf.stx_gid;

            let atime = if (buf.stx_mask & libc::STATX_ATIME) != 0 {
                timestamp_to_system_time(&buf.stx_atime)
            } else {
                None
            };

            let mtime = if (buf.stx_mask & libc::STATX_MTIME) != 0 {
                timestamp_to_system_time(&buf.stx_mtime)
            } else {
                None
            };

            let ctime = if (buf.stx_mask & libc::STATX_CTIME) != 0 {
                timestamp_to_system_time(&buf.stx_ctime)
            } else {
                None
            };

            let btime = if (buf.stx_mask & libc::STATX_BTIME) != 0 {
                timestamp_to_system_time(&buf.stx_btime)
            } else {
                None
            };

            Ok(EnhancedStat {
                dev_major,
                dev_minor,
                dev,
                ino,
                size,
                mode,
                nlink,
                uid,
                gid,
                atime,
                mtime,
                ctime,
                btime,
            })
        }
        Err(err) if err.raw_os_error() == Some(libc::ENOSYS) => {
            // Fallback for kernels or environments lacking statx syscall
            fallback_metadata_query(path, follow_symlinks)
        }
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                Err(FsError::not_found(path).into())
            } else if err.kind() == std::io::ErrorKind::PermissionDenied {
                Err(FsError::permission_denied(path).into())
            } else {
                Err(FsError::io(path, err).into())
            }
        }
    }
}

/// Fallback metadata inspection using `std::fs::metadata` / `std::fs::symlink_metadata`.
fn fallback_metadata_query(path: &Path, follow_symlinks: bool) -> Result<EnhancedStat> {
    let meta = if follow_symlinks {
        std::fs::metadata(path)
    } else {
        std::fs::symlink_metadata(path)
    }
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            FsError::not_found(path)
        } else if e.kind() == std::io::ErrorKind::PermissionDenied {
            FsError::permission_denied(path)
        } else {
            FsError::io(path, e)
        }
    })?;

    let dev = meta.dev();
    let dev_major = u32::try_from(nix::sys::stat::major(dev)).unwrap_or(0);
    let dev_minor = u32::try_from(nix::sys::stat::minor(dev)).unwrap_or(0);
    let ino = meta.ino();
    let size = meta.len();
    let mode = meta.mode();
    let nlink = meta.nlink();
    let uid = meta.uid();
    let gid = meta.gid();
    let atime = meta.accessed().ok();
    let mtime = meta.modified().ok();
    let ctime_nsec = u32::try_from(meta.ctime_nsec()).unwrap_or(0);
    let ctime = Some(
        SystemTime::UNIX_EPOCH
            + Duration::new(meta.ctime().unsigned_abs(), ctime_nsec),
    );
    let btime = meta.created().ok();

    Ok(EnhancedStat {
        dev_major,
        dev_minor,
        dev,
        ino,
        size,
        mode,
        nlink,
        uid,
        gid,
        atime,
        mtime,
        ctime,
        btime,
    })
}

/// Retrieves enhanced metadata for a file, following symbolic links.
///
/// # Errors
/// Returns an error if the path does not exist or metadata cannot be queried.
pub fn stat_path(path: &Path) -> Result<EnhancedStat> {
    query_stat(path, true)
}

/// Retrieves enhanced metadata for a file, without following symbolic links.
///
/// # Errors
/// Returns an error if the path does not exist or metadata cannot be queried.
pub fn lstat_path(path: &Path) -> Result<EnhancedStat> {
    query_stat(path, false)
}

/// Finds the nearest existing ancestor path for non-existent targets
/// to resolve destination volume boundaries.
fn resolve_existing_ancestor(path: &Path) -> Result<&Path> {
    let mut curr = path;
    while !curr.exists() {
        if let Some(parent) = curr.parent() {
            curr = parent;
        } else {
            break;
        }
    }

    if !curr.exists() {
        return Err(FsError::not_found(path).into());
    }

    Ok(curr)
}

/// Checks whether two filesystem paths reside on the same filesystem/device.
///
/// If either target does not yet exist, its nearest existing ancestor directory
/// is inspected to determine destination volume boundaries.
///
/// # Errors
/// Returns an error if querying metadata for either path (or their ancestors) fails.
pub fn is_same_filesystem(path_a: &Path, path_b: &Path) -> Result<bool> {
    let target_a = resolve_existing_ancestor(path_a)?;
    let target_b = resolve_existing_ancestor(path_b)?;

    let stat_a = stat_path(target_a)?;
    let stat_b = stat_path(target_b)?;

    Ok(stat_a.dev == stat_b.dev)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_stat_path_regular_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"test content 123").unwrap();
        file.flush().unwrap();

        let stat = stat_path(file.path()).unwrap();
        assert!(stat.is_file());
        assert!(!stat.is_dir());
        assert!(!stat.is_symlink());
        assert_eq!(stat.size, 16);
        assert!(stat.ino > 0);
        assert!(stat.mtime.is_some());

        // InodeKey check
        assert_eq!(stat.inode_key(), InodeKey::new(stat.dev, stat.ino));
    }

    #[test]
    fn test_lstat_path_symlink() {
        let file = NamedTempFile::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let symlink_path = temp_dir.path().join("test_symlink");

        std::os::unix::fs::symlink(file.path(), &symlink_path).unwrap();

        let lstat = lstat_path(&symlink_path).unwrap();
        assert!(lstat.is_symlink());
        assert!(!lstat.is_file());

        let stat = stat_path(&symlink_path).unwrap();
        assert!(stat.is_file());
        assert!(!stat.is_symlink());
    }

    #[test]
    fn test_is_same_filesystem() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        std::fs::write(&file1, b"hello").unwrap();
        std::fs::write(&file2, b"world").unwrap();

        assert!(is_same_filesystem(&file1, &file2).unwrap());

        // Target that does not exist yet inside same directory
        let nonexistent = temp_dir.path().join("not_yet_created.txt");
        assert!(is_same_filesystem(&file1, &nonexistent).unwrap());
    }

    #[test]
    fn test_stat_nonexistent_file() {
        let missing = Path::new("/tmp/findx_nonexistent_stat_file_00000.txt");
        assert!(stat_path(missing).is_err());
        assert!(lstat_path(missing).is_err());
    }
}
