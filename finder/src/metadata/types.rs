//! Domain models representing extracted POSIX, EXIF, media streams, and extended metadata.

use std::path::PathBuf;
use std::time::SystemTime;

use shared::models::{FileTag, TagColor};

/// Detailed POSIX permissions, ownership, and writeability information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionsInfo {
    /// Raw numerical mode bits including file type and permissions mask.
    pub mode: u32,
    /// Standard symbolic string representation (e.g. `"-rwxr-xr-x"` or `"drwxr-xr-x"`).
    pub symbolic: String,
    /// 4-digit octal permission representation with leading zero (e.g. `"0755"`).
    pub octal: String,
    /// POSIX user ID of the owner.
    pub owner_uid: u32,
    /// Resolved username of the owner, or numerical UID if unresolved.
    pub owner_name: String,
    /// POSIX group ID of the owning group.
    pub group_gid: u32,
    /// Resolved group name of the owner, or numerical GID if unresolved.
    pub group_name: String,
    /// Whether the file is writable by the current effective user process.
    pub is_user_writable: bool,
    /// Whether the file is marked read-only / locked across all entities (`(mode & 0o222) == 0`).
    pub is_locked: bool,
}

/// Format-specific EXIF metadata extracted from photographic image files.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageExifMeta {
    /// Horizontal pixel dimension.
    pub pixel_width: u32,
    /// Vertical pixel dimension.
    pub pixel_height: u32,
    /// Hardware manufacturer of the camera (e.g. `"Sony"`).
    pub camera_make: Option<String>,
    /// Camera model identification (e.g. `"ILCE-7M4"`).
    pub camera_model: Option<String>,
    /// Lens focal length in millimeters.
    pub focal_length: Option<f64>,
    /// ISO photographic sensitivity speed rating.
    pub iso_speed: Option<u32>,
    /// Lens aperture f-number value.
    pub f_number: Option<f64>,
    /// Exposure shutter speed string (e.g. `"1/250s"`).
    pub exposure_time: Option<String>,
    /// Detected color space profile name.
    pub color_profile: Option<String>,
}

/// Technical stream properties extracted from video and audio containers.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaStreamMeta {
    /// Total duration in seconds.
    pub duration_secs: f64,
    /// Video frame width in pixels, if video stream is present.
    pub video_width: Option<u32>,
    /// Video frame height in pixels, if video stream is present.
    pub video_height: Option<u32>,
    /// Video codec identifier string (e.g. `"h264"`, `"hevc"`, `"vp9"`).
    pub video_codec: Option<String>,
    /// Audio codec identifier string (e.g. `"aac"`, `"flac"`, `"mp3"`).
    pub audio_codec: Option<String>,
    /// Audio stream bitrate in bits per second.
    pub audio_bitrate: Option<u64>,
    /// Audio sampling rate in Hertz (e.g. `44100`, `48000`).
    pub sample_rate: Option<u32>,
    /// Number of audio channels (e.g. `1` for mono, `2` for stereo, `6` for 5.1).
    pub channels: Option<u16>,
}

/// Document-level metadata extracted from PDF files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfDocMeta {
    /// Total number of pages in the document.
    pub page_count: usize,
    /// PDF specification version (e.g. `"1.7"`, `"2.0"`).
    pub pdf_version: Option<String>,
    /// Document title from metadata dictionary.
    pub title: Option<String>,
    /// Document author.
    pub author: Option<String>,
    /// Producing application or creator.
    pub creator: Option<String>,
}

/// Extended attributes stored in the filesystem `user.*` namespace or local fallback database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedAttrsMeta {
    /// Freeform user comment string (`user.xdg.comment`).
    pub comment: String,
    /// Download origin URL (`user.xdg.origin.url`).
    pub origin_url: Option<String>,
    /// HTTP referrer URL where download was initiated (`user.xdg.origin.referrer`).
    pub origin_referrer: Option<String>,
    /// Associated user tags (`user.xdg.tags`).
    pub tags: Vec<FileTag>,
    /// Canonical macOS color label code (`user.xdg.color`).
    pub color_code: Option<TagColor>,
}

/// Deep inspection aggregate containing low-level filesystem, format, and extended metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct DeepMetadata {
    /// Absolute path to the inspected item.
    pub path: PathBuf,
    /// Basename of the file or directory.
    pub file_name: String,
    /// User-facing file kind display string (e.g. `"Folder"`, `"PNG Image"`).
    pub kind_display: String,
    /// Verified MIME type identifier string (e.g. `"image/png"`, `"application/pdf"`).
    pub mime_type: String,
    /// Logical size in bytes as reported by file metadata.
    pub size_logical: u64,
    /// Physical allocated on-disk byte size (`st_blocks * 512`).
    pub size_physical: u64,
    /// Whether the target is a directory.
    pub is_directory: bool,
    /// Whether the target is a symbolic link.
    pub is_symlink: bool,
    /// Target path pointed to by symlink, if `is_symlink` is true.
    pub symlink_target: Option<PathBuf>,
    /// Creation or birth timestamp (`stx_btime`), falling back to `stx_ctime`.
    pub created_time: Option<SystemTime>,
    /// Last content modification timestamp.
    pub modified_time: SystemTime,
    /// Last access timestamp.
    pub accessed_time: SystemTime,
    /// Detailed POSIX permission and ownership info.
    pub permissions: PermissionsInfo,
    /// Extracted EXIF camera metadata for photographic images.
    pub image_meta: Option<ImageExifMeta>,
    /// Technical media stream properties for audio/video files.
    pub media_meta: Option<MediaStreamMeta>,
    /// Document metadata for PDF files.
    pub pdf_meta: Option<PdfDocMeta>,
    /// Extended attributes and user tags.
    pub extended: ExtendedAttrsMeta,
}
