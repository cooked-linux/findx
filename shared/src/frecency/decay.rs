//! Exponential half-life decay engine for frecency calculation.
//!
//! Models engagement recency using an exponential decay formula where events
//! lose half of their weight every half-life period (default: 4 days).

use serde::{Deserialize, Serialize};

/// Default half-life period for exponential decay: 4 days (345,600 seconds).
pub const DEFAULT_HALF_LIFE_SECS: f64 = 4.0 * 86_400.0;

/// Maximum retention period for engagement events: 90 days in seconds.
pub const MAX_RETENTION_SECS: u64 = 90 * 86_400;

/// Minimum decay multiplier below which scores are capped to zero.
pub const MIN_DECAY_THRESHOLD: f64 = 1e-6;

/// Categorization of engagement events with associated weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventKind {
    /// Direct Enter or click activation (weight: 1.0).
    Launch,
    /// Document or file opened via handler (weight: 0.8).
    Open,
    /// Item revealed in file manager (weight: 0.5).
    Reveal,
}

impl EventKind {
    /// Returns the multiplier weight assigned to this event kind.
    #[must_use]
    pub const fn weight(self) -> f64 {
        match self {
            Self::Launch => 1.0,
            Self::Open => 0.8,
            Self::Reveal => 0.5,
        }
    }

    /// Returns the canonical database string representation of this event kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Open => "open",
            Self::Reveal => "reveal",
        }
    }

    /// Parses an event kind from a string slice.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "launch" => Some(Self::Launch),
            "open" => Some(Self::Open),
            "reveal" => Some(Self::Reveal),
            _ => None,
        }
    }
}

/// Computes the decayed score of an individual engagement event.
///
/// Formula:
/// $$S = 100 \cdot w_{\text{kind}} \cdot 2^{-\Delta t / T_{1/2}} = 100 \cdot w_{\text{kind}} \cdot e^{-\Delta t \cdot \ln(2) / T_{1/2}}$$
///
/// Events older than 90 days or producing a decay factor below `1e-6` evaluate to `0.0`
/// to eliminate subnormal floating-point operations.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn compute_event_score(
    event_time_epoch_secs: u64,
    now_epoch_secs: u64,
    kind: EventKind,
    half_life_secs: f64,
) -> f64 {
    let elapsed_secs = (now_epoch_secs.saturating_sub(event_time_epoch_secs)) as f64;

    // Prune events older than 90 days or invalid non-positive half-life
    if elapsed_secs > (MAX_RETENTION_SECS as f64) || half_life_secs <= 0.0 {
        return 0.0;
    }

    let decay = (-elapsed_secs * std::f64::consts::LN_2 / half_life_secs).exp();

    if decay < MIN_DECAY_THRESHOLD || decay.is_nan() {
        return 0.0;
    }

    100.0 * kind.weight() * decay
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::suboptimal_flops
)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-9;

    #[test]
    fn test_event_kind_properties() {
        assert!((EventKind::Launch.weight() - 1.0).abs() < EPSILON);
        assert!((EventKind::Open.weight() - 0.8).abs() < EPSILON);
        assert!((EventKind::Reveal.weight() - 0.5).abs() < EPSILON);

        assert_eq!(EventKind::Launch.as_str(), "launch");
        assert_eq!(EventKind::Open.as_str(), "open");
        assert_eq!(EventKind::Reveal.as_str(), "reveal");

        assert_eq!(EventKind::parse("launch"), Some(EventKind::Launch));
        assert_eq!(EventKind::parse("open"), Some(EventKind::Open));
        assert_eq!(EventKind::parse("reveal"), Some(EventKind::Reveal));
        assert_eq!(EventKind::parse("unknown"), None);
    }

    #[test]
    fn test_decay_half_life_exact_intervals() {
        let base_time: u64 = 1_700_000_000;
        let four_days_later = base_time + (4 * 86_400);
        let eight_days_later = base_time + (8 * 86_400);

        // Immediate: 100 * 1.0 * 1.0 = 100.0
        let score_now = compute_event_score(
            base_time,
            base_time,
            EventKind::Launch,
            DEFAULT_HALF_LIFE_SECS,
        );
        assert!((score_now - 100.0).abs() < EPSILON);

        // 4 days (1 half-life): 50% drop -> 50.0
        let score_4d = compute_event_score(
            base_time,
            four_days_later,
            EventKind::Launch,
            DEFAULT_HALF_LIFE_SECS,
        );
        assert!((score_4d - 50.0).abs() < EPSILON);

        // 8 days (2 half-lives): 75% drop -> 25.0
        let score_8d = compute_event_score(
            base_time,
            eight_days_later,
            EventKind::Launch,
            DEFAULT_HALF_LIFE_SECS,
        );
        assert!((score_8d - 25.0).abs() < EPSILON);
    }

    #[test]
    fn test_event_weight_ordering() {
        let now = 1_700_000_000;
        let launch_score = compute_event_score(now, now, EventKind::Launch, DEFAULT_HALF_LIFE_SECS);
        let open_score = compute_event_score(now, now, EventKind::Open, DEFAULT_HALF_LIFE_SECS);
        let reveal_score = compute_event_score(now, now, EventKind::Reveal, DEFAULT_HALF_LIFE_SECS);

        assert!(launch_score > open_score);
        assert!(open_score > reveal_score);
        assert!((launch_score - 100.0).abs() < EPSILON);
        assert!((open_score - 80.0).abs() < EPSILON);
        assert!((reveal_score - 50.0).abs() < EPSILON);
    }

    #[test]
    fn test_stale_events_pruned() {
        let base_time = 1_700_000_000;
        // 91 days later
        let future_time = base_time + (91 * 86_400);
        let score = compute_event_score(
            base_time,
            future_time,
            EventKind::Launch,
            DEFAULT_HALF_LIFE_SECS,
        );
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_future_event_saturates_to_zero_elapsed() {
        let base_time = 1_700_000_000;
        // Event timestamp is in the future relative to `now`
        let score = compute_event_score(
            base_time + 100,
            base_time,
            EventKind::Launch,
            DEFAULT_HALF_LIFE_SECS,
        );
        assert!((score - 100.0).abs() < EPSILON);
    }

    #[test]
    fn test_invalid_half_life() {
        let now = 1_700_000_000;
        let score = compute_event_score(now, now, EventKind::Launch, 0.0);
        assert_eq!(score, 0.0);

        let score_neg = compute_event_score(now, now, EventKind::Launch, -100.0);
        assert_eq!(score_neg, 0.0);
    }
}
