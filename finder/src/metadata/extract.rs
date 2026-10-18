//! Low-level filesystem and format-specific deep metadata extraction.
//!
//! Provides comprehensive inspection:
//! - POSIX & Linux `statx` birth time (`stx_btime`), physical allocation (`st_blocks * 512`),
//!   symbolic (`"-rwxr-xr-x"`) and octal permissions (`"0755"`), UID/GID resolution,
//!   and locked state (`(mode & 0o222) == 0`).
//! - Photographic EXIF metadata extraction (Camera make/model, focal length, ISO, aperture, exposure).
//! - Audio & video stream probing (duration, video dimensions, codecs, sample rate, channels, bitrate).
//! - Document metadata extraction for PDF files (page count, PDF version, title, author, creator).
//! - Symlink-safe extended attribute retrieval and user tag parsing.

use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::SystemTime;

use nix::unistd::{AccessFlags, Gid, Group, Uid, User, access};
use shared::fs::stat::{lstat_path, stat_path};
use shared::mime::detect_mime_type;
use shared::models::MimeCategory;

use super::error::{MetadataError, Result};
use super::types::{DeepMetadata, ImageExifMeta, MediaStreamMeta, PdfDocMeta, PermissionsInfo};
use super::xattr::ExtendedAttributesService;

/// Formats standard 10-character POSIX symbolic mode representation.
#[must_use]
pub fn format_symbolic_mode(mode: u32, is_symlink: bool) -> String {
    let file_type_char = if is_symlink {
        'l'
    } else if (mode & libc::S_IFMT) == libc::S_IFDIR {
        'd'
    } else if (mode & libc::S_IFMT) == libc::S_IFCHR {
        'c'
    } else if (mode & libc::S_IFMT) == libc::S_IFBLK {
        'b'
    } else if (mode & libc::S_IFMT) == libc::S_IFIFO {
        'p'
    } else if (mode & libc::S_IFMT) == libc::S_IFSOCK {
        's'
    } else {
        '-'
    };

    // User permissions
    let u_r = if (mode & 0o400) != 0 { 'r' } else { '-' };
    let u_w = if (mode & 0o200) != 0 { 'w' } else { '-' };
    let u_x = if (mode & 0o4000) != 0 {
        if (mode & 0o100) != 0 { 's' } else { 'S' }
    } else if (mode & 0o100) != 0 {
        'x'
    } else {
        '-'
    };

    // Group permissions
    let g_r = if (mode & 0o040) != 0 { 'r' } else { '-' };
    let g_w = if (mode & 0o020) != 0 { 'w' } else { '-' };
    let g_x = if (mode & 0o2000) != 0 {
        if (mode & 0o010) != 0 { 's' } else { 'S' }
    } else if (mode & 0o010) != 0 {
        'x'
    } else {
        '-'
    };

    // Other permissions
    let o_r = if (mode & 0o004) != 0 { 'r' } else { '-' };
    let o_w = if (mode & 0o002) != 0 { 'w' } else { '-' };
    let o_x = if (mode & 0o1000) != 0 {
        if (mode & 0o001) != 0 { 't' } else { 'T' }
    } else if (mode & 0o001) != 0 {
        'x'
    } else {
        '-'
    };

    format!("{file_type_char}{u_r}{u_w}{u_x}{g_r}{g_w}{g_x}{o_r}{o_w}{o_x}")
}

/// Formats a user-friendly kind display description for the item.
#[must_use]
pub fn format_kind_display(path: &Path, mime_type: &str, is_dir: bool, is_symlink: bool) -> String {
    if is_symlink {
        return "Symbolic Link".to_string();
    }
    if is_dir {
        return "Folder".to_string();
    }

    match mime_type {
        "application/pdf" => "PDF Document".to_string(),
        "image/png" => "PNG Image".to_string(),
        "image/jpeg" => "JPEG Image".to_string(),
        "image/gif" => "GIF Image".to_string(),
        "image/webp" => "WebP Image".to_string(),
        "image/svg+xml" => "SVG Image".to_string(),
        "text/plain" => "Plain Text Document".to_string(),
        "text/markdown" => "Markdown Document".to_string(),
        "text/rust" | "text/x-rust" => "Rust Source File".to_string(),
        "application/json" => "JSON Document".to_string(),
        "application/zip" => "ZIP Archive".to_string(),
        "application/gzip" => "GZIP Archive".to_string(),
        "application/x-tar" => "TAR Archive".to_string(),
        "video/mp4" => "MPEG-4 Movie".to_string(),
        "video/webm" => "WebM Video".to_string(),
        "audio/mpeg" => "MP3 Audio".to_string(),
        "audio/flac" => "FLAC Audio".to_string(),
        "audio/wav" => "WAV Audio".to_string(),
        _ => path.extension().and_then(|e| e.to_str()).map_or_else(
            || {
                let cat = MimeCategory::from_mime_str(mime_type);
                if cat == MimeCategory::Unknown {
                    "Document".to_string()
                } else {
                    format!("{} File", cat.as_str())
                }
            },
            |ext| format!("{} File", ext.to_ascii_uppercase()),
        ),
    }
}

