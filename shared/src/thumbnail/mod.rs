//! Freedesktop.org thumbnail cache pipeline.
//!
//! Provides canonical URI hashing, multi-tier size bucket resolution, cache validity
//! verification against source file `mtime`, EXIF orientation normalization, image scaling,
//! and atomic PNG writing with standard `tEXt` chunks according to the
//! Freedesktop Thumbnail Management Standard v0.8.0.

pub mod cache;
pub mod generator;
pub mod uri;
pub mod writer;

pub use cache::{ThumbnailCache, ThumbnailSize};
pub use generator::{apply_exif_orientation, generate_thumbnail_image};
pub use uri::{canonical_file_uri, compute_thumbnail_hash};
pub use writer::{
    ThumbnailMetadata, ThumbnailMetadataRecord, read_thumbnail_metadata, write_failed_thumbnail,
    write_thumbnail_png,
};

use std::path::{Path, PathBuf};

/// Domain error variants for thumbnail operations.
#[derive(Debug, thiserror::Error)]
pub enum ThumbnailError {
    /// Filesystem or I/O failure during thumbnail generation or caching.
    #[error("I/O error during thumbnail operation: {0}")]
    Io(#[from] std::io::Error),

    /// Image decoding, processing, or encoding failure.
    #[error("Image decoding or processing error: {0}")]
    Image(#[from] image::ImageError),

    /// Invalid or un-parseable canonical file URI.
    #[error("Invalid canonical file URI: {0}")]
    InvalidUri(String),

    /// Cache entry is missing, expired, or invalid.
    #[error("Thumbnail cache entry is invalid or expired")]
    CacheMiss,
}

/// Convenience alias for thumbnail results.
pub type Result<T> = std::result::Result<T, ThumbnailError>;

/// High-level entry point to retrieve a cached thumbnail or generate and cache one.
///
/// # Errors
///
/// Returns a [`ThumbnailError`] if:
/// - Source file metadata cannot be read.
/// - Canonical URI cannot be constructed.
/// - Thumbnail decoding, scaling, or atomic writing fails.
pub fn get_or_generate_thumbnail(
    file_path: &Path,
    size: ThumbnailSize,
    mime_type: Option<&str>,
) -> Result<PathBuf> {
    let cache = ThumbnailCache::default();
    cache.get_or_generate(file_path, size, mime_type)
}
