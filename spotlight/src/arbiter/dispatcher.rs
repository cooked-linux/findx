//! High-performance asynchronous query dispatcher and keystroke micro-debouncer.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use shared::models::{AggregatedSearchResults, SearchRecord};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::arbiter::normalizer::{
    DEFAULT_MAX_PER_CATEGORY, DEFAULT_TOP_HIT_THRESHOLD, synthesize_results,
};
use crate::arbiter::provider::{ProviderError, SearchProvider};

/// Explicit search scope filter parsed from query prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryScope {
    /// Dispatches query across all registered search providers.
    All,
    /// Restricts fan-out to Application Launcher (`app:`).
    Apps,
    /// Restricts fan-out to SQLite FTS5 File Indexer (`file:`).
    Files,
    /// Restricts fan-out to Calculator & Converter (`calc:`).
    Calculator,
    /// Restricts fan-out to Lexical Dictionary (`dict:`).
    Dictionary,
    /// Directs query to Web Search Fallback (`?`).
    Web,
}

/// Parsed query details separating scope prefix from query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery<'a> {
    /// Logical search scope determined by query prefix.
    pub scope: QueryScope,
    /// Original raw query input.
    pub raw_query: &'a str,
    /// Clean query text with prefix stripped.
    pub query: &'a str,
}

/// Parses scope prefix from user input (`app:`, `file:`, `calc:`, `dict:`, `?`).
#[must_use]
pub fn parse_query_scope(raw_query: &str) -> ParsedQuery<'_> {
    let trimmed = raw_query.trim_start();
    if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("app:") {
        ParsedQuery {
            scope: QueryScope::Apps,
            raw_query,
            query: trimmed[4..].trim_start(),
        }
    } else if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("file:") {
        ParsedQuery {
            scope: QueryScope::Files,
            raw_query,
            query: trimmed[5..].trim_start(),
        }
    } else if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("calc:") {
        ParsedQuery {
            scope: QueryScope::Calculator,
            raw_query,
            query: trimmed[5..].trim_start(),
        }
    } else if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("dict:") {
        ParsedQuery {
            scope: QueryScope::Dictionary,
            raw_query,
            query: trimmed[5..].trim_start(),
        }
    } else if let Some(rest) = trimmed.strip_prefix('?') {
        ParsedQuery {
            scope: QueryScope::Web,
            raw_query,
            query: rest.trim_start(),
        }
    } else {
        ParsedQuery {
            scope: QueryScope::All,
            raw_query,
            query: trimmed,
        }
    }
}

/// Progressive search result update stream emitted by the query dispatcher.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamingUpdate {
    /// Frame 1 fast-path update (~16ms) containing immediate synchronous/cached results.
    FastPath {
        /// Monotonically increasing query identifier.
        query_id: u64,
        /// Provisional synthesized search results.
        results: AggregatedSearchResults,
    },
    /// Frame 2 final update (<45ms) containing consolidated results from all surviving providers.
    Final {
        /// Monotonically increasing query identifier.
        query_id: u64,
        /// Final consolidated search results.
        results: AggregatedSearchResults,
    },
}

impl StreamingUpdate {
    /// Returns the monotonic query ID associated with this update.
    #[must_use]
    pub const fn query_id(&self) -> u64 {
        match self {
            Self::FastPath { query_id, .. } | Self::Final { query_id, .. } => *query_id,
        }
    }

    /// Returns a reference to the aggregated search results.
    #[must_use]
    pub const fn results(&self) -> &AggregatedSearchResults {
        match self {
            Self::FastPath { results, .. } | Self::Final { results, .. } => results,
        }
    }

    /// Returns `true` if this update is the final update for its query.
    #[must_use]
    pub const fn is_final(&self) -> bool {
        matches!(self, Self::Final { .. })
    }
}

/// Asynchronous multi-provider query dispatcher and keystroke micro-debouncer.
#[derive(Clone)]
pub struct QueryDispatcher {
    providers: Vec<Arc<dyn SearchProvider>>,
    current_query_id: Arc<AtomicU64>,
    result_sender: mpsc::UnboundedSender<StreamingUpdate>,
    current_token: Arc<std::sync::Mutex<Option<CancellationToken>>>,
    top_hit_threshold: f64,
    max_per_category: usize,
    runtime_handle: Option<tokio::runtime::Handle>,
}

