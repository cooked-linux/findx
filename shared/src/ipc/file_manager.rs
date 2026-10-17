//! Client implementation for `org.freedesktop.FileManager1`.

use std::path::{Path, PathBuf};
use std::time::Duration;
use zbus::Connection;
use zbus::proxy;

use crate::errors::{IpcError, ModelError, Result, SharedError};
use crate::ipc::connection::{DEFAULT_TIMEOUT, get_session_connection};

/// D-Bus proxy for `org.freedesktop.FileManager1`.
#[proxy(
    interface = "org.freedesktop.FileManager1",
    default_service = "org.freedesktop.FileManager1",
    default_path = "/org/freedesktop/FileManager1"
)]
pub trait FileManager {
    /// Selects and highlights items at the given URIs.
    fn show_items(&self, uris: &[&str], startup_id: &str) -> zbus::Result<()>;

    /// Opens folders at the given URIs.
    fn show_folders(&self, uris: &[&str], startup_id: &str) -> zbus::Result<()>;

    /// Displays property dialogs for items at the given URIs.
    fn show_item_properties(&self, uris: &[&str], startup_id: &str) -> zbus::Result<()>;
}

/// High-level client wrapper for desktop file manager interaction.
#[derive(Debug, Clone)]
pub struct FileManagerClient {
    connection: Option<Connection>,
    timeout: Duration,
}

impl Default for FileManagerClient {
    fn default() -> Self {
        Self::new()
    }
}

impl FileManagerClient {
    /// Constructs a new `FileManagerClient` using the lazy singleton session connection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            connection: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Constructs a new `FileManagerClient` with an explicit D-Bus connection.
    #[must_use]
    pub const fn with_connection(conn: Connection) -> Self {
        Self {
            connection: Some(conn),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Configures a custom timeout for file manager operations.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn get_conn(&self) -> Result<Connection> {
        if let Some(conn) = &self.connection {
            Ok(conn.clone())
        } else {
            get_session_connection().await
        }
    }

    /// Instructs the desktop file manager to display and select the items at `uris`.
    ///
    /// # Errors
    ///
    /// Returns an error if D-Bus communication fails or times out.
    pub async fn show_items(&self, uris: &[String], startup_id: &str) -> Result<()> {
        let conn = self.get_conn().await?;
        let proxy = FileManagerProxy::new(&conn).await?;
        let uris_ref: Vec<&str> = uris.iter().map(String::as_str).collect();
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(self.timeout, proxy.show_items(&uris_ref, startup_id)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "FileManager1.ShowItems",
                timeout_ms,
            ))),
        }
    }

    /// Instructs the desktop file manager to open folders containing `uris`.
    ///
    /// # Errors
    ///
    /// Returns an error if D-Bus communication fails or times out.
    pub async fn show_folders(&self, uris: &[String], startup_id: &str) -> Result<()> {
        let conn = self.get_conn().await?;
        let proxy = FileManagerProxy::new(&conn).await?;
        let uris_ref: Vec<&str> = uris.iter().map(String::as_str).collect();
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(self.timeout, proxy.show_folders(&uris_ref, startup_id)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "FileManager1.ShowFolders",
                timeout_ms,
            ))),
        }
    }

    /// Instructs the desktop file manager to show properties dialogs for `uris`.
    ///
    /// # Errors
    ///
    /// Returns an error if D-Bus communication fails or times out.
    pub async fn show_item_properties(&self, uris: &[String], startup_id: &str) -> Result<()> {
        let conn = self.get_conn().await?;
        let proxy = FileManagerProxy::new(&conn).await?;
        let uris_ref: Vec<&str> = uris.iter().map(String::as_str).collect();
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(
            self.timeout,
            proxy.show_item_properties(&uris_ref, startup_id),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "FileManager1.ShowItemProperties",
                timeout_ms,
            ))),
        }
    }

    /// Convenience method to show local file paths in the desktop file manager.
    ///
    /// # Errors
    ///
    /// Returns an error if path conversion or D-Bus communication fails.
    pub async fn show_paths(&self, paths: &[&Path], startup_id: &str) -> Result<()> {
        let mut uris = Vec::with_capacity(paths.len());
        for p in paths {
            uris.push(path_to_file_uri(p)?);
        }
        self.show_items(&uris, startup_id).await
    }
}

