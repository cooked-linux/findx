//! Single-instance D-Bus service and lifecycle arbitration for Spotlight.

use tokio::sync::mpsc;
use tracing::{info, warn};
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};

/// D-Bus service well-known name.
pub const SPOTLIGHT_DBUS_NAME: &str = "org.freedesktop.Spotlight";

/// D-Bus object path.
pub const SPOTLIGHT_DBUS_PATH: &str = "/org/freedesktop/Spotlight";

/// Actions requested by external D-Bus callers or hotkey activations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonAction {
    /// Toggle the visibility of the Spotlight overlay window.
    Toggle,
    /// Present and focus the Spotlight overlay window.
    Show,
    /// Dismiss and hide the Spotlight overlay window.
    Hide,
}

/// D-Bus service interface implementation.
pub struct SpotlightDaemon {
    window_sender: mpsc::UnboundedSender<DaemonAction>,
}

impl SpotlightDaemon {
    /// Constructs a new [`SpotlightDaemon`] dispatching commands to `window_sender`.
    #[must_use]
    pub const fn new(window_sender: mpsc::UnboundedSender<DaemonAction>) -> Self {
        Self { window_sender }
    }
}

#[zbus::interface(name = "org.freedesktop.Spotlight")]
impl SpotlightDaemon {
    fn toggle(&self) {
        let _ = self.window_sender.send(DaemonAction::Toggle);
    }

    fn show(&self) {
        let _ = self.window_sender.send(DaemonAction::Show);
    }

    fn hide(&self) {
        let _ = self.window_sender.send(DaemonAction::Hide);
    }
}

/// D-Bus client proxy for communicating with a running Spotlight instance.
#[zbus::proxy(
    interface = "org.freedesktop.Spotlight",
    default_service = "org.freedesktop.Spotlight",
    default_path = "/org/freedesktop/Spotlight"
)]
pub trait SpotlightClient {
    fn toggle(&self) -> zbus::Result<()>;
    fn show(&self) -> zbus::Result<()>;
    fn hide(&self) -> zbus::Result<()>;
}

/// Outcome of single-instance arbitration.
#[derive(Debug)]
pub enum SingleInstanceDecision {
    /// Successfully acquired D-Bus name and started daemon service.
    AcquiredPrimary(zbus::Connection),
    /// Another instance was running; forwarded action (or logged) and exiting cleanly.
    AlreadyRunningHandled,
    /// D-Bus session bus unavailable (e.g. running in headless CI without dbus-daemon).
    BusUnavailable,
}

/// Performs single-instance arbitration over D-Bus session bus.
///
/// # Errors
/// Returns an error only on unrecoverable D-Bus protocol failure.
pub async fn arbitrate_single_instance(
    _toggle: bool,
    replace: bool,
    action_tx: mpsc::UnboundedSender<DaemonAction>,
) -> Result<SingleInstanceDecision, Box<dyn std::error::Error + Send + Sync>> {
    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(err) => {
            warn!(
                "D-Bus session bus not available ({err}); proceeding without D-Bus single-instance service"
            );
            return Ok(SingleInstanceDecision::BusUnavailable);
        }
    };

    let dbus_proxy = DBusProxy::new(&conn).await?;

    let flags = if replace {
        RequestNameFlags::AllowReplacement | RequestNameFlags::ReplaceExisting
    } else {
        RequestNameFlags::AllowReplacement.into()
    };

    let reply = dbus_proxy
        .request_name(SPOTLIGHT_DBUS_NAME.try_into()?, flags)
        .await?;

    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => {
            let daemon = SpotlightDaemon::new(action_tx);
            conn.object_server().at(SPOTLIGHT_DBUS_PATH, daemon).await?;
            info!("Acquired D-Bus service name '{SPOTLIGHT_DBUS_NAME}'");
            Ok(SingleInstanceDecision::AcquiredPrimary(conn))
        }
        RequestNameReply::Exists | RequestNameReply::InQueue => {
            if replace {
                warn!("Could not replace existing Spotlight owner. Exiting.");
            } else {
                info!("Another Spotlight instance is running; sending Toggle via D-Bus");
                let proxy = SpotlightClientProxy::new(&conn).await?;
                if let Err(err) = proxy.toggle().await {
                    warn!("Failed to invoke Toggle on running instance: {err}");
                }
            }
            Ok(SingleInstanceDecision::AlreadyRunningHandled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_action_variants() {
        assert_eq!(DaemonAction::Toggle, DaemonAction::Toggle);
        assert_ne!(DaemonAction::Show, DaemonAction::Hide);
    }

    #[tokio::test]
    async fn test_daemon_action_channel_dispatch() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let daemon = SpotlightDaemon::new(tx);

        daemon.toggle();
        assert_eq!(rx.recv().await, Some(DaemonAction::Toggle));

        daemon.show();
        assert_eq!(rx.recv().await, Some(DaemonAction::Show));

        daemon.hide();
        assert_eq!(rx.recv().await, Some(DaemonAction::Hide));
    }
}
