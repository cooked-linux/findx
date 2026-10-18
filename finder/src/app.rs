//! Application controller and command-line argument dispatcher for Finder.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::prelude::*;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::coordinator::NavigationCoordinator;
use crate::operations::OperationsQueue;
use crate::operations::folder_size::FolderSizeManager;
use crate::service::{DbusServiceHandle, FileManagerAction, export_file_manager_service};
use crate::ui::FinderWindow;
use crate::undo::{
    UndoManager, connect_to_queue, default_journal_path, load_journal, save_journal,
};

/// Well-known application ID for the Finder desktop shell.
pub const FINDER_APP_ID: &str = "org.freedesktop.findx.Finder";

/// Parsed command line arguments controlling Finder window instantiation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinderCliArgs {
    /// Print version information and exit.
    pub show_version: bool,
    /// Force opening a new window instance even if another window is already active.
    pub new_window: bool,
    /// Initial directory path to display.
    pub target_path: Option<PathBuf>,
}

impl FinderCliArgs {
    /// Parses CLI flags and positional path arguments from an iterator of string-like arguments.
    #[must_use]
    pub fn parse<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: AsRef<OsStr>,
    {
        let mut show_version = false;
        let mut new_window = false;
        let mut target_path = None;

        let mut iter = args.into_iter();
        // Skip argv[0] if present
        let _argv0 = iter.next();

        for arg in iter {
            let lossy = arg.as_ref().to_string_lossy();
            let s = lossy.as_ref();
            match s {
                "--version" | "-v" | "-V" => {
                    show_version = true;
                }
                "--new-window" | "-n" => {
                    new_window = true;
                }
                path_str if !path_str.starts_with('-') && target_path.is_none() => {
                    target_path = Some(PathBuf::from(path_str));
                }
                _ => {}
            }
        }

        Self {
            show_version,
            new_window,
            target_path,
        }
    }
}

/// Resolves the effective target directory path from optional CLI arg and working directory.
#[must_use]
pub fn resolve_target_path(target: Option<&Path>, cwd: Option<&Path>) -> PathBuf {
    match target {
        Some(p) if p.is_absolute() => p.to_path_buf(),
        Some(p) => cwd.map_or_else(|| p.to_path_buf(), |dir| dir.join(p)),
        None => std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from),
    }
}

/// Primary Finder application controller managing single-instance lifecycle, windows, and D-Bus.
pub struct FinderApp {
    /// Inner libadwaita application instance.
    pub app: libadwaita::Application,
    /// Active top-level Finder windows.
    pub windows: Rc<RefCell<Vec<FinderWindow>>>,
    /// Shared background file operations queue engine.
    pub operations_queue: Arc<OperationsQueue>,
    /// Shared multi-level transactional undo/redo manager.
    pub undo_manager: Arc<Mutex<UndoManager>>,
    /// Shared asynchronous folder size calculation engine.
    pub folder_size_manager: Arc<FolderSizeManager>,
    action_tx: mpsc::UnboundedSender<FileManagerAction>,
    dbus_handle: Rc<RefCell<Option<DbusServiceHandle>>>,
}

impl Default for FinderApp {
    fn default() -> Self {
        Self::new()
    }
}

impl FinderApp {
    /// Constructs a new [`FinderApp`] wrapping `libadwaita::Application`.
    #[must_use]
    pub fn new() -> Self {
        let app = libadwaita::Application::builder()
            .application_id(FINDER_APP_ID)
            .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
            .build();

        let (action_tx, action_rx) = mpsc::unbounded_channel::<FileManagerAction>();
        let windows: Rc<RefCell<Vec<FinderWindow>>> = Rc::new(RefCell::new(Vec::new()));
        let dbus_handle = Rc::new(RefCell::new(None));

        // Load persistent undo journal if available, falling back to clean manager
        let journal_path = default_journal_path();
        let undo_manager = Arc::new(Mutex::new(
            load_journal(&journal_path, 50).unwrap_or_else(|_| UndoManager::new()),
        ));

        let operations_queue = Arc::new(OperationsQueue::new(2));
        connect_to_queue(&operations_queue, Arc::clone(&undo_manager));

        let folder_size_manager = Arc::new(FolderSizeManager::default_manager_fallback());

        app.connect_startup(|_| {
            crate::ui::theme::install_finder_theme();
        });

        Self::wire_command_line_and_activate(
            &app,
            &windows,
            &operations_queue,
            &undo_manager,
            &folder_size_manager,
        );
        Self::wire_dbus_actions(
            &app,
            &windows,
            action_rx,
            &operations_queue,
            &undo_manager,
            &folder_size_manager,
        );
        Self::wire_shutdown_signals(&app);
        Self::wire_shutdown_lifecycle(&app, &operations_queue, &undo_manager);

        Self {
            app,
            windows,
            operations_queue,
            undo_manager,
            folder_size_manager,
            action_tx,
            dbus_handle,
        }
    }

