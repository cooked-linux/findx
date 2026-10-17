//! Wayland global shortcuts client via XDG Desktop Portal (`org.freedesktop.portal.GlobalShortcuts`).

use std::collections::HashMap;
use std::time::Duration;
use zbus::Connection;
use zbus::proxy;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::errors::{IpcError, Result, SharedError};
use crate::ipc::connection::{DEFAULT_TIMEOUT, get_session_connection};
use zbus::export::ordered_stream::OrderedStreamExt;

/// Session handle representing an active global shortcuts session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionHandle {
    /// Object path of the portal session.
    pub path: OwnedObjectPath,
}

impl SessionHandle {
    /// Creates a new `SessionHandle` wrapping the specified object path.
    #[must_use]
    pub const fn new(path: OwnedObjectPath) -> Self {
        Self { path }
    }

    /// Returns the object path string reference.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.path.as_str()
    }
}

/// Event emitted when a registered global shortcut is activated.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShortcutActivation {
    /// Identifier of the activated shortcut.
    pub shortcut_id: String,
    /// Activation timestamp from compositor.
    pub timestamp: u64,
}

/// D-Bus proxy for `org.freedesktop.portal.GlobalShortcuts`.
#[proxy(
    interface = "org.freedesktop.portal.GlobalShortcuts",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
pub trait GlobalShortcuts {
    /// Creates a new global shortcut session.
    fn create_session(&self, options: &HashMap<&str, Value<'_>>) -> zbus::Result<OwnedObjectPath>;

    /// Binds shortcuts to the given session handle.
    fn bind_shortcuts(
        &self,
        session_handle: &ObjectPath<'_>,
        shortcuts: &[(&str, HashMap<&str, Value<'_>>)],
        parent_window: &str,
        options: &HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;

    /// Signal emitted when a registered shortcut is triggered.
    #[zbus(signal)]
    fn activated(
        &self,
        session_handle: OwnedObjectPath,
        shortcut_id: String,
        timestamp: u64,
        options: HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;
}

/// High-level client for managing global shortcuts.
#[derive(Debug, Clone)]
pub struct GlobalShortcutsClient {
    connection: Option<Connection>,
    session: Option<SessionHandle>,
    timeout: Duration,
}

impl Default for GlobalShortcutsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalShortcutsClient {
    /// Creates a new `GlobalShortcutsClient` with default settings.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            connection: None,
            session: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Creates a `GlobalShortcutsClient` with an explicit connection.
    #[must_use]
    pub const fn with_connection(conn: Connection) -> Self {
        Self {
            connection: Some(conn),
            session: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Configures custom timeout for portal calls.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Gets current active session handle, if created.
    #[must_use]
    pub const fn session(&self) -> Option<&SessionHandle> {
        self.session.as_ref()
    }

    async fn get_conn(&self) -> Result<Connection> {
        if let Some(conn) = &self.connection {
            Ok(conn.clone())
        } else {
            get_session_connection().await
        }
    }

    /// Creates or retrieves a portal shortcut session.
    ///
    /// # Errors
    ///
    /// Returns an error if portal interaction fails or times out.
    pub async fn create_session(&mut self) -> Result<SessionHandle> {
        if let Some(existing) = &self.session {
            return Ok(existing.clone());
        }

        let conn = self.get_conn().await?;
        let proxy = GlobalShortcutsProxy::new(&conn).await?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        let path = match tokio::time::timeout(self.timeout, proxy.create_session(&options)).await {
            Ok(Ok(p)) => p,
            Ok(Err(err)) => return Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => {
                return Err(SharedError::Ipc(IpcError::timeout(
                    "GlobalShortcuts.CreateSession",
                    timeout_ms,
                )));
            }
        };

        let handle = SessionHandle::new(path);
        self.session = Some(handle.clone());
        Ok(handle)
    }

    /// Binds a shortcut key combination to the active session.
    ///
    /// # Errors
    ///
    /// Returns an error if portal registration fails or times out.
    pub async fn bind_shortcut(
        &mut self,
        shortcut_id: &str,
        preferred_trigger: &str,
        description: &str,
    ) -> Result<()> {
        let session = self.create_session().await?;
        let conn = self.get_conn().await?;
        let proxy = GlobalShortcutsProxy::new(&conn).await?;

        let mut shortcut_opts: HashMap<&str, Value<'_>> = HashMap::new();
        shortcut_opts.insert("description", Value::from(description));
        shortcut_opts.insert("preferred_trigger", Value::from(preferred_trigger));

        let shortcuts = [(shortcut_id, shortcut_opts)];
        let call_opts: HashMap<&str, Value<'_>> = HashMap::new();
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(
            self.timeout,
            proxy.bind_shortcuts(&session.path.as_ref(), &shortcuts, "", &call_opts),
        )
        .await
        {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "GlobalShortcuts.BindShortcuts",
                timeout_ms,
            ))),
        }
    }

    /// Listens for `Activated` signals from the portal.
    ///
    /// # Errors
    ///
    /// Returns an error if the D-Bus connection or signal stream cannot be initialized.
    pub async fn listen_activations(
        &self,
    ) -> Result<tokio::sync::mpsc::Receiver<ShortcutActivation>> {
        let conn = self.get_conn().await?;
        let proxy = GlobalShortcutsProxy::new(&conn).await?;
        let mut stream = proxy.receive_activated().await?;
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            while let Some(signal) = stream.next().await {
                if let Ok(args) = signal.args() {
                    let activation = ShortcutActivation {
                        shortcut_id: args.shortcut_id,
                        timestamp: args.timestamp,
                    };
                    if tx.send(activation).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_session_handle_methods() {
        let path = OwnedObjectPath::try_from("/org/freedesktop/portal/desktop/session/1_23/findx")
            .unwrap();
        let handle = SessionHandle::new(path);
        assert_eq!(
            handle.as_str(),
            "/org/freedesktop/portal/desktop/session/1_23/findx"
        );
    }

    #[test]
    fn test_shortcut_activation_serde() {
        let activation = ShortcutActivation {
            shortcut_id: "open_overlay".to_string(),
            timestamp: 123_456_789,
        };

        let json = serde_json::to_string(&activation).unwrap();
        let deserialized: ShortcutActivation = serde_json::from_str(&json).unwrap();
        assert_eq!(activation, deserialized);
    }

    #[tokio::test]
    async fn test_shortcuts_client_graceful_missing_portal() {
        let mut client = GlobalShortcutsClient::new().with_timeout(Duration::from_millis(100));
        assert!(client.session().is_none());

        let res = client
            .bind_shortcut("trigger_overlay", "Super+Space", "Toggle FindX Overlay")
            .await;
        if let Err(SharedError::Ipc(err)) = res {
            assert!(matches!(
                err,
                IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
            ));
        }
    }
}
