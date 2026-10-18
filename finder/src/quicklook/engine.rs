//! Asynchronous request arbiter and monotonic cancellation engine for Quick Look.
//!
//! Orchestrates pluggable preview generators, Freedesktop thumbnail caching, and fast-path
//! resolution while enforcing monotonic request cancellation to ensure sub-100ms response
//! times without lag or UI blocking during rapid keyboard navigation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use shared::thumbnail::{ThumbnailCache, ThumbnailSize};

use crate::quicklook::error::{QuickLookError, Result};
use crate::quicklook::generator::{
    FallbackGenerator, ImageGenerator, MediaGenerator, PdfGenerator, PreviewGenerator,
    SourceCodeGenerator,
};
use crate::quicklook::types::{PreviewPayload, PreviewRequest};

/// Headless Quick Look preview generation engine.
pub struct QuickLookEngine {
    generators: Vec<Box<dyn PreviewGenerator>>,
    active_request_id: Arc<AtomicU64>,
    thumbnail_cache: Arc<ThumbnailCache>,
}

impl Default for QuickLookEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl QuickLookEngine {
    /// Creates a new [`QuickLookEngine`] configured with standard pluggable generators
    /// and default Freedesktop thumbnail cache.
    #[must_use]
    pub fn new() -> Self {
        let generators: Vec<Box<dyn PreviewGenerator>> = vec![
            Box::new(ImageGenerator),
            Box::new(SourceCodeGenerator),
            Box::new(PdfGenerator),
            Box::new(MediaGenerator),
            Box::new(FallbackGenerator),
        ];

        Self {
            generators,
            active_request_id: Arc::new(AtomicU64::new(0)),
            thumbnail_cache: Arc::new(ThumbnailCache::default()),
        }
    }

    /// Creates a [`QuickLookEngine`] with custom generators and thumbnail cache.
    #[must_use]
    pub fn with_generators(
        generators: Vec<Box<dyn PreviewGenerator>>,
        thumbnail_cache: Arc<ThumbnailCache>,
    ) -> Self {
        Self {
            generators,
            active_request_id: Arc::new(AtomicU64::new(0)),
            thumbnail_cache,
        }
    }

    /// Returns the current monotonic request identifier.
    #[must_use]
    pub fn current_request_id(&self) -> u64 {
        self.active_request_id.load(Ordering::SeqCst)
    }

    /// Returns a reference to the underlying [`ThumbnailCache`].
    #[must_use]
    pub const fn thumbnail_cache(&self) -> &Arc<ThumbnailCache> {
        &self.thumbnail_cache
    }

    /// Fast-path resolution (<10ms): checks whether a cached or instantly generated
    /// high-resolution thumbnail exists for the file to paint immediate visual feedback.
    #[must_use]
    pub fn get_fast_path_thumbnail(&self, path: &Path, mime_type: Option<&str>) -> Option<PathBuf> {
        let mime = match mime_type {
            Some(m) => m.to_string(),
            None => shared::mime::detect_mime_type(path).ok()?,
        };

        let cat = shared::models::MimeCategory::from_mime_str(&mime);
        if cat == shared::models::MimeCategory::Image
            || cat == shared::models::MimeCategory::Document
        {
            self.thumbnail_cache
                .get_or_generate(path, ThumbnailSize::Large, Some(&mime))
                .ok()
        } else {
            None
        }
    }

