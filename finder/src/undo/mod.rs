//! Transactional multi-level undo and redo engine for finder operations.

pub mod error;
pub mod inversion;
pub mod journal;
pub mod manager;
pub mod models;

pub use error::{Result, UndoError};
pub use inversion::{ConflictStrategy, execute_forward, execute_inverse};
pub use journal::{default_journal_path, load_journal, save_journal};
pub use manager::{DEFAULT_MAX_DEPTH, UndoManager};
pub use models::{FileOperationRecord, UndoOperationType, UndoTransaction};

use crate::operations::OperationsQueue;
use std::sync::{Arc, Mutex};

/// Connects an [`UndoManager`] to an [`OperationsQueue`] to automatically record completed operations.
pub fn connect_to_queue(queue: &OperationsQueue, undo_manager: Arc<Mutex<UndoManager>>) {
    queue.attach_undo_manager(undo_manager);
}
