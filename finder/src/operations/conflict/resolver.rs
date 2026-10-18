//! Interactive conflict resolution arbiter and safe staging coordinator.

use std::path::{Path, PathBuf};

use async_channel::Sender;
use shared::models::operation::OperationKind;
use tokio::sync::oneshot;

use super::models::{ConflictAction, ConflictContext, ConflictResolution};
use super::staging::StagedRollback;
use crate::operations::error::{OperationError, Result};
use crate::operations::merge::{MergeSummary, execute_directory_merge};
use crate::operations::naming::generate_unique_path;
use crate::operations::transfer::{atomic_move, copy_file_with_control};
use crate::operations::types::TaskControl;

/// Prompt message dispatched to the UI when user interaction is required.
#[derive(Debug)]
pub struct ConflictPrompt {
    /// Context describing colliding source and destination items.
    pub context: ConflictContext,
    /// Oneshot response channel to receive user's resolution decision.
    pub responder: oneshot::Sender<ConflictResolution>,
}

/// The concrete outcome after applying a conflict resolution action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictOutcome {
    /// Destination was replaced with source item (with staging protection).
    Replaced,
    /// Source item was transferred to a uniquely renamed destination path.
    KeptBoth {
        /// Generated destination path.
        new_destination: PathBuf,
    },
    /// Directory trees were recursively merged.
    Merged(MergeSummary),
    /// Conflicting item was skipped.
    Skipped,
    /// Batch operation was stopped by user request.
    Stopped,
}

/// Arbiter managing conflict resolution decisions and batch "Apply to All" state.
pub struct ConflictArbiter {
    prompt_tx: Sender<ConflictPrompt>,
    apply_to_all_action: Option<ConflictAction>,
}

impl ConflictArbiter {
    /// Constructs a new [`ConflictArbiter`] with the given UI prompt sender.
    #[must_use]
    pub const fn new(prompt_tx: Sender<ConflictPrompt>) -> Self {
        Self {
            prompt_tx,
            apply_to_all_action: None,
        }
    }

    /// Returns the currently cached "Apply to All" action, if any.
    #[must_use]
    pub const fn apply_to_all_action(&self) -> Option<ConflictAction> {
        self.apply_to_all_action
    }

    /// Sets or clears the active "Apply to All" resolution action.
    pub const fn set_apply_to_all_action(&mut self, action: Option<ConflictAction>) {
        self.apply_to_all_action = action;
    }

    /// Asynchronously obtains the resolution action for a conflict.
    ///
    /// If an "Apply to All" action is cached, returns it immediately.
    /// Otherwise, dispatches a [`ConflictPrompt`] to the UI and awaits the response.
    ///
    /// # Errors
    /// Returns [`OperationError::Cancelled`] if the prompt channel is closed or responder drops.
    pub async fn resolve_conflict(&mut self, context: &ConflictContext) -> Result<ConflictAction> {
        if let Some(action) = self.apply_to_all_action {
            return Ok(action);
        }

        let (resp_tx, resp_rx) = oneshot::channel();
        let prompt = ConflictPrompt {
            context: context.clone(),
            responder: resp_tx,
        };

        self.prompt_tx
            .send(prompt)
            .await
            .map_err(|_| OperationError::Cancelled)?;

        let resolution = resp_rx.await.map_err(|_| OperationError::Cancelled)?;

        if resolution.apply_to_all {
            self.apply_to_all_action = Some(resolution.action);
        }

        Ok(resolution.action)
    }

    /// Synchronously obtains the resolution action for a conflict (blocking).
    ///
    /// # Errors
    /// Returns [`OperationError::Cancelled`] if the prompt channel is closed or responder drops.
    pub fn resolve_conflict_blocking(
        &mut self,
        context: &ConflictContext,
    ) -> Result<ConflictAction> {
        if let Some(action) = self.apply_to_all_action {
            return Ok(action);
        }

        let (resp_tx, resp_rx) = oneshot::channel();
        let prompt = ConflictPrompt {
            context: context.clone(),
            responder: resp_tx,
        };

        self.prompt_tx
            .send_blocking(prompt)
            .map_err(|_| OperationError::Cancelled)?;

        let resolution = resp_rx
            .blocking_recv()
            .map_err(|_| OperationError::Cancelled)?;

        if resolution.apply_to_all {
            self.apply_to_all_action = Some(resolution.action);
        }

        Ok(resolution.action)
    }

