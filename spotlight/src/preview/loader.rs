//! Preview loading coordinator and metadata inspection.
//!
//! Provides two fast-path resolutions (<10ms) via XDG thumbnail caches and filesystem
//! `stat`, alongside asynchronous rich preview generation (syntect code highlighting,
//! vector PDF page-1 rendering, and image scaling) with monotonic request cancellation.

use std::fmt::Write;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use shared::thumbnail::{ThumbnailSize, get_or_generate_thumbnail};

/// Maximum file size permitted for syntax highlighting (2 MB).
pub const MAX_CODE_PREVIEW_SIZE: u64 = 2 * 1024 * 1024;

/// Maximum line length permitted for syntax highlighting (1,000 characters).
pub const MAX_LINE_LENGTH: usize = 1_000;

/// Maximum lines rendered before truncation notice (500 lines).
pub const MAX_CODE_LINES: usize = 500;

/// Notice appended when source code is truncated.
pub const TRUNCATION_NOTICE: &str = "\n... truncated for preview";

static SYNTAX_SET: OnceLock<syntect::parsing::SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<syntect::highlighting::ThemeSet> = OnceLock::new();

fn get_syntax_set() -> &'static syntect::parsing::SyntaxSet {
    SYNTAX_SET.get_or_init(syntect::parsing::SyntaxSet::load_defaults_newlines)
}

fn get_theme_set() -> &'static syntect::highlighting::ThemeSet {
    THEME_SET.get_or_init(syntect::highlighting::ThemeSet::load_defaults)
}

/// Formats raw byte size into a concise human-readable string.
#[must_use]
pub fn format_human_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    #[allow(clippy::cast_precision_loss)]
    let b = bytes as f64;

    if b < KB {
        format!("{bytes} B")
    } else if b < MB {
        let val = b / KB;
        if (val.fract() < 0.05 || val >= 100.0) && val.fract() > 0.95 {
            format!("{:.0} KB", val.round())
        } else {
            format!("{val:.1} KB")
        }
    } else if b < GB {
        let val = b / MB;
        if (val.fract() < 0.05 || val >= 100.0) && val.fract() > 0.95 {
            format!("{:.0} MB", val.round())
        } else {
            format!("{val:.1} MB")
        }
    } else if b < TB {
        let val = b / GB;
        if (val.fract() < 0.05 || val >= 100.0) && val.fract() > 0.95 {
            format!("{:.0} GB", val.round())
        } else {
            format!("{val:.1} GB")
        }
    } else {
        let val = b / TB;
        format!("{val:.1} TB")
    }
}

/// Formats a filesystem timestamp into "Month Day, Year" display format.
#[must_use]
pub fn format_timestamp(time: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = time.into();
    dt.format("%b %-d, %Y").to_string()
}

/// Computes human-readable breadcrumb representation for parent directories.
#[must_use]
pub fn format_breadcrumbs(path: &Path) -> String {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return "/".to_string(),
    };

    let display_path = std::env::var("HOME").map_or(parent, |home| {
        let home_path = Path::new(&home);
        parent.strip_prefix(home_path).unwrap_or(parent)
    });

    let components: Vec<&str> = display_path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    if components.is_empty() {
        "/".to_string()
    } else {
        let slice = if components.len() > 2 {
            &components[components.len() - 2..]
        } else {
            &components[..]
        };
        slice.join(" / ")
    }
}

/// Maps MIME type and extension to user-friendly "Kind" display name.
#[must_use]
pub fn format_kind_display(mime: &str, path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if path.is_dir() || mime == "inode/directory" {
        return "Folder".to_string();
    }

    match (mime, ext.as_str()) {
        ("application/pdf", _) | (_, "pdf") => "PDF Document".to_string(),
        ("image/png", _) | (_, "png") => "PNG Image".to_string(),
        ("image/jpeg", _) | (_, "jpg" | "jpeg") => "JPEG Image".to_string(),
        ("image/svg+xml", _) | (_, "svg") => "SVG Vector Image".to_string(),
        ("image/gif", _) | (_, "gif") => "GIF Image".to_string(),
        ("image/webp", _) | (_, "webp") => "WebP Image".to_string(),
        (_, "rs") => "Rust Source Code".to_string(),
        (_, "py") => "Python Script".to_string(),
        (_, "js") => "JavaScript File".to_string(),
        (_, "ts") => "TypeScript File".to_string(),
        (_, "html" | "htm") => "HTML Document".to_string(),
        (_, "css") => "CSS Stylesheet".to_string(),
        (_, "json") => "JSON Document".to_string(),
        (_, "toml") => "TOML Document".to_string(),
        (_, "yaml" | "yml") => "YAML Document".to_string(),
        (_, "md" | "markdown") => "Markdown Document".to_string(),
        (_, "sh" | "bash") => "Shell Script".to_string(),
        (_, "c") => "C Source Code".to_string(),
        (_, "cpp" | "cc" | "cxx") => "C++ Source Code".to_string(),
        (_, "h" | "hpp") => "C/C++ Header".to_string(),
        (_, "txt") => "Plain Text Document".to_string(),
        (_, "docx" | "doc") => "Microsoft Word Document".to_string(),
        (_, "xlsx" | "xls") => "Microsoft Excel Spreadsheet".to_string(),
        (_, "pptx" | "ppt") => "Microsoft PowerPoint Presentation".to_string(),
        (m, _) if m.starts_with("image/") => "Image".to_string(),
        (m, _) if m.starts_with("video/") => "Video".to_string(),
        (m, _) if m.starts_with("audio/") => "Audio".to_string(),
        (m, _) if m.starts_with("text/") => "Plain Text Document".to_string(),
        _ => "Document".to_string(),
    }
}

