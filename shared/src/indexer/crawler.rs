//! Headless asynchronous filesystem crawler with boundary safeguards.
//!
//! Crawls directory trees while enforcing blacklists, detecting cyclical
//! symlinks and duplicate hardlinks via `(dev, ino)` tracking, and maintaining
//! filesystem mount boundaries.

use std::collections::HashSet;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use walkdir::WalkDir;

use crate::fs::InodeKey;
use crate::indexer::IndexerError;

/// Default blacklisted directory and pseudo-filesystem names.
pub const DEFAULT_BLACKLISTED_DIR_NAMES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".cache",
    ".cargo",
    ".rustup",
    "/proc",
    "/sys",
    "/dev",
    "/run",
];

/// Configuration parameters for filesystem directory crawling.
#[derive(Debug, Clone)]
pub struct CrawlerConfig {
    /// Root directory paths to crawl.
    pub roots: Vec<PathBuf>,
    /// Set of directory names or absolute path prefixes to strictly ignore.
    pub blacklisted_dir_names: HashSet<String>,
    /// Maximum directory recursion depth.
    pub max_depth: usize,
    /// When true, traversal stays strictly within the root's filesystem device.
    pub stay_on_filesystem: bool,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        let mut blacklisted = HashSet::with_capacity(DEFAULT_BLACKLISTED_DIR_NAMES.len());
        for name in DEFAULT_BLACKLISTED_DIR_NAMES {
            blacklisted.insert((*name).to_string());
        }

        Self {
            roots: Vec::new(),
            blacklisted_dir_names: blacklisted,
            max_depth: 64,
            stay_on_filesystem: true,
        }
    }
}

impl CrawlerConfig {
    /// Creates a new crawler configuration for the given root paths.
    #[must_use]
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            ..Default::default()
        }
    }

    /// Sets the maximum directory traversal depth.
    #[must_use]
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    /// Controls whether crawler stays within the origin filesystem mount.
    #[must_use]
    pub const fn with_stay_on_filesystem(mut self, stay: bool) -> Self {
        self.stay_on_filesystem = stay;
        self
    }
}

/// Discovered candidate file descriptor ready for content extraction and indexing.
#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    /// Canonical or absolute path to the file.
    pub path: PathBuf,
    /// Standard filesystem metadata.
    pub metadata: Metadata,
}

impl DiscoveredFile {
    /// Returns the file size in bytes.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.metadata.len()
    }

    /// Returns the file modification time in epoch seconds, or 0 if unavailable.
    #[must_use]
    pub fn modified_time(&self) -> i64 {
        self.metadata.mtime()
    }
}

/// Headless filesystem crawler tracking visited inodes to prevent loops.
#[derive(Debug, Clone)]
pub struct DirectoryCrawler {
    config: CrawlerConfig,
    visited_inodes: Arc<Mutex<HashSet<InodeKey>>>,
}

