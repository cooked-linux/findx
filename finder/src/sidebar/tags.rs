//! Tags section models, macOS canonical color palettes, and database tag cache reader.

use std::path::PathBuf;

use shared::db::{Database, list_all_tags, query_paths_by_tag};
use shared::models::TagColor;

use crate::sidebar::models::{SidebarEntryKind, SidebarItem};

/// Canonical macOS color tags with exact design hex codes.
pub const MACOS_TAG_PALETTE: [(TagColor, &str, &str); 7] = [
    (TagColor::Red, "Red", "#ff5f57"),
    (TagColor::Orange, "Orange", "#ff9f0a"),
    (TagColor::Yellow, "Yellow", "#ffd60a"),
    (TagColor::Green, "Green", "#30d158"),
    (TagColor::Blue, "Blue", "#0a84ff"),
    (TagColor::Purple, "Purple", "#bf5af2"),
    (TagColor::Gray, "Gray", "#98989d"),
];

/// Domain model representing a tag in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagItem {
    /// Tag label name (e.g. "Red", "Work", "Client").
    pub label: String,
    /// Canonical color variant if associated with a macOS label.
    pub color: Option<TagColor>,
    /// CSS hex color code string.
    pub hex_color: &'static str,
    /// Number of tagged files or items.
    pub file_count: u64,
}

/// Navigation filter event emitted when a tag is clicked or filter is cleared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagFilterEvent {
    /// Filter active directory items by tag.
    Selected {
        /// Tag display name or filter label.
        tag_name: String,
        /// Color variant if this is a canonical color label.
        color: Option<TagColor>,
    },
    /// Clear active tag filter and restore standard directory listing.
    Cleared,
}

/// Resolves the design CSS hex color for a [`TagColor`].
#[must_use]
pub const fn resolve_tag_color_css(color: Option<TagColor>) -> &'static str {
    match color {
        Some(TagColor::Red) => "#ff5f57",
        Some(TagColor::Orange) => "#ff9f0a",
        Some(TagColor::Yellow) => "#ffd60a",
        Some(TagColor::Green) => "#30d158",
        Some(TagColor::Blue) => "#0a84ff",
        Some(TagColor::Purple) => "#bf5af2",
        Some(TagColor::Gray) | None => "#98989d",
    }
}

/// Returns the 7 canonical macOS color tag items in standard display order.
#[must_use]
pub fn macos_canonical_tags() -> Vec<TagItem> {
    MACOS_TAG_PALETTE
        .iter()
        .map(|&(color, name, hex)| TagItem {
            label: name.to_string(),
            color: Some(color),
            hex_color: hex,
            file_count: 0,
        })
        .collect()
}

/// Loads custom user tags and frequencies from `shared::db::Database`.
#[must_use]
pub fn load_user_tags() -> Vec<TagItem> {
    let Ok(db) = Database::open_default() else {
        return Vec::new();
    };

    load_user_tags_from_db(&db)
}

/// Reads user tags from an active database connection.
#[must_use]
pub fn load_user_tags_from_db(db: &Database) -> Vec<TagItem> {
    let Ok(raw_tags) = list_all_tags(db) else {
        return Vec::new();
    };

    raw_tags
        .into_iter()
        .map(|(name, count)| {
            let color = TagColor::from_name(&name);
            let hex = resolve_tag_color_css(color);
            TagItem {
                label: name,
                color,
                hex_color: hex,
                file_count: count,
            }
        })
        .collect()
}

/// Queries all filesystem paths indexed under `tag_name` from the database.
#[must_use]
pub fn query_tagged_paths(tag_name: &str) -> Vec<PathBuf> {
    let Ok(db) = Database::open_default() else {
        return Vec::new();
    };

    query_paths_by_tag(&db, tag_name).map_or_else(
        |_| Vec::new(),
        |paths| paths.into_iter().map(PathBuf::from).collect(),
    )
}

/// Converts a [`TagItem`] into a [`SidebarItem`].
#[must_use]
pub fn tag_to_sidebar_item(tag: &TagItem) -> SidebarItem {
    SidebarItem {
        id: format!("tag:{}", tag.label),
        display_name: tag.label.clone(),
        icon_name: "tag-symbolic".to_string(),
        kind: SidebarEntryKind::Tag {
            color: tag.color,
            label: tag.label.clone(),
        },
        is_active: false,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_tags_count_and_colors() {
        let tags = macos_canonical_tags();
        assert_eq!(tags.len(), 7);
        assert_eq!(tags[0].label, "Red");
        assert_eq!(tags[0].hex_color, "#ff5f57");
        assert_eq!(tags[0].color, Some(TagColor::Red));

        assert_eq!(tags[6].label, "Gray");
        assert_eq!(tags[6].hex_color, "#98989d");
        assert_eq!(tags[6].color, Some(TagColor::Gray));
    }

    #[test]
    fn test_resolve_tag_color_css() {
        assert_eq!(resolve_tag_color_css(Some(TagColor::Blue)), "#0a84ff");
        assert_eq!(resolve_tag_color_css(None), "#98989d");
    }

    #[test]
    fn test_tag_to_sidebar_item() {
        let item = TagItem {
            label: "Work".to_string(),
            color: None,
            hex_color: "#98989d",
            file_count: 5,
        };
        let sb_item = tag_to_sidebar_item(&item);
        assert_eq!(sb_item.id, "tag:Work");
        assert_eq!(sb_item.display_name, "Work");
        assert_eq!(
            sb_item.kind,
            SidebarEntryKind::Tag {
                color: None,
                label: "Work".to_string(),
            }
        );
    }
}
