//! Spotlight application binary entry point.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use spotlight::hotkey::HotkeyManager;
use spotlight::ui::SpotlightWindow;
use tracing::warn;

const APP_ID: &str = "org.findx.Spotlight";

/// Command line flags controlling Spotlight execution and lifecycle.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CliOptions {
    /// Toggle visibility of a running Spotlight window.
    pub toggle: bool,
    /// Replace an existing running Spotlight daemon.
    pub replace: bool,
    /// Run Spotlight as a persistent background daemon.
    pub daemon: bool,
    /// Print help information.
    pub help: bool,
}

impl CliOptions {
    /// Parses CLI flags from any iterator of string-like arguments.
    #[must_use]
    pub fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: AsRef<str>,
    {
        let mut options = Self::default();
        for arg in args {
            match arg.as_ref() {
                "--toggle" => options.toggle = true,
                "--replace" => options.replace = true,
                "--daemon" => options.daemon = true,
                "-h" | "--help" => options.help = true,
                _ => {}
            }
        }
        options
    }

    /// Parses CLI flags from `std::env::args()`.
    #[must_use]
    pub fn parse() -> Self {
        Self::parse_from(std::env::args().skip(1))
    }
}

/// Initializes structured logging respecting `$RUST_LOG`, defaulting to `info`.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn main() -> glib::ExitCode {
    init_logging();

    let cli = CliOptions::parse();
    if cli.help {
        print_help();
        return glib::ExitCode::SUCCESS;
    }

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(err) => {
            warn!("Failed to initialize tokio runtime: {err}");
            return glib::ExitCode::FAILURE;
        }
    };

    let (daemon_tx, daemon_rx) =
        tokio::sync::mpsc::unbounded_channel::<spotlight::daemon::DaemonAction>();

    let arbitration = rt.block_on(spotlight::daemon::arbitrate_single_instance(
        cli.toggle,
        cli.replace,
        daemon_tx.clone(),
    ));

    let dbus_conn = match arbitration {
        Ok(spotlight::daemon::SingleInstanceDecision::AlreadyRunningHandled) => {
            return glib::ExitCode::SUCCESS;
        }
        Ok(spotlight::daemon::SingleInstanceDecision::AcquiredPrimary(conn)) => Some(conn),
        Ok(spotlight::daemon::SingleInstanceDecision::BusUnavailable) => None,
        Err(err) => {
            warn!("D-Bus single-instance arbitration encountered error: {err}");
            None
        }
    };

    let app = gtk4::Application::builder().application_id(APP_ID).build();

    let window_holder: Rc<RefCell<Option<SpotlightWindow>>> = Rc::new(RefCell::new(None));
    let daemon_rx_holder = Rc::new(RefCell::new(Some(daemon_rx)));

    let is_daemon = cli.daemon;
    let leaked_rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(rt));

    app.connect_activate(glib::clone!(
        #[strong]
        window_holder,
        #[strong]
        daemon_tx,
        #[strong]
        daemon_rx_holder,
        move |app| {
            let mut holder = window_holder.borrow_mut();
            if let Some(win) = holder.as_ref() {
                // Secondary activation (e.g. from compositor shortcut spawn "spotlight")
                win.toggle();
            } else {
                let rx = daemon_rx_holder.borrow_mut().take();
                let win = initialize_active_instance(
                    app,
                    is_daemon,
                    rx,
                    &daemon_tx,
                    leaked_rt,
                    dbus_conn.clone(),
                );
                *holder = Some(win);
            }
        }
    ));

    app.run()
}