/// Extracts photographic EXIF metadata from an image file.
#[must_use]
pub fn extract_image_exif(path: &Path) -> Option<ImageExifMeta> {
    let (mut width, mut height) = image::image_dimensions(path).unwrap_or((0, 0));

    let mut camera_make = None;
    let mut camera_model = None;
    let mut focal_length = None;
    let mut iso_speed = None;
    let mut f_number = None;
    let mut exposure_time = None;
    let mut color_profile = None;

    if let Ok(file) = File::open(path) {
        let mut buf_reader = std::io::BufReader::new(file);
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut buf_reader) {
            if let Some(field) = exif.get_field(exif::Tag::Make, exif::In::PRIMARY) {
                camera_make = Some(
                    field
                        .display_value()
                        .to_string()
                        .trim_matches('"')
                        .to_string(),
                );
            }
            if let Some(field) = exif.get_field(exif::Tag::Model, exif::In::PRIMARY) {
                camera_model = Some(
                    field
                        .display_value()
                        .to_string()
                        .trim_matches('"')
                        .to_string(),
                );
            }
            if let Some(field) = exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY)
                && let exif::Value::Rational(ref rationals) = field.value
                && let Some(r) = rationals.first()
                && r.denom != 0
            {
                focal_length = Some(f64::from(r.num) / f64::from(r.denom));
            }
            if let Some(field) =
                exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
                && let Some(val) = field.value.get_uint(0)
            {
                iso_speed = Some(val);
            }
            if let Some(field) = exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY)
                && let exif::Value::Rational(ref rationals) = field.value
                && let Some(r) = rationals.first()
                && r.denom != 0
            {
                f_number = Some(f64::from(r.num) / f64::from(r.denom));
            }
            if let Some(field) = exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY) {
                exposure_time = Some(
                    field
                        .display_value()
                        .to_string()
                        .trim_matches('"')
                        .to_string(),
                );
            }
            if let Some(field) = exif.get_field(exif::Tag::ColorSpace, exif::In::PRIMARY) {
                color_profile = Some(field.display_value().to_string());
            }

            if width == 0
                && let Some(field) = exif.get_field(exif::Tag::PixelXDimension, exif::In::PRIMARY)
                && let Some(val) = field.value.get_uint(0)
            {
                width = val;
            }
            if height == 0
                && let Some(field) = exif.get_field(exif::Tag::PixelYDimension, exif::In::PRIMARY)
                && let Some(val) = field.value.get_uint(0)
            {
                height = val;
            }
        }
    }

    if width > 0 || height > 0 || camera_make.is_some() || camera_model.is_some() {
        Some(ImageExifMeta {
            pixel_width: width,
            pixel_height: height,
            camera_make,
            camera_model,
            focal_length,
            iso_speed,
            f_number,
            exposure_time,
            color_profile,
        })
    } else {
        None
    }
}

