//! Pluggable preview generators for images, source code, PDFs, media files, and fallback metadata cards.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use image::ImageDecoder;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::quicklook::error::{QuickLookError, Result};
use crate::quicklook::sandbox::{SandboxConfig, run_sandboxed, run_sandboxed_with_rw};
use crate::quicklook::types::{
    CodePreviewData, FallbackCardData, ImagePreviewData, MediaPreviewData, PdfPreviewData,
    PreviewPayload, PreviewRequest,
};

/// Maximum file size read for source code previews (2 MB).
pub const MAX_CODE_PREVIEW_SIZE: u64 = 2 * 1024 * 1024;
/// Maximum number of lines included in source code previews.
pub const MAX_CODE_LINES: usize = 500;
/// Truncation notice appended to highlighted HTML when line/size thresholds are exceeded.
pub const TRUNCATION_NOTICE: &str = "<p style=\"color:#888888; font-style:italic;\">--- Truncated: File exceeds preview limit ---</p>";

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn get_syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn get_theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn resolve_cache_base() -> PathBuf {
    if let Some(val) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(val).join("findx").join("quicklook");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".cache")
            .join("findx")
            .join("quicklook");
    }
    std::env::temp_dir().join("findx_quicklook")
}

/// Resolves or creates the local cache directory for Quick Look generated assets.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the directory cannot be created.
pub fn quicklook_cache_dir() -> std::io::Result<PathBuf> {
    let dir = resolve_cache_base();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

/// Asynchronous generator interface for format-specific preview generation.
#[async_trait]
pub trait PreviewGenerator: Send + Sync {
    /// Determines whether this generator is capable of producing a preview for the specified MIME type or path.
    fn can_handle(&self, mime: &str, path: &Path) -> bool;

    /// Produces a preview payload for the provided request.
    ///
    /// # Errors
    ///
    /// Returns [`QuickLookError`] if decoding, sandboxed rendering, or file reading fails.
    async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload>;
}

// ---------------------------------------------------------------------------
// 1. ImageGenerator
// ---------------------------------------------------------------------------

/// Generator for raster and vector image formats.
#[derive(Debug, Default, Clone, Copy)]
pub struct ImageGenerator;

#[async_trait]
impl PreviewGenerator for ImageGenerator {
    fn can_handle(&self, mime: &str, path: &Path) -> bool {
        if mime.starts_with("image/") {
            return true;
        }
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "webp"
                        | "gif"
                        | "bmp"
                        | "ico"
                        | "svg"
                        | "tiff"
                        | "tif"
                )
            })
    }

    async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload> {
        // Special-case SVG vector files
        if req.mime_type == "image/svg+xml"
            || req
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            let (w, h) = if req.preferred_size.0 > 0 && req.preferred_size.1 > 0 {
                req.preferred_size
            } else {
                (800, 600)
            };
            return Ok(PreviewPayload::Image(ImagePreviewData {
                image_path: req.path.clone(),
                width: w,
                height: h,
                mime_type: "image/svg+xml".to_string(),
            }));
        }

        let file = File::open(&req.path).map_err(|e| QuickLookError::io(&req.path, e))?;
        let buf_reader = BufReader::new(file);

        let mut reader = image::ImageReader::new(buf_reader)
            .with_guessed_format()
            .map_err(|e| QuickLookError::io(&req.path, e))?;

        let mut limits = image::Limits::default();
        limits.max_alloc = Some(256 * 1024 * 1024);
        limits.max_image_width = Some(16_384);
        limits.max_image_height = Some(16_384);
        reader.limits(limits);

        let mut decoder = reader
            .into_decoder()
            .map_err(|e| QuickLookError::decoding_failed(&req.path, e.to_string()))?;

        let orientation = decoder
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let (raw_w, raw_h) = decoder.dimensions();

        // Swap dimensions if orientation rotates 90 or 270 degrees
        let (width, height) = match orientation {
            image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH => (raw_h, raw_w),
            _ => (raw_w, raw_h),
        };

        Ok(PreviewPayload::Image(ImagePreviewData {
            image_path: req.path.clone(),
            width,
            height,
            mime_type: req.mime_type.clone(),
        }))
    }
}

