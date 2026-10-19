//! Crisp color badge rendering utilities and inode-based cache for Finder views.

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::SystemTime;

use gtk4::prelude::*;
use shared::models::{FileTag, TagColor};

use super::swatches::ColorSwatchBar;

#[derive(Debug, Clone)]
struct CachedTagRecord {
    tags: Vec<FileTag>,
    mtime: Option<SystemTime>,
}

/// In-memory cache for tag records indexed by `PathBuf` and `(dev, ino)`.
#[derive(Debug, Default)]
pub struct TagBadgeCache {
    by_path: Arc<RwLock<HashMap<PathBuf, CachedTagRecord>>>,
    by_inode: Arc<RwLock<HashMap<(u64, u64), CachedTagRecord>>>,
}

static GLOBAL_BADGE_CACHE: OnceLock<TagBadgeCache> = OnceLock::new();

impl TagBadgeCache {
    /// Returns a reference to the global [`TagBadgeCache`] singleton.
    #[must_use]
    pub fn global() -> &'static Self {
        GLOBAL_BADGE_CACHE.get_or_init(Self::new)
    }

    /// Constructs a new [`TagBadgeCache`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            by_path: Arc::new(RwLock::new(HashMap::new())),
            by_inode: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Retrieves cached tags for a path if mtime matches.
    #[must_use]
    pub fn get(&self, path: &Path, current_mtime: Option<SystemTime>) -> Option<Vec<FileTag>> {
        let guard = self.by_path.read().ok()?;
        let record = guard.get(path)?.clone();
        drop(guard);
        if current_mtime.is_some() && record.mtime != current_mtime {
            return None;
        }
        Some(record.tags)
    }

    /// Retrieves cached tags by inode `(dev, ino)` if mtime matches.
    #[must_use]
    pub fn get_by_inode(
        &self,
        dev: u64,
        ino: u64,
        current_mtime: Option<SystemTime>,
    ) -> Option<Vec<FileTag>> {
        let guard = self.by_inode.read().ok()?;
        let record = guard.get(&(dev, ino))?.clone();
        drop(guard);
        if current_mtime.is_some() && record.mtime != current_mtime {
            return None;
        }
        Some(record.tags)
    }

    /// Inserts tag records into the cache.
    pub fn insert(
        &self,
        path: PathBuf,
        dev_ino: Option<(u64, u64)>,
        mtime: Option<SystemTime>,
        tags: Vec<FileTag>,
    ) {
        let record = CachedTagRecord { tags, mtime };

        if let Ok(mut guard) = self.by_path.write() {
            guard.insert(path, record.clone());
        }

        if let (Some(dev_ino_pair), Ok(mut guard)) = (dev_ino, self.by_inode.write()) {
            guard.insert(dev_ino_pair, record);
        }
    }

    /// Invalidates cached entries for a given path.
    pub fn invalidate(&self, path: &Path) {
        if let Ok(mut guard) = self.by_path.write() {
            guard.remove(path);
        }

        if let Ok(meta) = std::fs::symlink_metadata(path) {
            let dev_ino = (meta.dev(), meta.ino());
            if let Ok(mut guard) = self.by_inode.write() {
                guard.remove(&dev_ino);
            }
        }
    }

    /// Clears the entire badge cache.
    pub fn invalidate_all(&self) {
        if let Ok(mut guard) = self.by_path.write() {
            guard.clear();
        }
        if let Ok(mut guard) = self.by_inode.write() {
            guard.clear();
        }
    }

    /// Resolves tags for `path` using the cache or reading extended attributes if missing.
    #[must_use]
    pub fn get_or_load(&self, path: &Path) -> Vec<FileTag> {
        let meta = std::fs::symlink_metadata(path).ok();
        let mtime = meta.as_ref().and_then(|m| m.modified().ok());
        let dev_ino = meta.as_ref().map(|m| (m.dev(), m.ino()));

        if let Some(cached) = self.get(path, mtime) {
            return cached;
        }

        if let Some((dev, ino)) = dev_ino
            && let Some(cached) = self.get_by_inode(dev, ino, mtime)
        {
            return cached;
        }

        let tags = shared::fs::get_tags(path).unwrap_or_default();
        self.insert(path.to_path_buf(), dev_ino, mtime, tags.clone());
        tags
    }
}

