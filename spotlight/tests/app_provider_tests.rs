//! Integration and unit tests for Application and Settings Launcher Provider.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone,
    clippy::assert_is_empty,
    clippy::missing_const_for_fn,
    clippy::unchecked_time_subtraction,
    clippy::similar_names
)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use shared::Database;
use shared::desktop::{DesktopApp, sanitize_exec};
use shared::frecency::{EventKind, FrecencyTracker};
use shared::models::SearchCategory;
use spotlight::arbiter::dispatcher::QueryScope;
use spotlight::arbiter::provider::SearchProvider;
use spotlight::providers::{
    ApplicationProvider, compute_ghost_completion, is_settings_panel, wrap_in_terminal_emulator,
};
use spotlight::ui::empty_state::{EmptyStateSelection, fetch_suggested_apps};

fn create_test_tracker() -> Arc<FrecencyTracker> {
    let db = Database::open_in_memory().expect("open in memory test db");
    Arc::new(FrecencyTracker::new(Arc::new(Mutex::new(db.into_inner()))))
}

fn make_test_app(
    desktop_id: &str,
    name: &str,
    exec_raw: &str,
    icon: Option<&str>,
    categories: Vec<&str>,
    keywords: Vec<&str>,
    is_terminal: bool,
) -> DesktopApp {
    DesktopApp {
        desktop_id: desktop_id.to_string(),
        path: PathBuf::from(format!("/usr/share/applications/{desktop_id}")),
        name: name.to_string(),
        generic_name: None,
        comment: None,
        exec_raw: exec_raw.to_string(),
        exec_sanitized: sanitize_exec(exec_raw),
        icon_name: icon.map(ToString::to_string),
        keywords: keywords.into_iter().map(ToString::to_string).collect(),
        categories: categories.into_iter().map(ToString::to_string).collect(),
        mime_types: Vec::new(),
        is_terminal,
        no_display: false,
        hidden: false,
        only_show_in: Vec::new(),
        not_show_in: Vec::new(),
    }
}

#[tokio::test]
async fn test_application_scoring_tiers() {
    let tracker = create_test_tracker();
    let apps = vec![
        make_test_app(
            "google-chrome.desktop",
            "Google Chrome",
            "/usr/bin/google-chrome-stable %U",
            Some("google-chrome"),
            vec!["Network", "WebBrowser"],
            vec!["internet", "browser"],
            false,
        ),
        make_test_app(
            "org.gnome.Calculator.desktop",
            "Calculator",
            "gnome-calculator",
            Some("org.gnome.Calculator"),
            vec!["Utility", "Calculator"],
            vec!["calc", "math"],
            false,
        ),
        make_test_app(
            "code.desktop",
            "Visual Studio Code",
            "code --unity-launch %F",
            Some("vscode"),
            vec!["Development", "IDE"],
            vec!["editor", "programming"],
            false,
        ),
    ];

    let provider = ApplicationProvider::with_apps(apps, tracker);
    let deadline = Instant::now() + Duration::from_millis(50);

    // 1. Exact match gets 1000.0 (or >= 1000.0)
    let results = provider
        .search("Calculator", None, deadline)
        .await
        .expect("search Calculator");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Calculator");
    assert!(
        results[0].score >= 1000.0,
        "Exact match score should be >= 1000.0, got {}",
        results[0].score
    );

    // 2. Prefix match gets 800+
    let results = provider
        .search("Calc", None, deadline)
        .await
        .expect("search Calc");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Calculator");
    assert!(
        results[0].score >= 800.0,
        "Prefix match score should be >= 800.0, got {}",
        results[0].score
    );

    // 3. Acronym ("gc" -> "Google Chrome", "vsc" -> "Visual Studio Code") gets 600+
    let results = provider
        .search("gc", None, deadline)
        .await
        .expect("search gc");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Google Chrome");
    assert!(
        results[0].score >= 600.0,
        "Acronym match score should be >= 600.0, got {}",
        results[0].score
    );

    let results = provider
        .search("vs", None, deadline)
        .await
        .expect("search vs");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Visual Studio Code");
    assert!(
        results[0].score >= 600.0,
        "Acronym match score should be >= 600.0, got {}",
        results[0].score
    );
}

