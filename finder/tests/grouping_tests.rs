//! Comprehensive integration and benchmark tests for dynamic grouping and sorting.

#![allow(clippy::too_many_lines, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use chrono::{Local, TimeZone};
use finder::grouping::config::{DirectoryGroupSettings, GroupSettingsStore};
use finder::grouping::date::{
    ID_EARLIER_THIS_YEAR, ID_PREV_7_DAYS, ID_PREV_30_DAYS, ID_PREVIOUS_YEARS, ID_TODAY,
    ID_YESTERDAY, classify_date,
};
use finder::grouping::kind::{
    ID_APPLICATIONS, ID_ARCHIVES, ID_AUDIO, ID_DEVELOPER_SOURCE, ID_FOLDERS, ID_IMAGES, ID_MOVIES,
    ID_OTHER_DOCUMENTS, ID_PDF, ID_PLAIN_TEXT, classify_kind,
};
use finder::grouping::models::{GroupDimension, SectionGroup};
use finder::grouping::pipeline::group_and_sort;
use finder::grouping::size::{
    BYTES_1_GB, BYTES_1_MB, ID_1_TO_10_GB, ID_1_TO_100_MB, ID_100_MB_TO_1_GB, ID_OVER_10_GB,
    ID_UNDER_1_MB, ID_ZERO_BYTES, classify_size,
};
use finder::sorting::comparator::{
    SortColumn, SortCriteria, SortDirection, compare_file_items, sort_file_items,
};
use shared::models::{FileItem, FileMetadata, FileTag, TagColor};

fn create_simulated_item(
    path_str: &str,
    is_dir: bool,
    size: u64,
    modified: Option<SystemTime>,
    mime: &str,
    is_exec: bool,
    tags: Vec<&str>,
) -> FileItem {
    let path = PathBuf::from(path_str);
    let file_name = path
        .file_name()
        .map_or_else(|| path_str.to_string(), |n| n.to_string_lossy().to_string());

    let meta = FileMetadata {
        size_bytes: size,
        modified,
        accessed: None,
        created: None,
        permissions: if is_dir || is_exec { 0o755 } else { 0o644 },
        inode: 1,
        device_id: 1,
        mime_type: if is_dir {
            "inode/directory".to_string()
        } else {
            mime.to_string()
        },
        is_hidden: file_name.starts_with('.'),
        is_executable: is_exec,
    };

    let file_tags = tags
        .into_iter()
        .map(|t| FileTag {
            name: t.to_string(),
            color: Some(TagColor::Blue),
        })
        .collect();

    FileItem::new(path, file_name, is_dir, false, None, meta, file_tags)
}

#[test]
fn test_natural_sort_ordering() {
    let mut items = vec![
        create_simulated_item(
            "/tmp/File 20.txt",
            false,
            100,
            None,
            "text/plain",
            false,
            vec![],
        ),
        create_simulated_item(
            "/tmp/File 1.txt",
            false,
            100,
            None,
            "text/plain",
            false,
            vec![],
        ),
        create_simulated_item(
            "/tmp/File 10.txt",
            false,
            100,
            None,
            "text/plain",
            false,
            vec![],
        ),
        create_simulated_item(
            "/tmp/File 2.txt",
            false,
            100,
            None,
            "text/plain",
            false,
            vec![],
        ),
    ];

    let criteria = SortCriteria {
        primary: SortColumn::Name,
        direction: SortDirection::Ascending,
        folders_first: true,
    };

    sort_file_items(&mut items, &criteria);

    let names: Vec<&str> = items.iter().map(FileItem::file_name).collect();
    assert_eq!(
        names,
        vec!["File 1.txt", "File 2.txt", "File 10.txt", "File 20.txt"]
    );
}

