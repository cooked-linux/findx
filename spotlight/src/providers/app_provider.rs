//! Application and system settings search provider.
//!
//! Provides in-memory search over installed desktop applications and system settings
//! panels, computes composite lexical + frecency scores, calculates inline ghost text
//! completions, and dispatches application processes with Wayland compositor token forwarding.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use shared::desktop::{DesktopApp, calculate_text_score, resolve_icon_path, scan_installed_apps};
use shared::frecency::{DEFAULT_FRECENCY_WEIGHT, EventKind, FrecencyTracker};
use shared::models::{SearchCategory, SearchPayload, SearchRecord};

use crate::arbiter::dispatcher::QueryScope;
use crate::arbiter::provider::{ProviderError, SearchProvider};

/// Standard terminal emulator fallback binaries checked in search order.
const TERMINAL_FALLBACKS: &[&str] = &[
    "x-terminal-emulator",
    "kitty",
    "alacritty",
    "foot",
    "gnome-terminal",
    "wezterm",
    "konsole",
    "xfce4-terminal",
    "xterm",
];

/// Known settings panel keyword routing table `(keyword_query, target_indicators)`.
const SETTINGS_KEYWORD_ROUTING: &[(&str, &[&str])] = &[
    ("wifi", &["wifi", "wi-fi", "wireless", "network"]),
    ("wi-fi", &["wifi", "wi-fi", "wireless", "network"]),
    ("wireless", &["wifi", "wi-fi", "wireless", "network"]),
    ("bluetooth", &["bluetooth", "bluez", "bt"]),
    (
        "display scale",
        &[
            "display",
            "displays",
            "screen",
            "monitor",
            "resolution",
            "scale",
        ],
    ),
    (
        "display",
        &["display", "displays", "screen", "monitor", "resolution"],
    ),
    (
        "audio output",
        &["sound", "audio", "volume", "output", "speaker"],
    ),
    ("sound", &["sound", "audio", "volume", "speaker"]),
    ("audio", &["sound", "audio", "volume", "speaker"]),
    ("volume", &["sound", "audio", "volume", "speaker"]),
    ("power", &["power", "battery", "energy", "sleep"]),
    ("battery", &["power", "battery", "energy", "sleep"]),
    ("printers", &["printer", "printers", "cups", "printing"]),
    ("mouse", &["mouse", "touchpad", "cursor", "pointer"]),
    ("touchpad", &["mouse", "touchpad", "gestures"]),
    ("keyboard", &["keyboard", "input", "typing", "shortcuts"]),
];

/// Errors that can occur when spawning or preparing an application launch.
#[derive(Debug, thiserror::Error)]
pub enum AppLaunchError {
    /// Failure during frecency event recording.
    #[error("Frecency recording error: {0}")]
    Frecency(#[from] shared::frecency::FrecencyError),

    /// Process execution or I/O failure.
    #[error("I/O error spawning process: {0}")]
    Io(#[from] std::io::Error),

    /// Missing or unsupported terminal emulator.
    #[error("Terminal emulator not found: {0}")]
    TerminalNotFound(String),

    /// Malformed or empty execution command line.
    #[error("Invalid execution command: {0}")]
    InvalidCommand(String),
}

/// Search provider querying installed desktop applications and control center settings panels.
#[derive(Clone)]
pub struct ApplicationProvider {
    apps: Vec<DesktopApp>,
    frecency: Arc<FrecencyTracker>,
}

impl ApplicationProvider {
    /// Discovers installed applications from standard XDG paths and creates the provider.
    ///
    /// # Errors
    ///
    /// Returns [`shared::desktop::DesktopError`] if scanning directories fails critically.
    pub fn new(frecency: Arc<FrecencyTracker>) -> Result<Self, shared::desktop::DesktopError> {
        let apps = scan_installed_apps()?;
        Ok(Self { apps, frecency })
    }

    /// Constructs an `ApplicationProvider` with explicit in-memory application entries.
    #[must_use]
    pub const fn with_apps(apps: Vec<DesktopApp>, frecency: Arc<FrecencyTracker>) -> Self {
        Self { apps, frecency }
    }

    /// Returns a slice of the indexed desktop applications.
    #[must_use]
    pub fn apps(&self) -> &[DesktopApp] {
        &self.apps
    }

    /// Returns the shared frecency tracker.
    #[must_use]
    pub const fn frecency(&self) -> &Arc<FrecencyTracker> {
        &self.frecency
    }

    /// Returns the top suggested applications based on historical frecency engagement.
    #[must_use]
    pub fn get_suggested_apps(&self, limit: usize) -> Vec<DesktopApp> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        let mut scored: Vec<(f64, &DesktopApp)> = self
            .apps
            .iter()
            .filter(|app| !app.hidden && !app.no_display)
            .map(|app| {
                let score = self
                    .frecency
                    .calculate_score(&app.desktop_id, now)
                    .unwrap_or(0.0);
                (score, app)
            })
            .collect();

        // Sort descending by frecency score; items without events maintain original order
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));