/// Probes media stream properties via `ffprobe` or `gst-discoverer-1.0`.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn probe_media_stream(path: &Path) -> Option<MediaStreamMeta> {
    let path_str = path.to_str()?;

    // 1. Try ffprobe first
    if let Ok(output) = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path_str,
        ])
        .output()
        && output.status.success()
        && let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&output.stdout)
    {
        let duration_secs = json_val
            .get("format")
            .and_then(|f| f.get("duration"))
            .and_then(serde_json::Value::as_str)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        let streams = json_val
            .get("streams")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut video_width = None;
        let mut video_height = None;
        let mut video_codec = None;
        let mut audio_codec = None;
        let mut audio_bitrate = None;
        let mut sample_rate = None;
        let mut channels = None;

        for stream in &streams {
            let codec_type = stream
                .get("codec_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if codec_type == "video" && video_width.is_none() {
                video_width = stream
                    .get("width")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|w| u32::try_from(w).ok());
                video_height = stream
                    .get("height")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|h| u32::try_from(h).ok());
                video_codec = stream
                    .get("codec_name")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string);
            } else if codec_type == "audio" && audio_codec.is_none() {
                audio_codec = stream
                    .get("codec_name")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string);
                audio_bitrate = stream
                    .get("bit_rate")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|b| b.parse::<u64>().ok())
                    .or_else(|| stream.get("bit_rate").and_then(serde_json::Value::as_u64));
                sample_rate = stream
                    .get("sample_rate")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|s| s.parse::<u32>().ok());
                channels = stream
                    .get("channels")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|c| u16::try_from(c).ok());
            }
        }

        if duration_secs > 0.0 || video_codec.is_some() || audio_codec.is_some() {
            return Some(MediaStreamMeta {
                duration_secs,
                video_width,
                video_height,
                video_codec,
                audio_codec,
                audio_bitrate,
                sample_rate,
                channels,
            });
        }
    }

    // 2. Fallback to gst-discoverer-1.0
    if let Ok(output) = std::process::Command::new("gst-discoverer-1.0")
        .arg(path_str)
        .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut duration_secs = 0.0;
        let mut video_width = None;
        let mut video_height = None;
        let mut video_codec = None;
        let mut audio_codec = None;
        let mut sample_rate = None;
        let mut channels = None;

        for line in stdout.lines() {
            let trimmed = line.trim();
            if let Some(dur_str) = trimmed.strip_prefix("Duration:") {
                let parts: Vec<&str> = dur_str.trim().split(':').collect();
                if parts.len() == 3 {
                    let h = parts[0].parse::<f64>().unwrap_or(0.0);
                    let m = parts[1].parse::<f64>().unwrap_or(0.0);
                    let s = parts[2].parse::<f64>().unwrap_or(0.0);
                    duration_secs = f64::mul_add(m, 60.0, h * 3600.0) + s;
                }
            } else if trimmed.starts_with("video:") && video_codec.is_none() {
                video_codec = Some(trimmed.replace("video:", "").trim().to_string());
            } else if trimmed.starts_with("audio:") && audio_codec.is_none() {
                audio_codec = Some(trimmed.replace("audio:", "").trim().to_string());
            } else if let Some(wh) = trimmed.strip_prefix("Width:") {
                video_width = wh.trim().parse::<u32>().ok();
            } else if let Some(hh) = trimmed.strip_prefix("Height:") {
                video_height = hh.trim().parse::<u32>().ok();
            } else if let Some(sr) = trimmed.strip_prefix("Sample rate:") {
                sample_rate = sr.trim().trim_end_matches("Hz").trim().parse::<u32>().ok();
            } else if let Some(ch) = trimmed.strip_prefix("Channels:") {
                channels = ch.trim().parse::<u16>().ok();
            }
        }

        if duration_secs > 0.0 || video_codec.is_some() || audio_codec.is_some() {
            return Some(MediaStreamMeta {
                duration_secs,
                video_width,
                video_height,
                video_codec,
                audio_codec,
                audio_bitrate: None,
                sample_rate,
                channels,
            });
        }
    }

    None
}

fn extract_pdf_dict_string(content: &str, key: &str) -> Option<String> {
    let key_pos = content.find(key)?;
    let after_key = content[key_pos + key.len()..].trim_start();
    if let Some(stripped) = after_key.strip_prefix('(') {
        let end = stripped.find(')')?;
        Some(stripped[..end].to_string())
    } else {
        None
    }
}