/// Comprehensive metadata summary displayed in the inspector card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMetadataSummary {
    /// Base name of the file or directory.
    pub name: String,
    /// Human-friendly file kind description (e.g. "PDF Document").
    pub kind: String,
    /// Raw size in bytes.
    pub size_bytes: u64,
    /// Formatted human-readable size (e.g. "2.4 MB").
    pub formatted_size: String,
    /// Last modification timestamp.
    pub modified: Option<SystemTime>,
    /// Formatted modification date string (e.g. "Oct 9, 2026").
    pub formatted_modified: String,
    /// Breadcrumb representation of parent directory (e.g. "Documents / Finance").
    pub where_path: String,
    /// Canonical filesystem path.
    pub raw_path: PathBuf,
    /// Detected MIME type string.
    pub mime_type: String,
}

/// Inspects synchronous filesystem metadata for `path`.
///
/// # Errors
///
/// Returns [`std::io::Error`] if the file cannot be stat'ed.
pub fn inspect_metadata(
    path: &Path,
    mime_type: Option<&str>,
) -> std::io::Result<FileMetadataSummary> {
    let stat = shared::fs::stat_path(path).map_err(|e| match e {
        shared::errors::SharedError::Fs(shared::errors::FsError::Io { source, .. }) => source,
        other => std::io::Error::other(other.to_string()),
    })?;

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    let mime = mime_type.map_or_else(
        || {
            shared::mime::detect_mime_type(path)
                .unwrap_or_else(|_| "application/octet-stream".to_string())
        },
        ToString::to_string,
    );

    let kind = format_kind_display(&mime, path);
    let size_bytes = stat.size;
    let formatted_size = format_human_bytes(size_bytes);
    let modified = stat.mtime;
    let formatted_modified = modified.map_or_else(|| "Unknown".to_string(), format_timestamp);
    let where_path = format_breadcrumbs(path);

    Ok(FileMetadataSummary {
        name,
        kind,
        size_bytes,
        formatted_size,
        modified,
        formatted_modified,
        where_path,
        raw_path: path.to_path_buf(),
        mime_type: mime,
    })
}

/// Content payload rendered within the preview pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewKind {
    /// Rendered raster image or cached thumbnail.
    Image {
        /// Absolute path to the cached PNG or source image.
        thumbnail_path: PathBuf,
    },
    /// Syntax-highlighted or plain-text code view.
    Code {
        /// Pango markup string containing syntax colors.
        highlighted_markup: String,
        /// Flag indicating whether lines were truncated at 500 lines limit.
        is_truncated: bool,
        /// Total lines parsed from the source.
        total_lines: usize,
        /// Language identifier.
        language: String,
    },
    /// Vector PDF document page-1 render.
    Pdf {
        /// Absolute path to the rendered page PNG.
        thumbnail_path: PathBuf,
    },
    /// Fallback generic representation when visual preview is unavailable.
    Generic {
        /// Optional cached thumbnail if generated.
        thumbnail_path: Option<PathBuf>,
    },
}

/// Full preview bundle containing inspected metadata and visual content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewData {
    /// Inspected file metadata card attributes.
    pub metadata: FileMetadataSummary,
    /// Resolved visual preview kind.
    pub kind: PreviewKind,
}

