//! Finder desktop file manager application binary entry point.

use gtk4::prelude::*;

const APP_ID: &str = "org.findx.Finder";

fn main() -> glib::ExitCode {
    let app = libadwaita::Application::builder()
        .application_id(APP_ID)
        .build();

    app.connect_activate(|app| {
        let win = finder::ui::FinderWindow::new(Some(app.upcast_ref()));
        win.present();
    });

    app.run()
}