fn initialize_active_instance(
    app: &gtk4::Application,
    is_daemon: bool,
    daemon_rx: Option<tokio::sync::mpsc::UnboundedReceiver<spotlight::daemon::DaemonAction>>,
    daemon_tx: &tokio::sync::mpsc::UnboundedSender<spotlight::daemon::DaemonAction>,
    leaked_rt: &'static tokio::runtime::Runtime,
    dbus_conn: Option<zbus::Connection>,
) -> SpotlightWindow {
    // Initial instance startup: hold active as background daemon
    std::mem::forget(app.hold());

    let window = SpotlightWindow::new(Some(app));
    if !is_daemon {
        window.present();
    }

    let win = window.clone();
    if let Some(mut rx) = daemon_rx {
        glib::MainContext::default().spawn_local(async move {
            while let Some(action) = rx.recv().await {
                match action {
                    spotlight::daemon::DaemonAction::Toggle => win.toggle(),
                    spotlight::daemon::DaemonAction::Show => win.present(),
                    spotlight::daemon::DaemonAction::Hide => win.dismiss(),
                }
            }
        });
    }

    // Initialize shared database connection pool & frecency tracker
    let (shared_conn, frecency) = match spotlight::app::init_database_and_frecency() {
        Ok(pair) => pair,
        Err(err) => {
            warn!("Failed to open system database ({err}), falling back to in-memory db");
            match spotlight::app::init_test_database_and_frecency() {
                Ok(fallback) => fallback,
                Err(fatal) => {
                    warn!("Fatal: could not initialize in-memory database: {fatal}");
                    return window;
                }
            }
        }
    };

    // Register all 6 search providers
    let (providers, app_provider) =
        match spotlight::app::init_search_providers(&shared_conn, &frecency) {
            Ok(res) => res,
            Err(err) => {
                warn!("Failed to initialize search providers: {err}");
                return window;
            }
        };

    // Instantiate QueryDispatcher with tokio runtime handle
    let (tx, update_rx) = tokio::sync::mpsc::unbounded_channel();
    let dispatcher =
        spotlight::arbiter::QueryDispatcher::with_handle(providers, tx, leaked_rt.handle().clone());

    // Wire up UI signal wiring and presentation event loop
    spotlight::app::wire_spotlight_application(
        window.clone(),
        dispatcher,
        app_provider,
        frecency,
        update_rx,
    );

    // Hotkey listener on global compositor shortcut
    let hotkey_tx = daemon_tx.clone();
    leaked_rt.spawn(async move {
        let mut hotkey_mgr = HotkeyManager::new();
        if let Err(err) = hotkey_mgr
            .start(move || {
                let _ = hotkey_tx.send(spotlight::daemon::DaemonAction::Toggle);
            })
            .await
        {
            warn!("HotkeyManager failed to start: {err}");
        }
    });

    // Clean teardown & signal trapping (SIGINT / SIGTERM)
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    let win_teardown = window.clone();
    let app_teardown = app.clone();
    glib::MainContext::default().spawn_local(async move {
        if shutdown_rx.recv().await.is_some() {
            win_teardown.dismiss();
            app_teardown.quit();
        }
    });

    spawn_shutdown_listener(
        leaked_rt.handle(),
        shutdown_tx,
        dbus_conn,
        std::sync::Arc::clone(&shared_conn),
    );

    window
}

fn print_help() {
    println!(
        "Spotlight - Fast desktop application and file launcher\n\n\
Usage: spotlight [OPTIONS]\n\n\
Options:\n\
      --toggle     Toggle visibility of the running Spotlight window\n\
      --replace    Replace an existing running Spotlight daemon\n\
      --daemon     Run Spotlight in the background as a daemon\n\
  -h, --help       Print help"
    );
}

fn spawn_shutdown_listener(
    handle: &tokio::runtime::Handle,
    shutdown_tx: tokio::sync::mpsc::UnboundedSender<()>,
    dbus_conn: Option<zbus::Connection>,
    shared_conn: std::sync::Arc<std::sync::Mutex<rusqlite::Connection>>,
) {
    handle.spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let sigint_res = signal(SignalKind::interrupt());
        let sigterm_res = signal(SignalKind::terminate());

        if let (Ok(mut sigint), Ok(mut sigterm)) = (sigint_res, sigterm_res) {
            tokio::select! {
                _ = sigint.recv() => {
                    tracing::info!("Received SIGINT, initiating clean teardown...");
                }
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM, initiating clean teardown...");
                }
            }

            // 1. Drop Wayland exclusive keyboard grabs and quit application
            let _ = shutdown_tx.send(());

            // 2. Release D-Bus well-known name
            if let Some(conn) = dbus_conn
                && let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await
                && let Ok(name) = spotlight::daemon::SPOTLIGHT_DBUS_NAME.try_into()
            {
                let _ = proxy.release_name(name).await;
            }

            // 3. Flush database WAL checkpoints cleanly
            if let Ok(conn) = shared_conn.lock() {
                let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_options_defaults() {
        let opts = CliOptions::default();
        assert!(!opts.toggle);
        assert!(!opts.replace);
        assert!(!opts.daemon);
        assert!(!opts.help);
    }

    #[test]
    fn test_cli_options_parsing() {
        let opts = CliOptions::parse_from(["--toggle", "--daemon"]);
        assert!(opts.toggle);
        assert!(opts.daemon);
        assert!(!opts.replace);
        assert!(!opts.help);

        let opts = CliOptions::parse_from(["--replace"]);
        assert!(!opts.toggle);
        assert!(opts.replace);

        let opts = CliOptions::parse_from(["--help"]);
        assert!(opts.help);

        let opts = CliOptions::parse_from(["-h"]);
        assert!(opts.help);
    }
}