type ProviderResult = (&'static str, Result<Vec<SearchRecord>, ProviderError>);
type ProviderReceiver = mpsc::Receiver<ProviderResult>;

impl QueryDispatcher {
    /// Creates a new [`QueryDispatcher`] with default ranking thresholds.
    #[must_use]
    pub fn new(
        providers: Vec<Arc<dyn SearchProvider>>,
        result_sender: mpsc::UnboundedSender<StreamingUpdate>,
    ) -> Self {
        Self::with_options(
            providers,
            result_sender,
            DEFAULT_TOP_HIT_THRESHOLD,
            DEFAULT_MAX_PER_CATEGORY,
            None,
        )
    }

    /// Creates a [`QueryDispatcher`] along with its paired [`mpsc::UnboundedReceiver`].
    #[must_use]
    pub fn new_with_channel(
        providers: Vec<Arc<dyn SearchProvider>>,
    ) -> (Self, mpsc::UnboundedReceiver<StreamingUpdate>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self::new(providers, tx), rx)
    }

    /// Creates a new [`QueryDispatcher`] with an explicit tokio runtime handle.
    #[must_use]
    pub fn with_handle(
        providers: Vec<Arc<dyn SearchProvider>>,
        result_sender: mpsc::UnboundedSender<StreamingUpdate>,
        handle: tokio::runtime::Handle,
    ) -> Self {
        Self::with_options(
            providers,
            result_sender,
            DEFAULT_TOP_HIT_THRESHOLD,
            DEFAULT_MAX_PER_CATEGORY,
            Some(handle),
        )
    }

    /// Creates a [`QueryDispatcher`] with configurable thresholds and runtime handle.
    #[must_use]
    pub fn with_options(
        providers: Vec<Arc<dyn SearchProvider>>,
        result_sender: mpsc::UnboundedSender<StreamingUpdate>,
        top_hit_threshold: f64,
        max_per_category: usize,
        runtime_handle: Option<tokio::runtime::Handle>,
    ) -> Self {
        Self {
            providers,
            current_query_id: Arc::new(AtomicU64::new(0)),
            result_sender,
            current_token: Arc::new(std::sync::Mutex::new(None)),
            top_hit_threshold,
            max_per_category,
            runtime_handle,
        }
    }

    /// Returns the currently active query ID.
    #[must_use]
    pub fn current_query_id(&self) -> u64 {
        self.current_query_id.load(Ordering::SeqCst)
    }

    /// Returns a slice of all registered search providers.
    #[must_use]
    pub fn providers(&self) -> &[Arc<dyn SearchProvider>] {
        &self.providers
    }

    /// Dispatches a new user query asynchronously.
    ///
    /// Micro-debounces keystrokes (15ms), increments monotonic query ID, cancels
    /// obsolete in-flight queries, and schedules fan-out execution.
    pub fn dispatch(&self, raw_query: &str) -> u64 {
        let query_id = self.current_query_id.fetch_add(1, Ordering::SeqCst) + 1;
        let token = CancellationToken::new();

        if let Ok(mut guard) = self.current_token.lock() {
            if let Some(prev) = guard.take() {
                prev.cancel();
            }
            *guard = Some(token.clone());
        }

        let this = self.clone();
        let query = raw_query.to_string();

        let handle = self
            .runtime_handle
            .clone()
            .or_else(|| tokio::runtime::Handle::try_current().ok());

        if let Some(h) = handle {
            h.spawn(async move {
                this.execute_query(query_id, query, token).await;
            });
        } else {
            tracing::error!("QueryDispatcher::dispatch called without active tokio runtime");
        }

        query_id
    }

    /// Dispatches a query change event (convenience alias for [`Self::dispatch`]).
    #[must_use]
    pub fn on_query_changed(&self, query: &str) -> u64 {
        self.dispatch(query)
    }

    /// Executes query fan-out directly on the calling task (useful for deterministic tests).
    pub async fn dispatch_and_wait(&self, raw_query: &str) -> u64 {
        let query_id = self.current_query_id.fetch_add(1, Ordering::SeqCst) + 1;
        let token = CancellationToken::new();

        if let Ok(mut guard) = self.current_token.lock() {
            if let Some(prev) = guard.take() {
                prev.cancel();
            }
            *guard = Some(token.clone());
        }

        self.execute_query(query_id, raw_query.to_string(), token)
            .await;
        query_id
    }

    /// Core query execution lifecycle: micro-debounce -> scope parsing -> concurrent fan-out -> 2-frame streaming.
    pub async fn execute_query(&self, query_id: u64, raw_query: String, token: CancellationToken) {
        // 1. Keystroke Micro-Debounce (15ms)
        tokio::time::sleep(Duration::from_millis(15)).await;

        if token.is_cancelled() || self.current_query_id.load(Ordering::SeqCst) != query_id {
            return;
        }

        // 2. Query Scope Prefix Parsing
        let parsed = parse_query_scope(&raw_query);
        if parsed.query.trim().is_empty() {
            let empty_results = AggregatedSearchResults::new();
            let _ = self.result_sender.send(StreamingUpdate::Final {
                query_id,
                results: empty_results,
            });
            return;
        }

        // 3. Provider Scoping & Filtering
        let matched_providers: Vec<Arc<dyn SearchProvider>> = self
            .providers
            .iter()
            .filter(|p| p.handles_scope(parsed.scope))
            .cloned()
            .collect();

        if matched_providers.is_empty() {
            let empty_results = AggregatedSearchResults::new();
            let _ = self.result_sender.send(StreamingUpdate::Final {
                query_id,
                results: empty_results,
            });
            return;
        }

        // 4. Concurrent Fan-Out Setup (<45ms Hard Deadline)
        let start_time = Instant::now();
        let frame_1_deadline = start_time + Duration::from_millis(16);
        let hard_deadline = start_time + Duration::from_millis(45);

        let (mut rx, handles) =
            Self::spawn_provider_tasks(&matched_providers, parsed.query, hard_deadline, &token);

        // 5. Frame 1 (~16ms): Gather available results and emit provisional FastPath
        let (collected_fast, completed_count) =
            Self::collect_frame_1(&mut rx, matched_providers.len(), frame_1_deadline).await;

        if token.is_cancelled() || self.current_query_id.load(Ordering::SeqCst) != query_id {
            for h in &handles {
                h.abort();
            }
            return;
        }

        let fast_results = synthesize_results(
            query_id,
            collected_fast.clone(),
            self.top_hit_threshold,
            self.max_per_category,
        );
        let _ = self.result_sender.send(StreamingUpdate::FastPath {
            query_id,
            results: fast_results,
        });

        // 6. Frame 2 (~32ms - 45ms): Wait for remaining tasks under strict hard deadline
        let all_records = Self::collect_frame_2(
            &mut rx,
            completed_count,
            matched_providers.len(),
            hard_deadline,
            collected_fast,
        )
        .await;

        // Abort any surviving provider tasks that exceeded the hard deadline
        for h in &handles {
            h.abort();
        }

        if token.is_cancelled() || self.current_query_id.load(Ordering::SeqCst) != query_id {
            return;
        }

        // Deduplicate records by unique item ID before final aggregation
        let mut deduped_records = Vec::with_capacity(all_records.len());
        let mut seen_ids = HashSet::new();
        for rec in all_records {
            if seen_ids.insert(rec.id.clone()) {
                deduped_records.push(rec);
            }
        }

        let final_results = synthesize_results(
            query_id,
            deduped_records,
            self.top_hit_threshold,
            self.max_per_category,
        );
        let _ = self.result_sender.send(StreamingUpdate::Final {
            query_id,
            results: final_results,
        });
    }

    fn spawn_provider_tasks(
        matched_providers: &[Arc<dyn SearchProvider>],
        query: &str,
        hard_deadline: Instant,
        token: &CancellationToken,
    ) -> (ProviderReceiver, Vec<tokio::task::JoinHandle<()>>) {
        let (tx, rx) = mpsc::channel(matched_providers.len());
        let mut handles = Vec::with_capacity(matched_providers.len());

        for provider in matched_providers {
            let p = Arc::clone(provider);
            let q = query.to_string();
            let tok = token.clone();
            let sender = tx.clone();
            let id = p.id();

            let handle = tokio::spawn(async move {
                tokio::select! {
                    () = tok.cancelled() => {
                        let _ = sender.send((id, Err(ProviderError::Cancelled))).await;
                    }
                    res = p.search(&q, None, hard_deadline) => {
                        let _ = sender.send((id, res)).await;
                    }
                }
            });
            handles.push(handle);
        }

        (rx, handles)
    }

    async fn collect_frame_1(
        rx: &mut ProviderReceiver,
        total_providers: usize,
        frame_1_deadline: Instant,
    ) -> (Vec<SearchRecord>, usize) {
        let mut collected_fast = Vec::new();
        let mut completed_count = 0;

        while completed_count < total_providers {
            let now = Instant::now();
            if now >= frame_1_deadline {
                break;
            }
            let remaining = frame_1_deadline.saturating_duration_since(now);
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some((_id, Ok(records)))) => {
                    completed_count += 1;
                    collected_fast.extend(records);
                    if completed_count == total_providers {
                        break;
                    }
                }
                Ok(Some((id, Err(err)))) => {
                    completed_count += 1;
                    tracing::debug!("Provider '{id}' returned error: {err}");
                    if completed_count == total_providers {
                        break;
                    }
                }
                Ok(None) => break,
                Err(_timeout) => break,
            }
        }

        (collected_fast, completed_count)
    }

    async fn collect_frame_2(
        rx: &mut ProviderReceiver,
        mut completed_count: usize,
        total_providers: usize,
        hard_deadline: Instant,
        mut all_records: Vec<SearchRecord>,
    ) -> Vec<SearchRecord> {
        while completed_count < total_providers {
            let now = Instant::now();
            if now >= hard_deadline {
                break;
            }
            let remaining = hard_deadline.saturating_duration_since(now);
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some((_id, Ok(records)))) => {
                    completed_count += 1;
                    all_records.extend(records);
                }
                Ok(Some((id, Err(err)))) => {
                    completed_count += 1;
                    tracing::debug!("Provider '{id}' returned error: {err}");
                }
                Ok(None) => break,
                Err(_timeout) => {
                    tracing::debug!("Hard deadline (45ms) reached, terminating pending providers");
                    break;
                }
            }
        }

        all_records
    }
}
