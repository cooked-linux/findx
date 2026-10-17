//! Integration tests for Sidebar models, Freedesktop bookmarks, volume service, and tag palettes.

use std::path::PathBuf;

use finder::bookmarks::{BookmarkEntry, default_places, parse_bookmarks, serialize_bookmarks};
use finder::sidebar::config::SidebarConfig;
use finder::sidebar::models::{SidebarEntryKind, SidebarItem, SidebarSection, SidebarSectionKind};
use finder::sidebar::tags::{
    MACOS_TAG_PALETTE, TagFilterEvent, TagItem, macos_canonical_tags, resolve_tag_color_css,
    tag_to_sidebar_item,
};
use finder::sidebar::volumes::{DiscoveredVolume, parse_mount_lines, unescape_octal};
use shared::ipc::{DriveConnectionType, MountedVolume};
use shared::models::TagColor;

#[test]
fn test_bookmarks_parser_roundtrip_with_special_characters_and_spaces() {
    let raw_entries = vec![
        BookmarkEntry {
            uri: "file:///home/user/Project%20Alpha".to_string(),
            label: Some("Project Alpha".to_string()),
            path: PathBuf::from("/home/user/Project Alpha"),
        },
        BookmarkEntry {
            uri: "file:///home/user/Caf%C3%A9%20Latte".to_string(),
            label: Some("Café Latte".to_string()),
            path: PathBuf::from("/home/user/Café Latte"),
        },
        BookmarkEntry {
            uri: "file:///home/user/Documents".to_string(),
            label: None,
            path: PathBuf::from("/home/user/Documents"),
        },
        BookmarkEntry {
            uri: "trash:///".to_string(),
            label: Some("Trash".to_string()),
            path: PathBuf::from("/home/user/.local/share/Trash"),
        },
    ];

    let serialized = serialize_bookmarks(&raw_entries);
    let parsed = parse_bookmarks(&serialized);

    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].uri, "file:///home/user/Project%20Alpha");
    assert_eq!(parsed[0].label.as_deref(), Some("Project Alpha"));
    assert_eq!(parsed[0].path, PathBuf::from("/home/user/Project Alpha"));

    assert_eq!(parsed[1].path, PathBuf::from("/home/user/Café Latte"));
    assert_eq!(parsed[1].label.as_deref(), Some("Café Latte"));

    assert_eq!(parsed[2].label, None);
    assert_eq!(parsed[2].path, PathBuf::from("/home/user/Documents"));

    assert_eq!(parsed[3].uri, "trash:///");
    assert_eq!(parsed[3].label.as_deref(), Some("Trash"));
}

#[test]
fn test_bookmark_entry_display_and_icon_resolutions() {
    let home = BookmarkEntry::from_path("/home/developer", Some("Home".to_string()));
    assert_eq!(home.display_name(), "Home");
    assert_eq!(home.icon_name(), "user-home-symbolic");

    let downloads = BookmarkEntry::from_path("/home/developer/Downloads", None);
    assert_eq!(downloads.display_name(), "Downloads");
    assert_eq!(downloads.icon_name(), "folder-download-symbolic");

    let custom = BookmarkEntry::from_path("/opt/custom", Some("Custom Folder".to_string()));
    assert_eq!(custom.display_name(), "Custom Folder");
    assert_eq!(custom.icon_name(), "folder-symbolic");

    let trash = BookmarkEntry::new("trash:///", "/home/user/.local/share/Trash", None);
    assert_eq!(trash.display_name(), "Trash");
    assert_eq!(trash.icon_name(), "user-trash-symbolic");
}

#[test]
fn test_default_places_generation() {
    let places = default_places();
    assert_eq!(places.len(), 5);

    let labels: Vec<String> = places
        .iter()
        .map(finder::BookmarkEntry::display_name)
        .collect();
    assert!(labels.contains(&"Home".to_string()));
    assert!(labels.contains(&"Desktop".to_string()));
    assert!(labels.contains(&"Documents".to_string()));
    assert!(labels.contains(&"Downloads".to_string()));
    assert!(labels.contains(&"Trash".to_string()));
}

#[test]
fn test_volume_entry_generation_and_eject_removability() {
    // 1. Removable USB flash drive
    let mounted_usb = MountedVolume {
        id: "/org/freedesktop/UDisks2/block_devices/sdb1".to_string(),
        label: "USB Backup".to_string(),
        mount_path: PathBuf::from("/run/media/user/USB Backup"),
        size_bytes: 32_000_000_000,
        connection_type: DriveConnectionType::Usb,
        is_removable: true,
        drive_path: Some("/org/freedesktop/UDisks2/drives/SanDisk".to_string()),
    };

    let vol = DiscoveredVolume::from_mounted_volume(&mounted_usb);
    assert_eq!(
        vol.device_path,
        "/org/freedesktop/UDisks2/block_devices/sdb1"
    );
    assert_eq!(vol.label, "USB Backup");
    assert_eq!(vol.icon_name, "drive-removable-media-symbolic");
    assert!(vol.is_removable);
    assert!(vol.is_ejectable);

    // 2. Internal fixed NVMe drive
    let mounted_nvme = MountedVolume {
        id: "/org/freedesktop/UDisks2/block_devices/nvme0n1p2".to_string(),
        label: "Internal SSD".to_string(),
        mount_path: PathBuf::from("/"),
        size_bytes: 1_000_000_000_000,
        connection_type: DriveConnectionType::Nvme,
        is_removable: false,
        drive_path: None,
    };

    let fixed_vol = DiscoveredVolume::from_mounted_volume(&mounted_nvme);
    assert_eq!(fixed_vol.label, "Internal SSD");
    assert_eq!(fixed_vol.icon_name, "drive-harddisk-symbolic");
    assert!(!fixed_vol.is_removable);
    assert!(!fixed_vol.is_ejectable);
}