// ---------------------------------------------------------------------------
// 2. SourceCodeGenerator
// ---------------------------------------------------------------------------

/// Generator for syntax-highlighted source code, markup, and plain-text files.
#[derive(Debug, Default, Clone, Copy)]
pub struct SourceCodeGenerator;

#[async_trait]
impl PreviewGenerator for SourceCodeGenerator {
    fn can_handle(&self, mime: &str, path: &Path) -> bool {
        if mime.starts_with("text/") {
            return true;
        }
        if shared::models::MimeCategory::from_mime_str(mime)
            == shared::models::MimeCategory::SourceCode
        {
            return true;
        }

        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "rs" | "toml"
                        | "json"
                        | "yaml"
                        | "yml"
                        | "py"
                        | "js"
                        | "ts"
                        | "jsx"
                        | "tsx"
                        | "c"
                        | "cpp"
                        | "h"
                        | "hpp"
                        | "sh"
                        | "bash"
                        | "zsh"
                        | "md"
                        | "html"
                        | "css"
                        | "xml"
                        | "sql"
                        | "go"
                        | "java"
                        | "lua"
                        | "ini"
                        | "conf"
                        | "log"
                        | "txt"
                )
            })
    }

    async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload> {
        let meta = std::fs::metadata(&req.path).map_err(|e| QuickLookError::io(&req.path, e))?;
        let file_len = meta.len();

        let file = File::open(&req.path).map_err(|e| QuickLookError::io(&req.path, e))?;
        let reader = BufReader::new(file.take(MAX_CODE_PREVIEW_SIZE + 1));

        let mut raw_lines = Vec::new();
        let mut total_lines = 0;
        let mut is_truncated = file_len > MAX_CODE_PREVIEW_SIZE;

        for line_res in reader.lines() {
            let line = line_res.map_err(|e| QuickLookError::io(&req.path, e))?;
            total_lines += 1;
            if raw_lines.len() < MAX_CODE_LINES {
                raw_lines.push(line);
            } else {
                is_truncated = true;
            }
        }

        let syntax_set = get_syntax_set();
        let theme_set = get_theme_set();

        let ext = req
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let file_name = req
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        let syntax = syntax_set
            .find_syntax_by_extension(ext)
            .or_else(|| syntax_set.find_syntax_by_token(file_name))
            .unwrap_or_else(|| syntax_set.find_syntax_plain_text());

        let language = syntax.name.clone();

        let theme = theme_set
            .themes
            .get("base16-ocean.dark")
            .or_else(|| theme_set.themes.get("InspiredGitHub"))
            .or_else(|| theme_set.themes.values().next())
            .ok_or_else(|| {
                QuickLookError::decoding_failed(&req.path, "No syntax highlighting theme found")
            })?;

        let mut text_to_highlight = raw_lines.join("\n");
        if !text_to_highlight.is_empty() {
            text_to_highlight.push('\n');
        }

        let mut highlighted_html = syntect::html::highlighted_html_for_string(
            &text_to_highlight,
            syntax_set,
            syntax,
            theme,
        )
        .map_err(|e| QuickLookError::decoding_failed(&req.path, e.to_string()))?;

        if is_truncated {
            highlighted_html.push_str(TRUNCATION_NOTICE);
        }

        Ok(PreviewPayload::SourceCode(CodePreviewData {
            highlighted_html,
            raw_lines,
            language,
            total_lines,
            is_truncated,
        }))
    }
}

// ---------------------------------------------------------------------------
// 3. PdfGenerator
// ---------------------------------------------------------------------------

/// Generator for multi-page PDF documents executing inside a Bubblewrap container.
#[derive(Debug, Default, Clone, Copy)]
pub struct PdfGenerator;

fn find_rendered_pdf_page(cache_dir: &Path, hash: &str, mtime: u64) -> Option<PathBuf> {
    let target_png = cache_dir.join(format!("{hash}_{mtime}_page-1.png"));
    if target_png.is_file() {
        return Some(target_png);
    }
    let target_png_alt = cache_dir.join(format!("{hash}_{mtime}_page.png"));
    if target_png_alt.is_file() {
        return Some(target_png_alt);
    }

    let prefix = format!("{hash}_{mtime}_page");
    if let Ok(entries) = std::fs::read_dir(cache_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_match = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));

            if is_match {
                return Some(path);
            }
        }
    }
    None
}

