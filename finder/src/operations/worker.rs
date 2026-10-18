//! Dedicated asynchronous background worker pipeline for batch operations.

use std::path::Path;
use std::time::{Duration, Instant};

use async_channel::Sender;
use shared::models::operation::{BatchOperation, OperationKind, ProgressReport};

use super::error::{OperationError, Result};
use super::preflight::run_preflight_scan;
use super::recovery::{RECOVERY_CHECKPOINT_INTERVAL, RecoveryManager, RecoveryState};
use super::transfer::execute_item_transfer;
use super::types::{OperationEvent, PreflightManifest, TaskControl};

/// Progress update throttling interval (50ms = 20Hz).
const PROGRESS_THROTTLE_INTERVAL: Duration = Duration::from_millis(50);

/// Exponential Moving Average smoothing factor (alpha = 0.25).
const EMA_ALPHA: f64 = 0.25;

/// Smoothing estimator for throughput rate and ETA.
#[derive(Debug)]
pub struct EmaRateEstimator {
    alpha: f64,
    smoothed_rate: f64,
    last_instant: Instant,
    last_bytes: u64,
    is_first: bool,
}

impl Default for EmaRateEstimator {
    fn default() -> Self {
        Self::new(EMA_ALPHA)
    }
}

impl EmaRateEstimator {
    /// Constructs a new [`EmaRateEstimator`] with the given smoothing factor.
    #[must_use]
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha,
            smoothed_rate: 0.0,
            last_instant: Instant::now(),
            last_bytes: 0,
            is_first: true,
        }
    }

    /// Updates throughput calculation with latest cumulative transferred bytes.
    #[allow(clippy::cast_precision_loss)]
    pub fn update(&mut self, current_bytes: u64) -> f64 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_instant).as_secs_f64();
        if elapsed < 0.001 {
            return self.smoothed_rate;
        }

        let delta_bytes = current_bytes.saturating_sub(self.last_bytes) as f64;
        let instant_rate = delta_bytes / elapsed;

        if self.is_first {
            self.smoothed_rate = instant_rate;
            self.is_first = false;
        } else {
            self.smoothed_rate =
                (1.0 - self.alpha).mul_add(self.smoothed_rate, self.alpha * instant_rate);
        }

        self.last_instant = now;
        self.last_bytes = current_bytes;
        self.smoothed_rate
    }

    /// Returns the currently smoothed transfer rate in bytes per second.
    #[must_use]
    pub const fn smoothed_rate(&self) -> f64 {
        self.smoothed_rate
    }

    /// Estimates remaining seconds to transfer remaining bytes.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn estimate_remaining_seconds(
        &self,
        total_bytes: u64,
        transferred_bytes: u64,
    ) -> Option<f64> {
        if transferred_bytes >= total_bytes {
            return Some(0.0);
        }
        if self.smoothed_rate <= 1.0 {
            return None;
        }
        let remaining_bytes = total_bytes.saturating_sub(transferred_bytes) as f64;
        Some(remaining_bytes / self.smoothed_rate)
    }
}

/// Headless asynchronous worker managing the lifecycle of an individual batch file operation.
pub struct OperationWorker {
    op: BatchOperation,
    control: TaskControl,
    event_tx: Sender<OperationEvent>,
    recovery: Option<RecoveryManager>,
}

impl OperationWorker {
    /// Creates a new [`OperationWorker`].
    #[must_use]
    pub const fn new(
        op: BatchOperation,
        control: TaskControl,
        event_tx: Sender<OperationEvent>,
    ) -> Self {
        Self {
            op,
            control,
            event_tx,
            recovery: None,
        }
    }

    /// Attaches a [`RecoveryManager`] for crash state logging and recovery checkpointing.
    #[must_use]
    pub fn with_recovery(mut self, recovery: RecoveryManager) -> Self {
        self.recovery = Some(recovery);
        self
    }

    /// Spawns the worker on Tokio's blocking threadpool and returns the join handle.
    #[must_use]
    pub fn spawn(self) -> tokio::task::JoinHandle<Result<()>> {
        tokio::task::spawn_blocking(move || self.run())
    }

