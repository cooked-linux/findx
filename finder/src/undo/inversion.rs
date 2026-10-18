//! Mathematical inverse operation computation and execution engine.
//!
//! Handles rollback and forward re-execution of filesystem operations with
//! inode safety checks, Freedesktop `.trashinfo` metadata restoration, and collision resolution.

use std::fs;
use std::path::{Path, PathBuf};

use shared::fs::clone::clone_file;
use shared::fs::stat::stat_path;
use shared::fs::trash::{parse_trash_info, send_to_trash};

use super::error::{Result, UndoError};
use super::models::{FileOperationRecord, UndoOperationType, UndoTransaction};
use crate::operations::naming::generate_unique_path;

/// Strategy to use when an inverse operation targets a path that is already occupied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictStrategy {
    /// Return an error without modifying the existing item.
    #[default]
    Error,
    /// Overwrite/replace the occupied destination by moving it to the trash first.
    Replace,
    /// Keep both items by auto-incrementing the restored item's filename.
    KeepBoth,
}

/// Executes the inverse of an [`UndoTransaction`], rolling back all filesystem changes.
///
/// # Errors
/// Returns [`UndoError`] if inode validation fails, files are missing, or I/O fails.
pub fn execute_inverse(tx: &UndoTransaction, strategy: ConflictStrategy) -> Result<()> {
    match tx.op_type {
        UndoOperationType::Move | UndoOperationType::Rename => {
            for record in tx.records.iter().rev() {
                execute_move_inverse(record, strategy)?;
            }
            Ok(())
        }
        UndoOperationType::Copy | UndoOperationType::Duplicate => {
            for record in tx.records.iter().rev() {
                execute_copy_inverse(record)?;
            }
            Ok(())
        }
        UndoOperationType::Trash => {
            for record in tx.records.iter().rev() {
                execute_trash_inverse(record, strategy)?;
            }
            Ok(())
        }
        UndoOperationType::NewFolderWithSelection => {
            execute_new_folder_with_selection_inverse(tx, strategy)
        }
    }
}

/// Re-executes the forward actions of an [`UndoTransaction`] for redo.
///
/// # Errors
/// Returns [`UndoError`] if target files cannot be moved, copied, or restored.
pub fn execute_forward(tx: &UndoTransaction, strategy: ConflictStrategy) -> Result<()> {
    match tx.op_type {
        UndoOperationType::Move | UndoOperationType::Rename => {
            for record in &tx.records {
                execute_move_forward(record, strategy)?;
            }
            Ok(())
        }
        UndoOperationType::Copy | UndoOperationType::Duplicate => {
            for record in &tx.records {
                execute_copy_forward(record, strategy)?;
            }
            Ok(())
        }
        UndoOperationType::Trash => {
            for record in &tx.records {
                if record.source_path.exists() {
                    send_to_trash(&record.source_path)?;
                }
            }
            Ok(())
        }
        UndoOperationType::NewFolderWithSelection => {
            execute_new_folder_with_selection_forward(tx, strategy)
        }
    }
}

/// Inverts a Move or Rename operation: moves `destination_path` back to `source_path`.
///
/// # Errors
/// Returns [`UndoError`] if inode safety verification fails or collision handling rejects the operation.
pub fn execute_move_inverse(
    record: &FileOperationRecord,
    strategy: ConflictStrategy,
) -> Result<PathBuf> {
    if !record.destination_path.exists() {
        return Err(UndoError::ItemNotFound(record.destination_path.clone()));
    }

    // Inode safety verification
    verify_inode_identity(
        &record.destination_path,
        record.device_id,
        record.inode_number,
    )?;

    let target_path = resolve_target_destination(&record.source_path, strategy)?;

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    atomic_rename_or_copy_fallback(&record.destination_path, &target_path)?;
    Ok(target_path)
}

