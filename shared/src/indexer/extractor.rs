//! Bounded text extractor with resource caps, MIME filtering, and timeouts.
//!
//! Enforces:
//! - File size ceiling (2MB max for text indexing)
//! - Character extraction limit (100,000 chars max)
//! - 2.0s extraction timeout
//! - Lossy UTF-8 decoding for corrupted or mixed-encoding files

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::indexer::IndexerError;

/// Maximum file size permitted for full-text extraction (2 Megabytes).
pub const MAX_INDEX_FILE_SIZE: u64 = 2 * 1024 * 1024;

/// Maximum number of characters extracted per document.
pub const MAX_EXTRACTED_CHARS: usize = 100_000;

/// Hard timeout ceiling for document text extraction.
pub const EXTRACTION_TIMEOUT: Duration = Duration::from_millis(2000);

/// Supported document, markup, and source code extensions.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "txt", "rtf", "org", "md", "markdown", "rs", "py", "js", "ts", "c", "cpp", "cc", "cxx", "h",
    "hpp", "go", "html", "htm", "css", "json", "toml", "yaml", "yml", "sh",
];

/// Determines whether a path is eligible for full-text content extraction.
#[must_use]
pub fn is_text_extractable(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        for supported in SUPPORTED_EXTENSIONS {
            if ext.eq_ignore_ascii_case(supported) {
                return true;
            }
        }
    }

    let mime = mime_guess::from_path(path).first_or_octet_stream();
    if mime.type_() == "text" {
        return true;
    }

    let sub = mime.subtype().as_str();
    matches!(
        sub,
        "json" | "toml" | "yaml" | "x-yaml" | "javascript" | "xml"
    )
}

/// Synchronously extracts text from a file with bounded size and length limits.
///
/// Returns an empty string if the file exceeds 2MB or is not an extractable text type.
///
/// # Errors
///
/// Returns [`IndexerError::Io`] if file metadata or reading encounters an I/O failure.
pub fn extract_file_content_sync(path: &Path) -> Result<String, IndexerError> {
    let metadata = std::fs::metadata(path)?;

    // Files exceeding 2MB are skipped from content indexing
    if metadata.len() > MAX_INDEX_FILE_SIZE || !is_text_extractable(path) {
        return Ok(String::new());
    }

    let mut file = File::open(path)?;
    let initial_cap =
        usize::try_from(metadata.len()).map_or(128 * 1024, |len| std::cmp::min(len, 128 * 1024));
    let mut buffer = Vec::with_capacity(initial_cap);

    // Limit bytes read to 2MB to guard against race condition file growth
    let mut bounded_reader = (&mut file).take(MAX_INDEX_FILE_SIZE);
    bounded_reader.read_to_end(&mut buffer)?;

    let lossy_string = String::from_utf8_lossy(&buffer);
    if lossy_string.chars().count() <= MAX_EXTRACTED_CHARS {
        Ok(lossy_string.into_owned())
    } else {
        Ok(lossy_string.chars().take(MAX_EXTRACTED_CHARS).collect())
    }
}

/// Asynchronously extracts text from a file with a 2.0s timeout ceiling.
///
/// # Errors
///
/// Returns [`IndexerError::ExtractionTimeout`] if extraction exceeds 2.0 seconds.
/// Returns [`IndexerError::Io`] if I/O reading fails.
pub async fn extract_file_content(path: &Path) -> Result<String, IndexerError> {
    let path_buf = path.to_path_buf();
    tokio::time::timeout(EXTRACTION_TIMEOUT, async move {
        tokio::task::spawn_blocking(move || extract_file_content_sync(&path_buf))
            .await
            .map_err(|e| IndexerError::Walk(e.to_string()))?
    })
    .await
    .map_err(|_| IndexerError::ExtractionTimeout(path.to_path_buf()))?
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_extractable_extensions_and_mimes() {
        assert!(is_text_extractable(Path::new("main.rs")));
        assert!(is_text_extractable(Path::new("README.md")));
        assert!(is_text_extractable(Path::new("notes.txt")));
        assert!(is_text_extractable(Path::new("config.toml")));
        assert!(is_text_extractable(Path::new("package.json")));
        assert!(is_text_extractable(Path::new("index.html")));
        assert!(is_text_extractable(Path::new("script.py")));

        // Non-extractable binary files
        assert!(!is_text_extractable(Path::new("photo.png")));
        assert!(!is_text_extractable(Path::new("archive.zip")));
        assert!(!is_text_extractable(Path::new("binary.iso")));
    }

    #[test]
    fn test_file_size_exceeding_2mb_skipped() {
        let temp_dir = tempfile::tempdir().unwrap();
        let large_file = temp_dir.path().join("large.txt");

        let f = File::create(&large_file).unwrap();
        // 2MB + 1 byte
        f.set_len(MAX_INDEX_FILE_SIZE + 1).unwrap();

        let extracted = extract_file_content_sync(&large_file).unwrap();
        assert!(
            extracted.is_empty(),
            "Files > 2MB must not have content extracted"
        );
    }

    #[test]
    fn test_invalid_utf8_lossy_decoding() {
        let temp_dir = tempfile::tempdir().unwrap();
        let binary_text = temp_dir.path().join("mixed.txt");

        let mut f = File::create(&binary_text).unwrap();
        // Valid ascii followed by invalid utf8 bytes
        f.write_all(b"Hello \xFF\xFE World").unwrap();

        let extracted = extract_file_content_sync(&binary_text).unwrap();
        assert!(extracted.contains("Hello "));
        assert!(extracted.contains(" World"));
    }

    #[test]
    fn test_character_truncation_ceiling() {
        let temp_dir = tempfile::tempdir().unwrap();
        let long_file = temp_dir.path().join("long.txt");

        let mut f = File::create(&long_file).unwrap();
        // 120,000 characters
        let chunk = "a".repeat(120_000);
        f.write_all(chunk.as_bytes()).unwrap();

        let extracted = extract_file_content_sync(&long_file).unwrap();
        assert_eq!(extracted.chars().count(), MAX_EXTRACTED_CHARS);
    }

    #[tokio::test]
    async fn test_async_extraction_within_timeout() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("async_test.rs");

        let mut f = File::create(&file_path).unwrap();
        writeln!(f, "pub fn add(a: i32, b: i32) -> i32 {{ a + b }}").unwrap();

        let extracted = extract_file_content(&file_path).await.unwrap();
        assert!(extracted.contains("pub fn add"));
    }
}
