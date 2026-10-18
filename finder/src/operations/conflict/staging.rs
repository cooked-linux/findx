//! Temporary rollback staging area for safe replacement operations.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::operations::error::{OperationError, Result};

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(1);

/// RAII rollback guard for file replacement operations.
///
/// When replacing a destination file, the original is moved to a staging directory
/// under `/tmp/.findx_replace_staging_*`. If the replacement operation succeeds,
/// [`StagedRollback::commit`] discards the staged copy. If the guard is dropped
/// before committing (due to an error, panic, or cancellation), it restores the
/// original destination file.
pub struct StagedRollback {
    staging_dir: PathBuf,
    staged_file: PathBuf,
    target_destination: PathBuf,
    committed: bool,
}

impl StagedRollback {
    /// Creates a staging area and moves `target_destination` into it.
    ///
    /// # Errors
    /// Returns [`OperationError::Io`] if creating the staging directory or moving
    /// the target destination into staging fails.
    pub fn stage(target_destination: &Path) -> Result<Self> {
        let counter = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let pid = std::process::id();

        let dir_name = format!(".findx_replace_staging_{pid}_{counter}_{timestamp}");
        let staging_dir = std::env::temp_dir().join(dir_name);

        std::fs::create_dir_all(&staging_dir).map_err(|err| OperationError::Io {
            path: staging_dir.clone(),
            source: err,
        })?;

        let file_name = target_destination
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("staged_item"));
        let staged_file = staging_dir.join(file_name);

        // Move target file to staging area
        if let Err(err) = std::fs::rename(target_destination, &staged_file) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return Err(OperationError::Io {
                path: target_destination.to_path_buf(),
                source: err,
            });
        }

        Ok(Self {
            staging_dir,
            staged_file,
            target_destination: target_destination.to_path_buf(),
            committed: false,
        })
    }

    /// Commits the replacement, discarding the original staged file and removing staging dir.
    ///
    /// # Errors
    /// Returns [`OperationError::Io`] if cleaning up the staging directory fails.
    pub fn commit(mut self) -> Result<()> {
        self.committed = true;
        let _ = std::fs::remove_file(&self.staged_file);
        let _ = std::fs::remove_dir_all(&self.staging_dir);
        Ok(())
    }

    /// Explicitly triggers a rollback, restoring the original target destination.
    pub fn rollback(&mut self) {
        if self.committed {
            return;
        }

        // Remove partially written target if it exists
        if self.target_destination.exists() {
            if self.target_destination.is_dir() {
                let _ = std::fs::remove_dir_all(&self.target_destination);
            } else {
                let _ = std::fs::remove_file(&self.target_destination);
            }
        }

        // Restore staged file
        let _ = std::fs::rename(&self.staged_file, &self.target_destination);
        let _ = std::fs::remove_dir_all(&self.staging_dir);
        self.committed = true;
    }
}

impl Drop for StagedRollback {
    fn drop(&mut self) {
        if !self.committed {
            self.rollback();
        }
    }
}