#[test]
fn test_mime_kind_categorization() {
    let folder = create_simulated_item(
        "/home/user/Projects",
        true,
        4096,
        None,
        "inode/directory",
        true,
        vec![],
    );
    assert_eq!(classify_kind(&folder).id, ID_FOLDERS);

    let binary = create_simulated_item(
        "/usr/bin/findx",
        false,
        25_000_000,
        None,
        "application/x-executable",
        true,
        vec![],
    );
    assert_eq!(classify_kind(&binary).id, ID_APPLICATIONS);

    let desktop = create_simulated_item(
        "/usr/share/applications/editor.desktop",
        false,
        1024,
        None,
        "application/x-desktop",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&desktop).id, ID_APPLICATIONS);

    let img = create_simulated_item(
        "/tmp/wallpaper.png",
        false,
        2_000_000,
        None,
        "image/png",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&img).id, ID_IMAGES);

    let mov = create_simulated_item(
        "/tmp/screencast.mp4",
        false,
        50_000_000,
        None,
        "video/mp4",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&mov).id, ID_MOVIES);

    let audio = create_simulated_item(
        "/tmp/track.flac",
        false,
        30_000_000,
        None,
        "audio/flac",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&audio).id, ID_AUDIO);

    let pdf = create_simulated_item(
        "/tmp/report.pdf",
        false,
        1_500_000,
        None,
        "application/pdf",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&pdf).id, ID_PDF);

    let txt = create_simulated_item(
        "/tmp/notes.txt",
        false,
        500,
        None,
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&txt).id, ID_PLAIN_TEXT);

    let code = create_simulated_item(
        "/tmp/main.rs",
        false,
        4000,
        None,
        "text/x-rust",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&code).id, ID_DEVELOPER_SOURCE);

    let archive_tar_gz = create_simulated_item(
        "/tmp/backup.tar.gz",
        false,
        10_000_000,
        None,
        "application/gzip",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&archive_tar_gz).id, ID_ARCHIVES);

    let archive_zip = create_simulated_item(
        "/tmp/assets.zip",
        false,
        5_000_000,
        None,
        "application/zip",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&archive_zip).id, ID_ARCHIVES);

    let other = create_simulated_item(
        "/tmp/document.docx",
        false,
        80_000,
        None,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        false,
        vec![],
    );
    assert_eq!(classify_kind(&other).id, ID_OTHER_DOCUMENTS);
}

