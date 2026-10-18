//! Concurrency-throttled batch file operations queue manager.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use async_channel::{Receiver, Sender};
use shared::models::operation::{BatchOperation, OperationKind, ProgressReport};

use super::error::{OperationError, Result};
use super::types::{OperationEvent, TaskControl};
use super::worker::OperationWorker;
use crate::undo::{FileOperationRecord, UndoManager, UndoOperationType, UndoTransaction};

/// Default maximum parallel active operations allowed simultaneously.
pub const DEFAULT_MAX_CONCURRENCY: usize = 2;

/// Tracking state for an in-flight background operation.
#[derive(Debug, Clone)]
pub struct ActiveTask {
    /// Task identifier matching [`BatchOperation::id`].
    pub operation_id: String,
    /// Operation kind.
    pub kind: OperationKind,
    /// Task control handle for pausing and cancelling.
    pub control: TaskControl,
    /// Latest progress report received from the worker.
    pub last_report: Option<ProgressReport>,
    /// Underlying batch operation request.
    pub operation: BatchOperation,
    /// Records of individual items successfully completed so far.
    pub completed_records: Vec<FileOperationRecord>,
}

/// Internal mutable state of the operations queue.
struct OperationsQueueState {
    max_concurrency: usize,
    pending_queue: VecDeque<BatchOperation>,
    active_tasks: HashMap<String, ActiveTask>,
    subscribers: Vec<Sender<OperationEvent>>,
    undo_manager: Option<Arc<Mutex<UndoManager>>>,
}

impl OperationsQueueState {
    fn new(max_concurrency: usize) -> Self {
        Self {
            max_concurrency,
            pending_queue: VecDeque::new(),
            active_tasks: HashMap::new(),
            subscribers: Vec::new(),
            undo_manager: None,
        }
    }

    fn enqueue(&mut self, op: BatchOperation, worker_tx: &Sender<OperationEvent>) {
        if self.active_tasks.len() < self.max_concurrency {
            self.spawn_task(op, worker_tx);
        } else {
            self.pending_queue.push_back(op);
        }
    }

    fn spawn_task(&mut self, op: BatchOperation, worker_tx: &Sender<OperationEvent>) {
        let control = TaskControl::new();
        let task = ActiveTask {
            operation_id: op.id.clone(),
            kind: op.kind,
            control: control.clone(),
            last_report: None,
            operation: op.clone(),
            completed_records: Vec::new(),
        };
        self.active_tasks.insert(op.id.clone(), task);
        let worker = OperationWorker::new(op, control, worker_tx.clone());
        drop(worker.spawn());
    }

    fn maybe_schedule_next(&mut self, worker_tx: &Sender<OperationEvent>) {
        while self.active_tasks.len() < self.max_concurrency {
            if let Some(next_op) = self.pending_queue.pop_front() {
                self.spawn_task(next_op, worker_tx);
            } else {
                break;
            }
        }
    }

    fn broadcast_event(&mut self, event: &OperationEvent) {
        self.subscribers
            .retain(|tx| tx.try_send(event.clone()).is_ok() || !tx.is_closed());
    }
}

/// Thread-safe manager coordinating queued and active file operations.
#[derive(Clone)]
pub struct OperationsQueue {
    state: Arc<Mutex<OperationsQueueState>>,
    worker_tx: Sender<OperationEvent>,
}

impl std::fmt::Debug for OperationsQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationsQueue")
            .field("active_count", &self.active_count())
            .finish_non_exhaustive()
    }
}

impl Default for OperationsQueue {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_CONCURRENCY)
    }
}

impl OperationsQueue {
    /// Constructs a new [`OperationsQueue`] with the specified concurrency throttle limit.
    #[must_use]
    pub fn new(max_concurrency: usize) -> Self {
        let (worker_tx, worker_rx) = async_channel::unbounded();
        let state = Arc::new(Mutex::new(OperationsQueueState::new(max_concurrency)));

        let state_clone = Arc::clone(&state);
        let worker_tx_clone = worker_tx.clone();

        Self::start_event_loop(worker_rx, state_clone, worker_tx_clone);

        Self { state, worker_tx }
    }