impl DirectoryCrawler {
    /// Creates a new [`DirectoryCrawler`] instance.
    #[must_use]
    pub fn new(config: CrawlerConfig) -> Self {
        Self {
            config,
            visited_inodes: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Returns a reference to the active [`CrawlerConfig`].
    #[must_use]
    pub const fn config(&self) -> &CrawlerConfig {
        &self.config
    }

    /// Returns a clone of currently tracked visited inode keys.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn visited_inodes(&self) -> HashSet<InodeKey> {
        self.visited_inodes
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Resets the visited inode cache.
    pub fn reset_visited(&self) {
        if let Ok(mut guard) = self.visited_inodes.lock() {
            guard.clear();
        }
    }

    /// Executes the directory crawl across all configured roots, returning an iterator of files.
    #[must_use]
    pub fn crawl(&self) -> CrawlIterator {
        CrawlIterator::new(self.config.clone(), Arc::clone(&self.visited_inodes))
    }

    /// Convenience helper executing full crawl into a collected vector.
    ///
    /// # Errors
    ///
    /// Returns [`IndexerError`] if an unrecoverable filesystem walk failure occurs.
    pub fn crawl_all(&self) -> Result<Vec<DiscoveredFile>, IndexerError> {
        let mut files = Vec::new();
        for item in self.crawl() {
            files.push(item?);
        }
        Ok(files)
    }
}

/// Checks whether a given path or directory name matches blacklisted names or paths.
fn is_blacklisted(path: &Path, blacklisted: &HashSet<String>) -> bool {
    if let Some(file_name) = path.file_name().and_then(|s| s.to_str())
        && blacklisted.contains(file_name)
    {
        return true;
    }

    for item in blacklisted {
        if item.starts_with('/') {
            let item_path = Path::new(item);
            if path == item_path || path.starts_with(item_path) {
                return true;
            }
        }
    }

    false
}

/// Iterator streaming discovered files across all configured directory roots.
pub struct CrawlIterator {
    config: CrawlerConfig,
    visited_inodes: Arc<Mutex<HashSet<InodeKey>>>,
    roots_remaining: Vec<PathBuf>,
    current_iter: Option<Box<dyn Iterator<Item = Result<walkdir::DirEntry, walkdir::Error>>>>,
}

impl CrawlIterator {
    fn new(config: CrawlerConfig, visited_inodes: Arc<Mutex<HashSet<InodeKey>>>) -> Self {
        let mut roots_remaining = config.roots.clone();
        roots_remaining.reverse(); // Pop from back to process in order

        let mut it = Self {
            config,
            visited_inodes,
            roots_remaining,
            current_iter: None,
        };
        it.advance_root();
        it
    }

    fn advance_root(&mut self) {
        self.current_iter = None;

        while let Some(root) = self.roots_remaining.pop() {
            if is_blacklisted(&root, &self.config.blacklisted_dir_names) {
                continue;
            }

            let Ok(root_meta) = std::fs::metadata(&root) else {
                continue;
            };

            let root_dev = root_meta.dev();
            let visited = Arc::clone(&self.visited_inodes);
            let blacklisted = self.config.blacklisted_dir_names.clone();
            let stay_on_fs = self.config.stay_on_filesystem;

            let walk = WalkDir::new(&root)
                .max_depth(self.config.max_depth)
                .follow_links(true)
                .into_iter()
                .filter_entry(move |entry| {
                    let path = entry.path();
                    if is_blacklisted(path, &blacklisted) {
                        return false;
                    }

                    let Ok(meta) = entry.metadata() else {
                        return false;
                    };

                    if stay_on_fs && meta.dev() != root_dev {
                        return false;
                    }

                    let key = InodeKey::from_metadata(&meta);
                    let Ok(mut guard) = visited.lock() else {
                        return false;
                    };

                    if guard.contains(&key) {
                        // Reject symlink cycles or duplicate hardlinks
                        return false;
                    }

                    guard.insert(key);
                    true
                });

            self.current_iter = Some(Box::new(walk));
            break;
        }
    }
}

impl Iterator for CrawlIterator {
    type Item = Result<DiscoveredFile, IndexerError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let iter = self.current_iter.as_mut()?;
            match iter.next() {
                Some(Ok(entry)) => {
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };

                    if meta.is_file() {
                        return Some(Ok(DiscoveredFile {
                            path: entry.into_path(),
                            metadata: meta,
                        }));
                    }
                    // Non-regular files (directories, fifos) continue iteration
                }
                Some(Err(err)) => {
                    // Skip symlink loop errors that walkdir itself might report
                    if err.loop_ancestor().is_some() {
                        continue;
                    }
                    return Some(Err(IndexerError::Walk(err.to_string())));
                }
                None => {
                    self.advance_root();
                    self.current_iter.as_ref()?;
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_crawler_skips_blacklisted_and_detects_cycles() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        // 1. Regular file
        let file1_path = root.join("valid.txt");
        let mut file1 = File::create(&file1_path).unwrap();
        writeln!(file1, "Valid content").unwrap();

        // 2. Blacklisted directory (.git)
        let git_dir = root.join(".git");
        std::fs::create_dir(&git_dir).unwrap();
        let git_file = git_dir.join("HEAD");
        let mut gf = File::create(&git_file).unwrap();
        writeln!(gf, "ref: refs/heads/main").unwrap();

        // 3. Blacklisted directory (node_modules)
        let nm_dir = root.join("node_modules");
        std::fs::create_dir(&nm_dir).unwrap();
        let nm_file = nm_dir.join("package.json");
        let mut nf = File::create(&nm_file).unwrap();
        writeln!(nf, "{{}}").unwrap();

        // 4. Subdirectory with a file
        let sub_dir = root.join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        let sub_file = sub_dir.join("sub.rs");
        let mut sf = File::create(&sub_file).unwrap();
        writeln!(sf, "fn main() {{}}").unwrap();

        // 5. Cyclical symlink pointing back to root
        let cycle_link = sub_dir.join("root_link");
        let _ = std::os::unix::fs::symlink(root, &cycle_link);

        // Run crawler
        let config = CrawlerConfig::new(vec![root.to_path_buf()]);
        let crawler = DirectoryCrawler::new(config);
        let discovered = crawler.crawl_all().unwrap();

        let paths: Vec<PathBuf> = discovered.into_iter().map(|f| f.path).collect();

        // Should contain valid.txt and sub.rs
        assert!(paths.iter().any(|p| p.ends_with("valid.txt")));
        assert!(paths.iter().any(|p| p.ends_with("sub.rs")));

        // Should NOT contain .git or node_modules
        assert!(!paths.iter().any(|p| p.to_string_lossy().contains(".git")));
        assert!(
            !paths
                .iter()
                .any(|p| p.to_string_lossy().contains("node_modules"))
        );

        // Crawler terminated without hanging on cycle_link!
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_duplicate_hardlink_deduplication() {
        let temp_dir = tempfile::tempdir().unwrap();
        let root = temp_dir.path();

        let orig = root.join("original.txt");
        let mut f = File::create(&orig).unwrap();
        writeln!(f, "hardlink test").unwrap();

        let hardlink = root.join("hardlink.txt");
        std::fs::hard_link(&orig, &hardlink).unwrap();

        let config = CrawlerConfig::new(vec![root.to_path_buf()]);
        let crawler = DirectoryCrawler::new(config);
        let discovered = crawler.crawl_all().unwrap();

        // Only one of the hardlinks should be yielded
        assert_eq!(discovered.len(), 1);
    }
}