async fn query_pdf_info(path: &Path, path_str: &str) -> (usize, Option<String>) {
    let mut page_count = 1;
    let mut title = None;

    let info_config = SandboxConfig::new(path).with_timeout(Duration::from_millis(1500));
    if let Ok(info_out) = run_sandboxed("pdfinfo", &[path_str], &info_config).await {
        let stdout = String::from_utf8_lossy(&info_out.stdout);
        for line in stdout.lines() {
            if let Some(val) = line.strip_prefix("Pages:") {
                if let Ok(count) = val.trim().parse::<usize>() {
                    page_count = count;
                }
            } else if let Some(val) = line.strip_prefix("Title:") {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    title = Some(trimmed.to_string());
                }
            }
        }
    }
    (page_count, title)
}

#[async_trait]
impl PreviewGenerator for PdfGenerator {
    fn can_handle(&self, mime: &str, path: &Path) -> bool {
        mime == "application/pdf"
            || path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    }

    async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload> {
        let cache_dir = quicklook_cache_dir().map_err(|e| QuickLookError::io(&req.path, e))?;

        let file_meta =
            std::fs::metadata(&req.path).map_err(|e| QuickLookError::io(&req.path, e))?;
        let mtime = file_meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());

        let uri = shared::thumbnail::canonical_file_uri(&req.path)
            .map_err(|e| QuickLookError::decoding_failed(&req.path, e.to_string()))?;
        let hash = shared::thumbnail::compute_thumbnail_hash(&uri);

        let rendered_path = if let Some(cached) = find_rendered_pdf_page(&cache_dir, &hash, mtime) {
            cached
        } else {
            let output_prefix = cache_dir.join(format!("{hash}_{mtime}_page"));
            let config = SandboxConfig::new(&req.path).with_timeout(Duration::from_millis(2000));
            let path_str = req.path.to_str().ok_or_else(|| {
                QuickLookError::decoding_failed(&req.path, "Invalid path encoding")
            })?;
            let out_str = output_prefix.to_str().ok_or_else(|| {
                QuickLookError::decoding_failed(&req.path, "Invalid path encoding")
            })?;

            let args = ["-png", "-r", "150", "-f", "1", "-l", "1", path_str, out_str];
            let rw_dirs = [&cache_dir as &Path];

            let _ = run_sandboxed_with_rw("pdftoppm", &args, &config, &rw_dirs).await?;

            find_rendered_pdf_page(&cache_dir, &hash, mtime).ok_or_else(|| {
                QuickLookError::decoding_failed(
                    &req.path,
                    "pdftoppm did not produce a page PNG file",
                )
            })?
        };

        let path_str = req
            .path
            .to_str()
            .ok_or_else(|| QuickLookError::decoding_failed(&req.path, "Invalid path encoding"))?;
        let (page_count, title) = query_pdf_info(&req.path, path_str).await;

        Ok(PreviewPayload::Pdf(PdfPreviewData {
            page_count,
            current_page: 1,
            rendered_page_path: rendered_path,
            title,
        }))
    }
}

// ---------------------------------------------------------------------------
// 4. MediaGenerator
// ---------------------------------------------------------------------------

struct ParsedMediaMetadata {
    duration_secs: f64,
    is_video: bool,
    width: Option<u32>,
    height: Option<u32>,
    codec: Option<String>,
    sample_rate: Option<u32>,
}

fn parse_media_metadata(json_val: &serde_json::Value) -> ParsedMediaMetadata {
    let duration_secs = json_val
        .pointer("/format/duration")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    let streams = json_val
        .get("streams")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut is_video = false;
    let mut width = None;
    let mut height = None;
    let mut codec = None;
    let mut sample_rate = None;

    for stream in &streams {
        let codec_type = stream
            .get("codec_type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if codec_type == "video" && !is_video {
            is_video = true;
            width = stream
                .get("width")
                .and_then(serde_json::Value::as_u64)
                .and_then(|w| u32::try_from(w).ok());
            height = stream
                .get("height")
                .and_then(serde_json::Value::as_u64)
                .and_then(|h| u32::try_from(h).ok());
            codec = stream
                .get("codec_name")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string);
        } else if codec_type == "audio" && sample_rate.is_none() {
            sample_rate = stream
                .get("sample_rate")
                .and_then(serde_json::Value::as_str)
                .and_then(|s| s.parse::<u32>().ok());
            if codec.is_none() {
                codec = stream
                    .get("codec_name")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string);
            }
        }
    }

    ParsedMediaMetadata {
        duration_secs,
        is_video,
        width,
        height,
        codec,
        sample_rate,
    }
}