    /// Runs the worker execution pipeline synchronously to completion.
    ///
    /// # Errors
    /// Returns [`OperationError`] if scanning, transferring, or cancellation occurs.
    pub fn run(mut self) -> Result<()> {
        let op_id = self.op.id.clone();
        let recovery = self.recovery.take();
        let res = self.run_pipeline(recovery.as_ref());

        if let Some(ref rec) = recovery {
            let _ = rec.clear_state();
        }

        match res {
            Ok(()) => {
                let _ = self.event_tx.send_blocking(OperationEvent::Completed {
                    operation_id: op_id,
                });
                Ok(())
            }
            Err(OperationError::Cancelled) => {
                let _ = self.event_tx.send_blocking(OperationEvent::Cancelled {
                    operation_id: op_id,
                });
                Err(OperationError::Cancelled)
            }
            Err(err) => {
                let _ = self.event_tx.send_blocking(OperationEvent::Failed {
                    operation_id: op_id,
                    error: err.to_string(),
                });
                Err(err)
            }
        }
    }

    /// Executes pre-flight scan followed by the item transfer loop.
    fn run_pipeline(&self, recovery: Option<&RecoveryManager>) -> Result<()> {
        let event_tx_clone = self.event_tx.clone();
        let manifest = run_preflight_scan(&self.op, &self.control, move |evt| {
            let _ = event_tx_clone.send_blocking(evt);
        })?;

        if manifest.total_items > 1
            && let Some(rec) = recovery
        {
            let state =
                RecoveryState::new(self.op.clone(), manifest.total_items, manifest.total_bytes);
            let _ = rec.save_state(&state);
        }

        self.execute_manifest_transfers(&manifest, recovery)?;

        if self.op.kind == OperationKind::Move && manifest.requires_cross_device_copy {
            self.cleanup_moved_source_directories();
        }

        Ok(())
    }

    /// Cleans up empty source directories after cross-device moves.
    fn cleanup_moved_source_directories(&self) {
        for src in &self.op.sources {
            if src.is_dir() {
                let _ = std::fs::remove_dir_all(src);
            }
        }
    }

    /// Executes transfer loop across all scheduled items in the manifest.
    fn execute_manifest_transfers(
        &self,
        manifest: &PreflightManifest,
        recovery: Option<&RecoveryManager>,
    ) -> Result<()> {
        let mut estimator = EmaRateEstimator::new(EMA_ALPHA);
        let mut last_report_time = Instant::now();
        let mut last_checkpoint_time = Instant::now();
        let mut completed_items: u64 = 0;
        let mut base_transferred: u64 = 0;

        for item in &manifest.items {
            self.handle_pause_and_cancel()?;

            let mut item_transferred: u64 = 0;
            let current_path = item.source.clone();

            execute_item_transfer(item, self.op.kind, &self.control, |copied, _total| {
                item_transferred = copied;
                let current_total = base_transferred.saturating_add(item_transferred);
                let now = Instant::now();

                if now.duration_since(last_report_time) >= PROGRESS_THROTTLE_INTERVAL {
                    let rate = estimator.update(current_total);
                    let remaining =
                        estimator.estimate_remaining_seconds(manifest.total_bytes, current_total);
                    let report = ProgressReport {
                        operation_id: self.op.id.clone(),
                        completed_items,
                        total_items: manifest.total_items,
                        transferred_bytes: current_total,
                        total_bytes: manifest.total_bytes,
                        bytes_per_second: rate,
                        estimated_remaining_seconds: remaining,
                        current_file: Some(current_path.clone()),
                    };
                    let _ = self
                        .event_tx
                        .send_blocking(OperationEvent::Progress(report));
                    last_report_time = now;
                }

                if now.duration_since(last_checkpoint_time) >= RECOVERY_CHECKPOINT_INTERVAL {
                    if let Some(rec) = recovery {
                        let _ = rec.update_checkpoint(
                            completed_items,
                            current_total,
                            Some(current_path.clone()),
                        );
                    }
                    last_checkpoint_time = now;
                }
            })?;

            completed_items = completed_items.saturating_add(1);
            base_transferred = base_transferred.saturating_add(item.size_bytes);

            let _ = self.event_tx.send_blocking(OperationEvent::ItemCompleted {
                operation_id: self.op.id.clone(),
                source: item.source.clone(),
                destination: item.destination.clone(),
            });

            self.emit_progress_checkpoint(
                &mut estimator,
                &mut last_report_time,
                manifest,
                completed_items,
                base_transferred,
                Some(&item.source),
            );
        }

        Ok(())
    }

