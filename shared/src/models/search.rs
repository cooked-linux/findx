//! Search record and category models.

use std::cmp::Ordering;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::errors::ModelError;

/// Category bucket for grouping and ranking search results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SearchCategory {
    /// Highest confidence match displayed at the top.
    TopHit,
    /// Desktop applications and launcher entries.
    Applications,
    /// Documents, spreadsheets, and presentations.
    Documents,
    /// Filesystem directories and folders.
    Folders,
    /// Direct calculator expressions and unit conversions.
    Calculations,
    /// Dictionary definitions and word lookups.
    Dictionary,
    /// System actions (lock screen, sleep, logout, reboot).
    SystemActions,
    /// Web search query fallback.
    WebSearch,
}

impl SearchCategory {
    /// All categories in standard display priority order.
    pub const ALL: [Self; 8] = [
        Self::TopHit,
        Self::Applications,
        Self::Calculations,
        Self::SystemActions,
        Self::Dictionary,
        Self::Documents,
        Self::Folders,
        Self::WebSearch,
    ];

    /// Returns the human-readable display label for the category header.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::TopHit => "Top Hit",
            Self::Applications => "Applications",
            Self::Documents => "Documents",
            Self::Folders => "Folders",
            Self::Calculations => "Calculations",
            Self::Dictionary => "Dictionary",
            Self::SystemActions => "System Actions",
            Self::WebSearch => "Web Search",
        }
    }

    /// Returns the ranking arbiter priority integer (lower value means higher display precedence).
    #[must_use]
    pub const fn priority(&self) -> u32 {
        match self {
            Self::TopHit => 0,
            Self::Applications => 1,
            Self::Calculations => 2,
            Self::SystemActions => 3,
            Self::Dictionary => 4,
            Self::Documents => 5,
            Self::Folders => 6,
            Self::WebSearch => 7,
        }
    }

    /// Returns the default Freedesktop themed icon name for this category.
    #[must_use]
    pub const fn default_icon(&self) -> &'static str {
        match self {
            Self::TopHit => "starred",
            Self::Applications => "system-run",
            Self::Documents => "x-office-document",
            Self::Folders => "folder",
            Self::Calculations => "accessories-calculator",
            Self::Dictionary => "accessories-dictionary",
            Self::SystemActions => "system-shutdown",
            Self::WebSearch => "applications-internet",
        }
    }
}

impl fmt::Display for SearchCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for SearchCategory {
    type Err = ModelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "top hit" | "tophit" => Ok(Self::TopHit),
            "applications" | "apps" | "app" => Ok(Self::Applications),
            "documents" | "docs" | "doc" => Ok(Self::Documents),
            "folders" | "folder" => Ok(Self::Folders),
            "calculations" | "calc" | "calculator" => Ok(Self::Calculations),
            "dictionary" | "dict" => Ok(Self::Dictionary),
            "system actions" | "systemactions" | "actions" => Ok(Self::SystemActions),
            "web search" | "websearch" | "web" => Ok(Self::WebSearch),
            _ => Err(ModelError::validation(format!(
                "unknown search category: '{s}'"
            ))),
        }
    }
}

/// Dynamic payload data carried by a [`SearchRecord`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SearchPayload {
    /// Desktop application launch payload.
    Application {
        /// Desktop entry ID (e.g., `"firefox.desktop"`).
        desktop_id: String,
        /// Command line invocation string.
        exec: String,
    },
    /// Filesystem path match payload.
    File {
        /// Absolute path to the file or folder.
        path: PathBuf,
    },
    /// Math evaluation or unit conversion result.
    Calculation {
        /// Input expression.
        expression: String,
        /// Formatted answer or output.
        result: String,
    },
    /// Lexical dictionary or quick answer payload.
    Dictionary {
        /// Looked-up word or term.
        word: String,
        /// Lexical definition text.
        definition: String,
    },
    /// System desktop control command.
    SystemAction {
        /// Identifier for the system action.
        action: String,
    },
    /// External web search query.
    WebSearch {
        /// Formatted web query URL.
        url: String,
    },
    /// Generic or plugin-specific JSON payload.
    Custom(serde_json::Value),
}

