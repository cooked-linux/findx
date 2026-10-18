//! Quick Look domain data models and payload types.

use std::path::PathBuf;
use std::time::SystemTime;

/// Classification of preview content payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// Raster or vector image preview.
    Image,
    /// Syntax-highlighted source code or plain text.
    SourceCode,
    /// Rendered PDF document page.
    Pdf,
    /// Video or audio stream preview with metadata.
    Media,
    /// Generic fallback card displaying file attributes and themed icon.
    FallbackCard,
}

/// Preview data for high-resolution images.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePreviewData {
    /// Filesystem path to the image or normalized preview image.
    pub image_path: PathBuf,
    /// Pixel width of the image.
    pub width: u32,
    /// Pixel height of the image.
    pub height: u32,
    /// Detected MIME type string.
    pub mime_type: String,
}

/// Preview data for source code and formatted text documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodePreviewData {
    /// Rendered HTML or Pango markup with syntax coloring tags.
    pub highlighted_html: String,
    /// Raw unformatted lines of source text up to line limit.
    pub raw_lines: Vec<String>,
    /// Detected programming language or syntax definition name.
    pub language: String,
    /// Total line count of the source file.
    pub total_lines: usize,
    /// Indicates whether the content was truncated due to size or line count limits.
    pub is_truncated: bool,
}

/// Preview data for PDF documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPreviewData {
    /// Total number of pages in the PDF document.
    pub page_count: usize,
    /// Currently rendered 1-indexed page number.
    pub current_page: usize,
    /// Filesystem path to the rendered page raster image (PNG).
    pub rendered_page_path: PathBuf,
    /// Optional document title extracted from PDF metadata.
    pub title: Option<String>,
}

/// Preview data for audio and video media files.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaPreviewData {
    /// Total duration of the media in seconds.
    pub duration_secs: f64,
    /// Video width in pixels, if video stream is present.
    pub width: Option<u32>,
    /// Video height in pixels, if video stream is present.
    pub height: Option<u32>,
    /// Codec name (e.g. "h264", "aac", "vp9").
    pub codec: Option<String>,
    /// Audio sample rate in Hz (e.g. 44100, 48000).
    pub sample_rate: Option<u32>,
    /// True if video stream exists, false for audio-only streams.
    pub is_video: bool,
    /// Filesystem path to a generated poster frame PNG, if video.
    pub poster_image: Option<PathBuf>,
}

/// Preview data for unparsable or unsupported file formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackCardData {
    /// Target file path.
    pub path: PathBuf,
    /// File name including extension.
    pub file_name: String,
    /// Size in bytes.
    pub file_size: u64,
    /// MIME type string.
    pub mime_type: String,
    /// Last modified system timestamp.
    pub modified: Option<SystemTime>,
    /// Standard Freedesktop themed icon name.
    pub icon_name: String,
}

/// Polymorphic preview payload returned by generators and the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewPayload {
    /// Image preview.
    Image(ImagePreviewData),
    /// Syntax-highlighted code preview.
    SourceCode(CodePreviewData),
    /// PDF document preview.
    Pdf(PdfPreviewData),
    /// Media file preview.
    Media(MediaPreviewData),
    /// Fallback metadata card.
    FallbackCard(FallbackCardData),
}

impl PreviewPayload {
    /// Returns the discriminator kind for this payload.
    #[must_use]
    pub const fn kind(&self) -> PreviewKind {
        match self {
            Self::Image(_) => PreviewKind::Image,
            Self::SourceCode(_) => PreviewKind::SourceCode,
            Self::Pdf(_) => PreviewKind::Pdf,
            Self::Media(_) => PreviewKind::Media,
            Self::FallbackCard(_) => PreviewKind::FallbackCard,
        }
    }
}

/// Inbound request descriptor passed to the Quick Look engine and generators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRequest {
    /// Monotonically increasing request identifier for cancellation tracking.
    pub request_id: u64,
    /// Filesystem path of the file to preview.
    pub path: PathBuf,
    /// Detected or provided MIME type.
    pub mime_type: String,
    /// Preferred bounding box dimensions `(width, height)`.
    pub preferred_size: (u32, u32),
}
