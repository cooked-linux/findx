//! Sidebar navigation tree, places, volumes, and tag filtering logic.

pub mod config;
pub mod models;
pub mod tags;
pub mod volumes;

pub use config::SidebarConfig;

pub use models::{SidebarEntryKind, SidebarItem, SidebarSection, SidebarSectionKind};
pub use tags::{
    MACOS_TAG_PALETTE, TagFilterEvent, TagItem, load_user_tags, macos_canonical_tags,
    query_tagged_paths, resolve_tag_color_css,
};
pub use volumes::{DiscoveredVolume, VolumeService, unmount_volume};