    /// Asynchronously generates a full preview payload for `path` with monotonic cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`QuickLookError::Cancelled`] if a newer request superseded this operation.
    /// Returns [`QuickLookError::Io`] if the target file cannot be found or read.
    pub async fn load_preview(
        &self,
        path: &Path,
        preferred_size: (u32, u32),
    ) -> Result<PreviewPayload> {
        let request_id = self.active_request_id.fetch_add(1, Ordering::SeqCst) + 1;

        if !path.exists() {
            return Err(QuickLookError::io(
                path,
                std::io::Error::from(std::io::ErrorKind::NotFound),
            ));
        }

        let mime_type = shared::mime::detect_mime_type(path)
            .unwrap_or_else(|_| "application/octet-stream".to_string());

        // Early cancellation check before generator lookup and execution
        if self.active_request_id.load(Ordering::SeqCst) != request_id {
            return Err(QuickLookError::Cancelled);
        }

        let req = PreviewRequest {
            request_id,
            path: path.to_path_buf(),
            mime_type,
            preferred_size,
        };

        let generator = self
            .generators
            .iter()
            .find(|g| g.can_handle(&req.mime_type, &req.path));

        let res = match generator {
            Some(generator) => generator.generate(&req).await,
            None => FallbackGenerator.generate(&req).await,
        };

        // Fail-safe resilience: catch sandbox or timeout errors and return fallback metadata card
        let payload = match res {
            Ok(p) => p,
            Err(QuickLookError::Sandbox { ref message, .. }) => {
                tracing::warn!(
                    "Quick Look sandbox execution failed for '{}': {message}; falling back to metadata card",
                    req.path.display()
                );
                FallbackGenerator.generate(&req).await?
            }
            Err(QuickLookError::Timeout { timeout_ms, .. }) => {
                tracing::warn!(
                    "Quick Look preview timed out after {timeout_ms}ms for '{}'; falling back to metadata card",
                    req.path.display()
                );
                FallbackGenerator.generate(&req).await?
            }
            Err(QuickLookError::Cancelled) => return Err(QuickLookError::Cancelled),
            Err(e) => {
                tracing::warn!(
                    "Quick Look preview generation encountered error: {e}; falling back to metadata card"
                );
                FallbackGenerator.generate(&req).await?
            }
        };

        // Monotonic cancellation check after generator completion
        if self.active_request_id.load(Ordering::SeqCst) != request_id {
            return Err(QuickLookError::Cancelled);
        }

        Ok(payload)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    struct SlowGenerator {
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl PreviewGenerator for SlowGenerator {
        fn can_handle(&self, _mime: &str, _path: &Path) -> bool {
            true
        }

        async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload> {
            tokio::time::sleep(self.delay).await;
            FallbackGenerator.generate(req).await
        }
    }

    #[tokio::test]
    async fn test_engine_single_request_success() {
        let mut temp_file = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(temp_file, "pub fn add(a: i32, b: i32) -> i32 {{ a + b }}").unwrap();

        let engine = QuickLookEngine::new();
        let payload = engine
            .load_preview(temp_file.path(), (800, 600))
            .await
            .unwrap();

        match payload {
            PreviewPayload::SourceCode(code) => {
                assert_eq!(code.language, "Rust");
                assert_eq!(code.total_lines, 1);
            }
            other => panic!("Expected SourceCode payload, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_engine_monotonic_cancellation_10_rapid_requests() {
        // Use a SlowGenerator with 40ms latency so that rapid concurrent requests overlap
        let slow_gen = Box::new(SlowGenerator {
            delay: Duration::from_millis(40),
        });
        let engine = Arc::new(QuickLookEngine::with_generators(
            vec![slow_gen],
            Arc::new(ThumbnailCache::default()),
        ));

        let mut files = Vec::new();
        for i in 0..10 {
            let mut file = NamedTempFile::with_suffix(".txt").unwrap();
            writeln!(file, "Rapid test file {i}").unwrap();
            files.push(file);
        }

        let mut handles = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let eng = Arc::clone(&engine);
            let path = file.path().to_path_buf();
            handles.push(tokio::spawn(async move {
                // Stagger dispatch slightly by 2ms to ensure request order 0..9
                tokio::time::sleep(Duration::from_millis(i as u64 * 2)).await;
                eng.load_preview(&path, (800, 600)).await
            }));
        }

        let mut results = Vec::new();
        for h in handles {
            let res = h.await.unwrap();
            results.push(res);
        }

        assert_eq!(results.len(), 10);

        // Preceding requests (0 through 8) must be cancelled
        for (idx, res) in results[..9].iter().enumerate() {
            assert!(
                matches!(res, Err(QuickLookError::Cancelled)),
                "Expected request {idx} to be Cancelled, got {res:?}"
            );
        }

        // The 10th request (index 9) must succeed
        assert!(
            results[9].is_ok(),
            "Expected final request (index 9) to succeed, got {:?}",
            results[9]
        );
    }

    #[tokio::test]
    async fn test_engine_fast_path_thumbnail() {
        let temp_file = NamedTempFile::with_suffix(".png").unwrap();
        let img = image::RgbaImage::new(128, 128);
        img.save(temp_file.path()).unwrap();

        let engine = QuickLookEngine::new();
        let thumb_path = engine.get_fast_path_thumbnail(temp_file.path(), Some("image/png"));
        assert!(
            thumb_path.is_some(),
            "Fast path thumbnail should be generated for images"
        );
        let path = thumb_path.unwrap();
        assert!(path.is_file());
    }

    #[tokio::test]
    async fn test_engine_failsafe_fallback_on_error() {
        struct FailingGenerator;

        #[async_trait::async_trait]
        impl PreviewGenerator for FailingGenerator {
            fn can_handle(&self, _mime: &str, _path: &Path) -> bool {
                true
            }

            async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload> {
                Err(QuickLookError::sandbox(
                    &req.path,
                    "Simulated container crash",
                ))
            }
        }

        let temp_file = NamedTempFile::with_suffix(".bin").unwrap();
        let engine = QuickLookEngine::with_generators(
            vec![Box::new(FailingGenerator)],
            Arc::new(ThumbnailCache::default()),
        );

        let res = engine.load_preview(temp_file.path(), (800, 600)).await;
        assert!(res.is_ok(), "Engine should fail-safe to FallbackCard");
        let payload = res.unwrap();
        assert!(matches!(payload, PreviewPayload::FallbackCard(_)));
    }
}