    fn start_event_loop(
        worker_rx: Receiver<OperationEvent>,
        state: Arc<Mutex<OperationsQueueState>>,
        worker_tx: Sender<OperationEvent>,
    ) {
        let pump = async move {
            while let Ok(event) = worker_rx.recv().await {
                let mut guard = state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);

                match &event {
                    OperationEvent::Progress(report) => {
                        if let Some(task) = guard.active_tasks.get_mut(&report.operation_id) {
                            task.last_report = Some(report.clone());
                        }
                    }
                    OperationEvent::ItemCompleted {
                        operation_id,
                        source,
                        destination,
                    } => {
                        if let Some(task) = guard.active_tasks.get_mut(operation_id) {
                            let dest_path = destination.clone().unwrap_or_else(|| source.clone());
                            let (ino, dev) = shared::fs::stat::stat_path(&dest_path)
                                .map(|s| (s.ino, s.dev))
                                .or_else(|_| {
                                    shared::fs::stat::stat_path(source).map(|s| (s.ino, s.dev))
                                })
                                .unwrap_or((0, 0));
                            task.completed_records.push(FileOperationRecord::new(
                                source.clone(),
                                dest_path,
                                ino,
                                dev,
                                SystemTime::now(),
                                None,
                            ));
                        }
                    }
                    OperationEvent::Completed { operation_id } => {
                        if let Some(task) = guard.active_tasks.remove(operation_id) {
                            if let Some(ref undo_mgr) = guard.undo_manager
                                && let Some(op_type) = map_operation_kind_to_undo(task.kind)
                            {
                                let records = if task.completed_records.is_empty() {
                                    create_records_from_batch_operation(&task.operation)
                                } else {
                                    task.completed_records
                                };

                                if !records.is_empty() {
                                    let description = format!(
                                        "{} of {} {}",
                                        task.kind,
                                        records.len(),
                                        if records.len() == 1 { "item" } else { "items" }
                                    );
                                    let tx = UndoTransaction::new(
                                        uuid::Uuid::new_v4().to_string(),
                                        op_type,
                                        description,
                                        records,
                                        SystemTime::now(),
                                    );
                                    let mut mgr = undo_mgr
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                                    mgr.record_transaction(tx);
                                }
                            }
                            guard.maybe_schedule_next(&worker_tx);
                        }
                    }
                    OperationEvent::Cancelled { operation_id }
                    | OperationEvent::Failed { operation_id, .. } => {
                        guard.active_tasks.remove(operation_id);
                        guard.maybe_schedule_next(&worker_tx);
                    }
                    _ => {}
                }

                guard.broadcast_event(&event);
            }
        };

        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(pump);
        } else {
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                if let Ok(runtime) = rt {
                    runtime.block_on(pump);
                }
            });
        }
    }

    /// Enqueues a batch operation for processing and returns its unique task identifier.
    pub fn enqueue_operation(&self, op: BatchOperation) -> String {
        let task_id = op.id.clone();
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.enqueue(op, &self.worker_tx);
        task_id
    }

    /// Pauses an active file operation.
    ///
    /// # Errors
    /// Returns [`OperationError::Invalid`] if the task ID is not found.
    pub fn pause_operation(&self, task_id: &str) -> Result<()> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(task) = guard.active_tasks.get(task_id) {
            task.control.pause();
            let evt = OperationEvent::Paused {
                operation_id: task_id.to_string(),
            };
            guard.broadcast_event(&evt);
            drop(guard);
            Ok(())
        } else {
            drop(guard);
            Err(OperationError::Invalid(format!(
                "Task '{task_id}' not found"
            )))
        }
    }

    /// Resumes a paused file operation.
    ///
    /// # Errors
    /// Returns [`OperationError::Invalid`] if the task ID is not found.
    pub fn resume_operation(&self, task_id: &str) -> Result<()> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(task) = guard.active_tasks.get(task_id) {
            task.control.resume();
            let evt = OperationEvent::Resumed {
                operation_id: task_id.to_string(),
            };
            guard.broadcast_event(&evt);
            drop(guard);
            Ok(())
        } else {
            drop(guard);
            Err(OperationError::Invalid(format!(
                "Task '{task_id}' not found"
            )))
        }
    }

    /// Cancels an in-flight or queued file operation.
    ///
    /// # Errors
    /// Returns [`OperationError::Invalid`] if the task ID is not found.
    pub fn cancel_operation(&self, task_id: &str) -> Result<()> {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(task) = guard.active_tasks.get(task_id) {
            task.control.cancel();
            drop(guard);
            return Ok(());
        }

        if let Some(pos) = guard.pending_queue.iter().position(|op| op.id == task_id) {
            if let Some(op) = guard.pending_queue.remove(pos) {
                let evt = OperationEvent::Cancelled {
                    operation_id: op.id,
                };
                guard.broadcast_event(&evt);
            }
            drop(guard);
            return Ok(());
        }

        drop(guard);
        Err(OperationError::Invalid(format!(
            "Task '{task_id}' not found"
        )))
    }

    /// Cancels all actively running and queued operations in the background.
    pub fn cancel_all_operations(&self) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for task in guard.active_tasks.values() {
            task.control.cancel();
        }

        while let Some(op) = guard.pending_queue.pop_front() {
            let evt = OperationEvent::Cancelled {
                operation_id: op.id,
            };
            guard.broadcast_event(&evt);
        }
        drop(guard);
    }

    /// Returns snapshot progress reports for all actively running operations.
    #[must_use]
    pub fn get_active_reports(&self) -> Vec<ProgressReport> {
        let guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard
            .active_tasks
            .values()
            .filter_map(|t| t.last_report.clone())
            .collect()
    }

    /// Returns snapshot records of all actively executing tasks.
    #[must_use]
    pub fn active_tasks(&self) -> Vec<ActiveTask> {
        let guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.active_tasks.values().cloned().collect()
    }

    /// Returns all operations waiting in the pending queue.
    #[must_use]
    pub fn pending_operations(&self) -> Vec<BatchOperation> {
        let guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.pending_queue.iter().cloned().collect()
    }

    /// Returns total count of active tasks currently running or paused.
    #[must_use]
    pub fn active_count(&self) -> usize {
        let guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.active_tasks.len()
    }

    /// Subscribes to the broadcast stream of operation lifecycle and progress events.
    #[must_use]
    pub fn subscribe_events(&self) -> Receiver<OperationEvent> {
        let (tx, rx) = async_channel::unbounded();
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.subscribers.push(tx);
        rx
    }

    /// Attaches an [`UndoManager`] to automatically record completed operations into undo history.
    pub fn attach_undo_manager(&self, undo_manager: Arc<Mutex<UndoManager>>) {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.undo_manager = Some(undo_manager);
    }
}

