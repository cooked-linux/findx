//! Finder desktop file manager application binary entry point.

use finder::app::FinderApp;
use tracing::warn;

/// Initializes structured logging respecting `$RUST_LOG`, defaulting to `info`.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn main() -> glib::ExitCode {
    init_logging();

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => Some(r),
        Err(err) => {
            warn!("Failed to start Tokio runtime ({err}); proceeding without background async IPC");
            None
        }
    };

    let _guard = rt.as_ref().map(tokio::runtime::Runtime::enter);

    let app = FinderApp::new();
    app.run()
}