/// Forward execution of Move / Rename for Redo.
fn execute_move_forward(
    record: &FileOperationRecord,
    strategy: ConflictStrategy,
) -> Result<PathBuf> {
    if !record.source_path.exists() {
        return Err(UndoError::ItemNotFound(record.source_path.clone()));
    }

    let target_path = resolve_target_destination(&record.destination_path, strategy)?;

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    atomic_rename_or_copy_fallback(&record.source_path, &target_path)?;
    Ok(target_path)
}

/// Inverts a Copy or Duplicate operation: deletes/trashes the created duplicate at `destination_path`.
///
/// # Errors
/// Returns [`UndoError`] if the file does not exist, fails inode check, or trashing fails.
pub fn execute_copy_inverse(record: &FileOperationRecord) -> Result<()> {
    if !record.destination_path.exists() {
        return Err(UndoError::ItemNotFound(record.destination_path.clone()));
    }

    // Inode safety verification
    let stat = stat_path(&record.destination_path)?;
    if stat.dev != record.device_id || stat.ino != record.inode_number {
        return Err(UndoError::InodeMismatch {
            path: record.destination_path.clone(),
            expected_dev: record.device_id,
            expected_ino: record.inode_number,
            actual_dev: stat.dev,
            actual_ino: stat.ino,
        });
    }

    // Warn if file modification timestamp changed since copy
    if let Some(mtime) = stat.mtime
        && mtime > record.timestamp
    {
        tracing::warn!(
            target = "findx::undo",
            path = %record.destination_path.display(),
            "File was modified after copy operation before undo rollback"
        );
    }

    send_to_trash(&record.destination_path)?;
    Ok(())
}

/// Forward execution of Copy / Duplicate for Redo.
fn execute_copy_forward(
    record: &FileOperationRecord,
    strategy: ConflictStrategy,
) -> Result<PathBuf> {
    if !record.source_path.exists() {
        return Err(UndoError::ItemNotFound(record.source_path.clone()));
    }

    let target_path = resolve_target_destination(&record.destination_path, strategy)?;

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if record.source_path.is_dir() {
        copy_directory_recursive(&record.source_path, &target_path)?;
    } else {
        clone_file(&record.source_path, &target_path)?;
    }

    Ok(target_path)
}

/// Inverts a Trash operation: restores the trashed item back to its original location.
///
/// # Errors
/// Returns [`UndoError`] if the item cannot be found in trash, or restoration fails.
pub fn execute_trash_inverse(
    record: &FileOperationRecord,
    strategy: ConflictStrategy,
) -> Result<PathBuf> {
    let (info_path, trashed_file) = locate_trashed_entry(record)?;

    if !trashed_file.exists() {
        return Err(UndoError::TrashRestorationFailed {
            path: record.source_path.clone(),
            reason: format!(
                "trashed data file not found at '{}'",
                trashed_file.display()
            ),
        });
    }

    // Inode safety verification
    verify_inode_identity(&trashed_file, record.device_id, record.inode_number)?;

    let target_path = resolve_target_destination(&record.source_path, strategy)?;

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    atomic_rename_or_copy_fallback(&trashed_file, &target_path)?;

    // Remove the .trashinfo metadata file
    if info_path.exists() {
        let _ = fs::remove_file(&info_path);
    }

    Ok(target_path)
}

/// Inverts a `NewFolderWithSelection` operation:
/// moves items back to their original locations and removes the empty created directory.
fn execute_new_folder_with_selection_inverse(
    tx: &UndoTransaction,
    strategy: ConflictStrategy,
) -> Result<()> {
    let mut folder_to_remove: Option<PathBuf> = None;

    for record in tx.records.iter().rev() {
        if record.source_path == record.destination_path && record.destination_path.is_dir() {
            // Explicit record for the created folder
            folder_to_remove = Some(record.destination_path.clone());
            continue;
        }

        if record.destination_path.exists() {
            if folder_to_remove.is_none() {
                folder_to_remove = record.destination_path.parent().map(Path::to_path_buf);
            }
            execute_move_inverse(record, strategy)?;
        }
    }

    // If folder was identified, remove it if it is empty
    if let Some(folder) = folder_to_remove
        && folder.exists()
        && folder.is_dir()
    {
        match fs::read_dir(&folder) {
            Ok(mut entries) => {
                if entries.next().is_none() {
                    let _ = fs::remove_dir(&folder);
                } else {
                    tracing::warn!(
                        target = "findx::undo",
                        path = %folder.display(),
                        "Directory is not empty after undo; leaving in place"
                    );
                }
            }
            Err(err) => {
                tracing::warn!(
                    target = "findx::undo",
                    path = %folder.display(),
                    error = %err,
                    "Could not read directory to verify emptiness for removal"
                );
            }
        }
    }

    Ok(())
}

