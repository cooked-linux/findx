//! Recursive directory merge engine preserving newer files.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::conflict::staging::StagedRollback;
use super::error::{OperationError, Result};
use super::naming::generate_unique_name;
use super::transfer::copy_file_with_control;
use super::types::TaskControl;

/// Summary statistics describing the result of a recursive directory merge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeSummary {
    /// Number of files copied into previously empty destination locations.
    pub files_copied: u64,
    /// Number of existing files overwritten by newer incoming files.
    pub files_replaced: u64,
    /// Number of existing files retained because the incoming files were older or identical.
    pub files_skipped: u64,
    /// Number of directories traversed and merged.
    pub dirs_merged: u64,
    /// Cumulative payload bytes copied or replaced.
    pub bytes_transferred: u64,
}

/// Recursively combines directory trees, keeping newer files when items collide.
///
/// # Errors
/// Returns [`OperationError::Invalid`] if cycle is detected or either path is invalid,
/// [`OperationError::Cancelled`] if operation was cancelled, or [`OperationError::Io`] on I/O error.
pub async fn execute_directory_merge<F>(
    src_dir: &Path,
    dst_dir: &Path,
    task_control: &TaskControl,
    on_progress: F,
) -> Result<MergeSummary>
where
    F: FnMut(u64) + Send,
{
    // Validate directory nature and prevent cycles
    let src_meta = std::fs::symlink_metadata(src_dir).map_err(|err| OperationError::Io {
        path: src_dir.to_path_buf(),
        source: err,
    })?;
    if !src_meta.is_dir() {
        return Err(OperationError::Invalid(format!(
            "Source '{}' is not a directory",
            src_dir.display()
        )));
    }

    if dst_dir.exists() {
        let dst_meta = std::fs::symlink_metadata(dst_dir).map_err(|err| OperationError::Io {
            path: dst_dir.to_path_buf(),
            source: err,
        })?;
        if !dst_meta.is_dir() {
            return Err(OperationError::Invalid(format!(
                "Destination '{}' exists and is not a directory",
                dst_dir.display()
            )));
        }

        let canon_src = src_dir
            .canonicalize()
            .unwrap_or_else(|_| src_dir.to_path_buf());
        let canon_dst = dst_dir
            .canonicalize()
            .unwrap_or_else(|_| dst_dir.to_path_buf());

        if canon_src == canon_dst {
            return Err(OperationError::Invalid(
                "Cannot copy or move item onto itself".into(),
            ));
        }

        if canon_dst.starts_with(&canon_src) {
            return Err(OperationError::Invalid(
                "Cannot copy or move folder into its own subfolder".into(),
            ));
        }
    } else {
        std::fs::create_dir_all(dst_dir).map_err(|err| OperationError::Io {
            path: dst_dir.to_path_buf(),
            source: err,
        })?;
    }

    let mut summary = MergeSummary {
        dirs_merged: 1,
        ..Default::default()
    };

    let mut progress_fn: Box<dyn FnMut(u64) + Send> = Box::new(on_progress);

    merge_recursive(
        src_dir,
        dst_dir,
        task_control,
        &mut progress_fn,
        &mut summary,
    )
    .await?;

    Ok(summary)
}

