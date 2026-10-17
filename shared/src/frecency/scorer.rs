//! Composite ranking combinator combining text match scores with frecency.
//!
//! Synthesizes lexical match quality and historical engagement recency into a unified
//! composite score, and arbitrates high-confidence "Top Hit" badge promotion.

use serde::{Deserialize, Serialize};

/// Multiplier applied to frecency score when computing composite ranking (0.4).
pub const DEFAULT_FRECENCY_WEIGHT: f64 = 0.4;

/// Minimum text score threshold (500.0) required for a candidate to be eligible for "Top Hit".
pub const DEFAULT_TOP_HIT_MIN_TEXT_SCORE: f64 = 500.0;

/// Text score threshold (1000.0) indicating an exact match, unconditionally qualifying as "Top Hit".
pub const DEFAULT_TOP_HIT_EXACT_MATCH_SCORE: f64 = 1000.0;

/// Minimum composite score lead margin (150.0) over the runner-up required for "Top Hit" election.
pub const DEFAULT_TOP_HIT_LEAD_MARGIN: f64 = 150.0;

/// An evaluated search candidate with decomposed and composite ranking metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RankedCandidate<T> {
    /// Inner candidate item (e.g., application desktop entry or file path).
    pub item: T,
    /// Lexical or fuzzy text match score (0.0 to 1000.0+).
    pub text_score: f64,
    /// Time-decayed frecency engagement score.
    pub frecency_score: f64,
    /// Synthesized composite ranking score.
    pub composite_score: f64,
    /// Whether this candidate decisively leads all competitors and earned the "Top Hit" badge.
    pub is_top_hit: bool,
}

impl<T> RankedCandidate<T> {
    /// Constructs a new `RankedCandidate` with composite score computed using default weights.
    #[must_use]
    #[allow(clippy::suboptimal_flops)]
    pub fn new(item: T, text_score: f64, frecency_score: f64) -> Self {
        let composite_score = text_score + (DEFAULT_FRECENCY_WEIGHT * frecency_score);
        Self {
            item,
            text_score,
            frecency_score,
            composite_score,
            is_top_hit: false,
        }
    }
}

/// Composite ranking engine combining text match scores with decayed frecency scores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeScorer {
    frecency_weight: f64,
    top_hit_min_text_score: f64,
    top_hit_exact_match_score: f64,
    top_hit_lead_margin: f64,
}

impl Default for CompositeScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeScorer {
    /// Creates a new `CompositeScorer` with standard default parameters.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frecency_weight: DEFAULT_FRECENCY_WEIGHT,
            top_hit_min_text_score: DEFAULT_TOP_HIT_MIN_TEXT_SCORE,
            top_hit_exact_match_score: DEFAULT_TOP_HIT_EXACT_MATCH_SCORE,
            top_hit_lead_margin: DEFAULT_TOP_HIT_LEAD_MARGIN,
        }
    }

    /// Creates a custom `CompositeScorer` specifying custom weights and thresholds.
    #[must_use]
    pub const fn with_config(
        frecency_weight: f64,
        top_hit_min_text_score: f64,
        top_hit_exact_match_score: f64,
        top_hit_lead_margin: f64,
    ) -> Self {
        Self {
            frecency_weight,
            top_hit_min_text_score,
            top_hit_exact_match_score,
            top_hit_lead_margin,
        }
    }

    /// Computes the composite score:
    /// $$\text{Composite} = \text{TextScore} + w_{\text{frecency}} \times \text{FrecencyScore}$$
    #[must_use]
    #[allow(clippy::suboptimal_flops)]
    pub const fn compute_composite(&self, text_score: f64, frecency_score: f64) -> f64 {
        text_score + (self.frecency_weight * frecency_score)
    }

    /// Constructs a `RankedCandidate` using this scorer's configuration.
    #[must_use]
    pub const fn score<T>(
        &self,
        item: T,
        text_score: f64,
        frecency_score: f64,
    ) -> RankedCandidate<T> {
        let composite_score = self.compute_composite(text_score, frecency_score);
        RankedCandidate {
            item,
            text_score,
            frecency_score,
            composite_score,
            is_top_hit: false,
        }
    }

    /// Computes composite scores, sorts candidates descending by composite score,
    /// and arbitrates "Top Hit" badge status.
    ///
    /// # Top Hit Arbitration Rules
    ///
    /// A candidate earns `is_top_hit = true` if and only if:
    /// 1. Its `text_score >= 500.0` (high quality word boundary or prefix match), AND
    /// 2. Either:
    ///    - It leads the second candidate's composite score by at least `150.0` points, OR
    ///    - Its text match is exact (`text_score >= 1000.0`).
    pub fn arbitrate_and_rank<T>(&self, candidates: &mut [RankedCandidate<T>]) {
        for candidate in candidates.iter_mut() {
            candidate.composite_score =
                self.compute_composite(candidate.text_score, candidate.frecency_score);
            candidate.is_top_hit = false;
        }

        candidates.sort_by(|a, b| b.composite_score.total_cmp(&a.composite_score));

        if candidates.is_empty() {
            return;
        }

        let first_text = candidates[0].text_score;
        if first_text >= self.top_hit_min_text_score {
            let is_exact = first_text >= self.top_hit_exact_match_score;
            let leads = if candidates.len() == 1 {
                true
            } else {
                (candidates[0].composite_score - candidates[1].composite_score)
                    >= self.top_hit_lead_margin
            };

            if is_exact || leads {
                candidates[0].is_top_hit = true;
            }
        }
    }
}

