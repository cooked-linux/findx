//! Persistent session journaling for the undo/redo engine.
//!
//! Serializes active undo and redo transaction stacks to disk (`~/.cache/findx/undo_journal.json`)
//! across application restarts, automatically pruning entries older than 7 days and validating
//! target file presence on disk.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::error::Result;
use super::manager::UndoManager;
use super::models::{UndoOperationType, UndoTransaction};

/// Maximum age of journaled transactions before automatic expiration (7 days).
pub const MAX_JOURNAL_AGE: Duration = Duration::from_hours(168);

/// Current schema version for the undo journal file.
pub const JOURNAL_SCHEMA_VERSION: u32 = 1;

/// Serialized payload written to the journal file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoJournalState {
    /// Schema format version.
    pub version: u32,
    /// Active undo stack transactions.
    pub undo_stack: VecDeque<UndoTransaction>,
    /// Active redo stack transactions.
    pub redo_stack: VecDeque<UndoTransaction>,
}

/// Returns the default journal path (`~/.cache/findx/undo_journal.json`).
#[must_use]
pub fn default_journal_path() -> PathBuf {
    if let Ok(cache_home) = std::env::var("XDG_CACHE_HOME")
        && !cache_home.trim().is_empty()
    {
        return PathBuf::from(cache_home)
            .join("findx")
            .join("undo_journal.json");
    }

    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home)
            .join(".cache")
            .join("findx")
            .join("undo_journal.json");
    }

    std::env::temp_dir().join("findx").join("undo_journal.json")
}

/// Saves the current [`UndoManager`] state to disk using an atomic temporary write and rename.
///
/// # Errors
/// Returns [`crate::undo::error::UndoError`] if serialization or file I/O fails.
pub fn save_journal(path: &Path, manager: &UndoManager) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let state = UndoJournalState {
        version: JOURNAL_SCHEMA_VERSION,
        undo_stack: manager.undo_stack().clone(),
        redo_stack: manager.redo_stack().clone(),
    };

    let serialized = serde_json::to_string_pretty(&state)?;

    let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    fs::write(&tmp_path, serialized)?;

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err.into());
    }

    Ok(())
}

/// Loads and validates an [`UndoManager`] from the journal file.
///
/// Prunes transactions older than 7 days and filters out transactions whose destination
/// files are no longer valid on disk.
///
/// # Errors
/// Returns [`crate::undo::error::UndoError`] if reading or parsing fails.
pub fn load_journal(path: &Path, max_depth: usize) -> Result<UndoManager> {
    if !path.exists() {
        return Ok(UndoManager::with_depth(max_depth));
    }

    let content = fs::read_to_string(path)?;
    let state: UndoJournalState = serde_json::from_str(&content)?;

    let now = SystemTime::now();

    let pruned_undo = filter_valid_transactions(state.undo_stack, now);
    let pruned_redo = filter_valid_transactions(state.redo_stack, now);

    Ok(UndoManager::from_stacks(
        pruned_undo,
        pruned_redo,
        max_depth,
    ))
}

/// Filters transactions, pruning entries older than 7 days and verifying file existence.
fn filter_valid_transactions(
    stack: VecDeque<UndoTransaction>,
    now: SystemTime,
) -> VecDeque<UndoTransaction> {
    stack
        .into_iter()
        .filter(|tx| {
            // 1. Age check: discard if older than 7 days
            if let Ok(elapsed) = now.duration_since(tx.created_at)
                && elapsed > MAX_JOURNAL_AGE
            {
                return false;
            }

            // 2. On-disk presence validation: ensure targets are still intact
            is_transaction_valid_on_disk(tx)
        })
        .collect()
}