/// Parses PDF trailer dictionary and document metadata.
#[must_use]
pub fn extract_pdf_metadata(path: &Path) -> Option<PdfDocMeta> {
    let mut page_count = 0;
    let mut pdf_version = None;
    let mut title = None;
    let mut author = None;
    let mut creator = None;

    // 1. In-process direct byte scanning
    if let Ok(file) = File::open(path) {
        let mut buffer = Vec::new();
        let _ = file.take(10 * 1024 * 1024).read_to_end(&mut buffer);

        if buffer.starts_with(b"%PDF-") {
            if let Some(line) = buffer.split(|&b| b == b'\n' || b == b'\r').next()
                && let Ok(header) = std::str::from_utf8(line)
                && let Some(ver) = header.strip_prefix("%PDF-")
            {
                pdf_version = Some(ver.trim().to_string());
            }

            let content_str = String::from_utf8_lossy(&buffer);
            let mut count = 0;
            for segment in content_str.split("/Type") {
                let trimmed = segment.trim_start();
                if let Some(after) = trimmed.strip_prefix("/Page")
                    && !after.starts_with('s')
                    && !after.starts_with('S')
                {
                    count += 1;
                }
            }
            if count > 0 {
                page_count = count;
            }

            if let Some(t) = extract_pdf_dict_string(&content_str, "/Title") {
                title = Some(t);
            }
            if let Some(a) = extract_pdf_dict_string(&content_str, "/Author") {
                author = Some(a);
            }
            if let Some(c) = extract_pdf_dict_string(&content_str, "/Creator") {
                creator = Some(c);
            }
        }
    }

    // 2. Refine using pdfinfo if available
    if let Some(path_str) = path.to_str()
        && let Ok(output) = std::process::Command::new("pdfinfo").arg(path_str).output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(val) = line.strip_prefix("Pages:") {
                if let Ok(c) = val.trim().parse::<usize>() {
                    page_count = c;
                }
            } else if let Some(val) = line.strip_prefix("PDF version:") {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    pdf_version = Some(trimmed.to_string());
                }
            } else if let Some(val) = line.strip_prefix("Title:") {
                let trimmed = val.trim();
                if !trimmed.is_empty() && title.is_none() {
                    title = Some(trimmed.to_string());
                }
            } else if let Some(val) = line.strip_prefix("Author:") {
                let trimmed = val.trim();
                if !trimmed.is_empty() && author.is_none() {
                    author = Some(trimmed.to_string());
                }
            } else if let Some(val) = line.strip_prefix("Creator:") {
                let trimmed = val.trim();
                if !trimmed.is_empty() && creator.is_none() {
                    creator = Some(trimmed.to_string());
                }
            }
        }
    }

    if page_count > 0 || pdf_version.is_some() || title.is_some() {
        Some(PdfDocMeta {
            page_count: if page_count == 0 { 1 } else { page_count },
            pdf_version,
            title,
            author,
            creator,
        })
    } else {
        None
    }
}