/// Highlights source code into Pango markup using `syntect` and desktop theme matching.
///
/// Truncates to first 500 lines and bypasses highlighting if line length exceeds 1,000 chars
/// or file size exceeds 2 MB.
#[must_use]
pub fn highlight_code_lines(
    lines: &[String],
    extension: &str,
    is_dark: bool,
    force_plain: bool,
) -> (String, bool, usize) {
    let total_lines = lines.len();
    let is_truncated = total_lines > MAX_CODE_LINES;
    let display_lines = if is_truncated {
        &lines[..MAX_CODE_LINES]
    } else {
        lines
    };

    let has_oversized_line = display_lines.iter().any(|l| l.len() > MAX_LINE_LENGTH);
    let should_highlight = !force_plain && !has_oversized_line;

    if !should_highlight {
        let mut markup = String::new();
        for (i, line) in display_lines.iter().enumerate() {
            if i > 0 {
                markup.push('\n');
            }
            markup.push_str(&glib::markup_escape_text(line));
        }
        if is_truncated {
            markup.push_str(TRUNCATION_NOTICE);
        }
        return (markup, is_truncated, total_lines);
    }

    let syntax_set = get_syntax_set();
    let theme_set = get_theme_set();

    let syntax = syntax_set
        .find_syntax_by_extension(extension)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());

    let theme_name = if is_dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    };

    let theme = theme_set
        .themes
        .get(theme_name)
        .or_else(|| theme_set.themes.values().next());

    let Some(theme) = theme else {
        let mut markup = String::new();
        for (i, line) in display_lines.iter().enumerate() {
            if i > 0 {
                markup.push('\n');
            }
            markup.push_str(&glib::markup_escape_text(line));
        }
        if is_truncated {
            markup.push_str(TRUNCATION_NOTICE);
        }
        return (markup, is_truncated, total_lines);
    };

    let mut highlighter = syntect::easy::HighlightLines::new(syntax, theme);
    let mut markup = String::new();

    for (i, line) in display_lines.iter().enumerate() {
        if i > 0 {
            markup.push('\n');
        }
        let line_with_nl = format!("{line}\n");
        if let Ok(ranges) = highlighter.highlight_line(&line_with_nl, syntax_set) {
            for (style, token) in ranges {
                let trimmed_token = token.trim_end_matches('\n');
                if trimmed_token.is_empty() {
                    continue;
                }
                let escaped = glib::markup_escape_text(trimmed_token);
                let fg = style.foreground;
                let color_hex = format!("#{:02x}{:02x}{:02x}", fg.r, fg.g, fg.b);
                let _ = write!(markup, "<span foreground=\"{color_hex}\">{escaped}</span>");
            }
        } else {
            markup.push_str(&glib::markup_escape_text(line));
        }
    }

    if is_truncated {
        markup.push_str(TRUNCATION_NOTICE);
    }

    (markup, is_truncated, total_lines)
}

/// Safely reads capped lines from a source file up to 501 lines.
///
/// # Errors
///
/// Returns [`std::io::Error`] if file reading fails.
pub fn read_capped_lines(path: &Path) -> std::io::Result<(Vec<String>, bool)> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    let is_large = metadata.len() > MAX_CODE_PREVIEW_SIZE;

    let reader = BufReader::new(file);
    let mut lines = Vec::with_capacity(MAX_CODE_LINES + 1);

    for line_res in reader.lines().take(MAX_CODE_LINES + 1) {
        let line = line_res?;
        lines.push(line);
    }

    Ok((lines, is_large))
}

/// Renders page 1 of a PDF file using the `pdftoppm` command line utility.
///
/// # Errors
///
/// Returns an error description if `pdftoppm` fails or is not installed.
pub fn render_pdf_page_1(pdf_path: &Path) -> Result<PathBuf, String> {
    let cache_dir = std::env::var("XDG_CACHE_HOME")
        .map_or_else(
            |_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(home).join(".cache")
            },
            PathBuf::from,
        )
        .join("findx")
        .join("pdf_previews");

    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let uri = shared::thumbnail::canonical_file_uri(pdf_path)
        .unwrap_or_else(|_| pdf_path.to_string_lossy().to_string());
    let hash_name = shared::thumbnail::compute_thumbnail_hash(&uri);
    let output_prefix = cache_dir.join(&hash_name);
    let final_png = cache_dir.join(format!("{hash_name}-1.png"));

    if final_png.is_file() {
        return Ok(final_png);
    }

    let status = std::process::Command::new("pdftoppm")
        .arg("-png")
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg("1")
        .arg("-scale-to")
        .arg("440")
        .arg(pdf_path)
        .arg(&output_prefix)
        .status()
        .map_err(|e| format!("failed to execute pdftoppm: {e}"))?;

    if status.success() && final_png.is_file() {
        Ok(final_png)
    } else {
        Err(format!("pdftoppm exited with failure: {status:?}"))
    }
}

