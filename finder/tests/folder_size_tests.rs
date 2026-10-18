#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown,
    clippy::collapsible_if,
    clippy::uninlined_format_args,
    clippy::significant_drop_tightening
)]
//! Comprehensive integration tests for FindX asynchronous folder size calculation engine,
//! two-tier LRU+SQLite cache, physical block allocation, and inotify debouncing.

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use finder::operations::folder_size::{
    FolderSizeCache, FolderSizeManager, FolderSizeResult, FolderSizeWatcher, Priority,
    calculate_folder_size_sync,
};
use shared::indexer::FsEvent;
use tempfile::tempdir;

#[test]
fn test_folder_size_models_and_formatting() {
    let now = SystemTime::now();
    let res = FolderSizeResult {
        total_bytes: 1_524_687_667, // ~ 1.5 GB
        file_count: 3_410,
        folder_count: 2,
        dir_mtime: now,
        calculated_at: now,
    };

    assert_eq!(res.total_items(), 3_412);
    let size_str = res.formatted_size();
    assert_ne!(size_str, "");
    let summary_str = res.formatted_summary();
    assert!(summary_str.contains("3412 items") || summary_str.contains("3,412 items"));
}

#[test]
fn test_physical_block_allocation_and_hardlink_deduplication() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("scan_root");
    let sub = root.join("sub_level");
    fs::create_dir_all(&sub).expect("create dir");

    // 1. Regular file of 8KB
    let file1 = root.join("file1.bin");
    {
        let mut f = File::create(&file1).expect("create file1");
        f.write_all(&vec![0xAAu8; 8192]).expect("write file1");
        f.sync_all().expect("sync");
    }

    // 2. Hard link to file1 inside sub_level
    let hardlink1 = sub.join("hardlink1.bin");
    fs::hard_link(&file1, &hardlink1).expect("create hardlink");

    // 3. Second file of 4KB
    let file2 = sub.join("file2.bin");
    {
        let mut f = File::create(&file2).expect("create file2");
        f.write_all(&vec![0xBBu8; 4096]).expect("write file2");
        f.sync_all().expect("sync");
    }

    let scan_result = calculate_folder_size_sync(&root, None).expect("calculate scan");

    // Item counts:
    // Files: file1.bin, hardlink1.bin, file2.bin = 3
    // Folders: sub_level = 1
    assert_eq!(scan_result.file_count, 3);
    assert_eq!(scan_result.folder_count, 1);
    assert_eq!(scan_result.total_items(), 4);

    // Byte size:
    // Hardlink must NOT double count the physical blocks!
    let meta1 = fs::symlink_metadata(&file1).expect("meta1");
    let meta2 = fs::symlink_metadata(&file2).expect("meta2");
    let expected_bytes = (meta1.blocks() + meta2.blocks()) * 512;
    assert_eq!(scan_result.total_bytes, expected_bytes);
}

#[test]
fn test_two_tier_cache_lru_and_persistent_sqlite() {
    let tmp = tempdir().expect("tempdir");
    let db_path = tmp.path().join("sizes_test.sqlite");
    let target = PathBuf::from("/home/user/music");
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_750_000_000);

    let res = FolderSizeResult {
        total_bytes: 104_857_600,
        file_count: 25,
        folder_count: 3,
        dir_mtime: now,
        calculated_at: now,
    };

    // Session 1: Populate cache and persist
    {
        let cache = FolderSizeCache::with_db_path(Some(&db_path)).expect("open cache1");
        cache.insert(target.clone(), res);
        assert_eq!(cache.get(&target, now), Some(res));
        cache.flush();
    }

    // Session 2: Fresh instance (cold L1 LRU), verify promotion from L2 SQLite
    {
        let cache2 = FolderSizeCache::with_db_path(Some(&db_path)).expect("open cache2");
        // Cold get queries SQLite, promotes to L1
        let loaded = cache2.get(&target, now);
        assert_eq!(loaded, Some(res));

        // Subsequent get served directly from L1
        assert_eq!(cache2.get(&target, now), Some(res));

        // Stale query with modified timestamp invalidates
        let later = now + Duration::from_secs(10);
        assert_eq!(cache2.get(&target, later), None);

        // Verification that stale query evicted from both tiers
        assert_eq!(cache2.get(&target, now), None);
    }
}