    /// Applies the chosen conflict action against source and destination with rollback safety.
    ///
    /// # Errors
    /// Returns [`OperationError`] on I/O error, invalid parameters, or if aborted/cancelled.
    pub async fn execute_resolution<F>(
        &mut self,
        action: ConflictAction,
        source: &Path,
        destination: &Path,
        kind: OperationKind,
        task_control: &TaskControl,
        mut on_progress: F,
    ) -> Result<ConflictOutcome>
    where
        F: FnMut(u64, u64) + Send,
    {
        match action {
            ConflictAction::Replace => {
                Self::safe_replace(source, destination, kind, task_control, on_progress)?;
                Ok(ConflictOutcome::Replaced)
            }
            ConflictAction::KeepBoth => {
                let unique_destination = generate_unique_path(destination);
                Self::transfer_non_colliding(
                    source,
                    &unique_destination,
                    kind,
                    task_control,
                    on_progress,
                )?;
                Ok(ConflictOutcome::KeptBoth {
                    new_destination: unique_destination,
                })
            }
            ConflictAction::Merge => {
                let summary = execute_directory_merge(source, destination, task_control, |bytes| {
                    on_progress(bytes, 0);
                })
                .await?;
                Ok(ConflictOutcome::Merged(summary))
            }
            ConflictAction::Skip => Ok(ConflictOutcome::Skipped),
            ConflictAction::Stop => Err(OperationError::Cancelled),
        }
    }