        scored
            .into_iter()
            .take(limit)
            .map(|(_, app)| app.clone())
            .collect()
    }
}

#[async_trait::async_trait]
impl SearchProvider for ApplicationProvider {
    fn id(&self) -> &'static str {
        "applications"
    }

    fn display_name(&self) -> &'static str {
        "Applications"
    }

    fn is_fast_path(&self) -> bool {
        true
    }

    fn handles_scope(&self, scope: QueryScope) -> bool {
        matches!(scope, QueryScope::All | QueryScope::Apps)
    }

    async fn search(
        &self,
        query: &str,
        _filter: Option<&str>,
        deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError> {
        if Instant::now() >= deadline {
            return Err(ProviderError::Timeout(Duration::from_millis(0)));
        }

        let trimmed_query = query.trim();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        // Empty query state: return top frecent applications
        if trimmed_query.is_empty() {
            let suggested = self.get_suggested_apps(4);
            let mut records = Vec::with_capacity(suggested.len());
            for app in suggested {
                let score = self
                    .frecency
                    .calculate_score(&app.desktop_id, now)
                    .unwrap_or(0.0);
                records.push(build_search_record(&app, score));
            }
            return Ok(records);
        }

        let mut records = Vec::new();

        for app in &self.apps {
            if app.hidden || app.no_display {
                continue;
            }

            let mut text_score = calculate_text_score(trimmed_query, app);

            // Check desktop settings keyword routing if app is a settings panel
            let is_settings = is_settings_panel(app);
            if is_settings && let Some(settings_score) = match_settings_keywords(trimmed_query, app)
            {
                text_score = f64::max(text_score, settings_score);
            }

            if text_score <= 0.0 {
                continue;
            }

            let frecency_score = self
                .frecency
                .calculate_score_with_query(&app.desktop_id, Some(trimmed_query), now)
                .unwrap_or(0.0);

            let composite_score = DEFAULT_FRECENCY_WEIGHT.mul_add(frecency_score, text_score);

            let mut record = build_search_record(app, composite_score);
            if is_settings {
                record.category = SearchCategory::SystemActions;
            }
            records.push(record);
        }

        // Sort descending by score
        records.sort_by(|a, b| b.score.total_cmp(&a.score));

        Ok(records)
    }
}

/// Constructs a canonical [`SearchRecord`] from a [`DesktopApp`].
#[must_use]
pub fn build_search_record(app: &DesktopApp, score: f64) -> SearchRecord {
    let payload = SearchPayload::Application {
        desktop_id: app.desktop_id.clone(),
        exec: app.exec_sanitized.clone(),
    };

    SearchRecord::new(
        format!("app:{}", app.desktop_id),
        app.name.clone(),
        app.generic_name.clone().or_else(|| app.comment.clone()),
        SearchCategory::Applications,
        app.icon_name.clone(),
        score,
        payload,
    )
}