async fn extract_video_poster(req: &PreviewRequest, path_str: &str) -> Option<PathBuf> {
    let cache_dir = quicklook_cache_dir().ok()?;
    let uri = shared::thumbnail::canonical_file_uri(&req.path).ok()?;
    let hash = shared::thumbnail::compute_thumbnail_hash(&uri);
    let poster_path = cache_dir.join(format!("{hash}_poster.png"));

    if poster_path.is_file() {
        return Some(poster_path);
    }

    let poster_str = poster_path.to_str()?;
    let ffmpeg_config = SandboxConfig::new(&req.path).with_timeout(Duration::from_millis(2000));
    let rw_dirs = [&cache_dir as &Path];
    let ffmpeg_args = [
        "-ss",
        "00:00:01.000",
        "-i",
        path_str,
        "-frames:v",
        "1",
        "-q:v",
        "2",
        "-y",
        poster_str,
    ];

    if run_sandboxed_with_rw("ffmpeg", &ffmpeg_args, &ffmpeg_config, &rw_dirs)
        .await
        .is_ok()
        && poster_path.is_file()
    {
        return Some(poster_path);
    }

    let retry_args = [
        "-ss",
        "00:00:00.000",
        "-i",
        path_str,
        "-frames:v",
        "1",
        "-q:v",
        "2",
        "-y",
        poster_str,
    ];
    if run_sandboxed_with_rw("ffmpeg", &retry_args, &ffmpeg_config, &rw_dirs)
        .await
        .is_ok()
        && poster_path.is_file()
    {
        Some(poster_path)
    } else {
        None
    }
}

/// Generator for audio and video media files extracting stream information and poster frames.
#[derive(Debug, Default, Clone, Copy)]
pub struct MediaGenerator;

#[async_trait]
impl PreviewGenerator for MediaGenerator {
    fn can_handle(&self, mime: &str, path: &Path) -> bool {
        if mime.starts_with("video/") || mime.starts_with("audio/") {
            return true;
        }

        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "mp4"
                        | "mkv"
                        | "webm"
                        | "avi"
                        | "mov"
                        | "flv"
                        | "wmv"
                        | "mp3"
                        | "flac"
                        | "wav"
                        | "ogg"
                        | "m4a"
                        | "aac"
                        | "opus"
                )
            })
    }

    async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload> {
        let path_str = req
            .path
            .to_str()
            .ok_or_else(|| QuickLookError::decoding_failed(&req.path, "Invalid path encoding"))?;

        let probe_config = SandboxConfig::new(&req.path).with_timeout(Duration::from_millis(2000));
        let probe_args = [
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path_str,
        ];

        let output = run_sandboxed("ffprobe", &probe_args, &probe_config).await?;
        let json_val: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|e| {
            QuickLookError::decoding_failed(&req.path, format!("Invalid ffprobe JSON: {e}"))
        })?;

        let meta = parse_media_metadata(&json_val);
        let poster_image = if meta.is_video {
            extract_video_poster(req, path_str).await
        } else {
            None
        };

        Ok(PreviewPayload::Media(MediaPreviewData {
            duration_secs: meta.duration_secs,
            width: meta.width,
            height: meta.height,
            codec: meta.codec,
            sample_rate: meta.sample_rate,
            is_video: meta.is_video,
            poster_image,
        }))
    }
}

// ---------------------------------------------------------------------------
// 5. FallbackGenerator
// ---------------------------------------------------------------------------

/// Generator for unrecognized, binary, or unsupported file formats.
#[derive(Debug, Default, Clone, Copy)]
pub struct FallbackGenerator;

#[async_trait]
impl PreviewGenerator for FallbackGenerator {
    fn can_handle(&self, _mime: &str, _path: &Path) -> bool {
        true
    }