    /// Executes replacement with staging rollback protection.
    fn safe_replace<F>(
        source: &Path,
        destination: &Path,
        kind: OperationKind,
        task_control: &TaskControl,
        on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(u64, u64),
    {
        // Move existing target to rollback staging area
        let rollback = StagedRollback::stage(destination)?;

        let transfer_res = if source.is_dir() {
            // Directory replacement: copy/move source into destination
            std::fs::create_dir_all(destination).map_err(|err| OperationError::Io {
                path: destination.to_path_buf(),
                source: err,
            })
        } else {
            // Regular file replacement
            match kind {
                OperationKind::Move => {
                    // Try fast atomic move first
                    if atomic_move(source, destination).is_ok() {
                        Ok(())
                    } else {
                        copy_file_with_control(source, destination, task_control, on_progress)
                            .and_then(|_| {
                                std::fs::remove_file(source).map_err(|err| OperationError::Io {
                                    path: source.to_path_buf(),
                                    source: err,
                                })
                            })
                    }
                }
                _ => copy_file_with_control(source, destination, task_control, on_progress)
                    .map(|_| ()),
            }
        };

        match transfer_res {
            Ok(()) => {
                rollback.commit()?;
                Ok(())
            }
            Err(err) => {
                // rollback drops here, automatically restoring destination
                Err(err)
            }
        }
    }

    /// Transfers an item to a non-colliding path.
    fn transfer_non_colliding<F>(
        source: &Path,
        destination: &Path,
        kind: OperationKind,
        task_control: &TaskControl,
        on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(u64, u64),
    {
        if source.is_dir() {
            std::fs::create_dir_all(destination).map_err(|err| OperationError::Io {
                path: destination.to_path_buf(),
                source: err,
            })?;
            return Ok(());
        }

        if kind == OperationKind::Move && atomic_move(source, destination).is_ok() {
            return Ok(());
        }

        copy_file_with_control(source, destination, task_control, on_progress)?;
        if kind == OperationKind::Move {
            let _ = std::fs::remove_file(source);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone,
    clippy::similar_names
)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::{Read, Write};
    use tempfile::tempdir;

    use crate::operations::conflict::detector::check_collision;

    #[tokio::test]
    async fn test_resolver_replace_with_staging() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("dest.txt");

        File::create(&src)
            .expect("create src")
            .write_all(b"new source content")
            .expect("write src");
        File::create(&dst)
            .expect("create dst")
            .write_all(b"old destination content")
            .expect("write dst");

        let (prompt_tx, prompt_rx) = async_channel::unbounded();
        let mut arbiter = ConflictArbiter::new(prompt_tx);

        // Spawn mock UI handler responding Replace
        tokio::spawn(async move {
            if let Ok(prompt) = prompt_rx.recv().await {
                let _ = prompt
                    .responder
                    .send(ConflictResolution::new(ConflictAction::Replace, false));
            }
        });

        let ctx = check_collision("op-replace", &src, &dst)
            .expect("check_collision")
            .expect("collision exists");
        let action = arbiter
            .resolve_conflict(&ctx)
            .await
            .expect("resolve_conflict");
        assert_eq!(action, ConflictAction::Replace);

        let control = TaskControl::new();
        let outcome = arbiter
            .execute_resolution(action, &src, &dst, OperationKind::Copy, &control, |_, _| {})
            .await
            .expect("execute_resolution");

        assert_eq!(outcome, ConflictOutcome::Replaced);

        let mut content = String::new();
        File::open(&dst)
            .expect("open dst")
            .read_to_string(&mut content)
            .expect("read dst");
        assert_eq!(content, "new source content");
    }

    #[tokio::test]
    async fn test_resolver_replace_rollback_on_cancellation() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("dest.txt");

        File::create(&src)
            .expect("create src")
            .write_all(b"new source content")
            .expect("write src");
        File::create(&dst)
            .expect("create dst")
            .write_all(b"old destination content")
            .expect("write dst");

        let control = TaskControl::new();
        control.cancel(); // Pre-cancelled to trigger failure during copy

        let (prompt_tx, _prompt_rx) = async_channel::unbounded();
        let mut arbiter = ConflictArbiter::new(prompt_tx);

        let res = arbiter
            .execute_resolution(
                ConflictAction::Replace,
                &src,
                &dst,
                OperationKind::Copy,
                &control,
                |_, _| {},
            )
            .await;

        assert!(res.is_err(), "should fail due to cancellation");

        // Assert original destination content was safely rolled back!
        let mut content = String::new();
        File::open(&dst)
            .expect("open dst")
            .read_to_string(&mut content)
            .expect("read dst");
        assert_eq!(content, "old destination content");
    }

    #[tokio::test]
    async fn test_resolver_keep_both() {
        let dir = tempdir().expect("tempdir");
        let src_dir = dir.path().join("src_folder");
        let dst_dir = dir.path().join("dst_folder");
        std::fs::create_dir(&src_dir).expect("create src dir");
        std::fs::create_dir(&dst_dir).expect("create dst dir");

        let src = src_dir.join("photo.jpg");
        let dst = dst_dir.join("photo.jpg");

        File::create(&src)
            .expect("create src")
            .write_all(b"incoming photo")
            .expect("write src");
        File::create(&dst)
            .expect("create dst")
            .write_all(b"existing photo")
            .expect("write dst");

        let (prompt_tx, prompt_rx) = async_channel::unbounded();
        let mut arbiter = ConflictArbiter::new(prompt_tx);

        tokio::spawn(async move {
            if let Ok(prompt) = prompt_rx.recv().await {
                let _ = prompt
                    .responder
                    .send(ConflictResolution::new(ConflictAction::KeepBoth, false));
            }
        });

        let ctx = check_collision("op-keep-both", &src, &dst)
            .expect("check_collision")
            .expect("collision exists");
        let action = arbiter
            .resolve_conflict(&ctx)
            .await
            .expect("resolve_conflict");
        assert_eq!(action, ConflictAction::KeepBoth);

        let control = TaskControl::new();
        let outcome = arbiter
            .execute_resolution(action, &src, &dst, OperationKind::Copy, &control, |_, _| {})
            .await
            .expect("execute_resolution");

        let expected_new = dst_dir.join("photo (1).jpg");
        assert_eq!(
            outcome,
            ConflictOutcome::KeptBoth {
                new_destination: expected_new.clone()
            }
        );
        assert!(expected_new.exists());
        assert!(dst.exists());
    }

    #[tokio::test]
    async fn test_resolver_merge() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src_dir");
        let dst = dir.path().join("dst_dir");

        std::fs::create_dir(&src).expect("create src");
        std::fs::create_dir(&dst).expect("create dst");

        File::create(src.join("file1.txt")).expect("create file1");
        File::create(dst.join("file2.txt")).expect("create file2");

        let (prompt_tx, prompt_rx) = async_channel::unbounded();
        let mut arbiter = ConflictArbiter::new(prompt_tx);

        tokio::spawn(async move {
            if let Ok(prompt) = prompt_rx.recv().await {
                let _ = prompt
                    .responder
                    .send(ConflictResolution::new(ConflictAction::Merge, false));
            }
        });

        let ctx = check_collision("op-merge", &src, &dst)
            .expect("check_collision")
            .expect("collision exists");
        let action = arbiter
            .resolve_conflict(&ctx)
            .await
            .expect("resolve_conflict");
        assert_eq!(action, ConflictAction::Merge);

        let control = TaskControl::new();
        let outcome = arbiter
            .execute_resolution(action, &src, &dst, OperationKind::Copy, &control, |_, _| {})
            .await
            .expect("execute_resolution");

        assert!(matches!(outcome, ConflictOutcome::Merged(_)));
        assert!(dst.join("file1.txt").exists());
        assert!(dst.join("file2.txt").exists());
    }

