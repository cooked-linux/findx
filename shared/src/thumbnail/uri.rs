//! Canonical URI construction and Freedesktop MD5 hash calculation.

use std::path::Path;

use md5::{Digest, Md5};

use super::ThumbnailError;

/// Constructs a canonical `file://` URI for the given filesystem path.
///
/// If the file exists and can be canonicalized (resolving symlinks, `.` and `..`),
/// the canonicalized path is used. Otherwise, if relative, it is resolved against
/// the current working directory. The resulting absolute path is percent-encoded per RFC 3986.
///
/// # Errors
///
/// Returns [`ThumbnailError::Io`] if relative path resolution fails.
/// Returns [`ThumbnailError::InvalidUri`] if URI percent-encoding fails.
pub fn canonical_file_uri(path: &Path) -> Result<String, ThumbnailError> {
    let resolved = match path.canonicalize() {
        Ok(canonical) => canonical,
        Err(_) => {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(ThumbnailError::Io)?
                    .join(path)
            }
        }
    };

    crate::ipc::path_to_file_uri(&resolved)
        .map_err(|err| ThumbnailError::InvalidUri(err.to_string()))
}

/// Derives the 32-character lowercase hex MD5 hash of a canonical URI
/// per the Freedesktop Thumbnail Management Standard v0.8.0.
#[must_use]
pub fn compute_thumbnail_hash(canonical_uri: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(canonical_uri.as_bytes());
    format!("{:032x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freedesktop_md5_vector() {
        let uri = "file:///home/user/images/photo.png";
        let hash = compute_thumbnail_hash(uri);
        assert_eq!(hash, "510f26f2e9a639f7e27797d787ba9e47");
    }

    #[test]
    fn test_canonical_file_uri_percent_encoding() -> Result<(), Box<dyn std::error::Error>> {
        let path = Path::new("/home/user/My Pictures/café.jpg");
        let uri = canonical_file_uri(path)?;
        assert_eq!(uri, "file:///home/user/My%20Pictures/caf%C3%A9.jpg");
        let hash = compute_thumbnail_hash(&uri);
        assert_ne!(hash, "");
        assert_eq!(hash.len(), 32);
        Ok(())
    }

    #[test]
    fn test_canonical_file_uri_with_symlink() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let target = dir.path().join("real_file.png");
        std::fs::write(&target, b"test")?;

        let link = dir.path().join("symlink.png");
        std::os::unix::fs::symlink(&target, &link)?;

        let uri_target = canonical_file_uri(&target)?;
        let uri_link = canonical_file_uri(&link)?;

        // Both link and target resolve to the same canonical file URI
        assert_eq!(uri_link, uri_target);
        assert_eq!(
            compute_thumbnail_hash(&uri_link),
            compute_thumbnail_hash(&uri_target)
        );
        Ok(())
    }
}