/// Forward execution of `NewFolderWithSelection` for Redo.
fn execute_new_folder_with_selection_forward(
    tx: &UndoTransaction,
    strategy: ConflictStrategy,
) -> Result<()> {
    for record in &tx.records {
        if record.source_path == record.destination_path {
            fs::create_dir_all(&record.destination_path)?;
        }
    }

    for record in &tx.records {
        if record.source_path != record.destination_path && record.source_path.exists() {
            execute_move_forward(record, strategy)?;
        }
    }

    Ok(())
}

/// Verifies that the item at `path` matches the expected filesystem device ID and inode number.
fn verify_inode_identity(path: &Path, expected_dev: u64, expected_ino: u64) -> Result<()> {
    let stat = stat_path(path)?;
    if stat.dev != expected_dev || stat.ino != expected_ino {
        return Err(UndoError::InodeMismatch {
            path: path.to_path_buf(),
            expected_dev,
            expected_ino,
            actual_dev: stat.dev,
            actual_ino: stat.ino,
        });
    }
    Ok(())
}

/// Determines the effective destination path, applying the conflict resolution strategy if occupied.
fn resolve_target_destination(target_path: &Path, strategy: ConflictStrategy) -> Result<PathBuf> {
    if !target_path.exists() {
        return Ok(target_path.to_path_buf());
    }

    match strategy {
        ConflictStrategy::Error => Err(UndoError::Conflict(target_path.to_path_buf())),
        ConflictStrategy::Replace => {
            // Safely move the existing occupant to trash before restoring target
            send_to_trash(target_path)?;
            Ok(target_path.to_path_buf())
        }
        ConflictStrategy::KeepBoth => {
            let unique_path = generate_unique_path(target_path);
            Ok(unique_path)
        }
    }
}

/// Atomically renames `from` to `to`, falling back to recursive copy and delete on cross-device boundary.
fn atomic_rename_or_copy_fallback(from: &Path, to: &Path) -> Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }

    if from.is_dir() {
        copy_directory_recursive(from, to)?;
        fs::remove_dir_all(from)?;
    } else {
        clone_file(from, to)?;
        fs::remove_file(from)?;
    }

    Ok(())
}

/// Recursively copies a directory tree.
fn copy_directory_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let target_entry_path = dst.join(entry.file_name());
        if entry_path.is_dir() {
            copy_directory_recursive(&entry_path, &target_entry_path)?;
        } else {
            clone_file(&entry_path, &target_entry_path)?;
        }
    }
    Ok(())
}