#[test]
fn test_ghost_text_calculation() {
    // "calc" with "Calculator" yields "ulator"
    let ghost = compute_ghost_completion("calc", "Calculator");
    assert_eq!(ghost, Some("ulator"));

    // "term" with "Terminal" yields "inal"
    let ghost = compute_ghost_completion("term", "Terminal");
    assert_eq!(ghost, Some("inal"));

    // Exact match yields empty suffix
    let ghost = compute_ghost_completion("calculator", "Calculator");
    assert_eq!(ghost, Some(""));

    // Non-prefix yields None
    let ghost = compute_ghost_completion("chrome", "Firefox");
    assert_eq!(ghost, None);

    // Blank query yields None
    let ghost = compute_ghost_completion("", "Calculator");
    assert_eq!(ghost, None);

    let ghost = compute_ghost_completion("   ", "Calculator");
    assert_eq!(ghost, None);
}

#[test]
fn test_field_code_stripping() {
    assert_eq!(
        sanitize_exec("google-chrome-stable %U"),
        "google-chrome-stable"
    );
    assert_eq!(
        sanitize_exec("/usr/bin/code --unity-launch %F"),
        "/usr/bin/code --unity-launch"
    );
    assert_eq!(
        sanitize_exec("vlc --started-from-file %U"),
        "vlc --started-from-file"
    );
    assert_eq!(sanitize_exec("gimp-2.10 %f %i -c"), "gimp-2.10 -c");
    assert_eq!(sanitize_exec("htop"), "htop");
}

#[tokio::test]
async fn test_empty_query_state_returns_frecent_apps() {
    let tracker = create_test_tracker();
    let app1 = make_test_app(
        "app1.desktop",
        "Browser",
        "browser",
        None,
        vec!["Network"],
        vec![],
        false,
    );
    let app2 = make_test_app(
        "app2.desktop",
        "Editor",
        "editor",
        None,
        vec!["Development"],
        vec![],
        false,
    );
    let app3 = make_test_app(
        "app3.desktop",
        "Terminal",
        "terminal",
        None,
        vec!["System"],
        vec![],
        false,
    );
    let app4 = make_test_app(
        "app4.desktop",
        "Music",
        "music",
        None,
        vec!["Audio"],
        vec![],
        false,
    );
    let app5 = make_test_app(
        "app5.desktop",
        "Video",
        "video",
        None,
        vec!["Video"],
        vec![],
        false,
    );

    // Record frecency launches: app3 launched 3 times, app1 launched 1 time
    tracker
        .record_event_now("app3.desktop", "app", EventKind::Launch, None)
        .expect("record launch");
    tracker
        .record_event_now("app3.desktop", "app", EventKind::Launch, None)
        .expect("record launch");
    tracker
        .record_event_now("app1.desktop", "app", EventKind::Launch, None)
        .expect("record launch");

    let apps = vec![
        app1.clone(),
        app2.clone(),
        app3.clone(),
        app4.clone(),
        app5.clone(),
    ];

    let provider = ApplicationProvider::with_apps(apps.clone(), Arc::clone(&tracker));

    // When query is empty, provider search returns top 4 frecent applications
    let deadline = Instant::now() + Duration::from_millis(50);
    let results = provider
        .search("", None, deadline)
        .await
        .expect("search empty query");

    assert_eq!(results.len(), 4);
    // app3 should be first (highest frecency score)
    assert_eq!(results[0].title, "Terminal");
    // app1 should be second
    assert_eq!(results[1].title, "Browser");

    // Also test fetch_suggested_apps directly
    let suggested = fetch_suggested_apps(&tracker, &apps, 4);
    assert_eq!(suggested.len(), 4);
    assert_eq!(suggested[0].name, "Terminal");
    assert_eq!(suggested[1].name, "Browser");
}