    fn create_window(
        app: &libadwaita::Application,
        target_dir: &Path,
        operations_queue: &Arc<OperationsQueue>,
        undo_manager: &Arc<Mutex<UndoManager>>,
        folder_size_manager: &Arc<FolderSizeManager>,
    ) -> FinderWindow {
        let coordinator = Rc::new(NavigationCoordinator::with_managers(
            target_dir,
            Arc::clone(operations_queue),
            Arc::clone(undo_manager),
            Arc::clone(folder_size_manager),
        ));
        FinderWindow::with_coordinator(Some(app.upcast_ref()), coordinator)
    }

    fn wire_command_line_and_activate(
        app: &libadwaita::Application,
        windows: &Rc<RefCell<Vec<FinderWindow>>>,
        operations_queue: &Arc<OperationsQueue>,
        undo_manager: &Arc<Mutex<UndoManager>>,
        folder_size_manager: &Arc<FolderSizeManager>,
    ) {
        let wins_cmd = Rc::clone(windows);
        let queue_cmd = Arc::clone(operations_queue);
        let undo_cmd = Arc::clone(undo_manager);
        let size_cmd = Arc::clone(folder_size_manager);

        app.connect_command_line(move |app, cmdline| {
            let args = cmdline.arguments();
            let parsed = FinderCliArgs::parse(&args);

            if parsed.show_version {
                println!("findx {}", env!("CARGO_PKG_VERSION"));
                return glib::ExitCode::SUCCESS.value();
            }

            let cwd = cmdline.cwd();
            let target_dir = resolve_target_path(parsed.target_path.as_deref(), cwd.as_deref());

            let mut wins = wins_cmd.borrow_mut();
            wins.retain(|w| w.window.is_visible());

            if parsed.new_window || wins.is_empty() {
                let win = Self::create_window(app, &target_dir, &queue_cmd, &undo_cmd, &size_cmd);
                win.present();
                wins.push(win);
            } else if let Some(last_win) = wins.last() {
                last_win.navigate_to(&target_dir);
                last_win.present();
            }

            glib::ExitCode::SUCCESS.value()
        });

        let wins_act = Rc::clone(windows);
        let queue_act = Arc::clone(operations_queue);
        let undo_act = Arc::clone(undo_manager);
        let size_act = Arc::clone(folder_size_manager);

        app.connect_activate(move |app| {
            let mut wins = wins_act.borrow_mut();
            wins.retain(|w| w.window.is_visible());

            if let Some(win) = wins.last() {
                win.present();
            } else {
                let default_path =
                    std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
                let win = Self::create_window(app, &default_path, &queue_act, &undo_act, &size_act);
                win.present();
                wins.push(win);
            }
        });
    }