    #[tokio::test]
    async fn test_resolver_skip_and_stop() {
        let dir = tempdir().expect("tempdir");
        let src_dir = dir.path().join("src_folder");
        let dst_dir = dir.path().join("dst_folder");
        std::fs::create_dir(&src_dir).expect("create src");
        std::fs::create_dir(&dst_dir).expect("create dst");

        let src = src_dir.join("file.txt");
        let dst = dst_dir.join("file.txt");

        File::create(&src).expect("create src");
        File::create(&dst).expect("create dst");

        // 1. Skip
        let (prompt_tx1, prompt_rx1) = async_channel::unbounded();
        let mut arbiter1 = ConflictArbiter::new(prompt_tx1);
        tokio::spawn(async move {
            if let Ok(prompt) = prompt_rx1.recv().await {
                let _ = prompt
                    .responder
                    .send(ConflictResolution::new(ConflictAction::Skip, false));
            }
        });

        let ctx = check_collision("op-skip", &src, &dst)
            .expect("check_collision")
            .expect("collision");
        let action = arbiter1.resolve_conflict(&ctx).await.expect("resolve");
        assert_eq!(action, ConflictAction::Skip);
        let control = TaskControl::new();
        let outcome = arbiter1
            .execute_resolution(action, &src, &dst, OperationKind::Copy, &control, |_, _| {})
            .await
            .expect("execute");
        assert_eq!(outcome, ConflictOutcome::Skipped);

        // 2. Stop
        let (prompt_tx2, prompt_rx2) = async_channel::unbounded();
        let mut arbiter2 = ConflictArbiter::new(prompt_tx2);
        tokio::spawn(async move {
            if let Ok(prompt) = prompt_rx2.recv().await {
                let _ = prompt
                    .responder
                    .send(ConflictResolution::new(ConflictAction::Stop, false));
            }
        });
        let action = arbiter2.resolve_conflict(&ctx).await.expect("resolve");
        assert_eq!(action, ConflictAction::Stop);
        let res = arbiter2
            .execute_resolution(action, &src, &dst, OperationKind::Copy, &control, |_, _| {})
            .await;
        assert!(matches!(res, Err(OperationError::Cancelled)));
    }

    #[tokio::test]
    async fn test_resolver_apply_to_all() {
        let dir = tempdir().expect("tempdir");
        let src1 = dir.path().join("file1.txt");
        let dst1 = dir.path().join("dst_file1.txt");
        let src2 = dir.path().join("file2.txt");
        let dst2 = dir.path().join("dst_file2.txt");

        File::create(&src1).expect("create");
        File::create(&dst1).expect("create");
        File::create(&src2).expect("create");
        File::create(&dst2).expect("create");

        let (prompt_tx, prompt_rx) = async_channel::unbounded();
        let mut arbiter = ConflictArbiter::new(prompt_tx);

        // UI handler only responds ONCE with apply_to_all: true
        tokio::spawn(async move {
            if let Ok(prompt) = prompt_rx.recv().await {
                let _ = prompt
                    .responder
                    .send(ConflictResolution::new(ConflictAction::Skip, true));
            }
        });

        let ctx1 = check_collision("op-all", &src1, &dst1)
            .expect("check")
            .expect("col");
        let act1 = arbiter.resolve_conflict(&ctx1).await.expect("res1");
        assert_eq!(act1, ConflictAction::Skip);
        assert_eq!(arbiter.apply_to_all_action(), Some(ConflictAction::Skip));

        // Second collision: should NOT prompt UI (channel closed/empty)
        let ctx2 = check_collision("op-all", &src2, &dst2)
            .expect("check")
            .expect("col");
        let act2 = arbiter.resolve_conflict(&ctx2).await.expect("res2");
        assert_eq!(act2, ConflictAction::Skip);
    }
}
