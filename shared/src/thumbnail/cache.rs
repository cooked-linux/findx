//! Multi-tier thumbnail cache manager and validation logic.

use std::path::{Path, PathBuf};

use super::ThumbnailError;
use super::generator::generate_thumbnail_image;
use super::uri::{canonical_file_uri, compute_thumbnail_hash};
use super::writer::{
    ThumbnailMetadata, read_thumbnail_metadata, write_failed_thumbnail, write_thumbnail_png,
};

/// Thumbnail size bucket conforming to Freedesktop Thumbnail Management Standard v0.8.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailSize {
    /// 128x128 standard thumbnail
    Normal,
    /// 256x256 high-res thumbnail
    Large,
    /// 512x512 extra large thumbnail
    XLarge,
}

impl ThumbnailSize {
    /// Maximum pixel dimension (bounding box width and height) for this size bucket.
    #[must_use]
    pub const fn pixel_dimension(self) -> u32 {
        match self {
            Self::Normal => 128,
            Self::Large => 256,
            Self::XLarge => 512,
        }
    }

    /// Subdirectory name under `$XDG_CACHE_HOME/thumbnails/`.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "large",
            Self::XLarge => "x-large",
        }
    }
}

/// Freedesktop multi-tier thumbnail cache manager.
#[derive(Debug, Clone, Default)]
pub struct ThumbnailCache {
    base_dir: Option<PathBuf>,
}

impl ThumbnailCache {
    /// Creates a new default [`ThumbnailCache`] resolving paths against `$XDG_CACHE_HOME/thumbnails`.
    #[must_use]
    pub const fn new() -> Self {
        Self { base_dir: None }
    }

    /// Creates a [`ThumbnailCache`] with a specific root cache directory (useful for isolated tests).
    #[must_use]
    pub const fn with_cache_dir(base_dir: PathBuf) -> Self {
        Self {
            base_dir: Some(base_dir),
        }
    }

    /// Resolves the base directory for thumbnails.
    ///
    /// # Errors
    ///
    /// Returns [`ThumbnailError::Io`] if base cache directory resolution fails.
    pub fn base_thumbnails_dir(&self) -> Result<PathBuf, ThumbnailError> {
        if let Some(ref dir) = self.base_dir {
            return Ok(dir.clone());
        }

        if let Ok(val) = std::env::var("XDG_CACHE_HOME") {
            let path = PathBuf::from(val);
            if !path.as_os_str().is_empty() {
                return Ok(path.join("thumbnails"));
            }
        }

        let xdg_dirs =
            xdg::BaseDirectories::new().map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(xdg_dirs.get_cache_home().join("thumbnails"))
    }

    /// Resolves the directory for a specific size bucket, e.g. `<base>/normal`.
    ///
    /// # Errors
    ///
    /// Returns [`ThumbnailError::Io`] if directory resolution fails.
    pub fn bucket_dir(&self, size: ThumbnailSize) -> Result<PathBuf, ThumbnailError> {
        let base = self.base_thumbnails_dir()?;
        Ok(base.join(size.dir_name()))
    }

    /// Resolves the failure cache directory, e.g. `<base>/fail`.
    ///
    /// # Errors
    ///
    /// Returns [`ThumbnailError::Io`] if directory resolution fails.
    pub fn fail_dir(&self) -> Result<PathBuf, ThumbnailError> {
        let base = self.base_thumbnails_dir()?;
        Ok(base.join("fail"))
    }

    /// Checks whether a valid cached thumbnail exists for the given MD5 hash, size, and source `mtime`.
    ///
    /// # Errors
    ///
    /// Returns [`ThumbnailError::CacheMiss`] if the thumbnail does not exist or `mtime` does not match.
    /// Returns [`ThumbnailError::Io`] if cache directory resolution or reading fails.
    pub fn validate_cached_thumbnail(
        &self,
        hash: &str,
        size: ThumbnailSize,
        source_mtime: i64,
    ) -> Result<PathBuf, ThumbnailError> {
        let bucket = self.bucket_dir(size)?;
        let cached_file = bucket.join(format!("{hash}.png"));

        if !cached_file.is_file() {
            return Err(ThumbnailError::CacheMiss);
        }

        let Ok(metadata) = read_thumbnail_metadata(&cached_file) else {
            return Err(ThumbnailError::CacheMiss);
        };

        if metadata.mtime == Some(source_mtime) {
            Ok(cached_file)
        } else {
            Err(ThumbnailError::CacheMiss)
        }
    }

