//! Responsive overflow layout engine for breadcrumb path bar.

use super::models::PathSegment;

/// An item to be rendered in the breadcrumbs path bar: either a segment button or an overflow ellipsis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreadcrumbDisplayItem {
    /// A single interactive breadcrumb segment.
    Segment(PathSegment),
    /// An ellipsis button `[...]` representing collapsed intermediate segments.
    Overflow {
        /// Collapsed intermediate segments that can be viewed in a popover.
        hidden_segments: Vec<PathSegment>,
    },
}

impl BreadcrumbDisplayItem {
    /// Returns `true` if this item is an overflow ellipsis.
    #[must_use]
    pub const fn is_overflow(&self) -> bool {
        matches!(self, Self::Overflow { .. })
    }

    /// Returns the inner [`PathSegment`] if this item is a segment.
    #[must_use]
    pub const fn segment(&self) -> Option<&PathSegment> {
        match self {
            Self::Segment(seg) => Some(seg),
            Self::Overflow { .. } => None,
        }
    }

    /// Returns the list of hidden segments if this item is an overflow ellipsis.
    #[must_use]
    pub fn hidden_segments(&self) -> Option<&[PathSegment]> {
        match self {
            Self::Segment(_) => None,
            Self::Overflow { hidden_segments } => Some(hidden_segments),
        }
    }
}

/// Estimates the pixel width of a segment button including icon, label, and spacing.
#[must_use]
pub fn estimate_segment_width(segment: &PathSegment, min_segment_width_px: i32) -> i32 {
    let char_width = 8;
    let base_overhead = 36; // icon + margins + padding
    let estimated = segment
        .name
        .len()
        .saturating_mul(char_width)
        .saturating_add(base_overhead);
    let estimated_i32 = i32::try_from(estimated).unwrap_or(i32::MAX);
    estimated_i32.max(min_segment_width_px)
}

/// Computes the responsive breadcrumb layout given available container width and segments.
///
/// If total required width exceeds `available_width_px` and there are more than 2 segments:
/// - Preserves left root anchor (index 0).
/// - Preserves right active leaf component (index $n - 1$).
/// - Collapses intermediate segments into an ellipsis item (`[...]`).
#[must_use]
pub fn layout_segments(
    segments: &[PathSegment],
    available_width_px: i32,
    min_segment_width_px: i32,
) -> Vec<BreadcrumbDisplayItem> {
    if segments.len() <= 2 {
        return segments
            .iter()
            .cloned()
            .map(BreadcrumbDisplayItem::Segment)
            .collect();
    }

    let separator_width = 14;
    let total_separators_width = i32::try_from(segments.len().saturating_sub(1))
        .unwrap_or(0)
        .saturating_mul(separator_width);

    let total_segment_width: i32 = segments
        .iter()
        .map(|seg| estimate_segment_width(seg, min_segment_width_px))
        .fold(0, i32::saturating_add);

    let total_needed = total_segment_width.saturating_add(total_separators_width);

    if total_needed <= available_width_px {
        // Fits completely, render all segments
        segments
            .iter()
            .cloned()
            .map(BreadcrumbDisplayItem::Segment)
            .collect()
    } else {
        // Exceeds available width: collapse intermediate segments
        let left_anchor = segments[0].clone();
        let right_anchor = segments[segments.len() - 1].clone();
        let hidden_segments = segments[1..segments.len() - 1].to_vec();

        vec![
            BreadcrumbDisplayItem::Segment(left_anchor),
            BreadcrumbDisplayItem::Overflow { hidden_segments },
            BreadcrumbDisplayItem::Segment(right_anchor),
        ]
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn dummy_segment(name: &str) -> PathSegment {
        PathSegment {
            name: name.to_string(),
            path: PathBuf::from(format!("/{name}")),
            icon_name: "folder-symbolic".to_string(),
            is_directory: true,
            is_root: name == "Computer",
            is_leaf: false,
        }
    }

    #[test]
    fn test_layout_segments_fit_wide() {
        let segs = vec![
            dummy_segment("Computer"),
            dummy_segment("home"),
            dummy_segment("user"),
        ];

        let items = layout_segments(&segs, 1000, 60);
        assert_eq!(items.len(), 3);
        assert!(!items.iter().any(BreadcrumbDisplayItem::is_overflow));
    }

    #[test]
    fn test_layout_segments_collapse_narrow() {
        let segs = vec![
            dummy_segment("Computer"),
            dummy_segment("home"),
            dummy_segment("user"),
            dummy_segment("workspace"),
            dummy_segment("src"),
            dummy_segment("main.rs"),
        ];

        let items = layout_segments(&segs, 150, 60);
        assert_eq!(items.len(), 3);

        assert_eq!(
            items[0].segment().map(|s| s.name.as_str()),
            Some("Computer")
        );
        assert!(items[1].is_overflow());
        assert_eq!(items[2].segment().map(|s| s.name.as_str()), Some("main.rs"));

        let hidden = items[1].hidden_segments().unwrap();
        assert_eq!(hidden.len(), 4);
        assert_eq!(hidden[0].name, "home");
        assert_eq!(hidden[1].name, "user");
        assert_eq!(hidden[2].name, "workspace");
        assert_eq!(hidden[3].name, "src");
    }

    #[test]
    fn test_layout_segments_two_items_no_collapse() {
        let segs = vec![dummy_segment("Computer"), dummy_segment("home")];

        let items = layout_segments(&segs, 50, 60);
        assert_eq!(items.len(), 2);
        assert!(!items.iter().any(BreadcrumbDisplayItem::is_overflow));
    }
}