    /// Emits a single throttled progress update checkpoint.
    fn emit_progress_checkpoint(
        &self,
        estimator: &mut EmaRateEstimator,
        last_report_time: &mut Instant,
        manifest: &PreflightManifest,
        completed_items: u64,
        transferred_bytes: u64,
        current_file: Option<&Path>,
    ) {
        let now = Instant::now();
        let rate = estimator.update(transferred_bytes);
        let remaining =
            estimator.estimate_remaining_seconds(manifest.total_bytes, transferred_bytes);
        let report = ProgressReport {
            operation_id: self.op.id.clone(),
            completed_items,
            total_items: manifest.total_items,
            transferred_bytes,
            total_bytes: manifest.total_bytes,
            bytes_per_second: rate,
            estimated_remaining_seconds: remaining,
            current_file: current_file.map(Path::to_path_buf),
        };
        let _ = self
            .event_tx
            .send_blocking(OperationEvent::Progress(report));
        *last_report_time = now;
    }

    /// Handles pause and cancellation state transitions between items.
    fn handle_pause_and_cancel(&self) -> Result<()> {
        if self.control.is_cancelled() {
            return Err(OperationError::Cancelled);
        }

        while self.control.is_paused() {
            if self.control.is_cancelled() {
                return Err(OperationError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_worker_batch_copy_50_files() {
        let dir = tempdir().expect("tempdir");
        let src_dir = dir.path().join("src");
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&src_dir).expect("create src");
        std::fs::create_dir(&dst_dir).expect("create dst");

        let file_count = 50;
        let mut sources = Vec::new();

        for i in 0..file_count {
            let file_path = src_dir.join(format!("file_{i}.bin"));
            let mut f = File::create(&file_path).expect("create file");
            f.write_all(format!("payload content {i}").as_bytes())
                .expect("write payload");
            sources.push(file_path);
        }

        let op = BatchOperation {
            id: "batch-worker-50".into(),
            kind: OperationKind::Copy,
            sources,
            destination: Some(dst_dir.clone()),
        };

        let control = TaskControl::new();
        let (tx, rx) = async_channel::unbounded();
        let worker = OperationWorker::new(op, control, tx);

        let handle = worker.spawn();
        let mut received_events = Vec::new();

        while let Ok(evt) = rx.recv().await {
            let is_terminal = matches!(
                evt,
                OperationEvent::Completed { .. }
                    | OperationEvent::Cancelled { .. }
                    | OperationEvent::Failed { .. }
            );
            received_events.push(evt);
            if is_terminal {
                break;
            }
        }

        let res = handle.await.expect("join handle");
        assert!(res.is_ok(), "worker run should succeed");

        let has_progress = received_events
            .iter()
            .any(|e| matches!(e, OperationEvent::Progress(_)));
        let has_item_completed = received_events
            .iter()
            .any(|e| matches!(e, OperationEvent::ItemCompleted { .. }));
        let has_completed = received_events
            .iter()
            .any(|e| matches!(e, OperationEvent::Completed { .. }));

        assert!(has_progress, "should emit progress events");
        assert!(has_item_completed, "should emit item completed events");
        assert!(has_completed, "should emit final completed event");

        for i in 0..file_count {
            let target_file = dst_dir.join(format!("file_{i}.bin"));
            assert!(target_file.exists(), "file_{i}.bin must exist in dst");
        }
    }
}
