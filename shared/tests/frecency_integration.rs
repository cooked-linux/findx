//! Integration tests for the frecency ranking engine, exponential half-life decay,
//! SQLite event persistence, query keyword boosts, and composite Top Hit arbitration.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation
)]

use rusqlite::Connection;
use shared::db::migrations::apply_migrations;
use shared::frecency::{
    CompositeScorer, DEFAULT_HALF_LIFE_SECS, EventKind, FrecencyTracker, RankedCandidate,
    compute_event_score, rank_and_sort,
};
use std::sync::{Arc, Mutex};

const EPSILON: f64 = 1e-6;

fn setup_in_memory_tracker() -> FrecencyTracker {
    let mut conn = Connection::open_in_memory().expect("open SQLite in-memory");
    apply_migrations(&mut conn).expect("apply database migrations");
    FrecencyTracker::new(Arc::new(Mutex::new(conn)))
}

#[test]
fn test_integration_half_life_decay_math() {
    let t0: u64 = 1_700_000_000;
    let four_days: u64 = 4 * 86_400;
    let eight_days: u64 = 8 * 86_400;
    let twelve_days: u64 = 12 * 86_400;

    // t = 0: 100.0
    let score_0 = compute_event_score(t0, t0, EventKind::Launch, DEFAULT_HALF_LIFE_SECS);
    assert!((score_0 - 100.0).abs() < EPSILON);

    // After exactly 4 days: 50% drop (score = 50.0)
    let score_4d = compute_event_score(
        t0,
        t0 + four_days,
        EventKind::Launch,
        DEFAULT_HALF_LIFE_SECS,
    );
    assert!(
        (score_4d - 50.0).abs() < EPSILON,
        "Expected score after 4 days to drop by 50% to 50.0, got {score_4d}"
    );

    // After exactly 8 days: 75% drop (score = 25.0)
    let score_8d = compute_event_score(
        t0,
        t0 + eight_days,
        EventKind::Launch,
        DEFAULT_HALF_LIFE_SECS,
    );
    assert!(
        (score_8d - 25.0).abs() < EPSILON,
        "Expected score after 8 days to drop by 75% to 25.0, got {score_8d}"
    );

    // After 12 days (3 half-lives): 87.5% drop (score = 12.5)
    let score_12d = compute_event_score(
        t0,
        t0 + twelve_days,
        EventKind::Launch,
        DEFAULT_HALF_LIFE_SECS,
    );
    assert!(
        (score_12d - 12.5).abs() < EPSILON,
        "Expected score after 12 days to drop to 12.5, got {score_12d}"
    );
}

#[test]
fn test_integration_event_weights() {
    let now = 1_700_000_000;

    let launch = compute_event_score(now, now, EventKind::Launch, DEFAULT_HALF_LIFE_SECS);
    let open = compute_event_score(now, now, EventKind::Open, DEFAULT_HALF_LIFE_SECS);
    let reveal = compute_event_score(now, now, EventKind::Reveal, DEFAULT_HALF_LIFE_SECS);

    // Launch (1.0 weight) > Open (0.8 weight) > Reveal (0.5 weight)
    assert!(launch > open);
    assert!(open > reveal);

    assert!((launch - 100.0).abs() < EPSILON);
    assert!((open - 80.0).abs() < EPSILON);
    assert!((reveal - 50.0).abs() < EPSILON);

    // Verify ordering is preserved when decayed over 4 days
    let four_days_later = now + (4 * 86_400);
    let launch_decayed = compute_event_score(
        now,
        four_days_later,
        EventKind::Launch,
        DEFAULT_HALF_LIFE_SECS,
    );
    let open_decayed = compute_event_score(
        now,
        four_days_later,
        EventKind::Open,
        DEFAULT_HALF_LIFE_SECS,
    );
    let reveal_decayed = compute_event_score(
        now,
        four_days_later,
        EventKind::Reveal,
        DEFAULT_HALF_LIFE_SECS,
    );

    assert!(launch_decayed > open_decayed);
    assert!(open_decayed > reveal_decayed);
    assert!((launch_decayed - 50.0).abs() < EPSILON);
    assert!((open_decayed - 40.0).abs() < EPSILON);
    assert!((reveal_decayed - 25.0).abs() < EPSILON);
}

