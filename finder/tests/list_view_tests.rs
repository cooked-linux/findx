#![allow(clippy::unwrap_used, clippy::similar_names)]
//! Unit and integration tests for Hierarchical List View.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use finder::views::list::{TreeListViewModel, TreeNode};
use shared::models::{FileItem, FileMetadata};

fn create_dummy_item(path: impl AsRef<Path>, is_dir: bool, size: u64, inode: u64) -> FileItem {
    let p = path.as_ref().to_path_buf();
    let name = p
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().to_string());
    FileItem::new(
        p,
        name,
        is_dir,
        false,
        None,
        FileMetadata {
            size_bytes: size,
            modified: Some(SystemTime::now()),
            accessed: None,
            created: None,
            permissions: if is_dir { 0o755 } else { 0o644 },
            inode,
            device_id: 1,
            mime_type: if is_dir {
                "inode/directory".to_string()
            } else {
                "text/plain".to_string()
            },
            is_hidden: false,
            is_executable: is_dir,
        },
        vec![],
    )
}

#[test]
fn test_node_flattening_and_index_mapping() {
    let root_path = PathBuf::from("/workspace");
    let mut model = TreeListViewModel::new_empty(root_path);

    // Initial root items: folder_a, file_b, folder_c
    let mut folder_a = TreeNode::new(create_dummy_item("/workspace/folder_a", true, 4096, 101), 0);
    let file_b = TreeNode::new(
        create_dummy_item("/workspace/file_b.txt", false, 120, 102),
        0,
    );
    let folder_c = TreeNode::new(create_dummy_item("/workspace/folder_c", true, 4096, 103), 0);

    // Child items inside folder_a: child_a1 (dir), child_a2 (file)
    let child_a1 = TreeNode::new(
        create_dummy_item("/workspace/folder_a/sub", true, 4096, 201),
        1,
    );
    let child_a2 = TreeNode::new(
        create_dummy_item("/workspace/folder_a/sub_file.txt", false, 50, 202),
        1,
    );
    folder_a.children = vec![child_a1, child_a2];

    model.set_root_children(vec![folder_a, file_b, folder_c]);

    // When all collapsed: visible rows = 3 (folder_a, file_b, folder_c)
    assert_eq!(model.row_count(), 3);
    assert_eq!(
        model.row_at(0).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/folder_a"))
    );
    assert_eq!(
        model.row_at(1).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/file_b.txt"))
    );
    assert_eq!(
        model.row_at(2).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/folder_c"))
    );
    assert_eq!(
        model.index_of_path(Path::new("/workspace/folder_a")),
        Some(0)
    );
    assert_eq!(
        model.index_of_path(Path::new("/workspace/file_b.txt")),
        Some(1)
    );
    assert_eq!(
        model.index_of_path(Path::new("/workspace/folder_c")),
        Some(2)
    );

    // Expand folder_a
    assert!(model.expand_node(Path::new("/workspace/folder_a")));
    assert_eq!(model.row_count(), 5);
    assert_eq!(
        model.row_at(0).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/folder_a"))
    );
    assert_eq!(
        model.row_at(1).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/folder_a/sub"))
    );
    assert_eq!(
        model.row_at(2).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/folder_a/sub_file.txt"))
    );
    assert_eq!(
        model.row_at(3).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/file_b.txt"))
    );
    assert_eq!(
        model.row_at(4).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/folder_c"))
    );

    // Verify depth
    assert_eq!(model.row_at(0).unwrap().depth, 0);
    assert_eq!(model.row_at(1).unwrap().depth, 1);
    assert_eq!(model.row_at(2).unwrap().depth, 1);
    assert_eq!(model.row_at(3).unwrap().depth, 0);

    // Collapse folder_a
    assert!(model.collapse_node(Path::new("/workspace/folder_a")));
    assert_eq!(model.row_count(), 3);
    assert_eq!(
        model.row_at(1).map(|n| n.path.as_path()),
        Some(Path::new("/workspace/file_b.txt"))
    );
}

