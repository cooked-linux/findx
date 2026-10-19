//! Operations progress, queue popover, and conflict resolution UI components.

pub mod conflict_dialog;
pub mod progress_dialog;
pub mod queue_popover;
pub mod theme;

pub use conflict_dialog::{ConflictResolutionDialog, guess_icon_for_path};
pub use progress_dialog::TransferProgressDialog;
pub use queue_popover::{OperationsQueuePopover, QueueBadgeController};
pub use theme::{OPERATIONS_CSS, install_operations_theme};
