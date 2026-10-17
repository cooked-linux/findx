//! Result score normalization, top hit arbitration, and selection stability.

use shared::models::{AggregatedSearchResults, SearchRecord};

/// Default threshold score required for an item to be promoted to Top Hit.
pub const DEFAULT_TOP_HIT_THRESHOLD: f64 = 400.0;

/// Default maximum number of items preserved per category section.
pub const DEFAULT_MAX_PER_CATEGORY: usize = 5;

/// Synthesizes, normalizes, and partitions search records into ranked categories.
#[must_use]
pub fn synthesize_results(
    _query_id: u64,
    records: Vec<SearchRecord>,
    top_hit_threshold: f64,
    max_per_category: usize,
) -> AggregatedSearchResults {
    AggregatedSearchResults::arbitrate_and_group(records, top_hit_threshold, max_per_category)
}

/// Flattens an [`AggregatedSearchResults`] structure into a sequential list of records
/// matching the exact visual display order (Top Hit first, followed by each section).
#[must_use]
pub fn flatten_records(results: &AggregatedSearchResults) -> Vec<&SearchRecord> {
    let mut flat = Vec::new();
    if let Some(ref top) = results.top_hit {
        flat.push(top);
    }
    for section in &results.sections {
        for item in &section.items {
            flat.push(item);
        }
    }
    flat
}

/// Tracks the active user selection by stable item ID across streaming frame updates
/// to prevent selection hopping and cursor jitter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionState {
    selected_id: Option<String>,
}

impl SelectionState {
    /// Constructs a new [`SelectionState`] with no item selected.
    #[must_use]
    pub const fn new() -> Self {
        Self { selected_id: None }
    }

    /// Explicitly sets the selected item ID (e.g. on mouse hover or arrow navigation).
    pub fn select_id(&mut self, id: Option<String>) {
        self.selected_id = id;
    }

    /// Returns the currently tracked item ID.
    #[must_use]
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    /// Re-anchors selection onto updated search results.
    ///
    /// Preserves the active selection by unique item ID. If the previously selected item
    /// is present in `results`, returns its new 0-based visual index. If no item was previously
    /// selected or the previous item is no longer present, defaults to the first item (index 0).
    /// Returns `None` if the result set is completely empty.
    pub fn reanchor(&mut self, results: &AggregatedSearchResults) -> Option<usize> {
        let flattened = flatten_records(results);
        if flattened.is_empty() {
            self.selected_id = None;
            return None;
        }

        if let Some(pos) = self
            .selected_id
            .as_ref()
            .and_then(|current_id| flattened.iter().position(|r| &r.id == current_id))
        {
            return Some(pos);
        }

        // Default to the first available item in the updated list
        if let Some(first) = flattened.first() {
            self.selected_id = Some(first.id.clone());
            Some(0)
        } else {
            self.selected_id = None;
            None
        }
    }

    /// Finds the visual index of a record by its unique ID.
    #[must_use]
    pub fn find_index(results: &AggregatedSearchResults, id: &str) -> Option<usize> {
        flatten_records(results).iter().position(|r| r.id == id)
    }
}
