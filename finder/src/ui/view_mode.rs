//! View mode representation and controls for Finder.

use std::fmt;

/// View mode options supported by Finder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Grid of large item icons.
    Icons = 0,
    /// Detailed list view with metadata columns.
    List = 1,
    /// Multi-column hierarchical cascading directory browser.
    #[default]
    Columns = 2,
}

impl ViewMode {
    /// Returns the static string representation of the view mode.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Icons => "Icons",
            Self::List => "List",
            Self::Columns => "Columns",
        }
    }
}

impl fmt::Display for ViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_mode_defaults_and_strings() {
        assert_eq!(ViewMode::default(), ViewMode::Columns);
        assert_eq!(ViewMode::Icons.as_str(), "Icons");
        assert_eq!(ViewMode::List.as_str(), "List");
        assert_eq!(ViewMode::Columns.as_str(), "Columns");
        assert_eq!(format!("{}", ViewMode::Columns), "Columns");
    }
}
