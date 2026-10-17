//! MIME type sniffing, spoofing detection, and desktop associations engine.
//!
//! Provides two-stage MIME resolution (extension heuristic and magic byte inspection),
//! masquerading/spoofing protection against executable content, XDG `mimeapps.list`
//! parsing and atomic modification, and "Open With" candidate aggregation.

pub mod apps;
pub mod dispatch;
pub mod sniff;

pub use apps::{
    MimeAppsConfig, atomic_write_file, default_mimeapps_paths, get_default_app_for_mime,
    get_default_app_for_mime_with_paths, set_default_app, set_default_app_in_file,
    user_mimeapps_path,
};
pub use dispatch::{
    AppHandlerInfo, get_open_with_candidates, get_open_with_candidates_for_mime,
    get_open_with_candidates_from_apps,
};
pub use sniff::{
    detect_mime_from_bytes, detect_mime_type, detect_mime_with_spoof_check, sniff_magic,
};

/// Domain errors encountered during MIME sniffing, configuration parsing, and routing.
#[derive(Debug, thiserror::Error)]
pub enum MimeError {
    /// Filesystem I/O error during MIME detection or configuration read/write.
    #[error("I/O error during MIME operation: {0}")]
    Io(#[from] std::io::Error),

    /// Syntax error in `mimeapps.list` configuration file.
    #[error("Invalid mimeapps.list syntax at line {0}: {1}")]
    InvalidConfig(usize, String),

    /// Executable content detected masquerading as a safe document or media type.
    #[error("MIME spoofing detected for {0}: expected {1}, but magic bytes indicate {2}")]
    SpoofingDetected(std::path::PathBuf, String, String),
}
