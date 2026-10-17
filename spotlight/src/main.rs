//! Spotlight application binary entry point.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use spotlight::hotkey::HotkeyManager;
use spotlight::ui::SpotlightWindow;
use tracing::warn;

const APP_ID: &str = "org.findx.Spotlight";

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt::init();

    let app = gtk4::Application::builder().application_id(APP_ID).build();

    let window_holder: Rc<RefCell<Option<SpotlightWindow>>> = Rc::new(RefCell::new(None));

    app.connect_activate(glib::clone!(
        #[strong]
        window_holder,
        move |app| {
            let mut holder = window_holder.borrow_mut();
            if let Some(win) = holder.as_ref() {
                // Secondary activation (e.g. from compositor shortcut spawn "spotlight")
                win.toggle();
            } else {
                // Initial instance startup: hold active as background daemon
                std::mem::forget(app.hold());

                let window = SpotlightWindow::new(Some(app));
                window.present();

                let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(8);

                let win = window.clone();
                glib::MainContext::default().spawn_local(async move {
                    while rx.recv().await.is_some() {
                        win.toggle();
                    }
                });

                let rt_res = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build();

                match rt_res {
                    Ok(runtime) => {
                        let leaked_rt: &'static mut tokio::runtime::Runtime =
                            Box::leak(Box::new(runtime));
                        leaked_rt.spawn(async move {
                            let mut hotkey_mgr = HotkeyManager::new();
                            if let Err(err) = hotkey_mgr
                                .start(move || {
                                    let _ = tx.blocking_send(());
                                })
                                .await
                            {
                                warn!("HotkeyManager failed to start: {err}");
                            }
                        });
                    }
                    Err(err) => {
                        warn!("Failed to initialize tokio runtime for hotkeys: {err}");
                    }
                }

                *holder = Some(window);
            }
        }
    ));

    app.run()
}
