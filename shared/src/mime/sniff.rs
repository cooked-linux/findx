//! Two-stage MIME type sniffer and dangerous executable masquerading detector.
//!
//! Stage 1 checks the filename extension heuristic via [`mime_guess`].
//! Stage 2 inspects up to the first 512 magic bytes of the file content.
//! If a file claims to be a safe media or document format but contains ELF binary
//! headers or script shebangs, a spoofing violation is reported.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::models::MimeCategory;

use super::MimeError;

/// Maximum number of bytes inspected for magic signature identification.
const MAGIC_SNIFF_BUFFER_SIZE: usize = 512;

/// Inspects up to 512 bytes of file content for known magic byte signatures.
///
/// Recognizes:
/// - PNG (`\x89PNG\r\n\x1a\n`)
/// - JPEG (`\xFF\xD8\xFF`)
/// - GIF (`GIF87a` / `GIF89a`)
/// - PDF (`%PDF-`)
/// - ZIP / JAR / Office `OpenXML` (`PK\x03\x04`)
/// - ELF binary (`\x7FELF`)
/// - Shell script shebang (`#!`)
/// - XML declaration (`<?xml`)
/// - WebP (`RIFF....WEBP`)
/// - Gzip (`\x1F\x8B`)
#[must_use]
pub fn sniff_magic(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }

    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }

    if bytes.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }

    if bytes.starts_with(b"\x7FELF") {
        return Some("application/x-executable");
    }

    if bytes.starts_with(b"#!") {
        return Some("text/x-shellscript");
    }

    if bytes.starts_with(b"<?xml") {
        return Some("application/xml");
    }

    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }

    if bytes.len() >= 2 && bytes[0] == 0x1F && bytes[1] == 0x8B {
        return Some("application/gzip");
    }

    if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return Some("application/zip");
    }

    None
}

/// Refines a generic magic MIME type with specific filename extension hints.
///
/// For instance, ZIP archives with `.docx` extension are refined to DOCX MIME types,
/// and XML files with `.svg` extension are refined to SVG MIME types.
#[must_use]
fn refine_magic_with_path(magic_mime: &'static str, path: Option<&Path>) -> String {
    let Some(path) = path else {
        return magic_mime.to_string();
    };

    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);

    match (magic_mime, extension.as_deref()) {
        ("application/zip", Some("docx")) => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string()
        }
        ("application/zip", Some("xlsx")) => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".to_string()
        }
        ("application/zip", Some("pptx")) => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation".to_string()
        }
        ("application/zip", Some("jar")) => "application/java-archive".to_string(),
        ("application/zip", Some("odt")) => "application/vnd.oasis.opendocument.text".to_string(),
        ("application/xml", Some("svg")) => "image/svg+xml".to_string(),
        ("application/xml", Some("xhtml" | "xht")) => "application/xhtml+xml".to_string(),
        _ => magic_mime.to_string(),
    }
}

/// Determines whether content appears to be printable plain text.
#[must_use]
fn is_plain_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    // Reject binary content containing NUL bytes in the initial sample
    if bytes.contains(&0) {
        return false;
    }
    std::str::from_utf8(bytes).is_ok()
}

/// Resolves the canonical MIME type of in-memory bytes and an optional file path.
#[must_use]
pub fn detect_mime_from_bytes(bytes: &[u8], path: Option<&Path>) -> String {
    // Stage 2: Magic byte inspection
    if let Some(magic_mime) = sniff_magic(bytes) {
        return refine_magic_with_path(magic_mime, path);
    }

    // Stage 1 (Fallback): Extension heuristic
    if let Some(guess) = path.and_then(|p| mime_guess::from_path(p).first_raw()) {
        return guess.to_string();
    }

    // Fallback: Text inspection or generic binary stream
    if is_plain_text(bytes) {
        "text/plain".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

/// Detects the canonical MIME type of a filesystem file via two-stage sniffing.
///
/// If the path points to a directory, returns `"inode/directory"`.
///
/// # Errors
///
/// Returns [`MimeError::Io`] if the file cannot be opened or read.
pub fn detect_mime_type(path: &Path) -> Result<String, MimeError> {
    if path.is_dir() {
        return Ok("inode/directory".to_string());
    }

    let mut file = File::open(path)?;
    let mut buf = [0u8; MAGIC_SNIFF_BUFFER_SIZE];
    let mut total_read = 0;

    while total_read < buf.len() {
        match file.read(&mut buf[total_read..]) {
            Ok(0) => break,
            Ok(n) => total_read += n,
            Err(e) => {
                if e.kind() != std::io::ErrorKind::Interrupted {
                    return Err(MimeError::Io(e));
                }
            }
        }
    }

    let bytes = &buf[..total_read];
    if bytes.is_empty() {
        // Zero-byte file: fallback to extension guess if available
        let guess = mime_guess::from_path(path)
            .first_raw()
            .map_or_else(|| "text/plain".to_string(), ToString::to_string);
        return Ok(guess);
    }

    Ok(detect_mime_from_bytes(bytes, Some(path)))
}

/// Checks whether a MIME type represents a safe media or document format.
#[must_use]
fn is_safe_media_or_document(mime: &str) -> bool {
    let cat = MimeCategory::from_mime_str(mime);
    matches!(
        cat,
        MimeCategory::Image
            | MimeCategory::Document
            | MimeCategory::Audio
            | MimeCategory::Video
            | MimeCategory::Font
    )
}

/// Checks whether a detected MIME type indicates executable binary or script content.
#[must_use]
fn is_dangerous_executable_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/x-executable"
            | "application/x-sharedlib"
            | "application/x-pie-executable"
            | "text/x-shellscript"
    )
}

