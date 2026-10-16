//! Inter-process communication and D-Bus error definitions.

/// Errors arising from D-Bus interactions, system daemons, or portals.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Underlying zbus D-Bus communication error.
    #[error("d-bus ipc error: {0}")]
    Zbus(#[from] zbus::Error),

    /// Target desktop service or daemon is not registered or running on the bus.
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

    /// IPC method invocation exceeded the specified timeout deadline.
    #[error("ipc call to '{method}' timed out after {timeout_ms}ms")]
    Timeout {
        /// D-Bus method name that timed out.
        method: String,
        /// Timeout duration in milliseconds.
        timeout_ms: u64,
    },

    /// XDG desktop portal interaction failed.
    #[error("desktop portal error: {0}")]
    PortalError(String),

    /// Daemon returned an unexpected, malformed, or invalid response payload.
    #[error("invalid ipc response: {0}")]
    InvalidResponse(String),
}

impl IpcError {
    /// Constructs an [`IpcError::ServiceUnavailable`] variant.
    #[must_use]
    pub fn service_unavailable(service: impl Into<String>) -> Self {
        Self::ServiceUnavailable(service.into())
    }

    /// Constructs an [`IpcError::Timeout`] variant.
    #[must_use]
    pub fn timeout(method: impl Into<String>, timeout_ms: u64) -> Self {
        Self::Timeout {
            method: method.into(),
            timeout_ms,
        }
    }

    /// Constructs an [`IpcError::PortalError`] variant.
    #[must_use]
    pub fn portal_error(message: impl Into<String>) -> Self {
        Self::PortalError(message.into())
    }

    /// Constructs an [`IpcError::InvalidResponse`] variant.
    #[must_use]
    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self::InvalidResponse(message.into())
    }
}
