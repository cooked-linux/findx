//! Web search provider offering search engine query shortcuts and fallback cards.

use std::time::Instant;

use shared::models::{SearchCategory, SearchPayload, SearchRecord};

use crate::arbiter::dispatcher::QueryScope;
use crate::arbiter::provider::{ProviderError, SearchProvider};

/// Supported web search engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchEngine {
    /// `DuckDuckGo` privacy-focused search engine (default).
    #[default]
    DuckDuckGo,
    /// Google search.
    Google,
    /// Wikipedia encyclopedia lookup.
    Wikipedia,
    /// Kagi search.
    Kagi,
}

impl SearchEngine {
    /// Returns the human-readable display name for the search engine.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Google => "Google",
            Self::Wikipedia => "Wikipedia",
            Self::Kagi => "Kagi",
        }
    }

    /// Formats the complete search URL with percent-encoded query.
    #[must_use]
    pub fn format_url(&self, query: &str) -> String {
        let encoded = urlencoding::encode(query);
        match self {
            Self::DuckDuckGo => format!("https://duckduckgo.com/?q={encoded}"),
            Self::Google => format!("https://www.google.com/search?q={encoded}"),
            Self::Wikipedia => format!("https://en.wikipedia.org/w/index.php?search={encoded}"),
            Self::Kagi => format!("https://kagi.com/search?q={encoded}"),
        }
    }

    /// Resolves search engine from a prefix keyword (`"g"`, `"google"`, `"ddg"`, `"w"`, `"wiki"`, `"kagi"`).
    #[must_use]
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix.trim().to_ascii_lowercase().as_str() {
            "ddg" | "duckduckgo" => Some(Self::DuckDuckGo),
            "g" | "google" => Some(Self::Google),
            "w" | "wiki" | "wikipedia" => Some(Self::Wikipedia),
            "kagi" => Some(Self::Kagi),
            _ => None,
        }
    }
}

/// Parses an explicit search engine prefix from a query string.
///
/// Returns the target [`SearchEngine`] and the trimmed query text if a valid prefix
/// (`"?"`, `"g"`, `"ddg"`, `"w"`, etc.) is present.
#[must_use]
pub fn parse_explicit_prefix(raw_query: &str) -> Option<(SearchEngine, &str)> {
    let trimmed = raw_query.trim_start();
    if let Some(rest) = trimmed.strip_prefix('?') {
        let query = rest.trim_start();
        return Some((SearchEngine::DuckDuckGo, query));
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let prefix = parts.next()?;
    let rest = parts.next()?.trim_start();

    SearchEngine::from_prefix(prefix).map(|engine| (engine, rest))
}

/// Safely opens a URL in the user's default browser or an incognito/private window.
///
/// Dispatches directly to `xdg-open` or browser executable with separate arguments,
/// preventing shell injection vulnerabilities.
///
/// # Errors
/// Returns an error if spawning the browser process fails.
pub fn open_browser_url(url: &str, private: bool) -> anyhow::Result<()> {
    if private {
        let browser = std::env::var("BROWSER").unwrap_or_default();
        let browser_lower = browser.to_ascii_lowercase();

        if browser_lower.contains("firefox") {
            let res = std::process::Command::new("firefox")
                .args(["--private-window", url])
                .spawn();
            if res.is_ok() {
                return Ok(());
            }
        } else if browser_lower.contains("chrome")
            || browser_lower.contains("chromium")
            || browser_lower.contains("brave")
        {
            let res = std::process::Command::new(&browser)
                .args(["--incognito", url])
                .spawn();
            if res.is_ok() {
                return Ok(());
            }
        }

        // Try standard browser commands in private mode before generic fallback
        for (cmd, flag) in [
            ("firefox", "--private-window"),
            ("google-chrome", "--incognito"),
            ("brave", "--incognito"),
            ("chromium", "--incognito"),
        ] {
            if std::process::Command::new(cmd)
                .args([flag, url])
                .spawn()
                .is_ok()
            {
                return Ok(());
            }
        }
    }

    // Default safe dispatch via xdg-open
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}

/// Search provider for web search queries and fallback trailing items.
#[derive(Debug, Clone)]
pub struct WebSearchProvider {
    default_engine: SearchEngine,
}

impl Default for WebSearchProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearchProvider {
    /// Creates a new `WebSearchProvider` using the default search engine ([`SearchEngine::DuckDuckGo`]).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            default_engine: SearchEngine::DuckDuckGo,
        }
    }

    /// Creates a new `WebSearchProvider` with an explicit default search engine.
    #[must_use]
    pub const fn with_engine(engine: SearchEngine) -> Self {
        Self {
            default_engine: engine,
        }
    }

    /// Returns the configured default search engine.
    #[must_use]
    pub const fn default_engine(&self) -> SearchEngine {
        self.default_engine
    }
}

#[async_trait::async_trait]
impl SearchProvider for WebSearchProvider {
    fn id(&self) -> &'static str {
        "web"
    }

    fn display_name(&self) -> &'static str {
        "Web Search"
    }

    fn is_fast_path(&self) -> bool {
        true
    }

    fn handles_scope(&self, scope: QueryScope) -> bool {
        matches!(scope, QueryScope::All | QueryScope::Web)
    }

    async fn search(
        &self,
        query: &str,
        _filter: Option<&str>,
        _deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();

        // 1. Check for explicit prefix (e.g. `? rust`, `g search`, `w linux`)
        if let Some((engine, clean_query)) = parse_explicit_prefix(trimmed) {
            if !clean_query.is_empty() {
                let url = engine.format_url(clean_query);
                records.push(SearchRecord::new(
                    "web:explicit",
                    format!("Search {} for '{clean_query}'", engine.name()),
                    Some(url.clone()),
                    SearchCategory::WebSearch,
                    Some("applications-internet".to_string()),
                    990.0,
                    SearchPayload::WebSearch { url },
                ));
            }
            return Ok(records);
        }

        // 2. Trailing fallback card for general query
        let url = self.default_engine.format_url(trimmed);
        records.push(SearchRecord::new(
            format!("web:trailing:{trimmed}"),
            format!("Search the web for '{trimmed}'"),
            Some(format!("Search with {}", self.default_engine.name())),
            SearchCategory::WebSearch,
            Some("applications-internet".to_string()),
            50.0,
            SearchPayload::WebSearch { url },
        ));

        Ok(records)
    }
}
