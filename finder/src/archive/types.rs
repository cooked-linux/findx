//! Domain types for archive formats, manifests, and compression jobs.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Supported compression archive formats.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum ArchiveFormat {
    /// Standard PKZIP format.
    #[default]
    Zip,
    /// Gzip-compressed tarball (`.tar.gz`).
    TarGz,
    /// XZ-compressed tarball (`.tar.xz`).
    TarXz,
    /// Zstandard-compressed tarball (`.tar.zst`).
    TarZst,
}

impl ArchiveFormat {
    /// Returns the standard file extension with leading dot.
    #[must_use]
    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Zip => ".zip",
            Self::TarGz => ".tar.gz",
            Self::TarXz => ".tar.xz",
            Self::TarZst => ".tar.zst",
        }
    }

    /// Returns the canonical MIME type string for this format.
    #[must_use]
    pub const fn mime_type(&self) -> &'static str {
        match self {
            Self::Zip => "application/zip",
            Self::TarGz => "application/gzip",
            Self::TarXz => "application/x-xz",
            Self::TarZst => "application/zstd",
        }
    }

    /// Returns the human-readable display name of the format.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Zip => "ZIP Archive (.zip)",
            Self::TarGz => "Gzip Tarball (.tar.gz)",
            Self::TarXz => "XZ Tarball (.tar.xz)",
            Self::TarZst => "Zstandard Tarball (.tar.zst)",
        }
    }

    /// Parses an [`ArchiveFormat`] from an extension string (with or without leading dot).
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        let lower = ext.to_lowercase();
        if lower == "zip" || lower == ".zip" {
            Some(Self::Zip)
        } else if lower == "tar.gz" || lower == ".tar.gz" || lower == "tgz" {
            Some(Self::TarGz)
        } else if lower == "tar.xz" || lower == ".tar.xz" || lower == "txz" {
            Some(Self::TarXz)
        } else if lower == "tar.zst" || lower == ".tar.zst" || lower == "tzst" {
            Some(Self::TarZst)
        } else {
            None
        }
    }
}

/// Progress report emitted during archive creation.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveProgress {
    /// Active relative file name being compressed.
    pub current_file: String,
    /// Number of items processed so far.
    pub files_processed: u64,
    /// Total count of items scheduled in the manifest.
    pub total_files: u64,
    /// Cumulative uncompressed bytes written so far.
    pub bytes_written: u64,
    /// Total uncompressed bytes across all items in the manifest.
    pub total_uncompressed_bytes: u64,
    /// Ratio completed between `0.0` and `100.0`.
    pub percentage: f64,
    /// Instantaneous throughput in bytes per second.
    pub bytes_per_second: f64,
    /// Estimated time remaining in seconds, if known.
    pub eta_seconds: Option<f64>,
}

/// Individual item discovered during preflight filesystem walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntryItem {
    /// Absolute path of the item on disk.
    pub disk_path: PathBuf,
    /// Relative path inside the created archive.
    pub archive_rel_path: String,
    /// Whether this entry is a directory.
    pub is_directory: bool,
    /// Whether this entry is a symbolic link.
    pub is_symlink: bool,
    /// Target path of the symlink, if `is_symlink` is true.
    pub symlink_target: Option<PathBuf>,
    /// File size in bytes (0 for directories and symlinks).
    pub file_size: u64,
    /// POSIX file mode bits (with setuid/setgid stripped).
    pub mode: u32,
    /// Modification timestamp in seconds.
    pub mtime_sec: i64,
    /// Modification timestamp nanoseconds.
    pub mtime_nsec: i64,
}

/// Preflight manifest summarizing archive workload before write starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveManifest {
    /// Flat list of entries to write into the archive.
    pub entries: Vec<ArchiveEntryItem>,
    /// Cumulative uncompressed bytes of all files.
    pub total_uncompressed_bytes: u64,
    /// Total count of entries (files, directories, symlinks).
    pub total_files: u64,
    /// Whether ZIP64 extensions are required (>2GB or >60,000 files).
    pub requires_zip64: bool,
}

/// Job description for an asynchronous compression task.
#[derive(Debug, Clone)]
pub struct ArchiveCompressionJob {
    /// Unique job identifier.
    pub job_id: String,
    /// Selected source paths to compress.
    pub source_paths: Vec<PathBuf>,
    /// Target destination directory.
    pub destination_dir: PathBuf,
    /// Compression format.
    pub format: ArchiveFormat,
    /// Final promoted archive file path.
    pub final_archive_path: PathBuf,
    /// Unlinked temporary staging file path.
    pub temp_archive_path: PathBuf,
    /// Shared atomic cancellation flag.
    pub is_cancelled: Arc<AtomicBool>,
}