/// Verifies whether the transaction's target files still exist on disk.
fn is_transaction_valid_on_disk(tx: &UndoTransaction) -> bool {
    match tx.op_type {
        UndoOperationType::Move | UndoOperationType::Rename => {
            // For Move/Rename, destination_path must currently exist to be undone
            tx.records.iter().all(|rec| rec.destination_path.exists())
        }
        UndoOperationType::Copy | UndoOperationType::Duplicate => {
            // For Copy/Duplicate, the created file at destination_path must exist
            tx.records.iter().all(|rec| rec.destination_path.exists())
        }
        UndoOperationType::NewFolderWithSelection => {
            // For NewFolderWithSelection, destination paths or parent folder must exist
            tx.records.iter().any(|rec| rec.destination_path.exists())
        }
        UndoOperationType::Trash => {
            // For Trash, trashinfo or trash file must be present
            tx.records.iter().all(|rec| {
                if let Some(ref info) = rec.trash_info_rel_path
                    && info.exists()
                {
                    return true;
                }
                if rec.destination_path.exists() {
                    return true;
                }
                // Check if candidate trash files exist
                let file_name = rec.source_path.file_name().unwrap_or_default();
                let uid = nix::unistd::getuid();
                let candidate_paths = [
                    dirs_cache_trash().join("files").join(file_name),
                    PathBuf::from(format!("/.Trash-{uid}"))
                        .join("files")
                        .join(file_name),
                ];
                candidate_paths.iter().any(|p| p.exists())
            })
        }
    }
}

fn dirs_cache_trash() -> PathBuf {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME")
        && !xdg_data.trim().is_empty()
    {
        return PathBuf::from(xdg_data).join("Trash");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home).join(".local/share/Trash");
    }
    PathBuf::from("/tmp/Trash")
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone
)]
mod tests {
    use super::*;
    use crate::undo::models::{FileOperationRecord, UndoOperationType};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_journal_save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let journal_file = dir.path().join("undo_journal.json");

        let mut manager = UndoManager::new();

        // Prepare dummy files that exist on disk
        let live_file = dir.path().join("live.txt");
        fs::write(&live_file, "live file").unwrap();

        let rec = FileOperationRecord::new(
            PathBuf::from("/src/live.txt"),
            live_file,
            1,
            1,
            SystemTime::now(),
            None,
        );

        let tx = UndoTransaction::new(
            "tx1",
            UndoOperationType::Move,
            "Move live file",
            vec![rec],
            SystemTime::now(),
        );

        manager.record_transaction(tx);

        // Save to journal
        save_journal(&journal_file, &manager).unwrap();
        assert!(journal_file.exists());

        // Restore into new manager (simulated restart)
        let restored_manager = load_journal(&journal_file, 50).unwrap();

        assert_eq!(restored_manager.undo_stack().len(), 1);
        assert_eq!(restored_manager.undo_stack()[0].id, "tx1");
        assert_eq!(
            restored_manager.current_undo_label().as_deref(),
            Some("Undo Move live file")
        );
    }

    #[test]
    fn test_journal_pruning_older_than_7_days() {
        let dir = tempdir().unwrap();
        let journal_file = dir.path().join("undo_journal.json");

        let live_file = dir.path().join("live.txt");
        fs::write(&live_file, "data").unwrap();

        let eight_days_ago = SystemTime::now() - Duration::from_hours(192);
        let rec_old = FileOperationRecord::new(
            PathBuf::from("/src/old.txt"),
            live_file,
            1,
            1,
            eight_days_ago,
            None,
        );
        let tx_old = UndoTransaction::new(
            "tx_old",
            UndoOperationType::Move,
            "Old op",
            vec![rec_old],
            eight_days_ago,
        );

        let mut manager = UndoManager::new();
        manager.record_transaction(tx_old);

        save_journal(&journal_file, &manager).unwrap();

        let restored = load_journal(&journal_file, 50).unwrap();
        assert_eq!(restored.undo_stack().len(), 0);
    }

    #[test]
    fn test_journal_discards_transactions_with_missing_destination_files() {
        let dir = tempdir().unwrap();
        let journal_file = dir.path().join("undo_journal.json");

        let non_existent_file = dir.path().join("deleted_afterwards.txt");
        // Don't create the file, so it does not exist on disk

        let rec = FileOperationRecord::new(
            PathBuf::from("/src/file.txt"),
            non_existent_file,
            1,
            1,
            SystemTime::now(),
            None,
        );
        let tx = UndoTransaction::new(
            "tx_missing",
            UndoOperationType::Move,
            "Missing op",
            vec![rec],
            SystemTime::now(),
        );

        let mut manager = UndoManager::new();
        manager.record_transaction(tx);

        save_journal(&journal_file, &manager).unwrap();

        let restored = load_journal(&journal_file, 50).unwrap();
        assert_eq!(restored.undo_stack().len(), 0);
    }
}
