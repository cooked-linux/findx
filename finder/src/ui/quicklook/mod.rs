//! Quick Look instant preview floating window and specialized viewports.

pub mod carousel;
pub mod prefs;
pub mod theme;
pub mod viewports;
pub mod window;

pub use carousel::{IndexSheetPopover, QuickLookCarousel};
pub use prefs::QuickLookConfig;
pub use theme::{QUICKLOOK_CSS, install_quicklook_theme};
pub use viewports::{FallbackViewport, ImageViewport, MediaViewport, PdfViewport, TextViewport};
pub use window::QuickLookWindow;