/// Convenience function that ranks and sorts a slice of candidates using default heuristics.
pub fn rank_and_sort<T>(candidates: &mut [RankedCandidate<T>]) {
    CompositeScorer::new().arbitrate_and_rank(candidates);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::suboptimal_flops
)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-9;

    #[test]
    fn test_composite_score_calculation() {
        let scorer = CompositeScorer::new();
        let candidate = scorer.score("firefox", 800.0, 100.0);
        // 800.0 + (0.4 * 100.0) = 840.0
        assert!((candidate.composite_score - 840.0).abs() < EPSILON);
        assert!(!candidate.is_top_hit);
    }

    #[test]
    fn test_top_hit_exact_match_wins_without_large_lead() {
        let mut candidates = vec![
            RankedCandidate::new("chrome", 1000.0, 0.0), // composite: 1000.0
            RankedCandidate::new("chromium", 850.0, 250.0), // composite: 850 + 100 = 950.0 (lead: 50.0 < 150)
        ];

        rank_and_sort(&mut candidates);

        assert_eq!(candidates[0].item, "chrome");
        assert!(candidates[0].is_top_hit);
        assert!(!candidates[1].is_top_hit);
    }

    #[test]
    fn test_top_hit_sufficient_lead_promotes() {
        let mut candidates = vec![
            RankedCandidate::new("terminal", 700.0, 200.0), // composite: 700 + 80 = 780.0
            RankedCandidate::new("terminator", 550.0, 50.0), // composite: 550 + 20 = 570.0 (lead: 210 >= 150)
        ];

        rank_and_sort(&mut candidates);

        assert_eq!(candidates[0].item, "terminal");
        assert!(candidates[0].is_top_hit);
    }

    #[test]
    fn test_top_hit_insufficient_lead_does_not_promote() {
        let mut candidates = vec![
            RankedCandidate::new("calc", 600.0, 100.0), // composite: 600 + 40 = 640.0
            RankedCandidate::new("calculator", 550.0, 100.0), // composite: 550 + 40 = 590.0 (lead: 50 < 150)
        ];

        rank_and_sort(&mut candidates);

        assert_eq!(candidates[0].item, "calc");
        assert!(!candidates[0].is_top_hit);
        assert!(!candidates[1].is_top_hit);
    }

    #[test]
    fn test_top_hit_rejected_if_text_score_below_500() {
        // High frecency leads significantly, but text score is only 400.0 (< 500)
        let mut candidates = vec![
            RankedCandidate::new("frequent_app", 400.0, 1000.0), // composite: 400 + 400 = 800.0
            RankedCandidate::new("runner_up", 300.0, 0.0), // composite: 300.0 (lead: 500 >= 150)
        ];

        rank_and_sort(&mut candidates);

        assert_eq!(candidates[0].item, "frequent_app");
        assert!(!candidates[0].is_top_hit);
    }

    #[test]
    fn test_single_candidate_behavior() {
        let mut single_good = vec![RankedCandidate::new("editor", 600.0, 50.0)];
        rank_and_sort(&mut single_good);
        assert!(single_good[0].is_top_hit);

        let mut single_poor = vec![RankedCandidate::new("obscure", 300.0, 50.0)];
        rank_and_sort(&mut single_poor);
        assert!(!single_poor[0].is_top_hit);
    }

    #[test]
    fn test_empty_candidates_slice() {
        let mut empty: Vec<RankedCandidate<&str>> = Vec::new();
        rank_and_sort(&mut empty);
        assert_eq!(empty.as_slice(), &[]);
    }

    #[test]
    fn test_candidates_sorted_descending() {
        let mut candidates = vec![
            RankedCandidate::new("third", 100.0, 0.0),
            RankedCandidate::new("first", 900.0, 0.0),
            RankedCandidate::new("second", 500.0, 0.0),
        ];

        rank_and_sort(&mut candidates);

        assert_eq!(candidates[0].item, "first");
        assert_eq!(candidates[1].item, "second");
        assert_eq!(candidates[2].item, "third");
    }
}
