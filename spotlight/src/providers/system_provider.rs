//! System session control, hardware power management, and volume/brightness commands.

use std::time::Instant;

use shared::ipc::Login1Client;
use shared::models::{SearchCategory, SearchPayload, SearchRecord};

use crate::arbiter::dispatcher::QueryScope;
use crate::arbiter::provider::{ProviderError, SearchProvider};

/// Specification of a static desktop system command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemCommand {
    /// Unique command identifier (e.g. `"lock"`, `"shutdown"`).
    pub id: &'static str,
    /// Accepted aliases and keywords that trigger this action.
    pub aliases: &'static [&'static str],
    /// Primary user-visible title.
    pub title: &'static str,
    /// Descriptive explanation of the command effect.
    pub subtitle: &'static str,
    /// Freedesktop icon theme name.
    pub icon_name: &'static str,
    /// Whether this action requires user confirmation before execution.
    pub is_destructive: bool,
}

/// Static registry of standard desktop session and power commands.
pub const SYSTEM_COMMANDS: &[SystemCommand] = &[
    SystemCommand {
        id: "lock",
        aliases: &["lock", "lock screen"],
        title: "Lock Screen",
        subtitle: "Lock the current session",
        icon_name: "system-lock-screen",
        is_destructive: false,
    },
    SystemCommand {
        id: "suspend",
        aliases: &["sleep", "suspend"],
        title: "Sleep",
        subtitle: "Suspend system to RAM",
        icon_name: "system-suspend",
        is_destructive: false,
    },
    SystemCommand {
        id: "restart",
        aliases: &["restart", "reboot"],
        title: "Restart",
        subtitle: "Restart the computer",
        icon_name: "system-reboot",
        is_destructive: true,
    },
    SystemCommand {
        id: "shutdown",
        aliases: &["shutdown", "shut down", "power off"],
        title: "Shut Down",
        subtitle: "Power off the computer",
        icon_name: "system-shutdown",
        is_destructive: true,
    },
    SystemCommand {
        id: "logout",
        aliases: &["logout", "log out", "sign out"],
        title: "Log Out",
        subtitle: "Terminate the desktop session",
        icon_name: "system-log-out",
        is_destructive: true,
    },
    SystemCommand {
        id: "trash",
        aliases: &["empty trash", "clear trash"],
        title: "Empty Trash",
        subtitle: "Permanently delete trashed items",
        icon_name: "user-trash-full",
        is_destructive: true,
    },
];

/// Returns `true` if `action_id` represents an irreversible destructive operation.
#[must_use]
pub fn is_destructive_action(action_id: &str) -> bool {
    matches!(
        action_id,
        "restart"
            | "reboot"
            | "shutdown"
            | "shut down"
            | "power off"
            | "power_off"
            | "logout"
            | "trash"
    )
}

/// Parses a hardware volume adjustment expression (e.g. `vol 80%` or `volume 120`).
///
/// Returns target percentage clamped between 0 and 100.
#[must_use]
pub fn parse_volume_command(query: &str) -> Option<u32> {
    let q = query.trim().to_ascii_lowercase();
    let num_part = q
        .strip_prefix("volume ")
        .or_else(|| q.strip_prefix("vol "))?;
    let clean = num_part.trim().trim_end_matches('%');
    clean.parse::<u32>().ok().map(|v| v.min(100))
}

/// Parses a hardware display brightness adjustment expression (e.g. `brightness 80%`).
///
/// Returns target percentage clamped between 0 and 100.
#[must_use]
pub fn parse_brightness_command(query: &str) -> Option<u32> {
    let q = query.trim().to_ascii_lowercase();
    let num_part = q
        .strip_prefix("brightness ")
        .or_else(|| q.strip_prefix("bright "))?;
    let clean = num_part.trim().trim_end_matches('%');
    clean.parse::<u32>().ok().map(|v| v.min(100))
}

