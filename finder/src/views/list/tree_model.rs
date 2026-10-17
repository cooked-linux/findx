//! Hierarchical tree node model and flattened visible list model.
//!
//! Provides [`TreeNode`] representing filesystem nodes with parent-child relationships,
//! disclosure expansion state, loading indicators, and loop-detection inode keys.
//! Provides [`TreeListViewModel`] maintaining visible flattened rows for tabular list view rendering.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use shared::models::FileItem;

use crate::sorting::comparator::{SortCriteria, compare_file_items};

/// Represents an individual filesystem tree node in the hierarchical list view.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    /// Canonical or absolute path of the filesystem item.
    pub path: PathBuf,
    /// Detailed file metadata and attributes.
    pub item: FileItem,
    /// Indentation depth relative to root (top-level items have `depth = 0`).
    pub depth: u32,
    /// Indicates if the item is a directory.
    pub is_directory: bool,
    /// Indicates if the directory is currently expanded in the view.
    pub is_expanded: bool,
    /// Indicates if child entries are currently being loaded asynchronously.
    pub is_loading: bool,
    /// Child tree nodes if this is an expanded directory.
    pub children: Vec<Self>,
    /// Unique inode identifier `(device_id, inode)` used for cycle detection.
    pub inode_key: (u64, u64),
}

impl TreeNode {
    /// Constructs a new [`TreeNode`] from a [`FileItem`] at the specified depth.
    #[must_use]
    pub fn new(item: FileItem, depth: u32) -> Self {
        let path = item.path.clone();
        let is_directory = item.is_dir || item.metadata.mime_type == "inode/directory";
        let inode_key = (item.metadata.device_id, item.metadata.inode);

        Self {
            path,
            item,
            depth,
            is_directory,
            is_expanded: false,
            is_loading: false,
            children: Vec::new(),
            inode_key,
        }
    }

    /// Returns a borrowed reference to the node's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the file or directory name.
    #[must_use]
    pub fn file_name(&self) -> &str {
        self.item.file_name()
    }

    /// Recursively searches for a node with the specified path in self or descendant children.
    #[must_use]
    pub fn find_node(&self, target_path: &Path) -> Option<&Self> {
        if self.path == target_path {
            return Some(self);
        }
        for child in &self.children {
            if let Some(found) = child.find_node(target_path) {
                return Some(found);
            }
        }
        None
    }

    /// Recursively searches for a mutable node with the specified path.
    pub fn find_node_mut(&mut self, target_path: &Path) -> Option<&mut Self> {
        if self.path == target_path {
            return Some(self);
        }
        for child in &mut self.children {
            if let Some(found) = child.find_node_mut(target_path) {
                return Some(found);
            }
        }
        None
    }

    /// Recursively sorts this node's children and all descendant children according to `criteria`.
    pub fn sort_recursive(&mut self, criteria: &SortCriteria) {
        self.children
            .sort_by(|a, b| compare_file_items(&a.item, &b.item, criteria));
        for child in &mut self.children {
            if child.is_directory {
                child.sort_recursive(criteria);
            }
        }
    }

    /// Removes a child or descendant node matching `target_path`.
    pub fn remove_node(&mut self, target_path: &Path) -> bool {
        let initial_len = self.children.len();
        self.children.retain(|c| c.path != target_path);
        if self.children.len() < initial_len {
            return true;
        }
        for child in &mut self.children {
            if child.is_directory && child.remove_node(target_path) {
                return true;
            }
        }
        false
    }
}

/// View model maintaining hierarchical directory tree and flattened visible rows.
#[derive(Debug, Clone)]
pub struct TreeListViewModel {
    /// Root path of the directory being inspected.
    pub root_path: PathBuf,
    /// Root pseudo-node hosting the top-level directory entries.
    pub root_node: TreeNode,
    /// Flat list of currently visible nodes for virtualized table display.
    pub flattened_rows: Vec<TreeNode>,
    /// Set of canonical directory paths currently expanded in this view.
    pub expanded_path_set: HashSet<PathBuf>,
}

impl TreeListViewModel {
    /// Constructs a new [`TreeListViewModel`] for `root_path` using a dummy root node.
    #[must_use]
    pub fn new_empty(root_path: PathBuf) -> Self {
        let root_item = FileItem::new(
            root_path.clone(),
            root_path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().to_string()),
            true,
            false,
            None,
            shared::models::FileMetadata {
                size_bytes: 0,
                modified: None,
                accessed: None,
                created: None,
                permissions: 0o755,
                inode: 0,
                device_id: 0,
                mime_type: "inode/directory".to_string(),
                is_hidden: false,
                is_executable: true,
            },
            Vec::new(),
        );

        let root_node = TreeNode::new(root_item, 0);

