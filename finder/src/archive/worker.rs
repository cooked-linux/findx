//! Background compression worker, preflight scanner, and `OperationsQueue` integration.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_channel::Sender;
use shared::models::operation::{BatchOperation, ProgressReport};
use walkdir::WalkDir;

use crate::archive::error::{ArchiveError, Result};
use crate::archive::naming::{determine_archive_path, generate_staging_path};
use crate::archive::types::{
    ArchiveCompressionJob, ArchiveEntryItem, ArchiveFormat, ArchiveManifest,
};
use crate::archive::writer::ArchiveWriter;
use crate::operations::types::{OperationEvent, TaskControl};
use crate::operations::worker::EmaRateEstimator;

/// Throttle interval for progress reporting (20 Hz / 50 ms).
const PROGRESS_THROTTLE_INTERVAL: Duration = Duration::from_millis(50);

/// Exponential Moving Average smoothing factor.
const EMA_ALPHA: f64 = 0.25;

/// Preflight scanner inspecting source files and calculating total workloads.
///
/// # Errors
/// Returns [`ArchiveError::EmptySelection`] if `source_paths` is empty,
/// [`ArchiveError::InvalidDestination`] if `destination_dir` does not exist or is not a directory,
/// [`ArchiveError::Cancelled`] if cancellation is signaled, or
/// [`ArchiveError::InsufficientSpace`] if destination filesystem has insufficient free space.
#[allow(clippy::too_many_lines, clippy::similar_names)]
pub fn run_archive_preflight(
    source_paths: &[PathBuf],
    destination_dir: &Path,
    is_cancelled: &AtomicBool,
) -> Result<ArchiveManifest> {
    if source_paths.is_empty() {
        return Err(ArchiveError::EmptySelection);
    }

    if !destination_dir.is_dir() {
        return Err(ArchiveError::InvalidDestination(
            destination_dir.to_path_buf(),
        ));
    }

    let base_parent = source_paths[0].parent().unwrap_or_else(|| Path::new("/"));

    let mut entries = Vec::new();
    let mut total_uncompressed_bytes: u64 = 0;
    let mut total_files: u64 = 0;

    for root in source_paths {
        if is_cancelled.load(Ordering::Relaxed) {
            return Err(ArchiveError::Cancelled);
        }

        if !root.exists() && std::fs::symlink_metadata(root).is_err() {
            return Err(ArchiveError::PreflightFailed {
                path: root.clone(),
                message: "Source path does not exist".to_string(),
            });
        }

        // Use WalkDir with follow_links(false) to strictly avoid symlink cycles
        for entry_res in WalkDir::new(root).follow_links(false).sort_by_file_name() {
            if is_cancelled.load(Ordering::Relaxed) {
                return Err(ArchiveError::Cancelled);
            }

            let entry = entry_res.map_err(|e| ArchiveError::PreflightFailed {
                path: root.clone(),
                message: e.to_string(),
            })?;

            let path = entry.path();
            let metadata = std::fs::symlink_metadata(path).map_err(|e| ArchiveError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;

            let is_symlink = metadata.is_symlink();
            let is_directory = metadata.is_dir();
            let symlink_target = if is_symlink {
                std::fs::read_link(path).ok()
            } else {
                None
            };

            let file_size = if is_directory || is_symlink {
                0
            } else {
                metadata.len()
            };

            let mode = metadata.mode();
            let mtime_sec = metadata.mtime();
            let mtime_nsec = metadata.mtime_nsec();

            let rel_path =
                path.strip_prefix(base_parent)
                    .map_err(|_| ArchiveError::PreflightFailed {
                        path: path.to_path_buf(),
                        message: "Failed to determine relative path from base parent".to_string(),
                    })?;

            let archive_rel_path = rel_path.to_string_lossy().to_string();

            entries.push(ArchiveEntryItem {
                disk_path: path.to_path_buf(),
                archive_rel_path,
                is_directory,
                is_symlink,
                symlink_target,
                file_size,
                mode,
                mtime_sec,
                mtime_nsec,
            });

            total_files = total_files.saturating_add(1);
            total_uncompressed_bytes = total_uncompressed_bytes.saturating_add(file_size);
        }
    }

    // ZIP64 auto-enable threshold: >2GB or >60,000 files
    let requires_zip64 = total_uncompressed_bytes > 2 * 1024 * 1024 * 1024 || total_files > 60_000;

    // Verify sufficient free space on destination device
    check_disk_space(destination_dir, total_uncompressed_bytes)?;

    Ok(ArchiveManifest {
        entries,
        total_uncompressed_bytes,
        total_files,
        requires_zip64,
    })
}

/// Verifies whether `destination_dir` contains at least `required_bytes` of available disk space.
///
/// # Errors
/// Returns [`ArchiveError::InsufficientSpace`] if available space is below `required_bytes`.
pub fn check_disk_space(destination_dir: &Path, required_bytes: u64) -> Result<()> {
    let probe_dir = if destination_dir.exists() {
        destination_dir.to_path_buf()
    } else {
        destination_dir
            .ancestors()
            .find(|p| p.exists())
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
    };

    let stat = nix::sys::statfs::statfs(&probe_dir).map_err(|e| ArchiveError::Io {
        path: probe_dir.clone(),
        source: std::io::Error::from_raw_os_error(e as i32),
    })?;

    #[allow(clippy::cast_sign_loss)]
    let available = (stat.blocks_available() as u64).saturating_mul(stat.block_size() as u64);

    verify_disk_space(available, required_bytes, destination_dir)
}

/// Compares available bytes against required bytes, returning [`ArchiveError::InsufficientSpace`] if violated.
///
/// # Errors
/// Returns [`ArchiveError::InsufficientSpace`] if `available < required`.
pub fn verify_disk_space(available: u64, required: u64, destination: &Path) -> Result<()> {
    if available < required {
        Err(ArchiveError::InsufficientSpace {
            path: destination.to_path_buf(),
            required,
            available,
        })
    } else {
        Ok(())
    }
}

/// Headless asynchronous worker managing the lifecycle of an archive compression operation.
pub struct ArchiveWorker {
    pub job: ArchiveCompressionJob,
    pub event_tx: Option<Sender<OperationEvent>>,
}

impl ArchiveWorker {
    /// Constructs a new [`ArchiveWorker`] from a raw compression job and optional event sink.
    #[must_use]
    pub const fn new(job: ArchiveCompressionJob, event_tx: Option<Sender<OperationEvent>>) -> Self {
        Self { job, event_tx }
    }

    /// Constructs an [`ArchiveWorker`] initialized from a [`BatchOperation`] and [`TaskControl`].
    #[must_use]
    pub fn from_batch_operation(
        op: BatchOperation,
        control: &TaskControl,
        event_tx: Sender<OperationEvent>,
    ) -> Self {
        let destination_dir = op.destination.clone().unwrap_or_else(|| {
            op.sources
                .first()
                .and_then(|p| p.parent())
                .unwrap_or_else(|| Path::new("/"))
                .to_path_buf()
        });

        let format = ArchiveFormat::Zip;
        let final_archive_path = determine_archive_path(&destination_dir, &op.sources, format);
        let temp_archive_path = generate_staging_path(&destination_dir, &final_archive_path);

        let job = ArchiveCompressionJob {
            job_id: op.id,
            source_paths: op.sources,
            destination_dir,
            format,
            final_archive_path,
            temp_archive_path,
            is_cancelled: Arc::clone(&control.is_cancelled),
        };

        Self::new(job, Some(event_tx))
    }

    /// Spawns the worker on Tokio's blocking threadpool and returns the join handle.
    #[must_use]
    pub fn spawn(self) -> tokio::task::JoinHandle<Result<()>> {
        tokio::task::spawn_blocking(move || self.run())
    }

    /// Executes the complete preflight, compression, atomic promotion, and cleanup pipeline synchronously.
    ///
    /// # Errors
    /// Returns [`ArchiveError`] if preflight, disk space check, writing, or promotion fails.
    #[allow(clippy::too_many_lines)]
    pub fn run(self) -> Result<()> {
        let job_id = self.job.job_id.clone();
        let event_tx_opt = self.event_tx.clone();

        let run_res = self.run_internal();

        match run_res {
            Ok(()) => {
                if let Some(ref tx) = event_tx_opt {
                    let final_archive = self.job.final_archive_path;
                    let _ = tx.send_blocking(OperationEvent::ItemCompleted {
                        operation_id: job_id.clone(),
                        source: final_archive.clone(),
                        destination: Some(final_archive),
                    });
                    let _ = tx.send_blocking(OperationEvent::Completed {
                        operation_id: job_id,
                    });
                }
                Ok(())
            }
            Err(ArchiveError::Cancelled) => {
                // Immediately remove temporary staging file on cancellation
                let _ = std::fs::remove_file(&self.job.temp_archive_path);
                if let Some(ref tx) = event_tx_opt {
                    let _ = tx.send_blocking(OperationEvent::Cancelled {
                        operation_id: job_id,
                    });
                }
                Err(ArchiveError::Cancelled)
            }
            Err(err) => {
                // Immediately remove temporary staging file on failure
                let _ = std::fs::remove_file(&self.job.temp_archive_path);
                if let Some(ref tx) = event_tx_opt {
                    let _ = tx.send_blocking(OperationEvent::Failed {
                        operation_id: job_id,
                        error: err.to_string(),
                    });
                }
                Err(err)
            }
        }
    }

    fn run_internal(&self) -> Result<()> {
        if self.job.is_cancelled.load(Ordering::Relaxed) {
            return Err(ArchiveError::Cancelled);
        }

        // 1. Run Preflight Inspection
        let manifest = run_archive_preflight(
            &self.job.source_paths,
            &self.job.destination_dir,
            &self.job.is_cancelled,
        )?;

        // 2. Stream Archive to Temporary Staging File
        let mut estimator = EmaRateEstimator::new(EMA_ALPHA);
        let mut last_progress_report = Instant::now();
        let event_tx_opt = self.event_tx.clone();
        let op_id = self.job.job_id.clone();
        let total_uncompressed_bytes = manifest.total_uncompressed_bytes;
        let total_files = manifest.total_files;

        ArchiveWriter::write_archive(
            &self.job,
            &manifest,
            |bytes_written, files_processed, current_file| {
                let rate = estimator.update(bytes_written);
                let eta =
                    estimator.estimate_remaining_seconds(total_uncompressed_bytes, bytes_written);

                let now = Instant::now();
                if now.duration_since(last_progress_report) >= PROGRESS_THROTTLE_INTERVAL
                    || files_processed >= total_files
                {
                    last_progress_report = now;
                    if let Some(ref tx) = event_tx_opt {
                        let report = ProgressReport::new(
                            op_id.clone(),
                            files_processed,
                            total_files,
                            bytes_written,
                            total_uncompressed_bytes,
                            rate,
                            eta,
                            Some(PathBuf::from(current_file)),
                        );
                        let _ = tx.send_blocking(OperationEvent::Progress(report));
                    }
                }
            },
        )?;

        // 3. Promote via Atomic POSIX rename(2)
        std::fs::rename(&self.job.temp_archive_path, &self.job.final_archive_path).map_err(
            |e| {
                let _ = std::fs::remove_file(&self.job.temp_archive_path);
                ArchiveError::Io {
                    path: self.job.final_archive_path.clone(),
                    source: e,
                }
            },
        )?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_preflight_scanner_accurate_calculation() {
        let tmp = tempdir().unwrap();
        let folder = tmp.path().join("test_dir");
        std::fs::create_dir(&folder).unwrap();

        let file1 = folder.join("file1.txt");
        File::create(&file1).unwrap().write_all(b"12345").unwrap(); // 5 bytes

        let file2 = folder.join("file2.txt");
        File::create(&file2)
            .unwrap()
            .write_all(b"6789012345")
            .unwrap(); // 10 bytes

        let subfolder = folder.join("sub");
        std::fs::create_dir(&subfolder).unwrap();

        let file3 = subfolder.join("file3.txt");
        File::create(&file3).unwrap().write_all(b"abc").unwrap(); // 3 bytes

        let cancelled = AtomicBool::new(false);
        let manifest = run_archive_preflight(&[folder], tmp.path(), &cancelled).unwrap();

        // 1 root dir, 2 files, 1 subdir, 1 subfile = 5 entries
        assert_eq!(manifest.total_files, 5);
        assert_eq!(manifest.total_uncompressed_bytes, 18);
        assert!(!manifest.requires_zip64);
    }

    #[test]
    fn test_cancellation_mid_write_cleans_up_staging_file() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("big_file.bin");
        File::create(&file)
            .unwrap()
            .write_all(&vec![0u8; 1024 * 1024])
            .unwrap();

        let final_path = tmp.path().join("Archive.zip");
        let temp_path = generate_staging_path(tmp.path(), &final_path);

        let cancelled = Arc::new(AtomicBool::new(true)); // Pre-cancelled
        let job = ArchiveCompressionJob {
            job_id: "test-cancel-op".to_string(),
            source_paths: vec![file],
            destination_dir: tmp.path().to_path_buf(),
            format: ArchiveFormat::Zip,
            final_archive_path: final_path.clone(),
            temp_archive_path: temp_path.clone(),
            is_cancelled: cancelled,
        };

        let worker = ArchiveWorker::new(job, None);
        let res = worker.run();

        assert!(matches!(res, Err(ArchiveError::Cancelled)));
        assert!(!temp_path.exists());
        assert!(!final_path.exists());
    }

    #[test]
    fn test_disk_space_check_insufficient_space() {
        let tmp = tempdir().unwrap();
        let res = verify_disk_space(1000, 5000, tmp.path());
        assert!(matches!(
            res,
            Err(ArchiveError::InsufficientSpace {
                required: 5000,
                available: 1000,
                ..
            })
        ));

        let ok_res = verify_disk_space(5000, 1000, tmp.path());
        assert!(ok_res.is_ok());
    }

    #[test]
    fn test_atomic_rename_promotes_staging_to_final() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("sample.txt");
        File::create(&file)
            .unwrap()
            .write_all(b"Sample payload")
            .unwrap();

        let final_path = tmp.path().join("Sample.zip");
        let temp_path = generate_staging_path(tmp.path(), &final_path);

        let job = ArchiveCompressionJob {
            job_id: "test-promote-op".to_string(),
            source_paths: vec![file],
            destination_dir: tmp.path().to_path_buf(),
            format: ArchiveFormat::Zip,
            final_archive_path: final_path.clone(),
            temp_archive_path: temp_path.clone(),
            is_cancelled: Arc::new(AtomicBool::new(false)),
        };

        let (tx, rx) = async_channel::unbounded();
        let worker = ArchiveWorker::new(job, Some(tx));
        let res = worker.run();

        assert!(res.is_ok());
        assert!(final_path.exists());
        assert!(!temp_path.exists());

        // Verify events emitted
        let mut events = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            events.push(evt);
        }
        assert!(
            events
                .iter()
                .any(|e| matches!(e, OperationEvent::Completed { .. }))
        );
    }
}
