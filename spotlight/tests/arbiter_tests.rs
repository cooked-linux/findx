//! Integration tests for Query Dispatcher and Ranking Arbiter.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone,
    clippy::assert_is_empty,
    clippy::missing_const_for_fn
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use shared::models::{SearchCategory, SearchPayload, SearchRecord};
use spotlight::arbiter::{
    DEFAULT_MAX_PER_CATEGORY, DEFAULT_TOP_HIT_THRESHOLD, ProviderError, QueryDispatcher,
    QueryScope, SearchProvider, SelectionState, flatten_records, parse_query_scope,
    synthesize_results,
};
use tokio::sync::mpsc;

// Mock provider with configurable delay, records, and fast-path status
struct MockProvider {
    id: &'static str,
    name: &'static str,
    is_fast: bool,
    delay: Duration,
    records: Vec<SearchRecord>,
    call_count: Arc<AtomicUsize>,
    fail: bool,
}

impl MockProvider {
    fn new(id: &'static str, name: &'static str, is_fast: bool, delay: Duration) -> Self {
        Self {
            id,
            name,
            is_fast,
            delay,
            records: Vec::new(),
            call_count: Arc::new(AtomicUsize::new(0)),
            fail: false,
        }
    }

    fn with_records(mut self, records: Vec<SearchRecord>) -> Self {
        self.records = records;
        self
    }

    const fn with_failure(mut self) -> Self {
        self.fail = true;
        self
    }
}

#[async_trait::async_trait]
impl SearchProvider for MockProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        self.name
    }

    fn is_fast_path(&self) -> bool {
        self.is_fast
    }

    async fn search(
        &self,
        _query: &str,
        _filter: Option<&str>,
        _deadline: Instant,
    ) -> Result<Vec<SearchRecord>, ProviderError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if self.fail {
            return Err(ProviderError::Failure("simulated error".to_string()));
        }
        Ok(self.records.clone())
    }
}

fn create_dummy_record(
    id: &str,
    title: &str,
    category: SearchCategory,
    score: f64,
) -> SearchRecord {
    SearchRecord::new(
        id,
        title,
        None,
        category,
        None,
        score,
        SearchPayload::Application {
            desktop_id: id.to_string(),
            exec: id.to_string(),
        },
    )
}

