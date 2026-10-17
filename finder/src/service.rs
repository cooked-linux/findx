//! D-Bus service implementation for `org.freedesktop.FileManager1`.

use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{info, warn};
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::interface;

use shared::ipc::file_uri_to_path;

/// Well-known D-Bus interface and service name for the Freedesktop file manager.
pub const FILE_MANAGER_DBUS_NAME: &str = "org.freedesktop.FileManager1";

/// Standard D-Bus object path for the Freedesktop file manager.
pub const FILE_MANAGER_DBUS_PATH: &str = "/org/freedesktop/FileManager1";

/// Actions requested by external D-Bus clients via `org.freedesktop.FileManager1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileManagerAction {
    /// Instructs the file manager to display and select the items at `paths`.
    ShowItems {
        /// Files or directories to display and highlight.
        paths: Vec<PathBuf>,
        /// Startup notification ID passed by the calling process.
        startup_id: String,
    },
    /// Instructs the file manager to open folders containing `paths`.
    ShowFolders {
        /// Folder directories to open.
        paths: Vec<PathBuf>,
        /// Startup notification ID passed by the calling process.
        startup_id: String,
    },
    /// Instructs the file manager to display properties / inspector for `paths`.
    ShowItemProperties {
        /// Items to inspect.
        paths: Vec<PathBuf>,
        /// Startup notification ID passed by the calling process.
        startup_id: String,
    },
}

/// D-Bus service implementing `org.freedesktop.FileManager1`.
#[derive(Clone, Debug)]
pub struct FileManagerService {
    action_tx: mpsc::UnboundedSender<FileManagerAction>,
}

impl FileManagerService {
    /// Constructs a new [`FileManagerService`] forwarding actions to `action_tx`.
    #[must_use]
    pub const fn new(action_tx: mpsc::UnboundedSender<FileManagerAction>) -> Self {
        Self { action_tx }
    }
}

#[interface(name = "org.freedesktop.FileManager1")]
#[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
impl FileManagerService {
    /// Displays and selects items at the given `uris`.
    ///
    /// # Errors
    /// Returns an error if any URI is not a valid `file://` URI.
    pub async fn show_items(&self, uris: Vec<String>, startup_id: String) -> zbus::fdo::Result<()> {
        let mut paths = Vec::with_capacity(uris.len());
        for uri in &uris {
            match file_uri_to_path(uri) {
                Ok(path) => paths.push(path),
                Err(err) => {
                    warn!("Invalid URI {uri} in show_items: {err}");
                    return Err(zbus::fdo::Error::InvalidArgs(format!(
                        "Invalid file URI: {uri}"
                    )));
                }
            }
        }
        let _ = self
            .action_tx
            .send(FileManagerAction::ShowItems { paths, startup_id });
        Ok(())
    }

    /// Opens folders containing the given `uris`.
    ///
    /// # Errors
    /// Returns an error if any URI is not a valid `file://` URI.
    pub async fn show_folders(
        &self,
        uris: Vec<String>,
        startup_id: String,
    ) -> zbus::fdo::Result<()> {
        let mut paths = Vec::with_capacity(uris.len());
        for uri in &uris {
            match file_uri_to_path(uri) {
                Ok(path) => paths.push(path),
                Err(err) => {
                    warn!("Invalid URI {uri} in show_folders: {err}");
                    return Err(zbus::fdo::Error::InvalidArgs(format!(
                        "Invalid file URI: {uri}"
                    )));
                }
            }
        }
        let _ = self
            .action_tx
            .send(FileManagerAction::ShowFolders { paths, startup_id });
        Ok(())
    }

    /// Displays properties or inspection information for items at `uris`.
    ///
    /// # Errors
    /// Returns an error if any URI is not a valid `file://` URI.
    pub async fn show_item_properties(
        &self,
        uris: Vec<String>,
        startup_id: String,
    ) -> zbus::fdo::Result<()> {
        let mut paths = Vec::with_capacity(uris.len());
        for uri in &uris {
            match file_uri_to_path(uri) {
                Ok(path) => paths.push(path),
                Err(err) => {
                    warn!("Invalid URI {uri} in show_item_properties: {err}");
                    return Err(zbus::fdo::Error::InvalidArgs(format!(
                        "Invalid file URI: {uri}"
                    )));
                }
            }
        }
        let _ = self
            .action_tx
            .send(FileManagerAction::ShowItemProperties { paths, startup_id });
        Ok(())
    }
}