#[test]
fn test_integration_recording_and_aggregate_scores() {
    let tracker = setup_in_memory_tracker();
    let now: u64 = 1_700_000_000;

    // Record events for "code.desktop":
    // 1. Launch at `now` (weight: 100.0)
    tracker
        .record_event("code.desktop", "app", EventKind::Launch, None, now)
        .expect("record launch 1");

    // 2. Launch 4 days ago (weight: 100 * 0.5 = 50.0)
    tracker
        .record_event(
            "code.desktop",
            "app",
            EventKind::Launch,
            None,
            now - (4 * 86_400),
        )
        .expect("record launch 2");

    // 3. Open 8 days ago (weight: 80 * 0.25 = 20.0)
    tracker
        .record_event(
            "code.desktop",
            "app",
            EventKind::Open,
            None,
            now - (8 * 86_400),
        )
        .expect("record open 1");

    // Expected aggregate: 100.0 + 50.0 + 20.0 = 170.0
    let code_score = tracker
        .calculate_score("code.desktop", now)
        .expect("calculate score");
    assert!(
        (code_score - 170.0).abs() < EPSILON,
        "Expected aggregate score of 170.0 for code.desktop, got {code_score}"
    );

    // Record events for a file:
    tracker
        .record_event("/doc.pdf", "file", EventKind::Open, None, now)
        .expect("record file open");
    let file_score = tracker
        .calculate_score("/doc.pdf", now)
        .expect("file score");
    assert!((file_score - 80.0).abs() < EPSILON);

    // Unrecorded item yields 0.0
    let unknown_score = tracker
        .calculate_score("missing.desktop", now)
        .expect("missing score");
    assert_eq!(unknown_score, 0.0);

    // Test prune_stale_events
    // Insert an old event 100 days ago
    tracker
        .record_event(
            "old.desktop",
            "app",
            EventKind::Launch,
            None,
            now - (100 * 86_400),
        )
        .expect("record old");
    assert_eq!(tracker.count_all_events().expect("count"), 5);

    let cutoff = now - (90 * 86_400);
    let pruned = tracker.prune_stale_events(cutoff).expect("prune");
    assert_eq!(pruned, 1);
    assert_eq!(tracker.count_all_events().expect("count"), 4);
}

#[test]
fn test_integration_keyword_boost() {
    let tracker = setup_in_memory_tracker();
    let now: u64 = 1_700_000_000;

    let app_id = "org.gnome.Terminal.desktop";

    // Record an event launched via query "term"
    tracker
        .record_event(app_id, "app", EventKind::Launch, Some("term"), now)
        .expect("record term launch");

    // Calculate score without query
    let base_score = tracker.calculate_score(app_id, now).expect("base score");
    assert!((base_score - 100.0).abs() < EPSILON);

    // Matching query "term" gives 1.2x multiplier
    let boost_term = tracker
        .calculate_keyword_boost(app_id, "term", now)
        .expect("boost term");
    assert!((boost_term - 1.2).abs() < EPSILON);

    // Calculate score with matching query: 100.0 * 1.2 = 120.0
    let score_with_term = tracker
        .calculate_score_with_query(app_id, Some("term"), now)
        .expect("score with query");
    assert!(
        score_with_term > base_score,
        "Score with matched query should exceed base score"
    );
    assert!((score_with_term - 120.0).abs() < EPSILON);

    // Case-insensitive query match ("TERM")
    let boost_term_caps = tracker
        .calculate_keyword_boost(app_id, "TERM", now)
        .expect("boost caps");
    assert!((boost_term_caps - 1.2).abs() < EPSILON);

    // Non-matching query ("browser") yields 1.0x boost
    let boost_other = tracker
        .calculate_keyword_boost(app_id, "browser", now)
        .expect("boost other");
    assert_eq!(boost_other, 1.0);

    let score_with_other = tracker
        .calculate_score_with_query(app_id, Some("browser"), now)
        .expect("score other");
    assert!((score_with_other - base_score).abs() < EPSILON);

    // Multiple matched events increase boost up to 1.5x cap
    tracker
        .record_event(app_id, "app", EventKind::Launch, Some("term"), now)
        .expect("record term launch 2");
    tracker
        .record_event(app_id, "app", EventKind::Launch, Some("term"), now)
        .expect("record term launch 3");

    let boost_capped = tracker
        .calculate_keyword_boost(app_id, "term", now)
        .expect("boost capped");
    assert!((boost_capped - 1.5).abs() < EPSILON);
}

