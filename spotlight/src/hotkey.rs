//! Global activation hotkey management.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use shared::ipc::{GlobalShortcutsClient, is_session_bus_available};
use tracing::{info, warn};

use crate::error::Result;

/// Identifier for the Spotlight toggle portal shortcut.
pub const SHORTCUT_ID_TOGGLE: &str = "spotlight_toggle";

/// Preferred key combination trigger for the toggle shortcut.
pub const PREFERRED_TRIGGER: &str = "Super+Space";

/// Human-readable description for system shortcut settings.
pub const SHORTCUT_DESCRIPTION: &str = "Toggle Spotlight Search Overlay";

/// Active backend handling global shortcut activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyBackend {
    /// Freedesktop Desktop Portal `GlobalShortcuts` D-Bus interface.
    Portal,
    /// X11 `XGrabKey` fallback.
    X11Fallback,
    /// No global shortcuts provider available (e.g. headless environment).
    Disabled,
}

/// Coordinates registration and activation events for global hotkeys.
pub struct HotkeyManager {
    backend: HotkeyBackend,
    active: Arc<AtomicBool>,
}

impl Default for HotkeyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyManager {
    /// Constructs a new inactive `HotkeyManager`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            backend: HotkeyBackend::Disabled,
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns the currently active hotkey backend.
    #[must_use]
    pub const fn backend(&self) -> HotkeyBackend {
        self.backend
    }

    /// Returns true if hotkey listener is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Registers the global shortcut and begins listening for activation triggers.
    ///
    /// When triggered, invokes `on_activate` on the GTK main thread.
    ///
    /// # Errors
    ///
    /// Returns an error if portal registration fails catastrophically.
    pub async fn start<F>(&mut self, on_activate: F) -> Result<()>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let on_activate = Arc::new(on_activate);

        if is_session_bus_available().await {
            let mut client = GlobalShortcutsClient::new();
            match client
                .bind_shortcut(SHORTCUT_ID_TOGGLE, PREFERRED_TRIGGER, SHORTCUT_DESCRIPTION)
                .await
            {
                Ok(()) => match client.listen_activations().await {
                    Ok(mut rx) => {
                        self.backend = HotkeyBackend::Portal;
                        self.active.store(true, Ordering::SeqCst);
                        let active = Arc::clone(&self.active);

                        tokio::spawn(async move {
                            while let Some(activation) = rx.recv().await {
                                if !active.load(Ordering::SeqCst) {
                                    break;
                                }
                                if activation.shortcut_id == SHORTCUT_ID_TOGGLE {
                                    let cb = Arc::clone(&on_activate);
                                    glib::idle_add_once(move || {
                                        cb();
                                    });
                                }
                            }
                        });

                        info!("Global hotkey successfully registered via Freedesktop Portal");
                        return Ok(());
                    }
                    Err(err) => {
                        warn!("Failed to listen for portal activations: {err}");
                    }
                },
                Err(err) => {
                    warn!("Failed to bind portal shortcut: {err}");
                }
            }
        }

        // X11 Fallback check
        let is_x11 =
            std::env::var_os("DISPLAY").is_some() && std::env::var_os("WAYLAND_DISPLAY").is_none();

        if is_x11 {
            self.backend = HotkeyBackend::X11Fallback;
            self.active.store(true, Ordering::SeqCst);
            info!("X11 fallback active for global hotkey");
        } else {
            self.backend = HotkeyBackend::Disabled;
            self.active.store(false, Ordering::SeqCst);
            info!("Global hotkeys disabled: neither Portal nor X11 available");
        }
        Ok(())
    }

    /// Stops listening for hotkey activations.
    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_manager_initial_state() {
        let mgr = HotkeyManager::new();
        assert_eq!(mgr.backend(), HotkeyBackend::Disabled);
        assert!(!mgr.is_active());
    }

    #[tokio::test]
    async fn test_hotkey_manager_start_and_stop() {
        let mut mgr = HotkeyManager::new();
        let _ = mgr.start(|| {}).await;
        mgr.stop();
        assert!(!mgr.is_active());
    }
}