/// Computes inline ghost text completion suffix for the search query given a candidate title.
///
/// Returns `Some(suffix)` if `top_candidate` begins case-insensitively with `query`,
/// preserving the casing of `top_candidate`. Returns `None` otherwise.
#[must_use]
pub fn compute_ghost_completion<'a>(query: &str, top_candidate: &'a str) -> Option<&'a str> {
    let q_trimmed = query.trim_start();
    if q_trimmed.is_empty() {
        return None;
    }

    let q_lower = q_trimmed.to_lowercase();
    let top_lower = top_candidate.to_lowercase();

    if !top_lower.starts_with(&q_lower) {
        return None;
    }

    let prefix_len = q_trimmed.len();
    if top_candidate.is_char_boundary(prefix_len) {
        Some(&top_candidate[prefix_len..])
    } else {
        // Fallback for multi-byte Unicode char boundary alignment
        let char_count = q_trimmed.chars().count();
        let mut byte_idx = None;
        for (idx, (c_idx, _)) in top_candidate.char_indices().enumerate() {
            if idx == char_count {
                byte_idx = Some(c_idx);
                break;
            }
        }
        if let Some(idx) = byte_idx
            && top_candidate.is_char_boundary(idx)
        {
            Some(&top_candidate[idx..])
        } else {
            None
        }
    }
}

/// Resolves an application icon name to an absolute filesystem path if available.
#[must_use]
pub fn resolve_app_icon_path(icon_name: &str) -> Option<PathBuf> {
    resolve_icon_path(icon_name)
}

/// Inspects whether a desktop entry represents a system control panel or configuration tool.
#[must_use]
pub fn is_settings_panel(app: &DesktopApp) -> bool {
    let exec_lower = app.exec_sanitized.to_lowercase();
    let id_lower = app.desktop_id.to_lowercase();

    if exec_lower.contains("gnome-control-center")
        || exec_lower.contains("systemsettings")
        || exec_lower.contains("xfce4-settings-manager")
        || id_lower.ends_with("-panel.desktop")
        || id_lower.contains("settings")
    {
        return true;
    }

    if app
        .categories
        .iter()
        .any(|c| c.eq_ignore_ascii_case("Settings") || c.eq_ignore_ascii_case("HardwareSettings"))
    {
        return true;
    }

    // Inspect underlying .desktop file header if present on disk
    if app.path.is_file()
        && let Ok(content) = std::fs::read_to_string(&app.path)
    {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("X-GNOME-Settings-Panel")
                || trimmed.starts_with("X-KDE-Settings-Panel")
            {
                return true;
            }
        }
    }

    false
}

/// Evaluates if a query matches known settings routing keywords for a configuration panel.
#[must_use]
pub fn match_settings_keywords(query: &str, app: &DesktopApp) -> Option<f64> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }

    let app_text = format!(
        "{} {} {} {} {}",
        app.name,
        app.generic_name.as_deref().unwrap_or(""),
        app.comment.as_deref().unwrap_or(""),
        app.desktop_id,
        app.keywords.join(" ")
    )
    .to_lowercase();

    for &(keyword, indicators) in SETTINGS_KEYWORD_ROUTING {
        if q == keyword || q.starts_with(keyword) || keyword.starts_with(&q) {
            let matched_indicator = indicators.iter().any(|&ind| app_text.contains(ind));
            if matched_indicator {
                return Some(520.0);
            }
        }
    }

    None
}

