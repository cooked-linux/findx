//! Crash recovery logging and temporary state persistence for batch file operations.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use shared::models::operation::BatchOperation;

use super::error::{OperationError, Result};

/// Progress checkpoint interval for recovery state persistence (5 seconds).
pub const RECOVERY_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(5);

/// Persistent recovery snapshot saved to disk during multi-file operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecoveryState {
    /// Active batch operation definition.
    pub operation: BatchOperation,
    /// Total items scheduled in pre-flight manifest.
    pub total_items: u64,
    /// Total byte weight scheduled.
    pub total_bytes: u64,
    /// Count of items successfully processed so far.
    pub completed_items: u64,
    /// Cumulative bytes transferred so far.
    pub transferred_bytes: u64,
    /// Epoch timestamp in seconds when the state was last persisted.
    pub last_updated_epoch_secs: u64,
    /// File path currently being transferred when snapshot was taken.
    pub current_file: Option<PathBuf>,
}

impl RecoveryState {
    /// Constructs a new [`RecoveryState`] initialized from an operation and manifest statistics.
    #[must_use]
    pub fn new(operation: BatchOperation, total_items: u64, total_bytes: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Self {
            operation,
            total_items,
            total_bytes,
            completed_items: 0,
            transferred_bytes: 0,
            last_updated_epoch_secs: now,
            current_file: None,
        }
    }

    /// Updates progress metric checkpoints on this state instance.
    pub fn update(
        &mut self,
        completed_items: u64,
        transferred_bytes: u64,
        current_file: Option<PathBuf>,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.completed_items = completed_items;
        self.transferred_bytes = transferred_bytes;
        self.last_updated_epoch_secs = now;
        self.current_file = current_file;
    }
}

/// Resolves the default recovery state JSON file path based on the user's UID.
#[must_use]
pub fn default_recovery_path() -> PathBuf {
    let uid = nix::unistd::getuid().as_raw();
    PathBuf::from(format!("/tmp/findx_transfers_{uid}.json"))
}

/// Manager handling recovery state persistence, checkpointing, and orphaned cleanup.
#[derive(Debug, Clone)]
pub struct RecoveryManager {
    path: PathBuf,
}