#[test]
fn test_integration_top_hit_election() {
    let scorer = CompositeScorer::new();

    // Scenario 1: High text score + frecency leads decisively -> Top Hit elected
    let mut candidates = vec![
        scorer.score("firefox.desktop", 800.0, 500.0), // composite = 800 + 200 = 1000.0
        scorer.score("file-roller.desktop", 550.0, 50.0), // composite = 550 + 20 = 570.0
        scorer.score("font-viewer.desktop", 500.0, 0.0), // composite = 500.0
    ];

    rank_and_sort(&mut candidates);

    assert_eq!(candidates[0].item, "firefox.desktop");
    assert!(
        candidates[0].is_top_hit,
        "Candidate with text >= 500 and lead >= 150 must earn Top Hit badge"
    );
    assert!(!candidates[1].is_top_hit);
    assert!(!candidates[2].is_top_hit);

    // Scenario 2: Exact text match (text_score >= 1000.0) earns Top Hit even with narrow lead
    let mut exact_match_candidates = vec![
        RankedCandidate::new("calc", 1000.0, 0.0), // composite = 1000.0
        RankedCandidate::new("calculator", 850.0, 250.0), // composite = 850 + 100 = 950.0 (lead: 50 < 150)
    ];

    rank_and_sort(&mut exact_match_candidates);

    assert_eq!(exact_match_candidates[0].item, "calc");
    assert!(
        exact_match_candidates[0].is_top_hit,
        "Exact text match (text >= 1000) must earn Top Hit regardless of lead margin"
    );
    assert!(!exact_match_candidates[1].is_top_hit);

    // Scenario 3: Narrow lead without exact text match fails Top Hit election
    let mut close_candidates = vec![
        RankedCandidate::new("app_a", 600.0, 100.0), // composite = 600 + 40 = 640.0
        RankedCandidate::new("app_b", 550.0, 100.0), // composite = 550 + 40 = 590.0 (lead: 50 < 150)
    ];

    rank_and_sort(&mut close_candidates);

    assert_eq!(close_candidates[0].item, "app_a");
    assert!(
        !close_candidates[0].is_top_hit,
        "Narrow lead (< 150 points) without exact match must NOT earn Top Hit"
    );
    assert!(!close_candidates[1].is_top_hit);

    // Scenario 4: Text score < 500.0 fails Top Hit election even with massive lead
    let mut low_text_candidates = vec![
        RankedCandidate::new("frequent_app", 450.0, 1000.0), // composite = 450 + 400 = 850.0
        RankedCandidate::new("runner_up", 300.0, 0.0),       // composite = 300.0 (lead: 550 >= 150)
    ];

    rank_and_sort(&mut low_text_candidates);

    assert_eq!(low_text_candidates[0].item, "frequent_app");
    assert!(
        !low_text_candidates[0].is_top_hit,
        "Candidates with text score < 500.0 must never earn Top Hit"
    );
}
