//! Sidebar data models, section definitions, and navigation item structures.

use std::path::PathBuf;

use shared::models::TagColor;

/// Categorized section identifier for the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SidebarSectionKind {
    /// Pinned user places and folders.
    Favorites,
    /// Connected storage devices, disk partitions, and removable volumes.
    Locations,
    /// Color labels and user tags.
    Tags,
}

/// Detailed payload discriminating sidebar item behavior and metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SidebarEntryKind {
    /// Pinned filesystem place or standard directory.
    Place {
        /// Target filesystem path for navigation.
        path: PathBuf,
        /// Whether this place can be removed/unpinned from favorites.
        is_removable_place: bool,
    },
    /// Mounted block device or removable storage volume.
    Volume {
        /// D-Bus block device object path or sysfs device node.
        device_path: String,
        /// Primary filesystem mount point directory.
        mount_point: PathBuf,
        /// Whether the volume supports software eject or safe power off.
        is_ejectable: bool,
        /// Whether an unmount or eject operation is currently in-flight.
        is_busy: bool,
    },
    /// macOS color label or user tag filter.
    Tag {
        /// Canonical tag color label, if defined.
        color: Option<TagColor>,
        /// Tag display name or filter query label.
        label: String,
    },
}

/// Individual item row within a sidebar section.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SidebarItem {
    /// Unique identifier for this item (e.g. URI, device path, or tag key).
    pub id: String,
    /// Human-readable title displayed in the row.
    pub display_name: String,
    /// Freedesktop symbolic icon name (e.g. "user-home-symbolic", "drive-removable-media-symbolic").
    pub icon_name: String,
    /// Domain-specific entry classification.
    pub kind: SidebarEntryKind,
    /// Whether this sidebar row represents the active main view location/filter.
    pub is_active: bool,
}

impl SidebarItem {
    /// Constructs a new [`SidebarItem`] representing a filesystem place.
    #[must_use]
    pub fn place(
        id: impl Into<String>,
        display_name: impl Into<String>,
        icon_name: impl Into<String>,
        path: PathBuf,
        is_removable_place: bool,
    ) -> Self {
        Self {
            id: id.into(),
            display_name: display_name.into(),
            icon_name: icon_name.into(),
            kind: SidebarEntryKind::Place {
                path,
                is_removable_place,
            },
            is_active: false,
        }
    }

    /// Constructs a new [`SidebarItem`] representing a mounted storage volume.
    #[must_use]
    pub fn volume(
        device_path: impl Into<String>,
        display_name: impl Into<String>,
        icon_name: impl Into<String>,
        mount_point: PathBuf,
        is_ejectable: bool,
    ) -> Self {
        let dev = device_path.into();
        Self {
            id: dev.clone(),
            display_name: display_name.into(),
            icon_name: icon_name.into(),
            kind: SidebarEntryKind::Volume {
                device_path: dev,
                mount_point,
                is_ejectable,
                is_busy: false,
            },
            is_active: false,
        }
    }

    /// Constructs a new [`SidebarItem`] representing a color tag or label.
    #[must_use]
    pub fn tag(
        label: impl Into<String>,
        color: Option<TagColor>,
        icon_name: impl Into<String>,
    ) -> Self {
        let lbl = label.into();
        Self {
            id: format!("tag:{lbl}"),
            display_name: lbl.clone(),
            icon_name: icon_name.into(),
            kind: SidebarEntryKind::Tag { color, label: lbl },
            is_active: false,
        }
    }
}

/// Collapsible categorized container holding a group of sidebar items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarSection {
    /// Category classification kind.
    pub kind: SidebarSectionKind,
    /// Header display title (e.g. "Favorites", "Locations", "Tags").
    pub title: &'static str,
    /// Whether this section is currently collapsed.
    pub is_collapsed: bool,
    /// List of child item entries in this section.
    pub items: Vec<SidebarItem>,
}

impl SidebarSection {
    /// Constructs a new [`SidebarSection`].
    #[must_use]
    pub const fn new(
        kind: SidebarSectionKind,
        title: &'static str,
        items: Vec<SidebarItem>,
    ) -> Self {
        Self {
            kind,
            title,
            is_collapsed: false,
            items,
        }
    }
}