impl Default for RecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryManager {
    /// Creates a [`RecoveryManager`] targeting `/tmp/findx_transfers_<UID>.json`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            path: default_recovery_path(),
        }
    }

    /// Creates a [`RecoveryManager`] targeting a custom path (useful for testing).
    #[must_use]
    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns a borrowed reference to the recovery file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Persists recovery state snapshot to disk.
    ///
    /// # Errors
    /// Returns [`OperationError`] if serialization or filesystem writing fails.
    pub fn save_state(&self, state: &RecoveryState) -> Result<()> {
        let json = serde_json::to_string_pretty(state).map_err(|e| {
            OperationError::Invalid(format!("Failed to serialize recovery state: {e}"))
        })?;
        std::fs::write(&self.path, json).map_err(|e| OperationError::Io {
            path: self.path.clone(),
            source: e,
        })
    }

    /// Updates existing recovery state with new checkpoint data and rewrites to disk.
    ///
    /// # Errors
    /// Returns [`OperationError`] if loading or saving fails.
    pub fn update_checkpoint(
        &self,
        completed_items: u64,
        transferred_bytes: u64,
        current_file: Option<PathBuf>,
    ) -> Result<()> {
        if let Some(mut state) = self.load_state()? {
            state.update(completed_items, transferred_bytes, current_file);
            self.save_state(&state)?;
        }
        Ok(())
    }

    /// Loads active recovery state if present on disk.
    ///
    /// # Errors
    /// Returns [`OperationError`] if reading or parsing existing recovery file fails.
    pub fn load_state(&self) -> Result<Option<RecoveryState>> {
        if !self.path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&self.path).map_err(|e| OperationError::Io {
            path: self.path.clone(),
            source: e,
        })?;

        let state = serde_json::from_str::<RecoveryState>(&content)
            .map_err(|e| OperationError::Invalid(format!("Failed to parse recovery state: {e}")))?;

        Ok(Some(state))
    }

    /// Removes temporary recovery state file from disk.
    ///
    /// # Errors
    /// Returns [`OperationError`] if file removal fails.
    pub fn clear_state(&self) -> Result<()> {
        if self.path.exists() {
            std::fs::remove_file(&self.path).map_err(|e| OperationError::Io {
                path: self.path.clone(),
                source: e,
            })?;
        }
        Ok(())
    }

    /// Detects whether an orphaned recovery transfer file exists from a previous crash.
    ///
    /// # Errors
    /// Returns [`OperationError`] if loading fails.
    pub fn detect_orphaned_transfers(&self) -> Result<Option<RecoveryState>> {
        self.load_state()
    }

    /// Cleans up partial transfer artifacts and removes recovery state file.
    ///
    /// # Errors
    /// Returns [`OperationError`] if cleanup fails.
    pub fn cleanup_orphaned_transfers(&self, state: &RecoveryState) -> Result<()> {
        if let Some(ref current) = state.current_file
            && current.exists()
            && state.operation.kind.requires_destination()
        {
            let _ = std::fs::remove_file(current);
        }
        self.clear_state()
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
    use shared::models::operation::OperationKind;
    use tempfile::tempdir;

    #[test]
    fn test_save_load_checkpoint_and_clear_recovery_state() {
        let dir = tempdir().expect("tempdir");
        let recovery_file = dir.path().join("test_recovery.json");
        let manager = RecoveryManager::with_path(&recovery_file);

        let op = BatchOperation {
            id: "recovery-test-op".into(),
            kind: OperationKind::Copy,
            sources: vec![PathBuf::from("/tmp/src1"), PathBuf::from("/tmp/src2")],
            destination: Some(PathBuf::from("/tmp/dst")),
        };

        let state = RecoveryState::new(op, 10, 5000);
        manager.save_state(&state).expect("save state");
        assert!(recovery_file.exists());

        let loaded = manager.load_state().expect("load state");
        assert_eq!(loaded, Some(state));

        manager
            .update_checkpoint(3, 1500, Some(PathBuf::from("/tmp/dst/src1")))
            .expect("update checkpoint");

        let updated = manager.load_state().expect("load updated state");
        let Some(up) = updated else {
            panic!("Expected state to be present");
        };
        assert_eq!(up.completed_items, 3);
        assert_eq!(up.transferred_bytes, 1500);
        assert_eq!(up.current_file, Some(PathBuf::from("/tmp/dst/src1")));

        manager.clear_state().expect("clear state");
        assert!(!recovery_file.exists());

        let after_clear = manager.detect_orphaned_transfers().expect("detect");
        assert_eq!(after_clear, None);
    }

    #[test]
    fn test_cleanup_orphaned_transfers() {
        let dir = tempdir().expect("tempdir");
        let recovery_file = dir.path().join("orphaned_recovery.json");
        let manager = RecoveryManager::with_path(&recovery_file);

        let partial_file = dir.path().join("partial_dest.bin");
        std::fs::write(&partial_file, b"corrupted partial").expect("write partial");
        assert!(partial_file.exists());

        let op = BatchOperation {
            id: "orphaned-op".into(),
            kind: OperationKind::Copy,
            sources: vec![PathBuf::from("/src/file")],
            destination: Some(dir.path().to_path_buf()),
        };

        let mut state = RecoveryState::new(op, 1, 100);
        state.current_file = Some(partial_file.clone());
        manager.save_state(&state).expect("save state");

        manager.cleanup_orphaned_transfers(&state).expect("cleanup");
        assert!(!partial_file.exists(), "partial file must be removed");
        assert!(!recovery_file.exists(), "recovery file must be cleared");
    }
}