/// Internal recursive helper handling directory traversal and item reconciliation.
#[allow(clippy::too_many_lines)]
fn merge_recursive<'a>(
    src_dir: &'a Path,
    dst_dir: &'a Path,
    task_control: &'a TaskControl,
    on_progress: &'a mut (dyn FnMut(u64) + Send),
    summary: &'a mut MergeSummary,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        check_pause_and_cancel_async(task_control).await?;

        let read_dir = std::fs::read_dir(src_dir).map_err(|err| OperationError::Io {
            path: src_dir.to_path_buf(),
            source: err,
        })?;

        // Read all entries and sort for deterministic traversal
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry_res in read_dir {
            let entry = entry_res.map_err(|err| OperationError::Io {
                path: src_dir.to_path_buf(),
                source: err,
            })?;
            entries.push(entry.path());
        }
        entries.sort();

        for child_src in entries {
            check_pause_and_cancel_async(task_control).await?;

            let file_name = child_src.file_name().ok_or_else(|| {
                OperationError::Invalid(format!("Invalid file name: {}", child_src.display()))
            })?;
            let child_dst = dst_dir.join(file_name);

            let src_entry_meta =
                std::fs::symlink_metadata(&child_src).map_err(|err| OperationError::Io {
                    path: child_src.clone(),
                    source: err,
                })?;

            let dst_exists = std::fs::symlink_metadata(&child_dst).is_ok();

            if dst_exists {
                // Colliding entry
                let dst_entry_meta =
                    std::fs::symlink_metadata(&child_dst).map_err(|err| OperationError::Io {
                        path: child_dst.clone(),
                        source: err,
                    })?;

                if src_entry_meta.is_dir() && dst_entry_meta.is_dir() {
                    summary.dirs_merged = summary.dirs_merged.saturating_add(1);
                    merge_recursive(&child_src, &child_dst, task_control, on_progress, summary)
                        .await?;
                } else if !src_entry_meta.is_dir() && !dst_entry_meta.is_dir() {
                    // Both are files: compare mtime
                    let src_mtime = src_entry_meta.modified().ok();
                    let dst_mtime = dst_entry_meta.modified().ok();

                    let incoming_is_newer = match (src_mtime, dst_mtime) {
                        (Some(s), Some(d)) => s > d,
                        (Some(_), None) => true,
                        _ => false,
                    };

                    if incoming_is_newer {
                        // Safe replacement using staging rollback
                        let rollback = StagedRollback::stage(&child_dst)?;
                        let mut item_transferred: u64 = 0;
                        let copied = copy_file_with_control(
                            &child_src,
                            &child_dst,
                            task_control,
                            |transferred, _| {
                                let delta = transferred.saturating_sub(item_transferred);
                                item_transferred = transferred;
                                on_progress(delta);
                            },
                        );

                        match copied {
                            Ok(bytes) => {
                                rollback.commit()?;
                                summary.files_replaced = summary.files_replaced.saturating_add(1);
                                summary.bytes_transferred =
                                    summary.bytes_transferred.saturating_add(bytes);
                            }
                            Err(err) => {
                                // rollback is dropped here, restoring original destination
                                return Err(err);
                            }
                        }
                    } else {
                        // Keep existing file, skip incoming older/identical
                        summary.files_skipped = summary.files_skipped.saturating_add(1);
                    }
                } else {
                    // Type mismatch: file vs directory collision -> auto-increment rename incoming item
                    let name_str = file_name.to_string_lossy();
                    let unique_name = generate_unique_name(dst_dir, &name_str);
                    let unique_dst = dst_dir.join(&unique_name);

                    if src_entry_meta.is_dir() {
                        std::fs::create_dir_all(&unique_dst).map_err(|err| OperationError::Io {
                            path: unique_dst.clone(),
                            source: err,
                        })?;
                        summary.dirs_merged = summary.dirs_merged.saturating_add(1);
                        merge_recursive(
                            &child_src,
                            &unique_dst,
                            task_control,
                            on_progress,
                            summary,
                        )
                        .await?;
                    } else {
                        let mut item_transferred: u64 = 0;
                        let copied = copy_file_with_control(
                            &child_src,
                            &unique_dst,
                            task_control,
                            |transferred, _| {
                                let delta = transferred.saturating_sub(item_transferred);
                                item_transferred = transferred;
                                on_progress(delta);
                            },
                        )?;
                        summary.files_copied = summary.files_copied.saturating_add(1);
                        summary.bytes_transferred =
                            summary.bytes_transferred.saturating_add(copied);
                    }
                }
            } else {
                // Non-colliding entry
                if src_entry_meta.is_dir() {
                    std::fs::create_dir_all(&child_dst).map_err(|err| OperationError::Io {
                        path: child_dst.clone(),
                        source: err,
                    })?;
                    summary.dirs_merged = summary.dirs_merged.saturating_add(1);
                    merge_recursive(&child_src, &child_dst, task_control, on_progress, summary)
                        .await?;
                } else {
                    let mut item_transferred: u64 = 0;
                    let copied = copy_file_with_control(
                        &child_src,
                        &child_dst,
                        task_control,
                        |transferred, _| {
                            let delta = transferred.saturating_sub(item_transferred);
                            item_transferred = transferred;
                            on_progress(delta);
                        },
                    )?;
                    summary.files_copied = summary.files_copied.saturating_add(1);
                    summary.bytes_transferred = summary.bytes_transferred.saturating_add(copied);
                }
            }
        }

        Ok(())
    })
}