const fn map_operation_kind_to_undo(kind: OperationKind) -> Option<UndoOperationType> {
    match kind {
        OperationKind::Move => Some(UndoOperationType::Move),
        OperationKind::Copy => Some(UndoOperationType::Copy),
        OperationKind::Duplicate => Some(UndoOperationType::Duplicate),
        OperationKind::Trash => Some(UndoOperationType::Trash),
        OperationKind::Restore | OperationKind::Delete => None,
    }
}

fn create_records_from_batch_operation(op: &BatchOperation) -> Vec<FileOperationRecord> {
    let mut records = Vec::with_capacity(op.sources.len());
    for src in &op.sources {
        let dst = op.destination.as_ref().map_or_else(
            || src.clone(),
            |dest_dir| {
                src.file_name()
                    .map_or_else(|| dest_dir.clone(), |name| dest_dir.join(name))
            },
        );

        let (ino, dev) = shared::fs::stat::stat_path(&dst)
            .map(|s| (s.ino, s.dev))
            .or_else(|_| shared::fs::stat::stat_path(src).map(|s| (s.ino, s.dev)))
            .unwrap_or((0, 0));

        records.push(FileOperationRecord::new(
            src.clone(),
            dst,
            ino,
            dev,
            SystemTime::now(),
            None,
        ));
    }
    records
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
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_queue_serial_execution_and_pause_resume() {
        let dir = tempdir().expect("tempdir");
        let src_dir = dir.path().join("src");
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&src_dir).expect("create src");
        std::fs::create_dir(&dst_dir).expect("create dst");

        // Create 3 files for 3 distinct operations
        for i in 1..=3 {
            let f = src_dir.join(format!("item_{i}.txt"));
            let mut file = File::create(&f).expect("create");
            file.write_all(format!("payload {i}").as_bytes())
                .expect("write");
        }

        // Concurrency throttle: 1 (strictly serial execution)
        let queue = OperationsQueue::new(1);
        let rx = queue.subscribe_events();

        let op1 = BatchOperation {
            id: "op-1".into(),
            kind: OperationKind::Copy,
            sources: vec![src_dir.join("item_1.txt")],
            destination: Some(dst_dir.clone()),
        };
        let op2 = BatchOperation {
            id: "op-2".into(),
            kind: OperationKind::Copy,
            sources: vec![src_dir.join("item_2.txt")],
            destination: Some(dst_dir.clone()),
        };
        let op3 = BatchOperation {
            id: "op-3".into(),
            kind: OperationKind::Copy,
            sources: vec![src_dir.join("item_3.txt")],
            destination: Some(dst_dir.clone()),
        };

        let id1 = queue.enqueue_operation(op1);
        let id2 = queue.enqueue_operation(op2);
        let id3 = queue.enqueue_operation(op3);

        assert_eq!(id1, "op-1");
        assert_eq!(id2, "op-2");
        assert_eq!(id3, "op-3");

        // Test pause & resume on active op-1
        let _ = queue.pause_operation("op-1");
        let _ = queue.resume_operation("op-1");

        let mut completed_ops = Vec::new();

        while let Ok(evt) = rx.recv().await {
            if let OperationEvent::Completed { operation_id } = evt {
                completed_ops.push(operation_id);
                if completed_ops.len() == 3 {
                    break;
                }
            }
        }

        assert_eq!(completed_ops, vec!["op-1", "op-2", "op-3"]);
        assert!(dst_dir.join("item_1.txt").exists());
        assert!(dst_dir.join("item_2.txt").exists());
        assert!(dst_dir.join("item_3.txt").exists());
    }
}