/// Detects canonical MIME type and guards against dangerous executable spoofing.
///
/// If the file extension suggests a safe document or media format (`.pdf`, `.png`,
/// `.jpg`, `.txt`, etc.) but magic bytes indicate an ELF executable or shell script,
/// returns [`MimeError::SpoofingDetected`].
///
/// # Errors
///
/// Returns [`MimeError::Io`] if reading fails, or [`MimeError::SpoofingDetected`] if
/// an executable masquerades as a safe media/document file.
pub fn detect_mime_with_spoof_check(path: &Path) -> Result<String, MimeError> {
    let detected_mime = detect_mime_type(path)?;

    // Check extension heuristic to see if the file claims to be a safe media or document
    if let Some(expected_mime) = mime_guess::from_path(path).first_raw()
        && is_safe_media_or_document(expected_mime)
        && is_dangerous_executable_mime(&detected_mime)
    {
        return Err(MimeError::SpoofingDetected(
            path.to_path_buf(),
            expected_mime.to_string(),
            detected_mime,
        ));
    }

    Ok(detected_mime)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_sniff_magic_signatures() {
        assert_eq!(
            sniff_magic(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"),
            Some("image/png")
        );
        assert_eq!(
            sniff_magic(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46]),
            Some("image/jpeg")
        );
        assert_eq!(sniff_magic(b"GIF89a\x01\x00\x01\x00"), Some("image/gif"));
        assert_eq!(sniff_magic(b"%PDF-1.7\n%abc"), Some("application/pdf"));
        assert_eq!(
            sniff_magic(&[0x50, 0x4B, 0x03, 0x04, 0x14, 0x00]),
            Some("application/zip")
        );
        assert_eq!(
            sniff_magic(b"\x7FELF\x02\x01\x01\x00"),
            Some("application/x-executable")
        );
        assert_eq!(
            sniff_magic(b"#!/bin/bash\necho ok"),
            Some("text/x-shellscript")
        );
        assert_eq!(
            sniff_magic(b"<?xml version=\"1.0\"?>"),
            Some("application/xml")
        );
        let webp = b"RIFF\x00\x00\x00\x00WEBPVP8 ".to_vec();
        assert_eq!(sniff_magic(&webp), Some("image/webp"));
        let gzip_hdr = [0x1F, 0x8B, 0x08, 0x00];
        assert_eq!(sniff_magic(&gzip_hdr), Some("application/gzip"));
    }

    #[test]
    fn test_detect_mime_type_on_disk() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"%PDF-1.4\n1 0 obj\n<<>>\nendobj").unwrap();
        let mime = detect_mime_type(file.path()).unwrap();
        assert_eq!(mime, "application/pdf");
    }

    #[test]
    fn test_detect_mime_plain_text() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"Hello world, this is plaintext.").unwrap();
        let mime = detect_mime_type(file.path()).unwrap();
        assert_eq!(mime, "text/plain");
    }

    #[test]
    fn test_spoofing_detection_elf_as_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let fake_pdf = dir.path().join("invoice.pdf");
        std::fs::write(&fake_pdf, b"\x7FELF\x02\x01\x01\x00fake_binary").unwrap();

        let err = detect_mime_with_spoof_check(&fake_pdf).unwrap_err();
        match err {
            MimeError::SpoofingDetected(p, expected, detected) => {
                assert_eq!(p, fake_pdf);
                assert_eq!(expected, "application/pdf");
                assert_eq!(detected, "application/x-executable");
            }
            other => panic!("Unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_spoofing_detection_script_as_png() {
        let dir = tempfile::tempdir().unwrap();
        let fake_png = dir.path().join("photo.png");
        std::fs::write(&fake_png, b"#!/bin/sh\nrm -rf /").unwrap();

        let err = detect_mime_with_spoof_check(&fake_png).unwrap_err();
        match err {
            MimeError::SpoofingDetected(p, expected, detected) => {
                assert_eq!(p, fake_png);
                assert_eq!(expected, "image/png");
                assert_eq!(detected, "text/x-shellscript");
            }
            other => panic!("Unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn test_genuine_shell_script_not_spoofed() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("install.sh");
        std::fs::write(&script, b"#!/bin/bash\necho hello").unwrap();

        let mime = detect_mime_with_spoof_check(&script).unwrap();
        assert_eq!(mime, "text/x-shellscript");
    }
}