/// Central preview coordinator managing fast paths, cache queries, and monotonic async jobs.
#[derive(Debug, Clone, Default)]
pub struct PreviewLoader {
    active_request_id: Arc<AtomicU64>,
}

impl PreviewLoader {
    /// Constructs a new [`PreviewLoader`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            active_request_id: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Triggers Fast Paths: increment request counter, query thumbnail cache, and stat path.
    ///
    /// Returns `(request_id, metadata_result, provisional_thumbnail)`.
    #[must_use]
    pub fn start_preview_request(
        &self,
        path: &Path,
        mime_type: Option<&str>,
    ) -> (u64, Option<FileMetadataSummary>, Option<PathBuf>) {
        let req_id = self.active_request_id.fetch_add(1, Ordering::SeqCst) + 1;

        // Fast path 1 (<10ms): Freedesktop thumbnail cache resolution
        let thumb_cached = get_or_generate_thumbnail(path, ThumbnailSize::Large, mime_type).ok();

        // Fast path 2 (<5ms): Synchronous metadata inspection
        let meta = inspect_metadata(path, mime_type).ok();

        (req_id, meta, thumb_cached)
    }

    /// Verifies if `req_id` is still the latest active preview request.
    #[must_use]
    pub fn is_request_active(&self, req_id: u64) -> bool {
        self.active_request_id.load(Ordering::SeqCst) == req_id
    }

    /// Asynchronously generates a full [`PreviewData`] bundle on Tokio worker pool.
    #[allow(clippy::too_many_lines)]
    pub async fn load_preview_async(
        &self,
        req_id: u64,
        path: PathBuf,
        mime_type: Option<String>,
        is_dark: bool,
    ) -> Option<PreviewData> {
        if !self.is_request_active(req_id) {
            return None;
        }

        let metadata = inspect_metadata(&path, mime_type.as_deref()).ok()?;

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let is_code_or_text = matches!(
            ext.as_str(),
            "rs" | "py"
                | "js"
                | "ts"
                | "c"
                | "cpp"
                | "h"
                | "hpp"
                | "sh"
                | "bash"
                | "toml"
                | "yaml"
                | "yml"
                | "json"
                | "md"
                | "markdown"
                | "txt"
                | "html"
                | "css"
        ) || metadata.mime_type.starts_with("text/")
            || metadata.mime_type == "application/json"
            || metadata.mime_type == "application/x-yaml";

        let is_pdf = ext == "pdf" || metadata.mime_type == "application/pdf";
        let is_image = metadata.mime_type.starts_with("image/");

        let kind = if is_code_or_text {
            let path_clone = path.clone();
            let ext_clone = ext.clone();
            let highlight_res = tokio::task::spawn_blocking(move || {
                let (lines, is_large) = read_capped_lines(&path_clone).unwrap_or_default();
                highlight_code_lines(&lines, &ext_clone, is_dark, is_large)
            })
            .await
            .ok()?;

            if !self.is_request_active(req_id) {
                return None;
            }

            let (highlighted_markup, is_truncated, total_lines) = highlight_res;
            PreviewKind::Code {
                highlighted_markup,
                is_truncated,
                total_lines,
                language: ext,
            }
        } else if is_pdf {
            let path_clone = path.clone();
            let pdf_render = tokio::task::spawn_blocking(move || render_pdf_page_1(&path_clone))
                .await
                .ok()?;

            if !self.is_request_active(req_id) {
                return None;
            }

            if let Ok(thumb_path) = pdf_render {
                PreviewKind::Pdf {
                    thumbnail_path: thumb_path,
                }
            } else {
                let fallback_thumb = get_or_generate_thumbnail(
                    &path,
                    ThumbnailSize::Large,
                    Some(&metadata.mime_type),
                )
                .ok();
                PreviewKind::Generic {
                    thumbnail_path: fallback_thumb,
                }
            }
        } else if is_image {
            let path_clone = path.clone();
            let mime_clone = metadata.mime_type.clone();
            let thumb = tokio::task::spawn_blocking(move || {
                get_or_generate_thumbnail(&path_clone, ThumbnailSize::Large, Some(&mime_clone)).ok()
            })
            .await
            .ok()??;

            if !self.is_request_active(req_id) {
                return None;
            }

            PreviewKind::Image {
                thumbnail_path: thumb,
            }
        } else {
            let fallback_thumb =
                get_or_generate_thumbnail(&path, ThumbnailSize::Large, Some(&metadata.mime_type))
                    .ok();
            PreviewKind::Generic {
                thumbnail_path: fallback_thumb,
            }
        };

        Some(PreviewData { metadata, kind })
    }
}