    async fn generate(&self, req: &PreviewRequest) -> Result<PreviewPayload> {
        let meta = std::fs::metadata(&req.path).map_err(|e| QuickLookError::io(&req.path, e))?;
        let file_name = req
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();

        let file_size = meta.len();
        let modified = meta.modified().ok();
        let icon_name = shared::models::MimeCategory::from_mime_str(&req.mime_type)
            .icon_name()
            .to_string();

        Ok(PreviewPayload::FallbackCard(FallbackCardData {
            path: req.path.clone(),
            file_name,
            file_size,
            mime_type: req.mime_type.clone(),
            modified,
            icon_name,
        }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_source_code_generator_highlighting() {
        let mut temp_file = NamedTempFile::with_suffix(".rs").unwrap();
        writeln!(
            temp_file,
            "fn main() {{\n    println!(\"Hello, QuickLook!\");\n}}"
        )
        .unwrap();

        let generator = SourceCodeGenerator;
        let req = PreviewRequest {
            request_id: 1,
            path: temp_file.path().to_path_buf(),
            mime_type: "text/rust".to_string(),
            preferred_size: (800, 600),
        };

        assert!(generator.can_handle(&req.mime_type, &req.path));
        let payload = generator.generate(&req).await.unwrap();

        match payload {
            PreviewPayload::SourceCode(code_data) => {
                assert_eq!(code_data.language, "Rust");
                assert_eq!(code_data.total_lines, 3);
                assert!(!code_data.is_truncated);
                assert!(code_data.highlighted_html.contains("span"));
                assert!(code_data.raw_lines[0].contains("fn main"));
            }
            other => panic!("Expected SourceCode payload, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_source_code_generator_truncation() {
        let mut temp_file = NamedTempFile::with_suffix(".txt").unwrap();
        for i in 0..600 {
            writeln!(temp_file, "Line {i}").unwrap();
        }

        let generator = SourceCodeGenerator;
        let req = PreviewRequest {
            request_id: 2,
            path: temp_file.path().to_path_buf(),
            mime_type: "text/plain".to_string(),
            preferred_size: (800, 600),
        };

        let payload = generator.generate(&req).await.unwrap();
        match payload {
            PreviewPayload::SourceCode(code_data) => {
                assert_eq!(code_data.total_lines, 600);
                assert_eq!(code_data.raw_lines.len(), MAX_CODE_LINES);
                assert!(code_data.is_truncated);
                assert!(code_data.highlighted_html.contains("Truncated"));
            }
            other => panic!("Expected SourceCode payload, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_image_generator_dimensions() {
        let temp_file = NamedTempFile::with_suffix(".png").unwrap();
        let img = image::RgbaImage::new(64, 48);
        img.save(temp_file.path()).unwrap();

        let generator = ImageGenerator;
        let req = PreviewRequest {
            request_id: 3,
            path: temp_file.path().to_path_buf(),
            mime_type: "image/png".to_string(),
            preferred_size: (800, 600),
        };

        assert!(generator.can_handle(&req.mime_type, &req.path));
        let payload = generator.generate(&req).await.unwrap();

        match payload {
            PreviewPayload::Image(img_data) => {
                assert_eq!(img_data.width, 64);
                assert_eq!(img_data.height, 48);
                assert_eq!(img_data.image_path, temp_file.path());
            }
            other => panic!("Expected Image payload, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_fallback_generator() {
        let mut temp_file = NamedTempFile::with_suffix(".bin").unwrap();
        temp_file.write_all(&[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

        let generator = FallbackGenerator;
        let req = PreviewRequest {
            request_id: 4,
            path: temp_file.path().to_path_buf(),
            mime_type: "application/octet-stream".to_string(),
            preferred_size: (800, 600),
        };

        assert!(generator.can_handle(&req.mime_type, &req.path));
        let payload = generator.generate(&req).await.unwrap();

        match payload {
            PreviewPayload::FallbackCard(card_data) => {
                assert_eq!(card_data.file_size, 4);
                assert_eq!(card_data.mime_type, "application/octet-stream");
                assert_eq!(card_data.icon_name, "unknown");
                assert!(card_data.modified.is_some());
            }
            other => panic!("Expected FallbackCard payload, got {other:?}"),
        }
    }
}
