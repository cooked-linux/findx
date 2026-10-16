//! Integration tests for filesystem operations, POSIX extended attributes,
//! natural numeric collation, fast reflink cloning, trash, and enhanced stat.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]

use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use shared::fs::{
    clone_file, clone_file_with_progress, get_comment, get_origin_url, get_tags,
    is_same_filesystem, list_trash_items, lstat_path, move_to_trash, natural_cmp, parse_trash_info,
    remove_comment, remove_origin_url, remove_tags, restore_from_trash, set_comment,
    set_origin_url, set_tags, stat_path, FileTag, TagColor,
};

#[test]
fn test_integration_xattr_lifecycle() {
    let dir = tempdir().expect("create tempdir");
    let file_path = dir.path().join("document.pdf");
    File::create(&file_path).expect("create file");

    // 1. Initial state: no xattrs
    assert!(get_tags(&file_path).expect("get_tags").is_empty());
    assert_eq!(get_comment(&file_path).expect("get_comment"), None);
    assert_eq!(get_origin_url(&file_path).expect("get_origin_url"), None);

    // 2. Set tags with colors
    let tags = vec![
        FileTag::new("Project Alpha", Some(TagColor::Blue)),
        FileTag::new("Urgent", Some(TagColor::Red)),
        FileTag::new("Uncolored", None),
    ];
    set_tags(&file_path, &tags).expect("set_tags");

    let retrieved_tags = get_tags(&file_path).expect("get_tags");
    assert_eq!(retrieved_tags.len(), 3);
    assert_eq!(retrieved_tags[0].name, "Project Alpha");
    assert_eq!(retrieved_tags[0].color, Some(TagColor::Blue));
    assert_eq!(retrieved_tags[1].name, "Urgent");
    assert_eq!(retrieved_tags[2].name, "Uncolored");

    // 3. User comment lifecycle
    let comment = "Quarterly financial summary for executive review.";
    set_comment(&file_path, comment).expect("set_comment");
    assert_eq!(
        get_comment(&file_path).expect("get_comment").as_deref(),
        Some(comment)
    );

    remove_comment(&file_path).expect("remove_comment");
    assert_eq!(get_comment(&file_path).expect("get_comment"), None);

    // 4. Origin download URL lifecycle
    let url = "https://reports.company.com/2026/q3_summary.pdf";
    set_origin_url(&file_path, url).expect("set_origin_url");
    assert_eq!(
        get_origin_url(&file_path).expect("get_origin_url").as_deref(),
        Some(url)
    );

    remove_origin_url(&file_path).expect("remove_origin_url");
    assert_eq!(get_origin_url(&file_path).expect("get_origin_url"), None);

    // 5. Clearing tags
    remove_tags(&file_path).expect("remove_tags");
    assert!(get_tags(&file_path).expect("get_tags").is_empty());
}

#[test]
fn test_integration_natural_sort() {
    let mut filenames = vec![
        "file10.txt",
        "file2.txt",
        "file1.txt",
        "file20.txt",
        "File3.txt",
        "file02.txt",
        "archive.tar.gz",
        "archive.1.tar.gz",
    ];

    filenames.sort_by(|a, b| natural_cmp(a, b));

    assert_eq!(
        filenames,
        vec![
            "archive.1.tar.gz",
            "archive.tar.gz",
            "file1.txt",
            "file2.txt",
            "file02.txt",
            "File3.txt",
            "file10.txt",
            "file20.txt",
        ]
    );
}

