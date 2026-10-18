//! Specialized viewport widgets for Quick Look instant previews.

pub mod code_view;
pub mod fallback_view;
pub mod image_view;
pub mod media_view;
pub mod pdf_view;

pub use code_view::TextViewport;
pub use fallback_view::FallbackViewport;
pub use image_view::ImageViewport;
pub use media_view::{MediaViewport, format_media_duration};
pub use pdf_view::{PageChangeCallback, PdfViewport};