#[test]
fn test_recursive_expansion_depth_ceiling() {
    use finder::sorting::comparator::SortCriteria;
    use finder::views::list::{MAX_RECURSION_DEPTH, read_subtree_recursive_sync};

    fn max_depth(nodes: &[TreeNode]) -> u32 {
        let mut max = 0;
        for node in nodes {
            max = max.max(node.depth);
            if !node.children.is_empty() {
                max = max.max(max_depth(&node.children));
            }
        }
        max
    }

    let test_id = format!(
        "findx_test_depth_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(test_id);
    std::fs::create_dir_all(&root).unwrap();

    // Create a directory tree 25 levels deep
    let mut curr = root.clone();
    for i in 0..25 {
        curr = curr.join(format!("level_{i}"));
        std::fs::create_dir(&curr).unwrap();
        // Also put a file inside each
        std::fs::write(curr.join("file.txt"), b"test").unwrap();
    }

    let mut seen_inodes = HashSet::new();
    let criteria = SortCriteria::default();
    let result = read_subtree_recursive_sync(&root, 0, &criteria, &mut seen_inodes).unwrap();

    let deepest = max_depth(&result);
    // Depth must strictly not exceed MAX_RECURSION_DEPTH (20)
    assert!(
        deepest <= MAX_RECURSION_DEPTH,
        "Deepest depth {deepest} exceeded limit {MAX_RECURSION_DEPTH}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_symlink_loop_cycle_detection() {
    use finder::sorting::comparator::SortCriteria;
    use finder::views::list::read_subtree_recursive_sync;

    let test_id = format!(
        "findx_test_loop_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(test_id);
    std::fs::create_dir_all(&root).unwrap();

    // Create dir_a
    let dir_a = root.join("dir_a");
    std::fs::create_dir(&dir_a).unwrap();
    std::fs::write(dir_a.join("file_in_a.txt"), b"hello").unwrap();

    // Create dir_b inside dir_a
    let dir_b = dir_a.join("dir_b");
    std::fs::create_dir(&dir_b).unwrap();

    // Create symlink inside dir_b pointing back to dir_a (creating a loop!)
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&dir_a, dir_b.join("loop_to_a")).unwrap();
    }

    let mut seen_inodes = HashSet::new();
    let criteria = SortCriteria::default();
    // This must terminate safely without infinite recursion or stack overflow
    let result = read_subtree_recursive_sync(&root, 0, &criteria, &mut seen_inodes);
    assert!(result.is_ok());
    let nodes = result.unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].file_name(), "dir_a");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn test_expansion_state_serialization() {
    use finder::views::list::ExpandedNodesCache;

    let mut cache = ExpandedNodesCache::new();
    let container = PathBuf::from("/home/user/workspace");
    let mut expanded = HashSet::new();
    expanded.insert(PathBuf::from("/home/user/workspace/src"));
    expanded.insert(PathBuf::from("/home/user/workspace/src/ui"));

    cache.set_expanded(&container, expanded);

    // Serialize to JSON string
    let json = serde_json::to_string(&cache).unwrap();
    assert!(json.contains("/home/user/workspace/src"));
    assert!(json.contains("/home/user/workspace/src/ui"));

    // Deserialize back
    let restored: ExpandedNodesCache = serde_json::from_str(&json).unwrap();
    let set = restored.get_expanded(&container);
    assert_eq!(set.len(), 2);
    assert!(set.contains(&PathBuf::from("/home/user/workspace/src")));
    assert!(set.contains(&PathBuf::from("/home/user/workspace/src/ui")));
}

#[tokio::test]
async fn test_lru_watch_pool_eviction() {
    use finder::views::list::LruWatchPool;
    use tokio::sync::mpsc;

    let (tx, _rx) = mpsc::channel(16);
    let mut pool = LruWatchPool::new(tx, 3);

    let p1 = PathBuf::from("/tmp/p1");
    let p2 = PathBuf::from("/tmp/p2");
    let p3 = PathBuf::from("/tmp/p3");
    let p4 = PathBuf::from("/tmp/p4");

    pool.add_watch(p1.clone());
    pool.add_watch(p2.clone());
    pool.add_watch(p3);
    assert_eq!(pool.active_count(), 3);
    assert!(pool.is_watching(&p1));

    // Adding 4th watch should evict the oldest (p1)
    pool.add_watch(p4.clone());
    assert_eq!(pool.active_count(), 3);
    assert!(!pool.is_watching(&p1));
    assert!(pool.is_watching(&p4));

    // Evict subtree under /tmp/p2
    pool.evict_subtree(&p2);
    assert_eq!(pool.active_count(), 2);
    assert!(!pool.is_watching(&p2));
}