impl SearchPayload {
    /// Returns the matched filesystem path if this payload is a [`SearchPayload::File`].
    #[must_use]
    pub fn file_path(&self) -> Option<&Path> {
        match self {
            Self::File { path } => Some(path.as_path()),
            _ => None,
        }
    }

    /// Returns the desktop ID if this payload is a [`SearchPayload::Application`].
    #[must_use]
    pub const fn desktop_id(&self) -> Option<&str> {
        match self {
            Self::Application { desktop_id, .. } => Some(desktop_id.as_str()),
            _ => None,
        }
    }

    /// Returns the execution command line if this payload is a [`SearchPayload::Application`].
    #[must_use]
    pub const fn app_exec(&self) -> Option<&str> {
        match self {
            Self::Application { exec, .. } => Some(exec.as_str()),
            _ => None,
        }
    }
}

/// A single unified search result item presented by the arbiter.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SearchRecord {
    /// Unique identifier or URI for the record.
    pub id: String,
    /// Primary title or label.
    pub title: String,
    /// Auxiliary description, path, or secondary line.
    pub subtitle: Option<String>,
    /// Logical category section for the item.
    pub category: SearchCategory,
    /// Themed icon name conforming to Freedesktop icon specification.
    pub icon_name: Option<String>,
    /// Composite ranking score calculated by the arbiter (0.0 to 1000.0).
    pub score: f64,
    /// Type-specific data required for activation or preview.
    pub payload: SearchPayload,
}

impl SearchRecord {
    /// Constructs a new [`SearchRecord`].
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        subtitle: Option<String>,
        category: SearchCategory,
        icon_name: Option<String>,
        score: f64,
        payload: SearchPayload,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle,
            category,
            icon_name,
            score,
            payload,
        }
    }

    /// Returns `true` if this record is currently categorized as the Top Hit.
    #[must_use]
    pub fn is_top_hit(&self) -> bool {
        self.category == SearchCategory::TopHit
    }

    /// Compares two search records by descending score using IEEE 754 total ordering.
    #[must_use]
    pub fn cmp_score_descending(&self, other: &Self) -> Ordering {
        other.score.total_cmp(&self.score)
    }
}

/// Category section within an aggregated result set.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CategorySection {
    /// The category represented by this section.
    pub category: SearchCategory,
    /// Ranked items belonging to this category.
    pub items: Vec<SearchRecord>,
}

/// Aggregated, ranked, and partitioned search results ready for UI presentation.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct AggregatedSearchResults {
    /// Highest confidence match (if score threshold is met).
    pub top_hit: Option<SearchRecord>,
    /// Result items grouped by category.
    pub sections: Vec<CategorySection>,
}