/// Asynchronously handles pause and cancellation states during merge traversal.
async fn check_pause_and_cancel_async(task_control: &TaskControl) -> Result<()> {
    if task_control.is_cancelled() {
        return Err(OperationError::Cancelled);
    }
    while task_control.is_paused() {
        if task_control.is_cancelled() {
            return Err(OperationError::Cancelled);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(())
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
    use std::fs::File;
    use std::io::{Read, Write};
    use std::time::SystemTime;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_directory_merge_overlapping_trees() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src_tree");
        let dst = dir.path().join("dst_tree");

        std::fs::create_dir_all(src.join("sub")).expect("create src sub");
        std::fs::create_dir_all(dst.join("sub")).expect("create dst sub");

        // 1. Non-colliding source file: src/unique.txt -> should be copied to dst/unique.txt
        let unique_src = src.join("unique.txt");
        File::create(&unique_src)
            .expect("create unique")
            .write_all(b"unique content")
            .expect("write unique");

        // 2. Colliding file in subfolder: sub/doc.txt
        // Make src newer, dst older -> src should replace dst
        let doc_src = src.join("sub/doc.txt");
        let doc_dst = dst.join("sub/doc.txt");

        File::create(&doc_src)
            .expect("create doc_src")
            .write_all(b"new incoming content")
            .expect("write doc_src");

        File::create(&doc_dst)
            .expect("create doc_dst")
            .write_all(b"old existing content")
            .expect("write doc_dst");

        let now = SystemTime::now();
        filetime::set_file_mtime(&doc_src, filetime::FileTime::from_system_time(now))
            .expect("set mtime doc_src");
        filetime::set_file_mtime(
            &doc_dst,
            filetime::FileTime::from_system_time(now - Duration::from_secs(60)),
        )
        .expect("set mtime doc_dst");

        // 3. Colliding file: sub/legacy.txt
        // Make dst newer, src older -> should skip replacement, preserving dst
        let legacy_src = src.join("sub/legacy.txt");
        let legacy_dst = dst.join("sub/legacy.txt");

        File::create(&legacy_src)
            .expect("create legacy_src")
            .write_all(b"old incoming content")
            .expect("write legacy_src");

        File::create(&legacy_dst)
            .expect("create legacy_dst")
            .write_all(b"new destination content")
            .expect("write legacy_dst");

        filetime::set_file_mtime(
            &legacy_src,
            filetime::FileTime::from_system_time(now - Duration::from_secs(60)),
        )
        .expect("set mtime legacy_src");
        filetime::set_file_mtime(&legacy_dst, filetime::FileTime::from_system_time(now))
            .expect("set mtime legacy_dst");

        // 4. Non-colliding existing destination file: dst/retained.txt -> should remain intact
        let retained_dst = dst.join("retained.txt");
        File::create(&retained_dst)
            .expect("create retained")
            .write_all(b"retained content")
            .expect("write retained");

        let control = TaskControl::new();
        let mut progress_accum: u64 = 0;
        let summary = execute_directory_merge(&src, &dst, &control, |bytes| {
            progress_accum = progress_accum.saturating_add(bytes);
        })
        .await
        .expect("execute_directory_merge");

        assert_eq!(summary.files_copied, 1, "should copy 1 unique file");
        assert_eq!(summary.files_replaced, 1, "should replace 1 older file");
        assert_eq!(summary.files_skipped, 1, "should skip 1 newer file");
        assert!(summary.bytes_transferred > 0);

        // Verify content in dst
        assert!(dst.join("unique.txt").exists());
        assert!(dst.join("retained.txt").exists());

        let mut doc_content = String::new();
        File::open(doc_dst)
            .expect("open doc_dst")
            .read_to_string(&mut doc_content)
            .expect("read doc_dst");
        assert_eq!(doc_content, "new incoming content");

        let mut legacy_content = String::new();
        File::open(legacy_dst)
            .expect("open legacy_dst")
            .read_to_string(&mut legacy_content)
            .expect("read legacy_dst");
        assert_eq!(legacy_content, "new destination content");
    }

    #[tokio::test]
    async fn test_directory_merge_cycle_prevention() {
        let dir = tempdir().expect("tempdir");
        let parent = dir.path().join("parent");
        let child = parent.join("child");
        std::fs::create_dir_all(&child).expect("create dirs");

        let control = TaskControl::new();
        let err = execute_directory_merge(&parent, &child, &control, |_| {})
            .await
            .unwrap_err();
        match err {
            OperationError::Invalid(msg) => {
                assert!(msg.contains("subfolder"));
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }
}
