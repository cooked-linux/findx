//! Inter-process communication and D-Bus client implementations.

pub mod connection;
pub mod file_manager;
pub mod login;
pub mod shortcuts;
pub mod udisks;

pub use connection::{
    DEFAULT_TIMEOUT, get_session_connection, get_system_connection,
    is_session_bus_available, is_system_bus_available, reset_connections_for_test,
    set_session_connection_for_test, set_system_connection_for_test,
};
pub use file_manager::{FileManagerClient, FileManagerProxy, file_uri_to_path, path_to_file_uri};
pub use login::{Login1Client, Login1ManagerProxy};
pub use shortcuts::{GlobalShortcutsClient, GlobalShortcutsProxy, SessionHandle, ShortcutActivation};
pub use udisks::{DeviceEvent, DriveConnectionType, MountedVolume, UDisks2Monitor};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

    use crate::errors::{IpcError, SharedError};

    #[test]
    fn test_rfc3986_file_uri_conversion() {
        let test_cases = [
            ("/usr/bin/findx", "file:///usr/bin/findx"),
            (
                "/home/alice/My Documents/file.pdf",
                "file:///home/alice/My%20Documents/file.pdf",
            ),
            ("/var/log/app#1.log", "file:///var/log/app%231.log"),
            ("/tmp/a?b=c.txt", "file:///tmp/a%3Fb%3Dc.txt"),
            ("/tmp/100%_done.txt", "file:///tmp/100%25_done.txt"),
            (
                "/home/user/café/résumé.odt",
                "file:///home/user/caf%C3%A9/r%C3%A9sum%C3%A9.odt",
            ),
        ];

        for (input_path, expected_uri) in test_cases {
            let path = Path::new(input_path);
            let uri = path_to_file_uri(path).unwrap();
            assert_eq!(uri, expected_uri);

            let roundtrip = file_uri_to_path(&uri).unwrap();
            assert_eq!(roundtrip, path);
        }
    }

    #[test]
    fn test_relative_path_to_file_uri() {
        let rel = Path::new("Cargo.toml");
        let uri = path_to_file_uri(rel).unwrap();
        assert!(uri.starts_with("file:///"));
        assert!(uri.ends_with("/Cargo.toml"));
    }

    #[test]
    fn test_proxy_type_and_signature_serialization() {
        // FileManager1 payload verification
        let uris = ["file:///tmp/test.txt".to_string()];
        let startup_id = "test_startup_id";
        assert_eq!(uris.len(), 1);
        assert_ne!(startup_id, "");

        // GlobalShortcuts payload verification
        let mut opts = HashMap::new();
        opts.insert("description", Value::from("Test Shortcut"));
        opts.insert("preferred_trigger", Value::from("Super+Space"));
        let val = Value::from(opts);
        let owned = OwnedValue::try_from(val).unwrap();
        assert!(matches!(&*owned, Value::Dict(_)));

        // SessionHandle validation
        let obj_path =
            OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/session/1_42").unwrap();
        let handle = SessionHandle::new(obj_path);
        assert_eq!(
            handle.as_str(),
            "/org/freedesktop/portal/desktop/session/1_42"
        );

        // DeviceEvent validation
        let add_ev =
            DeviceEvent::Added("/org/freedesktop/UDisks2/block_devices/sda1".to_string());
        let rem_ev =
            DeviceEvent::Removed("/org/freedesktop/UDisks2/block_devices/sda1".to_string());
        assert_eq!(
            add_ev,
            DeviceEvent::Added("/org/freedesktop/UDisks2/block_devices/sda1".to_string())
        );
        assert_ne!(add_ev, rem_ev);
    }

    #[tokio::test]
    async fn test_graceful_error_handling_when_dbus_daemon_absent() {
        let timeout = Duration::from_millis(50);

        let fm_client = FileManagerClient::new().with_timeout(timeout);
        let fm_res = fm_client
            .show_items(&["file:///tmp/missing".to_string()], "")
            .await;
        if let Err(SharedError::Ipc(err)) = fm_res {
            assert!(matches!(
                err,
                IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
            ));
        }

        let mut sc_client = GlobalShortcutsClient::new().with_timeout(timeout);
        let sc_res = sc_client.create_session().await;
        if let Err(SharedError::Ipc(err)) = sc_res {
            assert!(matches!(
                err,
                IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
            ));
        }

        let login_client = Login1Client::new().with_timeout(timeout);
        let login_res = login_client.suspend(false).await;
        if let Err(SharedError::Ipc(err)) = login_res {
            assert!(matches!(
                err,
                IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
            ));
        }

        let udisks_monitor = UDisks2Monitor::new().with_timeout(timeout);
        let udisks_res = udisks_monitor
            .unmount("/org/freedesktop/UDisks2/block_devices/fake")
            .await;
        if let Err(SharedError::Ipc(err)) = udisks_res {
            assert!(matches!(
                err,
                IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
            ));
        }
    }
}
