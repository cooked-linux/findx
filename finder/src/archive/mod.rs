//! Multi-format archive creation and compression engine.

pub mod error;
pub mod naming;
pub mod types;
pub mod worker;
pub mod writer;

pub use error::{ArchiveError, Result};
pub use naming::{
    determine_archive_base_name, determine_archive_name, determine_archive_path,
    generate_staging_path,
};
pub use types::{
    ArchiveCompressionJob, ArchiveEntryItem, ArchiveFormat, ArchiveManifest, ArchiveProgress,
};
pub use worker::{ArchiveWorker, check_disk_space, run_archive_preflight};
pub use writer::ArchiveWriter;

/// Helper to launch compression on a set of selected paths with a specified format.
#[must_use]
pub fn compress_selection(
    queue: &crate::operations::OperationsQueue,
    paths: Vec<std::path::PathBuf>,
    format: Option<ArchiveFormat>,
) -> Option<String> {
    if paths.is_empty() {
        return None;
    }
    let dest_dir = paths[0]
        .parent()
        .unwrap_or_else(|| std::path::Path::new("/"))
        .to_path_buf();
    let fmt = format.unwrap_or_default();
    let final_path = determine_archive_path(&dest_dir, &paths, fmt);
    let op_id = format!("compress-{}", uuid::Uuid::new_v4());
    let op = shared::models::operation::BatchOperation::new(
        op_id,
        shared::models::operation::OperationKind::Compress,
        paths,
        Some(final_path),
    );
    Some(queue.enqueue_operation(op))
}