/// Performs deep metadata inspection on the filesystem item at `path`.
///
/// # Errors
///
/// Returns [`MetadataError`] if the path cannot be accessed or inspected.
#[allow(clippy::too_many_lines)]
pub fn extract_deep_metadata(path: &Path) -> Result<DeepMetadata> {
    let symlink_meta = std::fs::symlink_metadata(path).map_err(|e| MetadataError::io(path, e))?;
    let is_symlink = symlink_meta.file_type().is_symlink();
    let symlink_target = if is_symlink {
        std::fs::read_link(path).ok()
    } else {
        None
    };

    let target_stat = stat_path(path).ok();
    let lstat = lstat_path(path)
        .map_err(|e| MetadataError::io(path, std::io::Error::other(e.to_string())))?;

    let effective_stat = target_stat.as_ref().unwrap_or(&lstat);
    let is_directory = effective_stat.is_dir();

    let target_meta = if is_symlink {
        std::fs::metadata(path).ok()
    } else {
        None
    };

    let size_logical = target_meta
        .as_ref()
        .map_or_else(|| symlink_meta.len(), std::fs::Metadata::len);
    let size_physical = target_meta
        .as_ref()
        .map_or_else(|| symlink_meta.blocks() * 512, |m| m.blocks() * 512);

    let created_time = effective_stat.btime.or(effective_stat.ctime);
    let modified_time = effective_stat.mtime.unwrap_or(SystemTime::UNIX_EPOCH);
    let accessed_time = effective_stat.atime.unwrap_or(SystemTime::UNIX_EPOCH);

    let mode = effective_stat.mode;
    let symbolic = format_symbolic_mode(mode, is_symlink);
    let octal = format!("{:04o}", mode & 0o7777);

    let owner_uid = effective_stat.uid;
    let owner_name = User::from_uid(Uid::from_raw(owner_uid))
        .ok()
        .flatten()
        .map_or_else(|| owner_uid.to_string(), |u| u.name);

    let group_gid = effective_stat.gid;
    let group_name = Group::from_gid(Gid::from_raw(group_gid))
        .ok()
        .flatten()
        .map_or_else(|| group_gid.to_string(), |g| g.name);

    let is_user_writable = access(path, AccessFlags::W_OK).is_ok();
    let is_locked = (mode & 0o222) == 0;

    let permissions = PermissionsInfo {
        mode,
        symbolic,
        octal,
        owner_uid,
        owner_name,
        group_gid,
        group_name,
        is_user_writable,
        is_locked,
    };

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();

    let mime_type = if is_directory {
        "inode/directory".to_string()
    } else if is_symlink && target_meta.is_none() {
        "inode/symlink".to_string()
    } else {
        detect_mime_type(path).unwrap_or_else(|_| "application/octet-stream".to_string())
    };

    let kind_display = format_kind_display(path, &mime_type, is_directory, is_symlink);

    let image_meta = if mime_type.starts_with("image/") {
        extract_image_exif(path)
    } else {
        None
    };

    let media_meta = if mime_type.starts_with("video/") || mime_type.starts_with("audio/") {
        probe_media_stream(path)
    } else {
        None
    };

    let pdf_meta = if mime_type == "application/pdf"
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        extract_pdf_metadata(path)
    } else {
        None
    };

    let extended = ExtendedAttributesService::get_all(path)?;

    Ok(DeepMetadata {
        path: path.to_path_buf(),
        file_name,
        kind_display,
        mime_type,
        size_logical,
        size_physical,
        is_directory,
        is_symlink,
        symlink_target,
        created_time,
        modified_time,
        accessed_time,
        permissions,
        image_meta,
        media_meta,
        pdf_meta,
        extended,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_format_symbolic_mode() {
        let dir_sym = format_symbolic_mode(libc::S_IFDIR | 0o755, false);
        assert_eq!(dir_sym, "drwxr-xr-x");

        let file_sym = format_symbolic_mode(libc::S_IFREG | 0o644, false);
        assert_eq!(file_sym, "-rw-r--r--");

        let link_sym = format_symbolic_mode(libc::S_IFLNK | 0o777, true);
        assert_eq!(link_sym, "lrwxrwxrwx");
    }

    #[test]
    fn test_extract_deep_metadata_regular_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"Hello metadata extraction world!").unwrap();
        file.flush().unwrap();

        let meta = extract_deep_metadata(file.path()).unwrap();
        assert!(!meta.is_directory);
        assert!(!meta.is_symlink);
        assert_eq!(meta.size_logical, 32);
        let _ = meta.size_physical;
        assert_ne!(meta.permissions.octal, "");
        assert_ne!(meta.permissions.symbolic, "");
        assert_ne!(meta.permissions.owner_name, "");
    }

    #[test]
    fn test_extract_deep_metadata_pdf_document() {
        let mut file = NamedTempFile::new().unwrap();
        let dummy_pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                          2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                          3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
                          4 0 obj\n<< /Title (Deep Metadata Test) /Author (Findx Engine) >>\nendobj\n\
                          trailer\n<< /Root 1 0 R /Info 4 0 R >>\n%%EOF";
        file.write_all(dummy_pdf).unwrap();
        file.flush().unwrap();

        let meta = extract_pdf_metadata(file.path());
        assert!(meta.is_some());
        let pdf = meta.unwrap();
        assert_eq!(pdf.pdf_version.as_deref(), Some("1.7"));
        assert_eq!(pdf.page_count, 1);
        assert_eq!(pdf.title.as_deref(), Some("Deep Metadata Test"));
        assert_eq!(pdf.author.as_deref(), Some("Findx Engine"));
    }

    #[test]
    fn test_extract_deep_metadata_symlink() {
        let file = NamedTempFile::new().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let symlink_path = temp_dir.path().join("link_to_file");

        std::os::unix::fs::symlink(file.path(), &symlink_path).unwrap();

        let meta = extract_deep_metadata(&symlink_path).unwrap();
        assert!(meta.is_symlink);
        assert_eq!(meta.symlink_target, Some(file.path().to_path_buf()));
        assert_eq!(meta.kind_display, "Symbolic Link");
    }
}