        Self {
            root_path,
            root_node,
            flattened_rows: Vec::new(),
            expanded_path_set: HashSet::new(),
        }
    }

    /// Constructs a new [`TreeListViewModel`] with explicit root node.
    #[must_use]
    pub fn new(root_path: PathBuf, root_node: TreeNode) -> Self {
        Self {
            root_path,
            root_node,
            flattened_rows: Vec::new(),
            expanded_path_set: HashSet::new(),
        }
    }

    /// Sets the top-level child nodes under the root container and rebuilds flattened rows.
    pub fn set_root_children(&mut self, children: Vec<TreeNode>) {
        self.root_node.children = children;
        self.rebuild_flattened_rows();
    }

    /// Rebuilds `flattened_rows` by traversing visible children starting from `root_node`.
    pub fn rebuild_flattened_rows(&mut self) {
        fn traverse(node: &TreeNode, rows: &mut Vec<TreeNode>) {
            for child in &node.children {
                rows.push(child.clone());
                if child.is_directory && child.is_expanded {
                    traverse(child, rows);
                }
            }
        }

        let mut rows = Vec::new();
        traverse(&self.root_node, &mut rows);
        self.flattened_rows = rows;
    }

    /// Sets the loading state on a directory node.
    pub fn set_loading(&mut self, target_path: &Path, is_loading: bool) -> bool {
        if let Some(node) = self.root_node.find_node_mut(target_path) {
            node.is_loading = is_loading;
            self.rebuild_flattened_rows();
            true
        } else {
            false
        }
    }

    /// Expands the directory node matching `target_path`.
    pub fn expand_node(&mut self, target_path: &Path) -> bool {
        if let Some(node) = self.root_node.find_node_mut(target_path)
            && node.is_directory
        {
            node.is_expanded = true;
            self.expanded_path_set.insert(target_path.to_path_buf());
            self.rebuild_flattened_rows();
            return true;
        }
        false
    }

    /// Collapses the directory node matching `target_path`.
    pub fn collapse_node(&mut self, target_path: &Path) -> bool {
        if let Some(node) = self.root_node.find_node_mut(target_path)
            && node.is_directory
        {
            node.is_expanded = false;
            self.expanded_path_set.remove(target_path);
            self.rebuild_flattened_rows();
            return true;
        }
        false
    }

    /// Toggles the expansion state of `target_path`.
    pub fn toggle_expand(&mut self, target_path: &Path) -> bool {
        if let Some(node) = self.root_node.find_node_mut(target_path)
            && node.is_directory
        {
            node.is_expanded = !node.is_expanded;
            if node.is_expanded {
                self.expanded_path_set.insert(target_path.to_path_buf());
            } else {
                self.expanded_path_set.remove(target_path);
            }
            self.rebuild_flattened_rows();
            return true;
        }
        false
    }

    /// Sets the child nodes of a directory, marks it expanded and loaded, and rebuilds rows.
    pub fn set_node_children(&mut self, target_path: &Path, children: Vec<TreeNode>) -> bool {
        if let Some(node) = self.root_node.find_node_mut(target_path)
            && node.is_directory
        {
            node.children = children;
            node.is_loading = false;
            node.is_expanded = true;
            self.expanded_path_set.insert(target_path.to_path_buf());
            self.rebuild_flattened_rows();
            return true;
        }
        false
    }

    /// Removes an item matching `target_path` from the hierarchy.
    pub fn remove_node(&mut self, target_path: &Path) -> bool {
        self.expanded_path_set.remove(target_path);
        let removed = self.root_node.remove_node(target_path);
        if removed {
            self.rebuild_flattened_rows();
        }
        removed
    }

    /// Searches for a node with the specified path.
    #[must_use]
    pub fn find_node(&self, target_path: &Path) -> Option<&TreeNode> {
        self.root_node.find_node(target_path)
    }

    /// Searches for a mutable node with the specified path.
    pub fn find_node_mut(&mut self, target_path: &Path) -> Option<&mut TreeNode> {
        self.root_node.find_node_mut(target_path)
    }

    /// Returns the total count of visible rows.
    #[must_use]
    pub const fn row_count(&self) -> usize {
        self.flattened_rows.len()
    }

    /// Returns a borrowed reference to the node at visible `index`.
    #[must_use]
    pub fn row_at(&self, index: usize) -> Option<&TreeNode> {
        self.flattened_rows.get(index)
    }

    /// Returns the visible row index of `path`, if currently visible.
    #[must_use]
    pub fn index_of_path(&self, path: &Path) -> Option<usize> {
        self.flattened_rows.iter().position(|r| r.path == path)
    }

    /// Returns the parent directory path of `target_path` in the visible hierarchy.
    #[must_use]
    pub fn parent_path_of(&self, target_path: &Path) -> Option<PathBuf> {
        fn find_parent(node: &TreeNode, target: &Path) -> Option<PathBuf> {
            for child in &node.children {
                if child.path == target {
                    return Some(node.path.clone());
                }
                if let Some(p) = find_parent(child, target) {
                    return Some(p);
                }
            }
            None
        }

        find_parent(&self.root_node, target_path)
    }

    /// Returns the path of the first visible child if `target_path` is an expanded directory.
    #[must_use]
    pub fn first_child_of(&self, target_path: &Path) -> Option<PathBuf> {
        let node = self.find_node(target_path)?;
        if node.is_directory && node.is_expanded {
            node.children.first().map(|c| c.path.clone())
        } else {
            None
        }
    }

    /// Sorts all tree nodes recursively using `criteria` and updates flattened rows.
    pub fn sort(&mut self, criteria: &SortCriteria) {
        self.root_node.sort_recursive(criteria);
        self.rebuild_flattened_rows();
    }
}
