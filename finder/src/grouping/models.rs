//! Domain models and types for dynamic categorical grouping.

use serde::{Deserialize, Serialize};

/// Metadata dimension used to group filesystem items into visual sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum GroupDimension {
    /// No grouping applied; flat list.
    #[default]
    None,
    /// Categorized by MIME kind (Folders, Images, Audio, Documents, etc.).
    Kind,
    /// Partitioned into relative temporal bins based on last modification time.
    DateModified,
    /// Partitioned into relative temporal bins based on file creation / addition time.
    DateAdded,
    /// Partitioned into standardized byte size buckets.
    Size,
    /// Partitioned by user-applied color and semantic tags.
    Tags,
}

impl GroupDimension {
    /// Returns the human-readable display label for the grouping dimension.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Kind => "Kind",
            Self::DateModified => "Date Modified",
            Self::DateAdded => "Date Added",
            Self::Size => "Size",
            Self::Tags => "Tags",
        }
    }
}

/// Unique sorting and identity key representing a category group.
///
/// Groups are sorted primarily by `priority` ascending (0 is top/first),
/// then by `title`, and finally by `id`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GroupKey {
    /// Sort priority for section placement (lower values appear first, e.g. 0 for Folders).
    pub priority: u32,
    /// Semantic identifier for the group (e.g. `"folders"`, `"images"`, `"today"`).
    pub id: String,
    /// User-facing display title for the section header (e.g. `"Folders"`, `"Images"`).
    pub title: String,
}

impl GroupKey {
    /// Creates a new [`GroupKey`].
    #[must_use]
    pub fn new(priority: u32, id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            priority,
            id: id.into(),
            title: title.into(),
        }
    }
}

/// A collapsible group section containing ordered items belonging to a categorical key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionGroup<T> {
    /// Identifying key and sort order for the section.
    pub key: GroupKey,
    /// Items partitioned into this section.
    pub items: Vec<T>,
    /// Whether this section is currently collapsed in the view.
    pub is_collapsed: bool,
}

impl<T> SectionGroup<T> {
    /// Creates a new [`SectionGroup`] with empty items and uncollapsed state.
    #[must_use]
    pub const fn new(key: GroupKey) -> Self {
        Self {
            key,
            items: Vec::new(),
            is_collapsed: false,
        }
    }

    /// Creates a new [`SectionGroup`] with the given items and collapsed state.
    #[must_use]
    pub const fn with_items(key: GroupKey, items: Vec<T>, is_collapsed: bool) -> Self {
        Self {
            key,
            items,
            is_collapsed,
        }
    }

    /// Returns the number of items in this section.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if the section contains no items.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