/// Queries the system environment or `GSettings` for the default terminal emulator binary.
#[must_use]
pub fn find_default_terminal() -> Option<String> {
    // 1. $TERMINAL environment variable
    if let Ok(term) = std::env::var("TERMINAL") {
        let trimmed = term.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    // 2. GSettings GNOME default terminal
    if let Some(term) = query_gsettings_terminal() {
        return Some(term);
    }

    // 3. Fallback binaries in PATH
    for &candidate in TERMINAL_FALLBACKS {
        if is_in_path(candidate) {
            return Some(candidate.to_string());
        }
    }

    None
}

/// Queries `GSettings` for the configured default terminal executable.
fn query_gsettings_terminal() -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args([
            "get",
            "org.gnome.desktop.default-applications.terminal",
            "exec",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        let trimmed = text.trim().trim_matches('\'').trim();
        if !trimmed.is_empty() && trimmed != "''" {
            return Some(trimmed.to_string());
        }
    }

    None
}

/// Checks whether an executable command exists in `$PATH`.
fn is_in_path(cmd: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let p = dir.join(cmd);
            if p.is_file() {
                return true;
            }
        }
    }
    false
}

/// Formats a command for execution inside a specified terminal emulator (`<terminal> -e <command>`).
#[must_use]
pub fn wrap_in_terminal(terminal: &str, exec: &str) -> (String, Vec<String>) {
    (
        terminal.to_string(),
        vec!["-e".to_string(), exec.to_string()],
    )
}

/// Formats a command for execution inside a terminal emulator (`<terminal> -e <command>`).
///
/// # Errors
///
/// Returns [`AppLaunchError::TerminalNotFound`] if no suitable terminal emulator can be located.
pub fn wrap_in_terminal_emulator(exec: &str) -> Result<(String, Vec<String>), AppLaunchError> {
    let term = find_default_terminal().ok_or_else(|| {
        AppLaunchError::TerminalNotFound("No supported terminal emulator found in system".into())
    })?;

    Ok(wrap_in_terminal(&term, exec))
}

/// Reads the active Wayland XDG activation token from the process environment if available.
#[must_use]
pub fn request_xdg_activation_token() -> Option<String> {
    std::env::var("XDG_ACTIVATION_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// Launches a desktop application process, recording frecency and applying activation tokens.
///
/// # Errors
///
/// Returns [`AppLaunchError`] if frecency recording fails, terminal emulator is missing,
/// command syntax is invalid, or process creation fails.
pub fn launch_desktop_app(
    app: &DesktopApp,
    frecency: &FrecencyTracker,
    query: &str,
) -> Result<(), AppLaunchError> {
    let token = request_xdg_activation_token();
    launch_desktop_app_with_token(app, frecency, query, token.as_deref())
}

/// Launches a desktop application process with an explicit XDG activation token.
///
/// # Errors
///
/// Returns [`AppLaunchError`] if frecency recording fails, terminal emulator is missing,
/// command syntax is invalid, or process creation fails.
pub fn launch_desktop_app_with_token(
    app: &DesktopApp,
    frecency: &FrecencyTracker,
    query: &str,
    token: Option<&str>,
) -> Result<(), AppLaunchError> {
    let query_param = if query.trim().is_empty() {
        None
    } else {
        Some(query)
    };
    frecency.record_event_now(&app.desktop_id, "app", EventKind::Launch, query_param)?;

    let exec_clean = shared::desktop::sanitize_exec(&app.exec_raw);

    let (command, args) = if app.is_terminal {
        wrap_in_terminal_emulator(&exec_clean)?
    } else {
        let parts: Vec<&str> = exec_clean.split_whitespace().collect();
        if parts.is_empty() {
            return Err(AppLaunchError::InvalidCommand(
                "Execution string is empty".into(),
            ));
        }
        (
            parts[0].to_string(),
            parts[1..].iter().map(|s| (*s).to_string()).collect(),
        )
    };

    let mut cmd = std::process::Command::new(command);
    cmd.args(args);
    if let Some(tok) = token {
        cmd.env("XDG_ACTIVATION_TOKEN", tok);
    }

    cmd.spawn()?;
    Ok(())
}