impl AggregatedSearchResults {
    /// Creates an empty result set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            top_hit: None,
            sections: Vec::new(),
        }
    }

    /// Synthesizes and partitions a raw list of search records into an aggregated result set.
    ///
    /// Items with score >= `top_hit_threshold` are eligible for promotion to Top Hit.
    #[must_use]
    pub fn arbitrate_and_group(
        mut records: Vec<SearchRecord>,
        top_hit_threshold: f64,
        max_per_category: usize,
    ) -> Self {
        if records.is_empty() {
            return Self::new();
        }

        // Global sort by descending score
        records.sort_by(|a, b| b.score.total_cmp(&a.score));

        let mut top_hit = None;
        let mut remaining = records;

        if remaining.first().is_some_and(|f| f.score >= top_hit_threshold) {
            let mut hit = remaining.remove(0);
            hit.category = SearchCategory::TopHit;
            top_hit = Some(hit);
        }

        // Group remaining by category
        let mut sections: Vec<CategorySection> = Vec::new();

        for record in remaining {
            if let Some(sec) = sections.iter_mut().find(|s| s.category == record.category) {
                if sec.items.len() < max_per_category {
                    sec.items.push(record);
                }
            } else {
                sections.push(CategorySection {
                    category: record.category,
                    items: vec![record],
                });
            }
        }

        // Sort sections by standard category priority
        sections.sort_by_key(|s| s.category.priority());

        Self { top_hit, sections }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_search_category_display_and_priority() {
        assert_eq!(SearchCategory::TopHit.display_name(), "Top Hit");
        assert_eq!(SearchCategory::Applications.priority(), 1);
        assert_eq!(SearchCategory::TopHit.priority(), 0);
        assert_eq!(SearchCategory::WebSearch.default_icon(), "applications-internet");

        assert_eq!("apps".parse::<SearchCategory>().unwrap(), SearchCategory::Applications);
        assert_eq!("calc".parse::<SearchCategory>().unwrap(), SearchCategory::Calculations);
        assert!("invalid_category".parse::<SearchCategory>().is_err());
    }

    #[test]
    fn test_search_payload_helpers() {
        let app_payload = SearchPayload::Application {
            desktop_id: "org.gnome.Calculator.desktop".to_string(),
            exec: "gnome-calculator".to_string(),
        };
        assert_eq!(app_payload.desktop_id(), Some("org.gnome.Calculator.desktop"));
        assert_eq!(app_payload.app_exec(), Some("gnome-calculator"));
        assert_eq!(app_payload.file_path(), None);

        let file_payload = SearchPayload::File {
            path: PathBuf::from("/etc/hosts"),
        };
        assert_eq!(file_payload.file_path(), Some(Path::new("/etc/hosts")));
        assert_eq!(file_payload.app_exec(), None);
    }

    #[test]
    fn test_search_record_sorting_and_arbitration() {
        let rec1 = SearchRecord::new(
            "app:calc",
            "Calculator",
            Some("accessories-calculator".to_string()),
            SearchCategory::Applications,
            Some("calculator".to_string()),
            500.0,
            SearchPayload::Application {
                desktop_id: "calc.desktop".to_string(),
                exec: "calc".to_string(),
            },
        );

        let rec2 = SearchRecord::new(
            "file:doc",
            "Notes.txt",
            Some("/home/user/Notes.txt".to_string()),
            SearchCategory::Documents,
            Some("text-x-generic".to_string()),
            300.0,
            SearchPayload::File {
                path: PathBuf::from("/home/user/Notes.txt"),
            },
        );

        let rec3 = SearchRecord::new(
            "calc:eval",
            "2 + 2 = 4",
            None,
            SearchCategory::Calculations,
            None,
            450.0,
            SearchPayload::Calculation {
                expression: "2 + 2".to_string(),
                result: "4".to_string(),
            },
        );

        let list = vec![rec2, rec1, rec3];
        let aggregated = AggregatedSearchResults::arbitrate_and_group(list, 400.0, 5);

        assert!(aggregated.top_hit.is_some());
        let top = aggregated.top_hit.unwrap();
        assert_eq!(top.id, "app:calc");
        assert_eq!(top.category, SearchCategory::TopHit);

        assert_eq!(aggregated.sections.len(), 2);
        assert_eq!(aggregated.sections[0].category, SearchCategory::Calculations);
        assert_eq!(aggregated.sections[1].category, SearchCategory::Documents);
    }

    #[test]
    fn test_search_record_serde() {
        let rec = SearchRecord::new(
            "web:rust",
            "Rust Programming",
            Some("https://www.rust-lang.org".to_string()),
            SearchCategory::WebSearch,
            None,
            120.0,
            SearchPayload::WebSearch {
                url: "https://www.rust-lang.org".to_string(),
            },
        );

        let json = serde_json::to_string(&rec).unwrap();
        let deserialized: SearchRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(rec, deserialized);
    }
}
