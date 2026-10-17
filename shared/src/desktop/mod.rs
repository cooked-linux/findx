//! XDG Desktop Application Indexer engine.
//!
//! Indexes `.desktop` files across user, Flatpak, Snap, and system directories
//! according to the Freedesktop Desktop Entry Specification v1.5, enforces
//! hierarchical directory precedence and desktop ID masking, resolves themed
//! icons to absolute paths, and scores search queries.

pub mod entry;
pub mod icon;
pub mod matcher;
pub mod scanner;

pub use entry::{DesktopApp, get_system_locale, sanitize_exec, split_semicolon_list};
pub use icon::{
    clear_icon_cache, default_icon_dirs, get_active_theme_name, resolve_icon_path,
    resolve_icon_path_with_dirs,
};
pub use matcher::calculate_text_score;
pub use scanner::{
    clear_app_cache, default_application_dirs, is_visible_in_desktop, scan_application_dirs,
    scan_installed_apps,
};

/// Domain errors encountered during desktop entry indexing and parsing.
#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    /// I/O error reading desktop entries from disk.
    #[error("I/O error reading desktop entries: {0}")]
    Io(#[from] std::io::Error),
    /// Malformed or invalid desktop entry file.
    #[error("Invalid desktop entry format at {0}: {1}")]
    InvalidFormat(std::path::PathBuf, String),
    /// Desktop entry was marked hidden or filtered by desktop environment rules.
    #[error("Desktop entry marked hidden or not visible in current desktop environment")]
    Filtered,
}
