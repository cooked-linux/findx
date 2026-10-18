//! File operations engine and worker queue for `FindX` Finder.

pub mod conflict;
pub mod error;
pub mod folder_size;
pub mod merge;
pub mod naming;
pub mod preflight;
pub mod queue;
pub mod recovery;
pub mod transfer;
pub mod types;
pub mod worker;

pub use conflict::{
    ConflictAction, ConflictArbiter, ConflictContext, ConflictItemMeta, ConflictOutcome,
    ConflictPrompt, ConflictResolution, RelativeTimeStatus, StagedRollback, check_collision,
};
pub use error::{OperationError, Result};
pub use folder_size::{
    FolderSizeCache, FolderSizeError, FolderSizeManager, FolderSizeResult, FolderSizeWatcher,
    Priority, calculate_folder_size_sync,
};
pub use merge::{MergeSummary, execute_directory_merge};
pub use naming::{
    generate_unique_name, generate_unique_name_with_case_sensitivity, generate_unique_path,
    split_stem_and_extension,
};
pub use preflight::{query_available_space, resolve_duplicate_destination, run_preflight_scan};
pub use queue::{ActiveTask, DEFAULT_MAX_CONCURRENCY, OperationsQueue};
pub use recovery::{RecoveryManager, RecoveryState, default_recovery_path};
pub use shared::models::operation::{
    BatchOperation, OperationKind, OperationState, ProgressReport, TransferItem,
};
pub use transfer::{atomic_move, copy_file_with_control, execute_item_transfer};
pub use types::{OperationEvent, PreflightManifest, TaskControl};
pub use worker::{EmaRateEstimator, OperationWorker};
