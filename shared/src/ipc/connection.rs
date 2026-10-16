//! D-Bus connection factory managing lazy singleton session and system bus connections.

use std::time::Duration;
use tokio::sync::RwLock;
use zbus::Connection;

use crate::errors::{IpcError, Result, SharedError};

/// Default timeout duration for D-Bus IPC calls (2500ms).
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(2500);

static SESSION_BUS: RwLock<Option<Connection>> = RwLock::const_new(None);
static SYSTEM_BUS: RwLock<Option<Connection>> = RwLock::const_new(None);

/// Retrieves or initializes the singleton D-Bus session bus connection.
///
/// If running in an environment without a running D-Bus session bus (e.g. headless CI),
/// this cleanly returns an [`IpcError`] rather than panicking or blocking indefinitely.
///
/// # Errors
///
/// Returns an [`IpcError`] if connecting to the session bus fails or times out.
pub async fn get_session_connection() -> Result<Connection> {
    {
        let reader = SESSION_BUS.read().await;
        if let Some(conn) = reader.as_ref() {
            return Ok(conn.clone());
        }
    }

    let mut writer = SESSION_BUS.write().await;
    if let Some(conn) = writer.as_ref() {
        return Ok(conn.clone());
    }

    let connect_fut = Connection::session();
    let conn = match tokio::time::timeout(DEFAULT_TIMEOUT, connect_fut).await {
        Ok(Ok(c)) => c,
        Ok(Err(err)) => return Err(SharedError::Ipc(IpcError::Zbus(err))),
        Err(_) => {
            let timeout_ms = u64::try_from(DEFAULT_TIMEOUT.as_millis()).unwrap_or(u64::MAX);
            return Err(SharedError::Ipc(IpcError::timeout("Connection::session", timeout_ms)));
        }
    };

    *writer = Some(conn.clone());
    drop(writer);
    Ok(conn)
}

/// Retrieves or initializes the singleton D-Bus system bus connection.
///
/// If running in an environment without a running D-Bus system bus (e.g. non-Linux or container),
/// this cleanly returns an [`IpcError`] rather than panicking or blocking indefinitely.
///
/// # Errors
///
/// Returns an [`IpcError`] if connecting to the system bus fails or times out.
pub async fn get_system_connection() -> Result<Connection> {
    {
        let reader = SYSTEM_BUS.read().await;
        if let Some(conn) = reader.as_ref() {
            return Ok(conn.clone());
        }
    }

    let mut writer = SYSTEM_BUS.write().await;
    if let Some(conn) = writer.as_ref() {
        return Ok(conn.clone());
    }

    let connect_fut = Connection::system();
    let conn = match tokio::time::timeout(DEFAULT_TIMEOUT, connect_fut).await {
        Ok(Ok(c)) => c,
        Ok(Err(err)) => return Err(SharedError::Ipc(IpcError::Zbus(err))),
        Err(_) => {
            let timeout_ms = u64::try_from(DEFAULT_TIMEOUT.as_millis()).unwrap_or(u64::MAX);
            return Err(SharedError::Ipc(IpcError::timeout("Connection::system", timeout_ms)));
        }
    };

    *writer = Some(conn.clone());
    drop(writer);
    Ok(conn)
}

/// Checks whether the session bus is reachable within a short budget.
#[must_use]
pub async fn is_session_bus_available() -> bool {
    get_session_connection().await.is_ok()
}

/// Checks whether the system bus is reachable within a short budget.
#[must_use]
pub async fn is_system_bus_available() -> bool {
    get_system_connection().await.is_ok()
}

/// Explicitly sets the session connection for testing environments.
pub async fn set_session_connection_for_test(conn: Connection) {
    let mut writer = SESSION_BUS.write().await;
    *writer = Some(conn);
}

/// Explicitly sets the system connection for testing environments.
pub async fn set_system_connection_for_test(conn: Connection) {
    let mut writer = SYSTEM_BUS.write().await;
    *writer = Some(conn);
}

/// Resets cached singleton connections (useful for isolated unit/integration tests).
pub async fn reset_connections_for_test() {
    let mut session_writer = SESSION_BUS.write().await;
    *session_writer = None;
    drop(session_writer);
    let mut system_writer = SYSTEM_BUS.write().await;
    *system_writer = None;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_bus_availability_check_does_not_panic() {
        let _ = is_session_bus_available().await;
        let _ = is_system_bus_available().await;
    }

    #[tokio::test]
    async fn test_get_session_connection_error_or_ok() {
        match get_session_connection().await {
            Ok(conn) => {
                assert!(!conn.is_closed());
            }
            Err(SharedError::Ipc(err)) => {
                assert!(matches!(
                    err,
                    IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
                ));
            }
            Err(other) => panic!("Unexpected error type: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_get_system_connection_error_or_ok() {
        match get_system_connection().await {
            Ok(conn) => {
                assert!(!conn.is_closed());
            }
            Err(SharedError::Ipc(err)) => {
                assert!(matches!(
                    err,
                    IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
                ));
            }
            Err(other) => panic!("Unexpected error type: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_reset_connections_clears_state() {
        reset_connections_for_test().await;
        let mut session_writer = SESSION_BUS.write().await;
        *session_writer = None;
        assert!(session_writer.is_none());
        drop(session_writer);

        let mut system_writer = SYSTEM_BUS.write().await;
        *system_writer = None;
        assert!(system_writer.is_none());
        drop(system_writer);
    }
}