#[test]
fn test_integration_clone_hash_identity() {
    let dir = tempdir().expect("create tempdir");
    let src_path = dir.path().join("source_data.bin");
    let dst_path = dir.path().join("cloned_data.bin");

    // Generate 256 KB deterministic test payload
    let mut payload = Vec::with_capacity(256 * 1024);
    for i in 0..(256 * 1024) {
        payload.push(((i * 31 + 17) % 251) as u8);
    }

    {
        let mut file = File::create(&src_path).expect("create src");
        file.write_all(&payload).expect("write src");
        file.flush().expect("flush src");
    }

    // Attach xattrs to source
    set_comment(&src_path, "Cloned file metadata").expect("set comment");

    // Perform clone with progress monitoring
    let mut progress_snapshots = Vec::new();
    let copied = clone_file_with_progress(
        &src_path,
        &dst_path,
        |curr, total| progress_snapshots.push((curr, total)),
        || false,
    )
    .expect("clone file");

    assert_eq!(copied, payload.len() as u64);
    assert!(!progress_snapshots.is_empty());
    assert_eq!(
        progress_snapshots.last().copied(),
        Some((payload.len() as u64, payload.len() as u64))
    );

    // Verify exact byte equality
    let dst_bytes = std::fs::read(&dst_path).expect("read dst");
    assert_eq!(dst_bytes, payload);

    // Verify metadata / comment preserved
    assert_eq!(
        get_comment(&dst_path).expect("get cloned comment").as_deref(),
        Some("Cloned file metadata")
    );

    // Also verify clone_file direct convenience function
    let dst_path2 = dir.path().join("cloned_data_direct.bin");
    let direct_copied = clone_file(&src_path, &dst_path2).expect("direct clone");
    assert_eq!(direct_copied, payload.len() as u64);
    assert_eq!(std::fs::read(&dst_path2).expect("read direct clone"), payload);
}

#[test]
fn test_integration_trash_and_trashinfo_parsing() {
    let dir = tempdir().expect("create tempdir");
    let file_path = dir.path().join("obsolete_note.txt");
    let content = b"This file is destined for the trash.";
    std::fs::write(&file_path, content).expect("write note");

    assert!(file_path.exists());

    // Move to Freedesktop trash
    move_to_trash(&file_path).expect("move_to_trash");
    assert!(!file_path.exists(), "file must no longer exist at original path");

    // Find in trash listing
    let trash_items = list_trash_items().expect("list_trash_items");
    let found = trash_items
        .iter()
        .find(|item| item.original_path == file_path);

    assert!(found.is_some(), "file must be listed in trash items");
    let trashed_item = found.expect("found item");

    // Verify .trashinfo file exists and parses properly
    let info_path = std::path::Path::new(&trashed_item.id);
    if info_path.exists() {
        let info_content = std::fs::read_to_string(info_path).expect("read trashinfo");
        let parsed = parse_trash_info(&info_content).expect("parse_trash_info");
        assert_eq!(parsed.original_path, file_path);
        assert!(parsed.deletion_date.is_some());
    }

    // Restore from trash
    restore_from_trash(trashed_item).expect("restore_from_trash");
    assert!(file_path.exists(), "file must be restored to original path");
    assert_eq!(
        std::fs::read(&file_path).expect("read restored"),
        content
    );
}

#[test]
fn test_integration_statx_and_boundary_detection() {
    let dir = tempdir().expect("create tempdir");
    let file_a = dir.path().join("node_a.txt");
    let file_b = dir.path().join("node_b.txt");

    std::fs::write(&file_a, b"Alpha").expect("write a");
    std::fs::write(&file_b, b"Beta").expect("write b");

    let stat_a = stat_path(&file_a).expect("stat file_a");
    let stat_b = stat_path(&file_b).expect("stat file_b");

    assert!(stat_a.is_file());
    assert!(stat_b.is_file());
    assert_eq!(stat_a.size, 5);
    assert_eq!(stat_b.size, 4);

    // Both files reside in the same temporary directory -> same device
    assert!(is_same_filesystem(&file_a, &file_b).expect("is_same_filesystem"));

    // Inode hardlink cycle / deduplication test
    let hardlink = dir.path().join("hardlink_to_a.txt");
    std::fs::hard_link(&file_a, &hardlink).expect("hardlink");

    let stat_link = stat_path(&hardlink).expect("stat link");
    assert_eq!(stat_a.inode_key(), stat_link.inode_key());
    assert_eq!(stat_link.nlink, 2);

    // Symlink vs regular stat
    let symlink = dir.path().join("symlink_to_b.txt");
    std::os::unix::fs::symlink(&file_b, &symlink).expect("symlink");

    let lstat_sym = lstat_path(&symlink).expect("lstat symlink");
    assert!(lstat_sym.is_symlink());
    assert!(!lstat_sym.is_file());

    let stat_sym = stat_path(&symlink).expect("stat symlink");
    assert!(stat_sym.is_file());
    assert!(!stat_sym.is_symlink());
}
