//! File operations engine and worker queue for `FindX` Finder.

pub mod error;
pub mod preflight;
pub mod queue;
pub mod recovery;
pub mod transfer;
pub mod types;
pub mod worker;

pub use error::{OperationError, Result};
pub use preflight::{query_available_space, resolve_duplicate_destination, run_preflight_scan};
pub use queue::{ActiveTask, DEFAULT_MAX_CONCURRENCY, OperationsQueue};
pub use recovery::{RecoveryManager, RecoveryState, default_recovery_path};
pub use shared::models::operation::{
    BatchOperation, OperationKind, OperationState, ProgressReport, TransferItem,
};
pub use transfer::{atomic_move, copy_file_with_control, execute_item_transfer};
pub use types::{OperationEvent, PreflightManifest, TaskControl};
pub use worker::{EmaRateEstimator, OperationWorker};
