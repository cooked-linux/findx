//! Asynchronous recursive folder size calculation engine and two-tier cache.

pub mod cache;
pub mod db;
pub mod manager;
pub mod models;
pub mod scanner;
pub mod watcher;

pub use cache::FolderSizeCache;
pub use manager::{DEFAULT_SCANNER_CONCURRENCY, FolderSizeManager};
pub use models::{FolderSizeError, FolderSizeResult, Priority};
pub use scanner::calculate_folder_size_sync;
pub use watcher::{DEFAULT_COOLDOWN_DURATION, FolderSizeWatcher, extract_affected_directories};
