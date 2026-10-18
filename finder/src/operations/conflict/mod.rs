//! Collision detection and interactive conflict resolution engine.

pub mod detector;
pub mod models;
pub mod resolver;
pub mod staging;

pub use detector::check_collision;
pub use models::{
    ConflictAction, ConflictContext, ConflictItemMeta, ConflictResolution, RelativeTimeStatus,
};
pub use resolver::{ConflictArbiter, ConflictOutcome, ConflictPrompt};
pub use staging::StagedRollback;
