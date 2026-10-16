//! Power and session management via `systemd-logind` (`org.freedesktop.login1.Manager`).

use std::time::Duration;
use zbus::Connection;
use zbus::proxy;

use crate::errors::{IpcError, Result, SharedError};
use crate::ipc::connection::{DEFAULT_TIMEOUT, get_system_connection};

/// D-Bus proxy for `systemd-logind` Manager interface on the system bus.
#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
pub trait Login1Manager {
    /// Puts the system into sleep/suspend mode.
    fn suspend(&self, interactive: bool) -> zbus::Result<()>;

    /// Reboots the system.
    fn reboot(&self, interactive: bool) -> zbus::Result<()>;

    /// Powers off / shuts down the system.
    fn power_off(&self, interactive: bool) -> zbus::Result<()>;

    /// Locks all active user desktop sessions.
    fn lock_sessions(&self) -> zbus::Result<()>;
}

/// High-level client for power and session management.
#[derive(Debug, Clone)]
pub struct Login1Client {
    connection: Option<Connection>,
    timeout: Duration,
}

impl Default for Login1Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Login1Client {
    /// Creates a new `Login1Client` using the lazy singleton system bus connection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            connection: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Creates a new `Login1Client` with an explicit D-Bus connection.
    #[must_use]
    pub const fn with_connection(conn: Connection) -> Self {
        Self {
            connection: Some(conn),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Configures a custom timeout for login manager calls.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn get_conn(&self) -> Result<Connection> {
        if let Some(conn) = &self.connection {
            Ok(conn.clone())
        } else {
            get_system_connection().await
        }
    }

    /// Requests system suspension (sleep to RAM).
    ///
    /// # Errors
    ///
    /// Returns an error if D-Bus communication fails or times out.
    pub async fn suspend(&self, interactive: bool) -> Result<()> {
        #[cfg(test)]
        if self.connection.is_none() {
            return Err(SharedError::Ipc(IpcError::service_unavailable(
                "Login1 power actions are disabled during tests to protect the host machine",
            )));
        }

        let conn = self.get_conn().await?;
        let proxy = Login1ManagerProxy::new(&conn).await?;
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(self.timeout, proxy.suspend(interactive)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "Login1.Suspend",
                timeout_ms,
            ))),
        }
    }

    /// Requests a system reboot.
    ///
    /// # Errors
    ///
    /// Returns an error if D-Bus communication fails or times out.
    pub async fn reboot(&self, interactive: bool) -> Result<()> {
        #[cfg(test)]
        if self.connection.is_none() {
            return Err(SharedError::Ipc(IpcError::service_unavailable(
                "Login1 power actions are disabled during tests to protect the host machine",
            )));
        }

        let conn = self.get_conn().await?;
        let proxy = Login1ManagerProxy::new(&conn).await?;
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(self.timeout, proxy.reboot(interactive)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "Login1.Reboot",
                timeout_ms,
            ))),
        }
    }

    /// Requests a system power off / shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if D-Bus communication fails or times out.
    pub async fn power_off(&self, interactive: bool) -> Result<()> {
        #[cfg(test)]
        if self.connection.is_none() {
            return Err(SharedError::Ipc(IpcError::service_unavailable(
                "Login1 power actions are disabled during tests to protect the host machine",
            )));
        }

        let conn = self.get_conn().await?;
        let proxy = Login1ManagerProxy::new(&conn).await?;
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(self.timeout, proxy.power_off(interactive)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "Login1.PowerOff",
                timeout_ms,
            ))),
        }
    }

    /// Requests locking of all active user sessions.
    ///
    /// # Errors
    ///
    /// Returns an error if D-Bus communication fails or times out.
    pub async fn lock_sessions(&self) -> Result<()> {
        #[cfg(test)]
        if self.connection.is_none() {
            return Err(SharedError::Ipc(IpcError::service_unavailable(
                "Login1 session actions are disabled during tests to protect the host machine",
            )));
        }

        let conn = self.get_conn().await?;
        let proxy = Login1ManagerProxy::new(&conn).await?;
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(self.timeout, proxy.lock_sessions()).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "Login1.LockSessions",
                timeout_ms,
            ))),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_login1_client_creation_and_config() {
        let client = Login1Client::new().with_timeout(Duration::from_millis(500));
        assert_eq!(client.timeout, Duration::from_millis(500));
    }

    #[tokio::test]
    async fn test_login1_client_graceful_missing_service() {
        let client = Login1Client::new().with_timeout(Duration::from_millis(100));
        // Calling lock_sessions without real active session or in headless CI
        // must return a typed error gracefully without panicking.
        let res = client.lock_sessions().await;
        if let Err(SharedError::Ipc(err)) = res {
            assert!(matches!(
                err,
                IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
            ));
        }
    }
}