/// Converts a local filesystem path to an RFC 3986 `file://` URI.
///
/// # Errors
///
/// Returns an error if the current working directory cannot be determined for relative paths.
pub fn path_to_file_uri(path: &Path) -> Result<String> {
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    let s = abs_path.to_string_lossy();
    let mut encoded = String::from("file://");
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(encoded, "%{b:02X}");
            }
        }
    }
    Ok(encoded)
}

/// Converts an RFC 3986 `file://` URI back into a local filesystem [`PathBuf`].
///
/// # Errors
///
/// Returns an error if the URI does not start with `file://` or contains invalid percent encoding.
pub fn file_uri_to_path(uri: &str) -> Result<PathBuf> {
    let stripped = uri
        .strip_prefix("file://")
        .ok_or_else(|| ModelError::validation("URI must start with file://"))?;

    // Handle optional localhost authority e.g. file://localhost/path
    let path_str = stripped.strip_prefix("localhost").unwrap_or(stripped);

    let mut bytes = Vec::with_capacity(path_str.len());
    let mut chars = path_str.bytes();

    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars
                .next()
                .ok_or_else(|| ModelError::validation("Incomplete percent escape"))?;
            let h2 = chars
                .next()
                .ok_or_else(|| ModelError::validation("Incomplete percent escape"))?;
            let hex_bytes = [h1, h2];
            let hex_str = std::str::from_utf8(&hex_bytes)
                .map_err(|_| ModelError::validation("Invalid hex in percent escape"))?;
            let val = u8::from_str_radix(hex_str, 16)
                .map_err(|_| ModelError::validation("Invalid hex digits in percent escape"))?;
            bytes.push(val);
        } else {
            bytes.push(b);
        }
    }

    let decoded = String::from_utf8(bytes)
        .map_err(|_| ModelError::validation("Decoded URI path is not valid UTF-8"))?;

    Ok(PathBuf::from(decoded))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_path_to_file_uri_standard() {
        let path = Path::new("/home/user/document.txt");
        let uri = path_to_file_uri(path).unwrap();
        assert_eq!(uri, "file:///home/user/document.txt");

        let roundtrip = file_uri_to_path(&uri).unwrap();
        assert_eq!(roundtrip, path);
    }

    #[test]
    fn test_path_to_file_uri_with_spaces() {
        let path = Path::new("/home/user/my documents/vacation photo.jpg");
        let uri = path_to_file_uri(path).unwrap();
        assert_eq!(uri, "file:///home/user/my%20documents/vacation%20photo.jpg");

        let roundtrip = file_uri_to_path(&uri).unwrap();
        assert_eq!(roundtrip, path);
    }

    #[test]
    fn test_path_to_file_uri_with_unicode() {
        let path = Path::new("/tmp/café_résumé.pdf");
        let uri = path_to_file_uri(path).unwrap();
        assert_eq!(uri, "file:///tmp/caf%C3%A9_r%C3%A9sum%C3%A9.pdf");

        let roundtrip = file_uri_to_path(&uri).unwrap();
        assert_eq!(roundtrip, path);
    }

    #[test]
    fn test_path_to_file_uri_with_symbols() {
        let path = Path::new("/tmp/file#1?query=yes%20.txt");
        let uri = path_to_file_uri(path).unwrap();
        assert_eq!(uri, "file:///tmp/file%231%3Fquery%3Dyes%2520.txt");

        let roundtrip = file_uri_to_path(&uri).unwrap();
        assert_eq!(roundtrip, path);
    }

    #[test]
    fn test_file_uri_to_path_localhost_prefix() {
        let uri = "file://localhost/etc/fstab";
        let path = file_uri_to_path(uri).unwrap();
        assert_eq!(path, PathBuf::from("/etc/fstab"));
    }

    #[test]
    fn test_file_uri_to_path_invalid_scheme() {
        let uri = "http://example.com/file.txt";
        let res = file_uri_to_path(uri);
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_file_manager_client_graceful_missing_service() {
        let client = FileManagerClient::new().with_timeout(Duration::from_millis(100));
        let uris = vec!["file:///tmp/test.txt".to_string()];
        // If no file manager is running or session bus absent, must return error gracefully
        let res = client.show_items(&uris, "").await;
        if let Err(SharedError::Ipc(err)) = res {
            assert!(matches!(
                err,
                IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
            ));
        }
    }
}