#[tokio::test]
async fn test_watcher_inotify_cooldown_debouncer() {
    let cache = Arc::new(FolderSizeCache::new_in_memory().expect("in-memory cache"));
    let cooldown = Duration::from_millis(60);
    let watcher = FolderSizeWatcher::new(Arc::clone(&cache), cooldown);
    let rx = watcher.subscribe_recalculations();

    let monitored_dir = PathBuf::from("/home/user/active_project");
    let now = SystemTime::now();

    cache.insert(
        monitored_dir.clone(),
        FolderSizeResult {
            total_bytes: 2048,
            file_count: 2,
            folder_count: 0,
            dir_mtime: now,
            calculated_at: now,
        },
    );
    cache.flush();

    // Burst of 8 rapid sequential modifications inside monitored_dir
    for i in 0..8 {
        watcher
            .handle_event(FsEvent::Created(
                monitored_dir.join(format!("temp_{i}.tmp")),
            ))
            .expect("handle event");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Must be invalidated immediately
    assert_eq!(cache.get(&monitored_dir, now), None);

    // Cooldown window has not elapsed yet; no recalculation should have fired
    assert!(rx.try_recv().is_err());

    // Wait for cooldown to expire
    let debounced_dir = tokio::time::timeout(Duration::from_millis(250), rx.recv())
        .await
        .expect("timeout")
        .expect("channel active");

    assert_eq!(debounced_dir, monitored_dir);

    // Ensure only one recalculation was dispatched
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn test_manager_concurrent_requests_and_fast_path() {
    let tmp = tempdir().expect("tempdir");
    let cache = Arc::new(FolderSizeCache::new_in_memory().expect("cache"));
    let manager = FolderSizeManager::new(Arc::clone(&cache), 4);
    let rx = manager.results_receiver().clone();

    let mut dirs = Vec::new();
    for i in 0..6 {
        let dir = tmp.path().join(format!("managed_dir_{i}"));
        fs::create_dir_all(&dir).expect("create dir");
        let fpath = dir.join("payload.dat");
        let mut f = File::create(&fpath).expect("create file");
        f.write_all(&vec![0x77u8; (i + 1) * 2048])
            .expect("write payload");
        f.sync_all().expect("sync");
        dirs.push(dir);
    }

    // Initial requests return None (slow-path background computation)
    for dir in &dirs {
        assert_eq!(manager.request_folder_size(dir, Priority::High), None);
    }

    // Receive all results over results channel
    let mut received = Vec::new();
    for _ in 0..6 {
        let (path, res) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("recv timeout")
            .expect("channel active");

        assert_eq!(res.file_count, 1);
        assert!(res.total_bytes > 0);
        received.push(path);
    }

    assert_eq!(received.len(), 6);
    for dir in &dirs {
        assert!(received.contains(dir));
    }

    // Subsequent requests hit fast-path immediately
    for dir in &dirs {
        let fast_res = manager.request_folder_size(dir, Priority::High);
        assert!(fast_res.is_some());
    }
}

#[test]
fn test_unreadable_subdirectories_gracefully_skipped() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path().join("perm_root");
    let readable_sub = root.join("readable");
    let unreadable_sub = root.join("unreadable");

    fs::create_dir_all(&readable_sub).expect("create readable");
    fs::create_dir_all(&unreadable_sub).expect("create unreadable");

    // Write file into readable subdir
    let f1 = readable_sub.join("readable.txt");
    {
        let mut f = File::create(&f1).expect("create f1");
        f.write_all(b"readable data").expect("write f1");
        f.sync_all().expect("sync");
    }

    // Write file into unreadable subdir and remove permissions
    let f2 = unreadable_sub.join("secret.txt");
    {
        let mut f = File::create(&f2).expect("create f2");
        f.write_all(b"secret data").expect("write f2");
        f.sync_all().expect("sync");
    }

    // Set 0000 permissions on unreadable_sub
    let original_perms = fs::metadata(&unreadable_sub).expect("meta").permissions();
    let mut no_perms = original_perms.clone();
    no_perms.set_mode(0o000);
    let _ = fs::set_permissions(&unreadable_sub, no_perms);

    // Scanner must not panic or abort, but proceed recording readable parts
    let res = calculate_folder_size_sync(&root, None);

    // Restore permissions so tempdir cleanup succeeds
    let mut restore_perms = original_perms;
    restore_perms.set_mode(0o755);
    let _ = fs::set_permissions(&unreadable_sub, restore_perms);

    let result = res.expect("scan should succeed even with unreadable subdirectory");
    // At least the readable file should be counted
    assert!(result.file_count >= 1);
    assert!(result.total_bytes > 0);
}
