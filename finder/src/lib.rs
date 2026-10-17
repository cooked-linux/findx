//! Finder desktop file manager application and library crate.

pub mod bookmarks;
pub mod error;
pub mod history;
pub mod sidebar;
pub mod ui;

pub use bookmarks::{BookmarkEntry, default_places, load_user_bookmarks, save_user_bookmarks};
pub use error::{FinderError, Result};