#[test]
fn test_proc_mounts_parser_and_octal_decoding() {
    let sample_mounts = r"
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
/dev/nvme0n1p2 / ext4 rw,relatime 0 0
/dev/sdb1 /media/user/My\040Flash\040Drive vfat rw,nosuid,nodev 0 0
/dev/loop5 /snap/core/1234 squashfs ro 0 0
tmpfs /dev/shm tmpfs rw 0 0
";

    let volumes = parse_mount_lines(sample_mounts);
    assert_eq!(volumes.len(), 2);

    assert_eq!(volumes[0].device_path, "/dev/nvme0n1p2");
    assert_eq!(volumes[0].mount_point, PathBuf::from("/"));
    assert_eq!(volumes[0].label, "System Root");
    assert!(!volumes[0].is_removable);

    assert_eq!(volumes[1].device_path, "/dev/sdb1");
    assert_eq!(
        volumes[1].mount_point,
        PathBuf::from("/media/user/My Flash Drive")
    );
    assert_eq!(volumes[1].label, "My Flash Drive");
    assert!(volumes[1].is_removable);
    assert!(volumes[1].is_ejectable);

    assert_eq!(
        unescape_octal("/media/Volume\\040With\\040Spaces"),
        "/media/Volume With Spaces"
    );
}

#[test]
fn test_macos_canonical_tag_palette_and_filter_mapping() {
    let canonical = macos_canonical_tags();
    assert_eq!(canonical.len(), 7);

    let expected = [
        ("Red", TagColor::Red, "#ff5f57"),
        ("Orange", TagColor::Orange, "#ff9f0a"),
        ("Yellow", TagColor::Yellow, "#ffd60a"),
        ("Green", TagColor::Green, "#30d158"),
        ("Blue", TagColor::Blue, "#0a84ff"),
        ("Purple", TagColor::Purple, "#bf5af2"),
        ("Gray", TagColor::Gray, "#98989d"),
    ];

    for (index, &(exp_name, exp_color, exp_hex)) in expected.iter().enumerate() {
        assert_eq!(canonical[index].label, exp_name);
        assert_eq!(canonical[index].color, Some(exp_color));
        assert_eq!(canonical[index].hex_color, exp_hex);
        assert_eq!(resolve_tag_color_css(Some(exp_color)), exp_hex);
    }

    assert_eq!(MACOS_TAG_PALETTE.len(), 7);
    let sample_tag = TagItem {
        label: "Archive".to_string(),
        color: None,
        hex_color: "#98989d",
        file_count: 3,
    };
    let sb_tag = tag_to_sidebar_item(&sample_tag);
    assert_eq!(sb_tag.id, "tag:Archive");
    assert_eq!(sb_tag.display_name, "Archive");

    // Tag filter event variants
    let event = TagFilterEvent::Selected {
        tag_name: "Important".to_string(),
        color: Some(TagColor::Red),
    };
    assert_eq!(
        event,
        TagFilterEvent::Selected {
            tag_name: "Important".to_string(),
            color: Some(TagColor::Red),
        }
    );

    let clear_event = TagFilterEvent::Cleared;
    assert_eq!(clear_event, TagFilterEvent::Cleared);
}

#[test]
fn test_sidebar_section_and_item_constructors() {
    let place = SidebarItem::place(
        "home",
        "Home",
        "user-home-symbolic",
        PathBuf::from("/home/user"),
        false,
    );
    assert_eq!(place.id, "home");
    assert_eq!(place.display_name, "Home");
    assert_eq!(place.icon_name, "user-home-symbolic");
    assert!(!place.is_active);
    assert_eq!(
        place.kind,
        SidebarEntryKind::Place {
            path: PathBuf::from("/home/user"),
            is_removable_place: false,
        }
    );

    let volume = SidebarItem::volume(
        "/dev/sdc1",
        "Backup",
        "drive-removable-media-symbolic",
        PathBuf::from("/media/backup"),
        true,
    );
    assert_eq!(volume.id, "/dev/sdc1");
    assert_eq!(
        volume.kind,
        SidebarEntryKind::Volume {
            device_path: "/dev/sdc1".to_string(),
            mount_point: PathBuf::from("/media/backup"),
            is_ejectable: true,
            is_busy: false,
        }
    );

    let tag = SidebarItem::tag("Work", Some(TagColor::Blue), "tag-symbolic");
    assert_eq!(tag.id, "tag:Work");
    assert_eq!(
        tag.kind,
        SidebarEntryKind::Tag {
            color: Some(TagColor::Blue),
            label: "Work".to_string(),
        }
    );

    let section = SidebarSection::new(SidebarSectionKind::Favorites, "Favorites", vec![place]);
    assert_eq!(section.kind, SidebarSectionKind::Favorites);
    assert_eq!(section.title, "Favorites");
    assert!(!section.is_collapsed);
    assert_eq!(section.items.len(), 1);
}

#[test]
fn test_sidebar_config_persistence_and_collapse_toggles() {
    let mut config = SidebarConfig::default();
    assert_eq!(config.width, 210);
    assert!(config.is_visible);
    assert!(!config.is_section_collapsed("Favorites"));

    config.set_section_collapsed("Favorites", true);
    assert!(config.is_section_collapsed("Favorites"));
    assert!(config.is_section_collapsed("favorites"));

    config.set_section_collapsed("Locations", true);
    assert!(config.is_section_collapsed("Locations"));

    config.set_section_collapsed("Favorites", false);
    assert!(!config.is_section_collapsed("Favorites"));
    assert!(config.is_section_collapsed("Locations"));
}