/// Locates the `.trashinfo` metadata and corresponding trashed data file for a record.
///
/// Searches `$XDG_DATA_HOME/Trash` and volume trash directories (`.Trash-$UID`, `.Trash/$UID`).
fn locate_trashed_entry(record: &FileOperationRecord) -> Result<(PathBuf, PathBuf)> {
    // 1. If explicit relative or absolute trashinfo path was saved
    if let Some(ref rel_info) = record.trash_info_rel_path
        && rel_info.is_absolute()
        && rel_info.exists()
        && let Some(trash_dir) = rel_info.parent().and_then(Path::parent)
    {
        let file_name = rel_info.file_stem().unwrap_or_default();
        let data_file = trash_dir.join("files").join(file_name);
        if data_file.exists() {
            return Ok((rel_info.clone(), data_file));
        }
    }

    // 2. Search candidate trash directories
    let candidates = get_candidate_trash_directories(&record.source_path);

    // Check by destination_path if it was recorded inside a trash folder
    if record.destination_path.exists()
        && let Some(files_dir) = record.destination_path.parent()
        && files_dir.file_name().and_then(|s| s.to_str()) == Some("files")
        && let Some(trash_dir) = files_dir.parent()
    {
        let file_name = record.destination_path.file_name().unwrap_or_default();
        let info_name = format!("{}.trashinfo", file_name.to_string_lossy());
        let info_path = trash_dir.join("info").join(info_name);
        if info_path.exists() {
            return Ok((info_path, record.destination_path.clone()));
        }
    }

    for trash_dir in candidates {
        let info_dir = trash_dir.join("info");
        let files_dir = trash_dir.join("files");

        if !info_dir.exists() || !files_dir.exists() {
            continue;
        }

        // Check if rel_info matches directly in this trash info dir
        if let Some(ref rel_info) = record.trash_info_rel_path {
            let file_stem = rel_info
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let direct_info = info_dir.join(&file_stem);
            if direct_info.exists() {
                let stem = direct_info.file_stem().unwrap_or_default();
                let data_file = files_dir.join(stem);
                if data_file.exists() {
                    return Ok((direct_info, data_file));
                }
            }
        }

        // Read all entries in info_dir to match Path= in .trashinfo
        if let Ok(entries) = fs::read_dir(&info_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("trashinfo")
                    && let Ok(content) = fs::read_to_string(&path)
                    && let Ok(parsed) = parse_trash_info(&content)
                    && parsed.original_path == record.source_path
                {
                    let stem = path.file_stem().unwrap_or_default();
                    let data_file = files_dir.join(stem);
                    if data_file.exists() {
                        return Ok((path, data_file));
                    }
                }
            }
        }
    }

    Err(UndoError::TrashRestorationFailed {
        path: record.source_path.clone(),
        reason: "could not find metadata or data files in Freedesktop Trash".to_string(),
    })
}