    fn wire_dbus_actions(
        app: &libadwaita::Application,
        windows: &Rc<RefCell<Vec<FinderWindow>>>,
        mut action_rx: mpsc::UnboundedReceiver<FileManagerAction>,
        operations_queue: &Arc<OperationsQueue>,
        undo_manager: &Arc<Mutex<UndoManager>>,
        folder_size_manager: &Arc<FolderSizeManager>,
    ) {
        let wins_dbus = Rc::clone(windows);
        let app_weak = app.downgrade();
        let queue = Arc::clone(operations_queue);
        let undo = Arc::clone(undo_manager);
        let size_mgr = Arc::clone(folder_size_manager);

        glib::spawn_future_local(async move {
            while let Some(action) = action_rx.recv().await {
                let Some(app) = app_weak.upgrade() else {
                    break;
                };

                match action {
                    FileManagerAction::ShowFolders { paths, .. } => {
                        let target = paths.first().cloned().unwrap_or_else(|| {
                            std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from)
                        });
                        let mut wins = wins_dbus.borrow_mut();
                        wins.retain(|w| w.window.is_visible());

                        if let Some(win) = wins.last() {
                            win.navigate_to(&target);
                            win.present();
                        } else {
                            let win = Self::create_window(&app, &target, &queue, &undo, &size_mgr);
                            win.present();
                            wins.push(win);
                        }
                    }
                    FileManagerAction::ShowItems { paths, .. }
                    | FileManagerAction::ShowItemProperties { paths, .. } => {
                        if let Some(first) = paths.first() {
                            let (folder, item) = if first.is_dir() {
                                (first.clone(), None)
                            } else {
                                (
                                    first
                                        .parent()
                                        .unwrap_or_else(|| Path::new("/"))
                                        .to_path_buf(),
                                    Some(first.as_path()),
                                )
                            };

                            let mut wins = wins_dbus.borrow_mut();
                            wins.retain(|w| w.window.is_visible());

                            if let Some(win) = wins.last() {
                                win.navigate_to(&folder);
                                win.coordinator.set_selection(item);
                                win.present();
                            } else {
                                let win =
                                    Self::create_window(&app, &folder, &queue, &undo, &size_mgr);
                                win.coordinator.set_selection(item);
                                win.present();
                                wins.push(win);
                            }
                        }
                    }
                }
            }
        });
    }

    fn wire_shutdown_lifecycle(
        app: &libadwaita::Application,
        operations_queue: &Arc<OperationsQueue>,
        undo_manager: &Arc<Mutex<UndoManager>>,
    ) {
        let queue = Arc::clone(operations_queue);
        let undo = Arc::clone(undo_manager);

        app.connect_shutdown(move |_| {
            info!(
                "Finder application shutting down; persisting undo journal and cancelling operations"
            );
            let path = default_journal_path();
            if let Ok(guard) = undo.lock() {
                match save_journal(&path, &guard) {
                    Ok(()) => info!("Successfully saved undo journal to {:?}", path),
                    Err(err) => warn!("Failed to persist undo journal on shutdown: {err}"),
                }
            }
            queue.cancel_all_operations();
        });
    }

    /// Performs graceful shutdown: persists undo journal and cancels background operations.
    pub fn shutdown(&self) {
        let path = default_journal_path();
        if let Ok(guard) = self.undo_manager.lock()
            && let Err(err) = save_journal(&path, &guard)
        {
            warn!("Failed to persist undo journal on shutdown: {err}");
        }
        self.operations_queue.cancel_all_operations();
    }

    fn wire_shutdown_signals(app: &libadwaita::Application) {
        let app_weak_sigint = app.downgrade();
        glib::source::unix_signal_add_local(2, move || {
            info!("Received SIGINT; initiating graceful shutdown");
            if let Some(app) = app_weak_sigint.upgrade() {
                app.quit();
            }
            glib::ControlFlow::Break
        });

        let app_weak_sigterm = app.downgrade();
        glib::source::unix_signal_add_local(15, move || {
            info!("Received SIGTERM; initiating graceful shutdown");
            if let Some(app) = app_weak_sigterm.upgrade() {
                app.quit();
            }
            glib::ControlFlow::Break
        });
    }

    /// Exports the `org.freedesktop.FileManager1` D-Bus service using a background Tokio task.
    pub fn export_dbus_service(&self) {
        let tx = self.action_tx.clone();
        let handle_holder = Rc::clone(&self.dbus_handle);

        glib::spawn_future_local(async move {
            match export_file_manager_service(tx).await {
                Ok(handle) => {
                    info!("FileManager1 D-Bus service status: {handle:?}");
                    *handle_holder.borrow_mut() = Some(handle);
                }
                Err(err) => {
                    warn!("Failed to export FileManager1 D-Bus service: {err}");
                }
            }
        });
    }

    /// Enters the application event loop and blocks until application quits.
    #[must_use]
    pub fn run(&self) -> glib::ExitCode {
        self.export_dbus_service();
        self.app.run()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cli_args_empty() {
        let args = vec!["findx".to_string()];
        let parsed = FinderCliArgs::parse(&args);
        assert!(!parsed.show_version);
        assert!(!parsed.new_window);
        assert_eq!(parsed.target_path, None);
    }

    #[test]
    fn test_parse_cli_args_version() {
        let args = vec!["findx".to_string(), "--version".to_string()];
        let parsed = FinderCliArgs::parse(&args);
        assert!(parsed.show_version);
    }

    #[test]
    fn test_parse_cli_args_path() {
        let args = vec!["findx".to_string(), "/home/user/Downloads".to_string()];
        let parsed = FinderCliArgs::parse(&args);
        assert!(!parsed.show_version);
        assert!(!parsed.new_window);
        assert_eq!(
            parsed.target_path,
            Some(PathBuf::from("/home/user/Downloads"))
        );
    }

    #[test]
    fn test_parse_cli_args_new_window_and_path() {
        let args = vec![
            "findx".to_string(),
            "--new-window".to_string(),
            "/tmp".to_string(),
        ];
        let parsed = FinderCliArgs::parse(&args);
        assert!(!parsed.show_version);
        assert!(parsed.new_window);
        assert_eq!(parsed.target_path, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn test_resolve_target_path_absolute() {
        let path = Path::new("/var/log");
        let resolved = resolve_target_path(Some(path), None);
        assert_eq!(resolved, PathBuf::from("/var/log"));
    }

    #[test]
    fn test_resolve_target_path_relative() {
        let rel = Path::new("sub/folder");
        let cwd = Path::new("/home/user");
        let resolved = resolve_target_path(Some(rel), Some(cwd));
        assert_eq!(resolved, PathBuf::from("/home/user/sub/folder"));
    }
}