#[tokio::test]
async fn test_micro_debouncer_rapid_typing() {
    let call_count = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(MockProvider {
        id: "calc",
        name: "Calculator",
        is_fast: true,
        delay: Duration::from_millis(1),
        records: vec![create_dummy_record(
            "calc:1",
            "Result",
            SearchCategory::Calculations,
            500.0,
        )],
        call_count: Arc::clone(&call_count),
        fail: false,
    });

    let (tx, mut rx) = mpsc::unbounded_channel();
    let dispatcher = QueryDispatcher::new(vec![provider], tx);

    // Rapid keystroke burst: 4 keystrokes 3ms apart (< 15ms debounce window)
    dispatcher.dispatch("c");
    tokio::time::sleep(Duration::from_millis(3)).await;
    dispatcher.dispatch("ca");
    tokio::time::sleep(Duration::from_millis(3)).await;
    dispatcher.dispatch("cal");
    tokio::time::sleep(Duration::from_millis(3)).await;
    let final_qid = dispatcher.dispatch("calc");

    // Gather emitted updates
    let mut received_updates = Vec::new();
    while let Ok(update) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        if let Some(u) = update {
            received_updates.push(u);
        } else {
            break;
        }
    }

    assert!(
        !received_updates.is_empty(),
        "Should receive updates for the final query"
    );
    for u in &received_updates {
        assert_eq!(
            u.query_id(),
            final_qid,
            "All received updates must belong to the final keystroke"
        );
    }

    // Only the final keystroke should have caused provider execution
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_out_of_order_race() {
    let provider = Arc::new(
        MockProvider::new("apps", "Applications", true, Duration::from_millis(2)).with_records(
            vec![create_dummy_record(
                "app:calc",
                "Calculator",
                SearchCategory::Applications,
                450.0,
            )],
        ),
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    let dispatcher = QueryDispatcher::new(vec![provider], tx);

    // Dispatch query 1
    let qid1 = dispatcher.dispatch("query1");
    // Wait past debounce so query 1 starts
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Dispatch query 2
    let qid2 = dispatcher.dispatch("query2");

    let mut q1_seen = false;
    let mut q2_seen = false;

    while let Ok(Some(update)) = tokio::time::timeout(Duration::from_millis(120), rx.recv()).await {
        if update.query_id() == qid1 {
            assert!(
                !q2_seen,
                "Updates for query 1 must never arrive after query 2 has been received"
            );
            q1_seen = true;
        }
        if update.query_id() == qid2 {
            q2_seen = true;
            // Once qid2 is seen, no future updates for qid1 should ever arrive
            if update.is_final() {
                break;
            }
        }
    }

    let _ = q1_seen;
    assert!(q2_seen, "Must receive update for query 2");
    // Ensure that once qid2 is dispatched, any subsequent update is monotonic
    assert!(dispatcher.current_query_id() >= qid2);
}

#[tokio::test]
async fn test_deadline_cutoff() {
    let fast_provider = Arc::new(
        MockProvider::new("calc", "Calculator", true, Duration::from_millis(1)).with_records(vec![
            create_dummy_record("calc:ans", "42", SearchCategory::Calculations, 500.0),
        ]),
    );
    // Mock slow provider that sleeps 150ms (well past the 45ms hard deadline)
    let slow_provider = Arc::new(
        MockProvider::new("files", "File Indexer", false, Duration::from_millis(150)).with_records(
            vec![create_dummy_record(
                "file:doc",
                "doc.pdf",
                SearchCategory::Documents,
                300.0,
            )],
        ),
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    let dispatcher = QueryDispatcher::new(vec![fast_provider, slow_provider], tx);

    let start = Instant::now();
    let _ = dispatcher.dispatch("test query");

    let mut final_update = None;
    while let Ok(Some(update)) = tokio::time::timeout(Duration::from_millis(150), rx.recv()).await {
        if update.is_final() {
            final_update = Some(update);
            break;
        }
    }

    let elapsed = start.elapsed();
    assert!(final_update.is_some(), "Must receive final update");
    // 15ms debounce + 45ms hard deadline + scheduling epsilon < 100ms (far less than 150ms)
    assert!(
        elapsed < Duration::from_millis(100),
        "Total elapsed time ({elapsed:?}) must be capped under deadline without waiting 150ms"
    );

    let results = final_update.unwrap().results().clone();
    assert!(results.top_hit.is_some());
    assert_eq!(results.top_hit.unwrap().id, "calc:ans");
}

#[tokio::test]
async fn test_top_hit_election() {
    let rec_high = create_dummy_record(
        "app:calc",
        "Calculator",
        SearchCategory::Applications,
        450.0,
    );
    let rec_low = create_dummy_record("file:notes", "Notes.txt", SearchCategory::Documents, 320.0);
    let rec_calc = create_dummy_record(
        "calc:expr",
        "10 + 2 = 12",
        SearchCategory::Calculations,
        250.0,
    );

    let results = synthesize_results(
        1,
        vec![rec_low.clone(), rec_high.clone(), rec_calc.clone()],
        DEFAULT_TOP_HIT_THRESHOLD,
        DEFAULT_MAX_PER_CATEGORY,
    );

    assert!(results.top_hit.is_some());
    let top = results.top_hit.unwrap();
    assert_eq!(top.id, "app:calc");
    assert_eq!(top.category, SearchCategory::TopHit);

    // Remaining items grouped by category
    assert_eq!(results.sections.len(), 2);
    assert_eq!(results.sections[0].category, SearchCategory::Calculations);
    assert_eq!(results.sections[1].category, SearchCategory::Documents);

    // Below threshold case
    let below_thresh = create_dummy_record("file:a", "a.txt", SearchCategory::Documents, 380.0);
    let below_results = synthesize_results(
        2,
        vec![below_thresh],
        DEFAULT_TOP_HIT_THRESHOLD,
        DEFAULT_MAX_PER_CATEGORY,
    );
    assert!(
        below_results.top_hit.is_none(),
        "Score < 400.0 must not be elected as Top Hit"
    );
    assert_eq!(below_results.sections.len(), 1);
}

#[tokio::test]
async fn test_selection_preservation() {
    let rec_top = create_dummy_record(
        "app:calc",
        "Calculator",
        SearchCategory::Applications,
        500.0,
    );
    let rec_app2 = create_dummy_record("app:term", "Terminal", SearchCategory::Applications, 300.0);

    // Frame 1 results
    let frame_1 = synthesize_results(
        1,
        vec![rec_top.clone(), rec_app2.clone()],
        DEFAULT_TOP_HIT_THRESHOLD,
        DEFAULT_MAX_PER_CATEGORY,
    );

    let mut selection = SelectionState::new();
    let initial_idx = selection.reanchor(&frame_1);
    assert_eq!(initial_idx, Some(0));
    assert_eq!(selection.selected_id(), Some("app:calc"));

    // User navigates down to select Terminal
    selection.select_id(Some("app:term".to_string()));
    assert_eq!(selection.selected_id(), Some("app:term"));

    // Frame 2 arrives: a document is added with score 380.0
    let rec_doc = create_dummy_record(
        "file:report",
        "Report.pdf",
        SearchCategory::Documents,
        380.0,
    );
    let frame_2 = synthesize_results(
        1,
        vec![rec_top, rec_app2, rec_doc],
        DEFAULT_TOP_HIT_THRESHOLD,
        DEFAULT_MAX_PER_CATEGORY,
    );

    // Re-anchor on Frame 2 results
    let updated_idx = selection.reanchor(&frame_2);
    // Visual order: TopHit (calc), Applications (term), Documents (report)
    let flat = flatten_records(&frame_2);
    assert_eq!(flat[updated_idx.unwrap()].id, "app:term");
    assert_eq!(selection.selected_id(), Some("app:term"));
}

#[test]
fn test_query_scope_prefix_parser() {
    let p1 = parse_query_scope("app: terminal");
    assert_eq!(p1.scope, QueryScope::Apps);
    assert_eq!(p1.query, "terminal");

    let p2 = parse_query_scope("file: budget.xlsx");
    assert_eq!(p2.scope, QueryScope::Files);
    assert_eq!(p2.query, "budget.xlsx");

    let p3 = parse_query_scope("calc: 15 * 8");
    assert_eq!(p3.scope, QueryScope::Calculator);
    assert_eq!(p3.query, "15 * 8");

    let p4 = parse_query_scope("dict: pragmatic");
    assert_eq!(p4.scope, QueryScope::Dictionary);
    assert_eq!(p4.query, "pragmatic");

    let p5 = parse_query_scope("?tokio async rust");
    assert_eq!(p5.scope, QueryScope::Web);
    assert_eq!(p5.query, "tokio async rust");

    let p6 = parse_query_scope("normal search query");
    assert_eq!(p6.scope, QueryScope::All);
    assert_eq!(p6.query, "normal search query");

    // Case-insensitivity
    let p7 = parse_query_scope("APP: gedit");
    assert_eq!(p7.scope, QueryScope::Apps);
    assert_eq!(p7.query, "gedit");

    let p8 = parse_query_scope("FILE: /tmp/test");
    assert_eq!(p8.scope, QueryScope::Files);
    assert_eq!(p8.query, "/tmp/test");
}

#[tokio::test]
async fn test_failing_provider_isolation() {
    let failing_provider = Arc::new(
        MockProvider::new("broken", "Broken Provider", true, Duration::from_millis(1))
            .with_failure(),
    );
    let healthy_provider = Arc::new(
        MockProvider::new("calc", "Calculator", true, Duration::from_millis(1)).with_records(vec![
            create_dummy_record("calc:1", "42", SearchCategory::Calculations, 500.0),
        ]),
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    let dispatcher = QueryDispatcher::new(vec![failing_provider, healthy_provider], tx);

    let qid = dispatcher.dispatch("40 + 2");

    let mut final_update = None;
    while let Ok(Some(update)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        if update.query_id() == qid && update.is_final() {
            final_update = Some(update);
            break;
        }
    }

    assert!(
        final_update.is_some(),
        "Must receive final update despite one provider failing"
    );
    let results = final_update.unwrap().results().clone();
    assert!(results.top_hit.is_some());
    assert_eq!(results.top_hit.unwrap().id, "calc:1");
}

#[tokio::test]
async fn test_empty_query_handling() {
    let provider = Arc::new(
        MockProvider::new("calc", "Calculator", true, Duration::from_millis(1)).with_records(vec![
            create_dummy_record("calc:1", "42", SearchCategory::Calculations, 500.0),
        ]),
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    let dispatcher = QueryDispatcher::new(vec![provider], tx);

    let qid = dispatcher.dispatch("   ");

    let mut final_update = None;
    while let Ok(Some(update)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
        if update.query_id() == qid && update.is_final() {
            final_update = Some(update);
            break;
        }
    }

    assert!(final_update.is_some(), "Empty query must emit final update");
    let results = final_update.unwrap().results().clone();
    assert!(results.top_hit.is_none());
    assert!(results.sections.is_empty());
}
