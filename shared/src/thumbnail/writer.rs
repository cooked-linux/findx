//! PNG metadata chunk writer, reader, and atomic file ingestion.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use image::RgbaImage;

use super::ThumbnailError;

/// Input metadata required by the Freedesktop Thumbnail Management Standard v0.8.0.
#[derive(Debug, Clone, Copy)]
pub struct ThumbnailMetadata<'a> {
    /// Canonical `file://` URI.
    pub uri: &'a str,
    /// Modification timestamp in decimal seconds.
    pub mtime: i64,
    /// Size of the source file in bytes.
    pub size: u64,
    /// MIME type string of the source file.
    pub mime_type: &'a str,
}

/// Parsed metadata chunks read from a cached thumbnail PNG.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThumbnailMetadataRecord {
    /// Canonical `file://` URI from `Thumb::URI`.
    pub uri: Option<String>,
    /// Source file `mtime` timestamp from `Thumb::MTime`.
    pub mtime: Option<i64>,
    /// Source file byte size from `Thumb::Size`.
    pub size: Option<u64>,
    /// Source MIME type from `Thumb::Mimetype`.
    pub mime_type: Option<String>,
    /// Generator identifier from `Software`.
    pub software: Option<String>,
}

/// Reads Freedesktop standard `tEXt` (and `iTXt`) metadata chunks from a PNG file.
///
/// # Errors
///
/// Returns [`ThumbnailError::Io`] if reading or decoding PNG headers fails.
pub fn read_thumbnail_metadata(path: &Path) -> Result<ThumbnailMetadataRecord, ThumbnailError> {
    let file = File::open(path)?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let reader = decoder
        .read_info()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let info = reader.info();

    let mut record = ThumbnailMetadataRecord::default();

    for chunk in &info.uncompressed_latin1_text {
        match chunk.keyword.as_str() {
            "Thumb::URI" => record.uri = Some(chunk.text.clone()),
            "Thumb::MTime" => record.mtime = chunk.text.parse::<i64>().ok(),
            "Thumb::Size" => record.size = chunk.text.parse::<u64>().ok(),
            "Thumb::Mimetype" => record.mime_type = Some(chunk.text.clone()),
            "Software" => record.software = Some(chunk.text.clone()),
            _ => {}
        }
    }

    for chunk in &info.utf8_text {
        if let Ok(text) = chunk.get_text() {
            match chunk.keyword.as_str() {
                "Thumb::URI" if record.uri.is_none() => record.uri = Some(text),
                "Thumb::MTime" if record.mtime.is_none() => {
                    record.mtime = text.parse::<i64>().ok();
                }
                "Thumb::Size" if record.size.is_none() => {
                    record.size = text.parse::<u64>().ok();
                }
                "Thumb::Mimetype" if record.mime_type.is_none() => {
                    record.mime_type = Some(text);
                }
                "Software" if record.software.is_none() => {
                    record.software = Some(text);
                }
                _ => {}
            }
        }
    }

    Ok(record)
}

/// Atomically writes an RGBA thumbnail image with standard Freedesktop metadata.
///
/// Ensures target directory has 0700 permissions and output file has 0600 permissions.
///
/// # Errors
///
/// Returns [`ThumbnailError::Io`] if file creation, encoding, or renaming fails.
pub fn write_thumbnail_png(
    dir: &Path,
    dest_path: &Path,
    image: &RgbaImage,
    metadata: &ThumbnailMetadata<'_>,
) -> Result<(), ThumbnailError> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }

    let temp_file = tempfile::Builder::new()
        .prefix(".tmp_")
        .suffix(".png")
        .tempfile_in(dir)?;

    {
        let mut encoder = png::Encoder::new(
            BufWriter::new(temp_file.as_file()),
            image.width(),
            image.height(),
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        encoder
            .add_text_chunk("Thumb::URI".to_string(), metadata.uri.to_string())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        encoder
            .add_text_chunk("Thumb::MTime".to_string(), metadata.mtime.to_string())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        encoder
            .add_text_chunk("Thumb::Size".to_string(), metadata.size.to_string())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        encoder
            .add_text_chunk(
                "Thumb::Mimetype".to_string(),
                metadata.mime_type.to_string(),
            )
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        encoder
            .add_text_chunk("Software".to_string(), "findx 0.1.0".to_string())
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        let mut writer = encoder
            .write_header()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        writer
            .write_image_data(image.as_raw())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = temp_file
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600));
    }

    temp_file.persist(dest_path).map_err(|e| e.error)?;
    Ok(())
}

/// Atomically writes a 1x1 failure marker PNG to record that thumbnail generation failed.
///
/// # Errors
///
/// Returns [`ThumbnailError::Io`] if file creation, encoding, or renaming fails.
pub fn write_failed_thumbnail(
    fail_dir: &Path,
    dest_path: &Path,
    metadata: &ThumbnailMetadata<'_>,
) -> Result<(), ThumbnailError> {
    let empty_pixel = RgbaImage::new(1, 1);
    write_thumbnail_png(fail_dir, dest_path, &empty_pixel, metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn test_write_and_read_thumbnail_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("thumb.png");

        let mut img = RgbaImage::new(16, 16);
        img.put_pixel(0, 0, Rgba([255, 128, 0, 255]));

        let meta = ThumbnailMetadata {
            uri: "file:///home/user/photo.jpg",
            mtime: 1_700_000_000,
            size: 42_000,
            mime_type: "image/jpeg",
        };

        write_thumbnail_png(dir.path(), &path, &img, &meta)?;
        assert!(path.is_file());

        let record = read_thumbnail_metadata(&path)?;
        assert_eq!(record.uri.as_deref(), Some("file:///home/user/photo.jpg"));
        assert_eq!(record.mtime, Some(1_700_000_000));
        assert_eq!(record.size, Some(42_000));
        assert_eq!(record.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(record.software.as_deref(), Some("findx 0.1.0"));
        Ok(())
    }
}