#[tokio::test]
async fn test_desktop_settings_deep_linking() {
    let tracker = create_test_tracker();
    let apps = vec![
        make_test_app(
            "gnome-wifi-panel.desktop",
            "Wi-Fi",
            "gnome-control-center wifi",
            Some("network-wireless"),
            vec!["Settings", "HardwareSettings"],
            vec!["network", "wireless", "wifi"],
            false,
        ),
        make_test_app(
            "gnome-bluetooth-panel.desktop",
            "Bluetooth",
            "gnome-control-center bluetooth",
            Some("bluetooth"),
            vec!["Settings", "HardwareSettings"],
            vec!["bluetooth", "device"],
            false,
        ),
        make_test_app(
            "gnome-display-panel.desktop",
            "Displays",
            "gnome-control-center display",
            Some("preferences-desktop-display"),
            vec!["Settings", "HardwareSettings"],
            vec!["screen", "monitor", "resolution", "scale"],
            false,
        ),
        make_test_app(
            "gnome-sound-panel.desktop",
            "Sound",
            "gnome-control-center sound",
            Some("multimedia-volume-control"),
            vec!["Settings", "HardwareSettings"],
            vec!["audio", "volume", "output", "speaker"],
            false,
        ),
    ];

    assert!(is_settings_panel(&apps[0]));
    assert!(is_settings_panel(&apps[1]));
    assert!(is_settings_panel(&apps[2]));
    assert!(is_settings_panel(&apps[3]));

    let provider = ApplicationProvider::with_apps(apps, tracker);
    let deadline = Instant::now() + Duration::from_millis(50);

    // Route keyword query "wifi"
    let results = provider
        .search("wifi", None, deadline)
        .await
        .expect("search wifi");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Wi-Fi");
    assert_eq!(results[0].category, SearchCategory::SystemActions);

    // Route keyword query "display scale"
    let results = provider
        .search("display scale", None, deadline)
        .await
        .expect("search display scale");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Displays");
    assert_eq!(results[0].category, SearchCategory::SystemActions);

    // Route keyword query "audio output"
    let results = provider
        .search("audio output", None, deadline)
        .await
        .expect("search audio output");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Sound");
    assert_eq!(results[0].category, SearchCategory::SystemActions);

    // Route keyword query "bluetooth"
    let results = provider
        .search("bluetooth", None, deadline)
        .await
        .expect("search bluetooth");
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Bluetooth");
    assert_eq!(results[0].category, SearchCategory::SystemActions);
}

#[test]
fn test_terminal_application_wrapping() {
    let wrapped = spotlight::providers::wrap_in_terminal("kitty", "htop");
    assert_eq!(wrapped.0, "kitty");
    assert_eq!(wrapped.1, vec!["-e".to_string(), "htop".to_string()]);

    let wrapped_with_args =
        spotlight::providers::wrap_in_terminal("alacritty", "tail -f /var/log/syslog");
    assert_eq!(wrapped_with_args.0, "alacritty");
    assert_eq!(
        wrapped_with_args.1,
        vec!["-e".to_string(), "tail -f /var/log/syslog".to_string()]
    );

    // If a default terminal exists on the system, wrap_in_terminal_emulator should succeed
    if let Some(term) = spotlight::providers::find_default_terminal() {
        let res = wrap_in_terminal_emulator("htop").expect("wrap with default terminal");
        assert_eq!(res.0, term);
        assert_eq!(res.1, vec!["-e".to_string(), "htop".to_string()]);
    }
}

#[tokio::test]
async fn test_provider_scope_and_deadline() {
    let tracker = create_test_tracker();
    let apps = vec![make_test_app(
        "app.desktop",
        "App",
        "app",
        None,
        vec![],
        vec![],
        false,
    )];
    let provider = ApplicationProvider::with_apps(apps, tracker);

    assert_eq!(provider.id(), "applications");
    assert_eq!(provider.display_name(), "Applications");
    assert!(provider.is_fast_path());

    assert!(provider.handles_scope(QueryScope::All));
    assert!(provider.handles_scope(QueryScope::Apps));
    assert!(!provider.handles_scope(QueryScope::Files));
    assert!(!provider.handles_scope(QueryScope::Calculator));
    assert!(!provider.handles_scope(QueryScope::Dictionary));
    assert!(!provider.handles_scope(QueryScope::Web));

    // Deadline already passed
    let expired_deadline = Instant::now() - Duration::from_millis(10);
    let err = provider
        .search("query", None, expired_deadline)
        .await
        .expect_err("should timeout");
    assert!(matches!(
        err,
        spotlight::arbiter::provider::ProviderError::Timeout(_)
    ));
}

#[test]
fn test_empty_state_selection_model() {
    let mut sel = EmptyStateSelection::new(3);
    assert_eq!(sel.selected_index(), Some(0));

    sel.select_next();
    assert_eq!(sel.selected_index(), Some(1));

    sel.select_next();
    assert_eq!(sel.selected_index(), Some(2));

    // Wrap around to 0
    sel.select_next();
    assert_eq!(sel.selected_index(), Some(0));

    // Wrap backward to 2
    sel.select_prev();
    assert_eq!(sel.selected_index(), Some(2));

    sel.select_prev();
    assert_eq!(sel.selected_index(), Some(1));

    // Update count down to 1
    sel.update_count(1);
    assert_eq!(sel.selected_index(), Some(0));

    // Update count to 0
    sel.update_count(0);
    assert_eq!(sel.selected_index(), None);
}