    /// Checks if a failure marker exists for this file and matching `mtime`.
    ///
    /// # Errors
    ///
    /// Returns [`ThumbnailError::Io`] if fail directory resolution fails.
    pub fn is_marked_failed(&self, hash: &str, source_mtime: i64) -> Result<bool, ThumbnailError> {
        let fail_dir = self.fail_dir()?;
        let fail_file = fail_dir.join(format!("{hash}.png"));

        if !fail_file.is_file() {
            return Ok(false);
        }

        let Ok(metadata) = read_thumbnail_metadata(&fail_file) else {
            return Ok(false);
        };

        Ok(metadata.mtime == Some(source_mtime))
    }

    /// Returns a cached thumbnail if valid, or generates and stores one in the cache.
    ///
    /// # Errors
    ///
    /// Returns [`ThumbnailError`] if:
    /// - Source file metadata cannot be read.
    /// - The file is known to have failed thumbnail generation.
    /// - Generation or writing fails.
    pub fn get_or_generate(
        &self,
        file_path: &Path,
        size: ThumbnailSize,
        mime_type: Option<&str>,
    ) -> Result<PathBuf, ThumbnailError> {
        let file_meta = std::fs::metadata(file_path)?;
        let source_mtime = get_mtime_secs(&file_meta);
        let source_size = file_meta.len();

        let uri = canonical_file_uri(file_path)?;
        let hash = compute_thumbnail_hash(&uri);

        // 1. Check if cached thumbnail exists and is valid
        if let Ok(cached_path) = self.validate_cached_thumbnail(&hash, size, source_mtime) {
            return Ok(cached_path);
        }

        // 2. Check if the file is recorded as failed for this mtime
        if self.is_marked_failed(&hash, source_mtime)? {
            return Err(ThumbnailError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "thumbnail generation previously failed for this file",
            )));
        }

        // 3. Resolve target paths
        let target_dir = self.bucket_dir(size)?;
        let target_file = target_dir.join(format!("{hash}.png"));

        let detected_mime = mime_type.map_or_else(
            || {
                mime_guess::from_path(file_path)
                    .first_raw()
                    .unwrap_or("application/octet-stream")
                    .to_string()
            },
            ToString::to_string,
        );

        // 4. Generate thumbnail image
        let thumbnail_res = generate_thumbnail_image(file_path, size.pixel_dimension());

        match thumbnail_res {
            Ok(rgba_img) => {
                let meta = ThumbnailMetadata {
                    uri: &uri,
                    mtime: source_mtime,
                    size: source_size,
                    mime_type: &detected_mime,
                };
                write_thumbnail_png(&target_dir, &target_file, &rgba_img, &meta)?;
                Ok(target_file)
            }
            Err(err) => {
                let fail_dir = self.fail_dir()?;
                let fail_file = fail_dir.join(format!("{hash}.png"));
                let meta = ThumbnailMetadata {
                    uri: &uri,
                    mtime: source_mtime,
                    size: source_size,
                    mime_type: &detected_mime,
                };
                let _ = write_failed_thumbnail(&fail_dir, &fail_file, &meta);
                Err(err)
            }
        }
    }
}

/// Helper to extract seconds since epoch from metadata on unix.
#[cfg(unix)]
fn get_mtime_secs(meta: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.mtime()
}

/// Fallback for non-unix platforms.
#[cfg(not(unix))]
fn get_mtime_secs(meta: &std::fs::Metadata) -> i64 {
    let Ok(mtime) = meta.modified() else { return 0 };
    let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) else {
        return 0;
    };
    i64::try_from(dur.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thumbnail_size_properties() {
        assert_eq!(ThumbnailSize::Normal.pixel_dimension(), 128);
        assert_eq!(ThumbnailSize::Normal.dir_name(), "normal");

        assert_eq!(ThumbnailSize::Large.pixel_dimension(), 256);
        assert_eq!(ThumbnailSize::Large.dir_name(), "large");

        assert_eq!(ThumbnailSize::XLarge.pixel_dimension(), 512);
        assert_eq!(ThumbnailSize::XLarge.dir_name(), "x-large");
    }

    #[test]
    fn test_custom_cache_directory_resolution() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let cache = ThumbnailCache::with_cache_dir(temp.path().to_path_buf());

        assert_eq!(cache.base_thumbnails_dir()?, temp.path());
        assert_eq!(
            cache.bucket_dir(ThumbnailSize::Normal)?,
            temp.path().join("normal")
        );
        assert_eq!(
            cache.bucket_dir(ThumbnailSize::Large)?,
            temp.path().join("large")
        );
        assert_eq!(
            cache.bucket_dir(ThumbnailSize::XLarge)?,
            temp.path().join("x-large")
        );
        assert_eq!(cache.fail_dir()?, temp.path().join("fail"));
        Ok(())
    }
}
