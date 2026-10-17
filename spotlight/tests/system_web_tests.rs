//! Integration tests for System Actions, Hardware Controls, Confirmation Chip, and Web Search Provider.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone,
    clippy::assert_is_empty,
    clippy::missing_const_for_fn
)]

use std::time::Instant;

use gtk4::prelude::*;
use shared::models::{SearchCategory, SearchPayload};
use spotlight::arbiter::SearchProvider;
use spotlight::providers::{
    SYSTEM_COMMANDS, SearchEngine, SystemActionProvider, WebSearchProvider, execute_system_action,
    is_destructive_action, match_system_actions, parse_brightness_command, parse_explicit_prefix,
    parse_volume_command,
};
use spotlight::ui::{
    CONFIRM_TIMEOUT_SECS, ConfirmChipWidget, ConfirmState, ConfirmationStatus, NoResultsState,
    NoResultsWidget,
};

#[test]
fn test_system_command_registry_constants() {
    assert_eq!(SYSTEM_COMMANDS.len(), 6);
    let ids: Vec<&str> = SYSTEM_COMMANDS.iter().map(|c| c.id).collect();
    assert!(ids.contains(&"lock"));
    assert!(ids.contains(&"suspend"));
    assert!(ids.contains(&"restart"));
    assert!(ids.contains(&"shutdown"));
    assert!(ids.contains(&"logout"));
    assert!(ids.contains(&"trash"));

    // Destructive flags verification
    assert!(
        !SYSTEM_COMMANDS
            .iter()
            .find(|c| c.id == "lock")
            .unwrap()
            .is_destructive
    );
    assert!(
        !SYSTEM_COMMANDS
            .iter()
            .find(|c| c.id == "suspend")
            .unwrap()
            .is_destructive
    );
    assert!(
        SYSTEM_COMMANDS
            .iter()
            .find(|c| c.id == "restart")
            .unwrap()
            .is_destructive
    );
    assert!(
        SYSTEM_COMMANDS
            .iter()
            .find(|c| c.id == "shutdown")
            .unwrap()
            .is_destructive
    );
    assert!(
        SYSTEM_COMMANDS
            .iter()
            .find(|c| c.id == "logout")
            .unwrap()
            .is_destructive
    );
    assert!(
        SYSTEM_COMMANDS
            .iter()
            .find(|c| c.id == "trash")
            .unwrap()
            .is_destructive
    );
}

#[test]
fn test_command_matching_shut_and_lock() {
    // Exact match for "lock"
    let lock_results = match_system_actions("lock");
    assert!(!lock_results.is_empty());
    let lock_top = &lock_results[0];
    assert_eq!(lock_top.title, "Lock Screen");
    assert_eq!(lock_top.score, 1000.0);
    assert_eq!(lock_top.category, SearchCategory::SystemActions);
    assert_eq!(
        lock_top.payload,
        SearchPayload::SystemAction {
            action: "lock".to_string()
        }
    );

    // Prefix match for "shut" -> matches "Shut Down"
    let shut_results = match_system_actions("shut");
    assert!(!shut_results.is_empty());
    let shut_cmd = shut_results.iter().find(|r| r.title == "Shut Down");
    assert!(shut_cmd.is_some());
    let shut = shut_cmd.unwrap();
    assert_eq!(shut.score, 750.0);
    assert_eq!(
        shut.payload,
        SearchPayload::SystemAction {
            action: "shutdown".to_string()
        }
    );

    // Exact match for "reboot" alias of "restart"
    let reboot_results = match_system_actions("reboot");
    assert!(!reboot_results.is_empty());
    assert_eq!(reboot_results[0].title, "Restart");
    assert_eq!(reboot_results[0].score, 1000.0);
}

#[test]
fn test_volume_parsing() {
    assert_eq!(parse_volume_command("vol 80%"), Some(80));
    assert_eq!(parse_volume_command("volume 120"), Some(100));
    assert_eq!(parse_volume_command("volume 0%"), Some(0));
    assert_eq!(parse_volume_command("vol 55"), Some(55));
    assert_eq!(parse_volume_command("vol 100%"), Some(100));

    // Invalid formats
    assert_eq!(parse_volume_command("volume"), None);
    assert_eq!(parse_volume_command("vol"), None);
    assert_eq!(parse_volume_command("vol abc"), None);
    assert_eq!(parse_volume_command("loudness 50"), None);
}