/// Matches user query against registered system commands and hardware controls.
#[must_use]
pub fn match_system_actions(query: &str) -> Vec<SearchRecord> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    // 1. Hardware Volume control expression
    if let Some(level) = parse_volume_command(&q) {
        results.push(SearchRecord::new(
            format!("system:volume:{level}"),
            format!("Set Volume to {level}%"),
            Some(format!("Adjust system audio volume to {level}%")),
            SearchCategory::SystemActions,
            Some("audio-volume-high".to_string()),
            950.0,
            SearchPayload::SystemAction {
                action: format!("volume:{level}"),
            },
        ));
    }

    // 2. Hardware Brightness control expression
    if let Some(level) = parse_brightness_command(&q) {
        results.push(SearchRecord::new(
            format!("system:brightness:{level}"),
            format!("Set Brightness to {level}%"),
            Some(format!("Adjust screen brightness to {level}%")),
            SearchCategory::SystemActions,
            Some("display-brightness".to_string()),
            950.0,
            SearchPayload::SystemAction {
                action: format!("brightness:{level}"),
            },
        ));
    }

    // 3. Audio Mute toggle keyword
    if q == "mute" || q == "unmute" || q == "silence" {
        results.push(SearchRecord::new(
            "system:mute",
            "Toggle Mute",
            Some("Mute or unmute system audio".to_string()),
            SearchCategory::SystemActions,
            Some("audio-volume-muted".to_string()),
            1000.0,
            SearchPayload::SystemAction {
                action: "mute".to_string(),
            },
        ));
    } else if q.len() >= 2 && ("mute".starts_with(&q) || "unmute".starts_with(&q)) {
        results.push(SearchRecord::new(
            "system:mute",
            "Toggle Mute",
            Some("Mute or unmute system audio".to_string()),
            SearchCategory::SystemActions,
            Some("audio-volume-muted".to_string()),
            750.0,
            SearchPayload::SystemAction {
                action: "mute".to_string(),
            },
        ));
    }

    // 4. Static session & power commands
    for cmd in SYSTEM_COMMANDS {
        let mut best_score = None;
        for alias in cmd.aliases {
            if *alias == q {
                best_score = Some(1000.0);
                break;
            }
            if q.len() >= 2 && alias.starts_with(&q) && best_score.is_none() {
                best_score = Some(750.0);
            }
        }

        if let Some(score) = best_score {
            results.push(SearchRecord::new(
                format!("system:{}", cmd.id),
                cmd.title.to_string(),
                Some(cmd.subtitle.to_string()),
                SearchCategory::SystemActions,
                Some(cmd.icon_name.to_string()),
                score,
                SearchPayload::SystemAction {
                    action: cmd.id.to_string(),
                },
            ));
        }
    }

    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results
}

/// Executes a system session, power management, or hardware adjustment action asynchronously.
///
/// # Errors
/// Returns an error if D-Bus communication or child process invocation fails.
pub async fn execute_system_action(action_id: &str) -> anyhow::Result<()> {
    let client = Login1Client::new();
    match action_id {
        "lock" => {
            std::process::Command::new("loginctl")
                .arg("lock-session")
                .spawn()?;
        }
        "suspend" => {
            client.suspend(false).await?;
        }
        "restart" | "reboot" => {
            client.reboot(false).await?;
        }
        "shutdown" | "power_off" | "shut down" | "power off" => {
            client.power_off(false).await?;
        }
        "logout" => {
            std::process::Command::new("loginctl")
                .arg("terminate-session")
                .arg("")
                .spawn()?;
        }
        "trash" => {
            shared::fs::empty_trash()?;
        }
        "mute" => {
            if std::process::Command::new("wpctl")
                .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
                .spawn()
                .is_err()
            {
                let _ = std::process::Command::new("pactl")
                    .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
                    .spawn();
            }
        }
        other => {
            if let Some(level_str) = other.strip_prefix("volume:") {
                let level: u32 = level_str.parse().unwrap_or(50).min(100);
                let percent_str = format!("{level}%");
                if std::process::Command::new("wpctl")
                    .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &percent_str])
                    .spawn()
                    .is_err()
                {
                    let _ = std::process::Command::new("pactl")
                        .args(["set-sink-volume", "@DEFAULT_SINK@", &percent_str])
                        .spawn();
                }
            } else if let Some(level_str) = other.strip_prefix("brightness:") {
                let level: u32 = level_str.parse().unwrap_or(50).min(100);
                let percent_str = format!("{level}%");
                let _ = std::process::Command::new("brightnessctl")
                    .args(["set", &percent_str])
                    .spawn();
            }
        }
    }
    Ok(())
}

/// Ultra-fast in-memory provider for system actions and hardware management.
#[derive(Debug, Default, Clone)]
pub struct SystemActionProvider;

/// Type alias matching integration naming.
pub type SystemActionsProvider = SystemActionProvider;

impl SystemActionProvider {
    /// Creates a new `SystemActionProvider`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl SearchProvider for SystemActionProvider {
    fn id(&self) -> &'static str {
        "system"
    }

    fn display_name(&self) -> &'static str {
        "System Actions"
    }

    fn is_fast_path(&self) -> bool {
        true
    }

    fn handles_scope(&self, scope: QueryScope) -> bool {
        matches!(scope, QueryScope::All)
    }

    async fn search(
        &self,
        query: &str,
        _filter: Option<&str>,
        _deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError> {
        Ok(match_system_actions(query))
    }
}