#[test]
fn test_date_binning_boundaries() {
    let now = Local
        .with_ymd_and_hms(2026, 10, 17, 12, 0, 0)
        .single()
        .expect("Valid local reference time");

    // 1. Today (2 hours ago)
    let t_today = Local
        .with_ymd_and_hms(2026, 10, 17, 10, 0, 0)
        .single()
        .expect("Valid time");
    let item_today = create_simulated_item(
        "/tmp/today.txt",
        false,
        100,
        Some(SystemTime::from(t_today)),
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(
        classify_date(&item_today, GroupDimension::DateModified, now).id,
        ID_TODAY
    );

    // 2. Yesterday
    let t_yesterday = Local
        .with_ymd_and_hms(2026, 10, 16, 15, 0, 0)
        .single()
        .expect("Valid time");
    let item_yesterday = create_simulated_item(
        "/tmp/yesterday.txt",
        false,
        100,
        Some(SystemTime::from(t_yesterday)),
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(
        classify_date(&item_yesterday, GroupDimension::DateModified, now).id,
        ID_YESTERDAY
    );

    // 3. Previous 7 Days (5 days ago)
    let t_5days = Local
        .with_ymd_and_hms(2026, 10, 12, 10, 0, 0)
        .single()
        .expect("Valid time");
    let item_5days = create_simulated_item(
        "/tmp/5days.txt",
        false,
        100,
        Some(SystemTime::from(t_5days)),
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(
        classify_date(&item_5days, GroupDimension::DateModified, now).id,
        ID_PREV_7_DAYS
    );

    // 4. Previous 30 Days (20 days ago)
    let t_20days = Local
        .with_ymd_and_hms(2026, 9, 27, 10, 0, 0)
        .single()
        .expect("Valid time");
    let item_20days = create_simulated_item(
        "/tmp/20days.txt",
        false,
        100,
        Some(SystemTime::from(t_20days)),
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(
        classify_date(&item_20days, GroupDimension::DateModified, now).id,
        ID_PREV_30_DAYS
    );

    // 5. Earlier this Year (5 months ago)
    let t_earlier = Local
        .with_ymd_and_hms(2026, 5, 10, 10, 0, 0)
        .single()
        .expect("Valid time");
    let item_earlier = create_simulated_item(
        "/tmp/earlier.txt",
        false,
        100,
        Some(SystemTime::from(t_earlier)),
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(
        classify_date(&item_earlier, GroupDimension::DateModified, now).id,
        ID_EARLIER_THIS_YEAR
    );

    // 6. Previous Years (2025)
    let t_prev_years = Local
        .with_ymd_and_hms(2025, 11, 20, 10, 0, 0)
        .single()
        .expect("Valid time");
    let item_prev_years = create_simulated_item(
        "/tmp/prev_years.txt",
        false,
        100,
        Some(SystemTime::from(t_prev_years)),
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(
        classify_date(&item_prev_years, GroupDimension::DateModified, now).id,
        ID_PREVIOUS_YEARS
    );
}

#[test]
fn test_size_binning_categories() {
    let folder = create_simulated_item(
        "/tmp/dir",
        true,
        50 * BYTES_1_GB,
        None,
        "inode/directory",
        true,
        vec![],
    );
    assert_eq!(classify_size(&folder).id, ID_FOLDERS);

    let zero = create_simulated_item("/tmp/zero", false, 0, None, "text/plain", false, vec![]);
    assert_eq!(classify_size(&zero).id, ID_ZERO_BYTES);

    let under_1mb = create_simulated_item(
        "/tmp/small",
        false,
        500 * 1024,
        None,
        "text/plain",
        false,
        vec![],
    );
    assert_eq!(classify_size(&under_1mb).id, ID_UNDER_1_MB);

    let mb_1_to_100 = create_simulated_item(
        "/tmp/med",
        false,
        45 * BYTES_1_MB,
        None,
        "image/png",
        false,
        vec![],
    );
    assert_eq!(classify_size(&mb_1_to_100).id, ID_1_TO_100_MB);

    let mb_100_to_1gb = create_simulated_item(
        "/tmp/large",
        false,
        600 * BYTES_1_MB,
        None,
        "video/mp4",
        false,
        vec![],
    );
    assert_eq!(classify_size(&mb_100_to_1gb).id, ID_100_MB_TO_1_GB);

    let gb_1_to_10 = create_simulated_item(
        "/tmp/huge",
        false,
        4 * BYTES_1_GB,
        None,
        "application/x-iso",
        false,
        vec![],
    );
    assert_eq!(classify_size(&gb_1_to_10).id, ID_1_TO_10_GB);

    let over_10gb = create_simulated_item(
        "/tmp/massive",
        false,
        25 * BYTES_1_GB,
        None,
        "application/x-raw-image",
        false,
        vec![],
    );
    assert_eq!(classify_size(&over_10gb).id, ID_OVER_10_GB);
}

#[test]
fn test_group_settings_persistence() {
    let mut store = GroupSettingsStore::default();
    let downloads_path = PathBuf::from("/home/user/Downloads");
    let settings = DirectoryGroupSettings {
        group_by: GroupDimension::DateModified,
        sort_by: SortColumn::Name,
        sort_direction: SortDirection::Ascending,
        collapsed_groups: vec!["Earlier".to_string()],
    };
    store.set_for_dir(downloads_path.clone(), settings.clone());

    let json = serde_json::to_string(&store).expect("Serialize settings");
    let loaded: GroupSettingsStore = serde_json::from_str(&json).expect("Deserialize settings");

    assert_eq!(loaded.get_for_dir(&downloads_path), settings);
    assert!(
        loaded
            .get_for_dir(&downloads_path)
            .is_group_collapsed("Earlier")
    );
    assert!(
        !loaded
            .get_for_dir(&downloads_path)
            .is_group_collapsed("Today")
    );
}

#[test]
fn test_benchmark_partitioning_10000_items() {
    // Generate 10,000 realistic items across various formats and dates
    let mut items = Vec::with_capacity(10_000);
    let sample_kinds = [
        ("image/png", false, "png"),
        ("video/mp4", false, "mp4"),
        ("text/x-rust", false, "rs"),
        ("application/pdf", false, "pdf"),
        ("application/zip", false, "zip"),
        ("inode/directory", true, ""),
    ];

    let base_time = SystemTime::now();

    for i in 0..10_000 {
        let (mime, is_dir, ext) = sample_kinds[i % sample_kinds.len()];
        let name = if is_dir {
            format!("folder_{i}")
        } else {
            format!("file_{i}.{ext}")
        };
        let size = ((i as u64) * 1024) % (5 * BYTES_1_GB);
        let mod_time = base_time - Duration::from_secs((i as u64) * 3600);

        items.push(create_simulated_item(
            &format!("/simulated/{name}"),
            is_dir,
            size,
            Some(mod_time),
            mime,
            is_dir,
            if i % 5 == 0 { vec!["Work"] } else { vec![] },
        ));
    }

    assert_eq!(items.len(), 10_000);

    let criteria = SortCriteria {
        primary: SortColumn::Name,
        direction: SortDirection::Ascending,
        folders_first: true,
    };
    let collapsed = HashSet::new();

    // Benchmark grouping by Kind
    let start = Instant::now();
    let sections: Vec<SectionGroup<FileItem>> =
        group_and_sort(&items, GroupDimension::Kind, criteria, &collapsed);
    let elapsed = start.elapsed();

    println!("Benchmarked 10,000 items partitioned by Kind in: {elapsed:?}");

    // Verify all 10,000 items are partitioned
    let total_partitioned: usize = sections.iter().map(SectionGroup::len).sum();
    assert_eq!(total_partitioned, 10_000);

    // Verify groups are non-empty and properly sorted
    for section in &sections {
        assert!(!section.is_empty());
        // Verify natural sort within group
        for pair in section.items.windows(2) {
            let ord = compare_file_items(&pair[0], &pair[1], &criteria);
            assert!(
                ord == std::cmp::Ordering::Less || ord == std::cmp::Ordering::Equal,
                "Items out of order in section {}: {:?} vs {:?}",
                section.key.title,
                pair[0].file_name(),
                pair[1].file_name()
            );
        }
    }

    // Performance target: verify <20ms execution in release / fast debug
    // In unoptimized debug test runs, allow reasonable execution, but verify it runs swiftly
    assert!(
        elapsed < Duration::from_millis(150),
        "Grouping 10,000 items took {elapsed:?}, exceeding 150ms ceiling in debug mode"
    );
}