#[test]
fn test_brightness_parsing() {
    assert_eq!(parse_brightness_command("brightness 80%"), Some(80));
    assert_eq!(parse_brightness_command("bright 50"), Some(50));
    assert_eq!(parse_brightness_command("brightness 150"), Some(100));
    assert_eq!(parse_brightness_command("bright 0%"), Some(0));

    // Invalid formats
    assert_eq!(parse_brightness_command("brightness"), None);
    assert_eq!(parse_brightness_command("bright"), None);
    assert_eq!(parse_brightness_command("screen 80"), None);
}

#[test]
fn test_hardware_control_records() {
    let vol_records = match_system_actions("vol 75%");
    assert!(!vol_records.is_empty());
    assert_eq!(vol_records[0].title, "Set Volume to 75%");
    assert_eq!(vol_records[0].score, 950.0);
    assert_eq!(
        vol_records[0].payload,
        SearchPayload::SystemAction {
            action: "volume:75".to_string()
        }
    );

    let bright_records = match_system_actions("bright 60");
    assert!(!bright_records.is_empty());
    assert_eq!(bright_records[0].title, "Set Brightness to 60%");
    assert_eq!(bright_records[0].score, 950.0);

    let mute_records = match_system_actions("mute");
    assert!(!mute_records.is_empty());
    assert_eq!(mute_records[0].title, "Toggle Mute");
    assert_eq!(mute_records[0].score, 1000.0);
}

#[test]
fn test_is_destructive_action() {
    assert!(is_destructive_action("restart"));
    assert!(is_destructive_action("reboot"));
    assert!(is_destructive_action("shutdown"));
    assert!(is_destructive_action("shut down"));
    assert!(is_destructive_action("logout"));
    assert!(is_destructive_action("trash"));

    assert!(!is_destructive_action("lock"));
    assert!(!is_destructive_action("suspend"));
    assert!(!is_destructive_action("mute"));
    assert!(!is_destructive_action("volume:80"));
    assert!(!is_destructive_action("brightness:50"));
}

#[test]
fn test_web_search_url_formatting() {
    let url = SearchEngine::DuckDuckGo.format_url("findx & rust");
    assert_eq!(url, "https://duckduckgo.com/?q=findx%20%26%20rust");

    let google_url = SearchEngine::Google.format_url("rust 2024");
    assert_eq!(google_url, "https://www.google.com/search?q=rust%202024");

    let wiki_url = SearchEngine::Wikipedia.format_url("Linux kernel");
    assert_eq!(
        wiki_url,
        "https://en.wikipedia.org/w/index.php?search=Linux%20kernel"
    );
}

#[test]
fn test_explicit_prefix_detection() {
    // `? test` routes to default search engine (DuckDuckGo)
    let parsed_question = parse_explicit_prefix("? test");
    assert!(parsed_question.is_some());
    let (engine, query) = parsed_question.unwrap();
    assert_eq!(engine, SearchEngine::DuckDuckGo);
    assert_eq!(query, "test");

    // Google prefix
    let parsed_g = parse_explicit_prefix("g rust programming");
    assert!(parsed_g.is_some());
    let (engine, query) = parsed_g.unwrap();
    assert_eq!(engine, SearchEngine::Google);
    assert_eq!(query, "rust programming");

    // DuckDuckGo prefix
    let parsed_ddg = parse_explicit_prefix("ddg linux");
    assert!(parsed_ddg.is_some());
    let (engine, query) = parsed_ddg.unwrap();
    assert_eq!(engine, SearchEngine::DuckDuckGo);
    assert_eq!(query, "linux");

    // Wikipedia prefix
    let parsed_w = parse_explicit_prefix("w GTK4");
    assert!(parsed_w.is_some());
    let (engine, query) = parsed_w.unwrap();
    assert_eq!(engine, SearchEngine::Wikipedia);
    assert_eq!(query, "GTK4");

    // No prefix
    assert_eq!(parse_explicit_prefix("hello world"), None);
}

#[tokio::test]
async fn test_web_search_provider_search() {
    let provider = WebSearchProvider::new();

    // Explicit prefix -> Elevated Top Hit candidate score 990.0
    let explicit_results = provider
        .search("? findx & rust", None, Instant::now())
        .await
        .unwrap();
    assert_eq!(explicit_results.len(), 1);
    assert_eq!(explicit_results[0].score, 990.0);
    assert_eq!(explicit_results[0].category, SearchCategory::WebSearch);
    assert_eq!(
        explicit_results[0].payload,
        SearchPayload::WebSearch {
            url: "https://duckduckgo.com/?q=findx%20%26%20rust".to_string()
        }
    );

    // Trailing fallback card -> Score 50.0
    let trailing_results = provider
        .search("random query", None, Instant::now())
        .await
        .unwrap();
    assert_eq!(trailing_results.len(), 1);
    assert_eq!(trailing_results[0].score, 50.0);
    assert_eq!(
        trailing_results[0].title,
        "Search the web for 'random query'"
    );

    // Empty query -> Empty results
    let empty_results = provider.search("   ", None, Instant::now()).await.unwrap();
    assert!(empty_results.is_empty());
}