/// Global hook to invalidate the tag badge cache when extended attributes are modified.
pub fn invalidate_badge_cache(path: &Path) {
    TagBadgeCache::global().invalidate(path);
}

/// Renders a horizontal cluster of up to 3 colored circles for Column View.
/// If more than 3 tags, renders first 2 dots plus a `+N` indicator.
#[must_use]
pub fn render_column_badge_cluster(tags: &[FileTag]) -> gtk4::Box {
    let container = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(2)
        .css_classes(["tag-badge-cluster"])
        .valign(gtk4::Align::Center)
        .build();

    if tags.is_empty() {
        return container;
    }

    if tags.len() <= 3 {
        for tag in tags {
            let dot = create_badge_dot(tag.color);
            container.append(&dot);
        }
    } else {
        // First 2 dots
        for tag in tags.iter().take(2) {
            let dot = create_badge_dot(tag.color);
            container.append(&dot);
        }

        // +N indicator
        let remaining = tags.len().saturating_sub(2);
        let count_label = gtk4::Label::builder()
            .label(format!("+{remaining}"))
            .css_classes(["tag-badge-count"])
            .valign(gtk4::Align::Center)
            .build();
        container.append(&count_label);
    }

    container
}

/// Renders compact colored capsule pills for the List View "Tags" column.
/// For items with no tags, returns an empty box.
#[must_use]
pub fn render_list_view_tags_cell(tags: &[FileTag]) -> gtk4::Box {
    let container = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(4)
        .css_classes(["dc", "dc-tags"])
        .halign(gtk4::Align::Start)
        .valign(gtk4::Align::Center)
        .build();

    if tags.is_empty() {
        return container;
    }

    for tag in tags {
        let pill = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(3)
            .css_classes(["tag-pill"])
            .build();

        if let Some(color) = tag.color {
            let dot = create_badge_dot(Some(color));
            pill.append(&dot);
        }

        let lbl = gtk4::Label::builder()
            .label(&tag.name)
            .halign(gtk4::Align::Start)
            .build();
        pill.append(&lbl);

        container.append(&pill);
    }

    container
}

fn create_badge_dot(color: Option<TagColor>) -> gtk4::Box {
    let dot = gtk4::Box::builder().css_classes(["tag-badge-dot"]).build();
    let class_name = color.map_or("tag-color-gray", ColorSwatchBar::color_css_class);
    dot.add_css_class(class_name);
    dot
}

/// Managed badge view widget displaying either a Column View dot cluster or a List View cell.
#[derive(Debug, Clone)]
pub struct TagBadgeView {
    /// Root GTK container box.
    pub container: gtk4::Box,
    is_column_style: bool,
}

impl TagBadgeView {
    /// Constructs a Column View badge cluster.
    #[must_use]
    pub fn new_column_cluster(tags: &[FileTag]) -> Self {
        let container = render_column_badge_cluster(tags);
        Self {
            container,
            is_column_style: true,
        }
    }

    /// Constructs a List View tabular cell.
    #[must_use]
    pub fn new_list_cell(tags: &[FileTag]) -> Self {
        let container = render_list_view_tags_cell(tags);
        Self {
            container,
            is_column_style: false,
        }
    }

    /// Updates rendered badges with new tag data.
    pub fn update(&self, tags: &[FileTag]) {
        while let Some(child) = self.container.first_child() {
            self.container.remove(&child);
        }

        let new_box = if self.is_column_style {
            render_column_badge_cluster(tags)
        } else {
            render_list_view_tags_cell(tags)
        };

        while let Some(child) = new_box.first_child() {
            new_box.remove(&child);
            self.container.append(&child);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_cache_insertion_retrieval_and_invalidation() {
        let cache = TagBadgeCache::new();
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();
        let mtime = Some(SystemTime::now());

        let tags = vec![
            FileTag::new("Work", Some(TagColor::Red)),
            FileTag::new("Client", Some(TagColor::Blue)),
        ];

        cache.insert(path.clone(), None, mtime, tags.clone());

        let hit = cache.get(&path, mtime);
        assert_eq!(hit, Some(tags));

        // Different mtime -> miss
        let stale = cache.get(&path, Some(SystemTime::UNIX_EPOCH));
        assert_eq!(stale, None);

        // Invalidate
        cache.invalidate(&path);
        let miss = cache.get(&path, mtime);
        assert_eq!(miss, None);
    }
}