/// Status of D-Bus service export attempt.
#[derive(Debug)]
pub enum DbusServiceHandle {
    /// Successfully acquired well-known name and registered service on session bus.
    Registered(zbus::Connection),
    /// Another instance or file manager currently holds the name.
    NameAlreadyAcquired,
    /// D-Bus session bus is unavailable (e.g. headless CI).
    BusUnavailable,
}

/// Registers and exports the `org.freedesktop.FileManager1` service on the user's session D-Bus.
///
/// # Errors
/// Returns an error only on unrecoverable D-Bus protocol failure.
pub async fn export_file_manager_service(
    action_tx: mpsc::UnboundedSender<FileManagerAction>,
) -> Result<DbusServiceHandle, Box<dyn std::error::Error + Send + Sync>> {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(err) => {
            warn!(
                "D-Bus session bus unavailable ({err}); proceeding without FileManager1 D-Bus service"
            );
            return Ok(DbusServiceHandle::BusUnavailable);
        }
    };

    let service = FileManagerService::new(action_tx);

    if let Err(err) = conn
        .object_server()
        .at(FILE_MANAGER_DBUS_PATH, service)
        .await
    {
        warn!("Failed to serve FileManager1 at {FILE_MANAGER_DBUS_PATH}: {err}");
        return Ok(DbusServiceHandle::BusUnavailable);
    }

    let dbus_proxy = DBusProxy::new(&conn).await?;
    let flags = RequestNameFlags::AllowReplacement.into();
    let name = match zbus::names::WellKnownName::try_from(FILE_MANAGER_DBUS_NAME) {
        Ok(n) => n,
        Err(err) => {
            warn!("Invalid well-known name {FILE_MANAGER_DBUS_NAME}: {err}");
            return Ok(DbusServiceHandle::BusUnavailable);
        }
    };

    match dbus_proxy.request_name(name, flags).await {
        Ok(RequestNameReply::PrimaryOwner) => {
            info!("Successfully acquired D-Bus name {FILE_MANAGER_DBUS_NAME}");
            Ok(DbusServiceHandle::Registered(conn))
        }
        Ok(
            RequestNameReply::InQueue | RequestNameReply::Exists | RequestNameReply::AlreadyOwner,
        ) => {
            info!("D-Bus name {FILE_MANAGER_DBUS_NAME} already owned or queued");
            Ok(DbusServiceHandle::NameAlreadyAcquired)
        }
        Err(err) => {
            warn!("Error requesting D-Bus name {FILE_MANAGER_DBUS_NAME}: {err}");
            Ok(DbusServiceHandle::BusUnavailable)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_file_manager_service_show_folders_routing() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let service = FileManagerService::new(tx);

        let uris = vec!["file:///tmp/my_test_dir".to_string()];
        let res = service.show_folders(uris, "test_startup".to_string()).await;
        assert!(res.is_ok());

        let action = rx.recv().await.expect("action must be emitted");
        assert_eq!(
            action,
            FileManagerAction::ShowFolders {
                paths: vec![PathBuf::from("/tmp/my_test_dir")],
                startup_id: "test_startup".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn test_file_manager_service_show_items_percent_encoded() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let service = FileManagerService::new(tx);

        let uris = vec!["file:///tmp/my%20folder/my%20doc.txt".to_string()];
        let res = service.show_items(uris, "id123".to_string()).await;
        assert!(res.is_ok());

        let action = rx.recv().await.expect("action must be emitted");
        assert_eq!(
            action,
            FileManagerAction::ShowItems {
                paths: vec![PathBuf::from("/tmp/my folder/my doc.txt")],
                startup_id: "id123".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn test_file_manager_service_show_item_properties() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let service = FileManagerService::new(tx);

        let uris = vec!["file:///home/user/photo.jpg".to_string()];
        let res = service
            .show_item_properties(uris, "id456".to_string())
            .await;
        assert!(res.is_ok());

        let action = rx.recv().await.expect("action must be emitted");
        assert_eq!(
            action,
            FileManagerAction::ShowItemProperties {
                paths: vec![PathBuf::from("/home/user/photo.jpg")],
                startup_id: "id456".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn test_file_manager_service_invalid_uri_error() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let service = FileManagerService::new(tx);

        let uris = vec!["http://example.com/not_file".to_string()];
        let res = service.show_folders(uris, "id".to_string()).await;
        assert!(res.is_err());
    }
}