#[tokio::test]
async fn test_system_action_provider_search() {
    let provider = SystemActionProvider::new();
    assert_eq!(provider.id(), "system");
    assert_eq!(provider.display_name(), "System Actions");
    assert!(provider.is_fast_path());

    let results = provider
        .search("suspend", None, Instant::now())
        .await
        .unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].title, "Sleep");
}

#[test]
fn test_confirm_state_lifecycle() {
    let mut state = ConfirmState::new();
    assert_eq!(*state.status(), ConfirmationStatus::Idle);
    assert!(!state.is_awaiting());

    // Request confirmation
    state.request("shutdown", "Shut Down");
    assert!(state.is_awaiting());
    assert_eq!(state.seconds_left(), Some(CONFIRM_TIMEOUT_SECS));

    // Countdown ticks
    assert!(state.tick());
    assert_eq!(state.seconds_left(), Some(CONFIRM_TIMEOUT_SECS - 1));

    // Confirm
    let confirmed_action = state.confirm();
    assert_eq!(confirmed_action, Some("shutdown".to_string()));
    assert_eq!(
        *state.status(),
        ConfirmationStatus::Confirmed("shutdown".to_string())
    );
    assert!(!state.is_awaiting());

    // Timeout / Auto-cancel behavior
    let mut timeout_state = ConfirmState::new();
    timeout_state.request("restart", "Restart");
    for _ in 0..CONFIRM_TIMEOUT_SECS - 1 {
        assert!(timeout_state.tick());
    }
    // Final tick elapses timer
    assert!(!timeout_state.tick());
    assert_eq!(*timeout_state.status(), ConfirmationStatus::Cancelled);
    assert!(!timeout_state.is_awaiting());

    // Manual cancellation
    let mut cancel_state = ConfirmState::new();
    cancel_state.request("logout", "Log Out");
    cancel_state.cancel();
    assert_eq!(*cancel_state.status(), ConfirmationStatus::Cancelled);
    assert!(!cancel_state.is_awaiting());
}

#[test]
fn test_no_results_state() {
    let state = NoResultsState::new("custom unknown token");
    assert_eq!(state.title, "No Results");
    assert_eq!(
        state.subtitle,
        "Nothing on this computer matches “custom unknown token”."
    );
    assert_eq!(
        state.web_action_label,
        "Search the web for “custom unknown token”"
    );
    assert_eq!(
        state.web_url,
        "https://duckduckgo.com/?q=custom%20unknown%20token"
    );
}

#[tokio::test]
async fn test_execute_system_action_mock() {
    // Non-destructive unknown action returns Ok
    assert!(execute_system_action("unknown_action").await.is_ok());

    // Hardware adjustments return Ok
    assert!(execute_system_action("volume:50").await.is_ok());
    assert!(execute_system_action("brightness:75").await.is_ok());
    assert!(execute_system_action("mute").await.is_ok());
}

#[test]
fn test_confirm_chip_widget_gtk_rendering() {
    if gtk4::init().is_ok() {
        let chip = ConfirmChipWidget::new();
        assert!(!chip.is_active());
        assert!(!chip.container().is_visible());

        chip.show_confirmation("shutdown", "Shut Down", |_| {}, || {});
        assert!(chip.is_active());
        assert!(chip.container().is_visible());
        assert!(
            chip.confirm_button
                .label()
                .unwrap()
                .contains("Confirm Shut Down")
        );

        chip.cancel();
        assert!(!chip.is_active());
        assert!(!chip.container().is_visible());
    }
}

#[test]
fn test_no_results_widget_gtk_rendering() {
    if gtk4::init().is_ok() {
        let widget = NoResultsWidget::new("sample search");
        assert_eq!(widget.title_label.text(), "No Results");
        assert!(widget.subtitle_label.text().contains("sample search"));
        assert!(widget.web_card_label.text().contains("sample search"));
        assert_eq!(
            widget.web_url(),
            "https://duckduckgo.com/?q=sample%20search"
        );

        widget.update_query("updated term");
        assert!(widget.subtitle_label.text().contains("updated term"));
        assert!(widget.web_card_label.text().contains("updated term"));
    }
}