/// Discovers candidate Freedesktop Trash directories for a given path.
fn get_candidate_trash_directories(source_path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let uid = nix::unistd::getuid();

    // 1. $XDG_DATA_HOME/Trash or ~/.local/share/Trash
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME")
        && !xdg_data.trim().is_empty()
    {
        dirs.push(PathBuf::from(xdg_data).join("Trash"));
    }
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/Trash"));
    }

    // 2. Volume trash directories from /proc/mounts
    if let Ok(mounts_content) = fs::read_to_string("/proc/mounts") {
        let mut mount_points = Vec::new();
        for line in mounts_content.lines() {
            let mut parts = line.split_whitespace();
            let _dev = parts.next();
            if let Some(mount_point) = parts.next() {
                mount_points.push(PathBuf::from(mount_point));
            }
        }
        mount_points.sort_by_key(|b| std::cmp::Reverse(b.as_os_str().len()));

        for mount in mount_points {
            if source_path.starts_with(&mount) && mount != Path::new("/") {
                dirs.push(mount.join(format!(".Trash-{uid}")));
                dirs.push(mount.join(".Trash").join(uid.to_string()));
                break;
            }
        }
    }

    // 3. Check ancestor paths for local test Trash directories
    let mut current = source_path.parent();
    while let Some(parent) = current {
        let local_trash = parent.join("Trash");
        if local_trash.exists() {
            dirs.push(local_trash);
        }
        let volume_trash = parent.join(format!(".Trash-{uid}"));
        if volume_trash.exists() {
            dirs.push(volume_trash);
        }
        current = parent.parent();
    }

    dirs
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
    use std::time::SystemTime;
    use tempfile::tempdir;

    #[test]
    fn test_move_inverse_success() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("destination.txt");

        fs::write(&dst, "hello world").unwrap();
        let stat = stat_path(&dst).unwrap();

        let record = FileOperationRecord::new(
            src.clone(),
            dst.clone(),
            stat.ino,
            stat.dev,
            SystemTime::now(),
            None,
        );

        let tx = UndoTransaction::new(
            "tx1",
            UndoOperationType::Move,
            "Move 1 file",
            vec![record],
            SystemTime::now(),
        );

        assert!(!src.exists());
        assert!(dst.exists());

        execute_inverse(&tx, ConflictStrategy::Error).unwrap();

        assert!(src.exists());
        assert!(!dst.exists());
        assert_eq!(fs::read_to_string(&src).unwrap(), "hello world");
    }

    #[test]
    fn test_move_inverse_inode_mismatch_fails() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("destination.txt");

        fs::write(&dst, "hello world").unwrap();
        let stat = stat_path(&dst).unwrap();

        // Record with wrong inode
        let record = FileOperationRecord::new(
            src.clone(),
            dst.clone(),
            stat.ino + 9999,
            stat.dev,
            SystemTime::now(),
            None,
        );

        let tx = UndoTransaction::new(
            "tx2",
            UndoOperationType::Move,
            "Move 1 file",
            vec![record],
            SystemTime::now(),
        );

        let result = execute_inverse(&tx, ConflictStrategy::Error);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            UndoError::InodeMismatch { .. }
        ));
        assert!(dst.exists());
    }

    #[test]
    fn test_move_inverse_conflict_handling() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("file.txt");
        let dst = dir.path().join("moved_file.txt");

        fs::write(&dst, "moved content").unwrap();
        let stat = stat_path(&dst).unwrap();

        let record = FileOperationRecord::new(
            src.clone(),
            dst.clone(),
            stat.ino,
            stat.dev,
            SystemTime::now(),
            None,
        );
        let tx = UndoTransaction::new(
            "tx3",
            UndoOperationType::Move,
            "Move 1 file",
            vec![record],
            SystemTime::now(),
        );

        // 1. ConflictStrategy::Error when src already exists
        fs::write(&src, "colliding content").unwrap();
        let err = execute_inverse(&tx, ConflictStrategy::Error);
        assert!(matches!(err.unwrap_err(), UndoError::Conflict(_)));

        // 2. ConflictStrategy::KeepBoth
        execute_inverse(&tx, ConflictStrategy::KeepBoth).unwrap();
        assert!(src.exists());
        assert_eq!(fs::read_to_string(&src).unwrap(), "colliding content");
        let auto_named = dir.path().join("file (1).txt");
        assert!(auto_named.exists());
        assert_eq!(fs::read_to_string(&auto_named).unwrap(), "moved content");
    }

    #[test]
    fn test_copy_inverse_sends_to_trash() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("copy.txt");

        fs::write(&src, "original").unwrap();
        fs::write(&dst, "copied").unwrap();
        let stat = stat_path(&dst).unwrap();

        let record = FileOperationRecord::new(
            src.clone(),
            dst.clone(),
            stat.ino,
            stat.dev,
            SystemTime::now(),
            None,
        );
        let tx = UndoTransaction::new(
            "tx4",
            UndoOperationType::Copy,
            "Copy 1 file",
            vec![record],
            SystemTime::now(),
        );

        execute_inverse(&tx, ConflictStrategy::Error).unwrap();

        assert!(src.exists());
        assert!(!dst.exists());
    }

    #[test]
    fn test_trash_inverse_restoration() {
        let dir = tempdir().unwrap();
        let original_file = dir.path().join("my_doc.txt");
        let trash_dir = dir.path().join("Trash");
        let trash_files = trash_dir.join("files");
        let trash_info = trash_dir.join("info");

        fs::create_dir_all(&trash_files).unwrap();
        fs::create_dir_all(&trash_info).unwrap();

        let trashed_file = trash_files.join("my_doc.txt");
        fs::write(&trashed_file, "restored text").unwrap();
        let stat = stat_path(&trashed_file).unwrap();

        let info_content = format!(
            "[Trash Info]\nPath={}\nDeletionDate=2026-10-17T22:00:00\n",
            original_file.display()
        );
        let info_path = trash_info.join("my_doc.txt.trashinfo");
        fs::write(&info_path, info_content).unwrap();

        let record = FileOperationRecord::new(
            original_file.clone(),
            trashed_file.clone(),
            stat.ino,
            stat.dev,
            SystemTime::now(),
            Some(info_path.clone()),
        );

        let tx = UndoTransaction::new(
            "tx5",
            UndoOperationType::Trash,
            "Trash 1 file",
            vec![record],
            SystemTime::now(),
        );

        assert!(!original_file.exists());
        assert!(trashed_file.exists());
        assert!(info_path.exists());

        execute_inverse(&tx, ConflictStrategy::Error).unwrap();

        assert!(original_file.exists());
        assert_eq!(fs::read_to_string(&original_file).unwrap(), "restored text");
        assert!(!trashed_file.exists());
        assert!(!info_path.exists());
    }

    #[test]
    fn test_new_folder_with_selection_inverse() {
        let dir = tempdir().unwrap();
        let item1_orig = dir.path().join("item1.txt");
        let item2_orig = dir.path().join("item2.txt");
        let new_folder = dir.path().join("New Folder");
        let item1_in_folder = new_folder.join("item1.txt");
        let item2_in_folder = new_folder.join("item2.txt");

        fs::create_dir_all(&new_folder).unwrap();
        fs::write(&item1_in_folder, "item 1").unwrap();
        fs::write(&item2_in_folder, "item 2").unwrap();

        let stat1 = stat_path(&item1_in_folder).unwrap();
        let stat2 = stat_path(&item2_in_folder).unwrap();

        let rec_folder = FileOperationRecord::new(
            new_folder.clone(),
            new_folder.clone(),
            stat_path(&new_folder).unwrap().ino,
            stat_path(&new_folder).unwrap().dev,
            SystemTime::now(),
            None,
        );

        let rec1 = FileOperationRecord::new(
            item1_orig.clone(),
            item1_in_folder.clone(),
            stat1.ino,
            stat1.dev,
            SystemTime::now(),
            None,
        );

        let rec2 = FileOperationRecord::new(
            item2_orig.clone(),
            item2_in_folder.clone(),
            stat2.ino,
            stat2.dev,
            SystemTime::now(),
            None,
        );

        let tx = UndoTransaction::new(
            "tx6",
            UndoOperationType::NewFolderWithSelection,
            "New Folder with Selection",
            vec![rec_folder, rec1, rec2],
            SystemTime::now(),
        );

        assert!(new_folder.exists());
        assert!(item1_in_folder.exists());
        assert!(item2_in_folder.exists());
        assert!(!item1_orig.exists());
        assert!(!item2_orig.exists());

        execute_inverse(&tx, ConflictStrategy::Error).unwrap();

        assert!(!new_folder.exists());
        assert!(item1_orig.exists());
        assert!(item2_orig.exists());
        assert_eq!(fs::read_to_string(&item1_orig).unwrap(), "item 1");
        assert_eq!(fs::read_to_string(&item2_orig).unwrap(), "item 2");
    }

    #[test]
    fn test_redo_forward_execution() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");

        fs::write(&src, "redo content").unwrap();

        let record = FileOperationRecord::new(
            src.clone(),
            dst.clone(),
            stat_path(&src).unwrap().ino,
            stat_path(&src).unwrap().dev,
            SystemTime::now(),
            None,
        );

        let tx = UndoTransaction::new(
            "tx7",
            UndoOperationType::Move,
            "Move 1 file",
            vec![record],
            SystemTime::now(),
        );

        execute_forward(&tx, ConflictStrategy::Error).unwrap();

        assert!(!src.exists());
        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "redo content");
    }
}
