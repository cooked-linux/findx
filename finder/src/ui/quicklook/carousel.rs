//! Multi-file selection carousel and thumbnail mosaic index sheet popover.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gtk4::prelude::*;
use shared::thumbnail::{ThumbnailCache, ThumbnailSize};

/// Callback invoked when an item index is selected from the index sheet.
pub type IndexSheetCallback = Box<dyn Fn(usize)>;

/// Carousel state manager tracking multi-file selection and active viewing index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickLookCarousel {
    paths: Vec<PathBuf>,
    active_index: usize,
}

impl Default for QuickLookCarousel {
    fn default() -> Self {
        Self::new()
    }
}

impl QuickLookCarousel {
    /// Creates a new empty [`QuickLookCarousel`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            paths: Vec::new(),
            active_index: 0,
        }
    }

    /// Creates a new [`QuickLookCarousel`] initialized with files and active index.
    #[must_use]
    pub fn with_files(paths: Vec<PathBuf>, initial_index: usize) -> Self {
        let mut carousel = Self::new();
        carousel.set_files(paths, initial_index);
        carousel
    }

    /// Sets the list of selected files and initial viewing index.
    pub fn set_files(&mut self, paths: Vec<PathBuf>, initial_index: usize) {
        let len = paths.len();
        self.paths = paths;
        self.active_index = if len == 0 {
            0
        } else {
            initial_index.min(len - 1)
        };
    }

    /// Returns the currently active filesystem path, if any.
    #[must_use]
    pub fn current_path(&self) -> Option<&PathBuf> {
        self.paths.get(self.active_index)
    }

    /// Returns the active 0-indexed position.
    #[must_use]
    pub const fn current_index(&self) -> usize {
        self.active_index
    }

    /// Returns the total count of files in the carousel.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.paths.len()
    }

    /// Returns true if the carousel contains no files.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Returns all file paths managed by this carousel.
    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Advances to the next item, cycling back to the start if at the end.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&PathBuf> {
        if self.paths.is_empty() {
            return None;
        }

        if self.active_index + 1 < self.paths.len() {
            self.active_index += 1;
        } else {
            self.active_index = 0;
        }

        self.current_path()
    }

    /// Steps back to the previous item, cycling to the end if at the start.
    pub fn previous(&mut self) -> Option<&PathBuf> {
        if self.paths.is_empty() {
            return None;
        }

        if self.active_index > 0 {
            self.active_index -= 1;
        } else {
            self.active_index = self.paths.len() - 1;
        }

        self.current_path()
    }

    /// Jumps directly to the specified item index if valid.
    pub fn jump_to(&mut self, index: usize) -> Option<&PathBuf> {
        if index < self.paths.len() {
            self.active_index = index;
            self.current_path()
        } else {
            None
        }
    }
}

/// Floating popover displaying a scrollable 96x96 mosaic thumbnail grid of all selected items.
#[derive(Clone)]
pub struct IndexSheetPopover {
    /// Underlying GTK 4 popover.
    pub popover: gtk4::Popover,
    /// Scrolled container.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Mosaic flow box containing item tiles.
    pub flow_box: gtk4::FlowBox,
    /// Selection callback.
    on_item_selected: Rc<RefCell<Option<IndexSheetCallback>>>,
}

impl std::fmt::Debug for IndexSheetPopover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndexSheetPopover").finish_non_exhaustive()
    }
}

impl Default for IndexSheetPopover {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexSheetPopover {
    pub const THUMBNAIL_SIZE: i32 = 96;

    /// Constructs a new [`IndexSheetPopover`].
    #[must_use]
    pub fn new() -> Self {
        let flow_box = gtk4::FlowBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .max_children_per_line(4)
            .min_children_per_line(2)
            .row_spacing(12)
            .column_spacing(12)
            .homogeneous(true)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Start)
            .build();

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .max_content_width(460)
            .max_content_height(380)
            .propagate_natural_width(true)
            .propagate_natural_height(true)
            .child(&flow_box)
            .build();

        let popover = gtk4::Popover::builder()
            .position(gtk4::PositionType::Bottom)
            .css_classes(["quicklook-mosaic-grid"])
            .child(&scrolled_window)
            .build();

        let on_item_selected = Rc::new(RefCell::new(None));

        let sheet = Self {
            popover,
            scrolled_window,
            flow_box,
            on_item_selected,
        };

        sheet.wire_signals();
        sheet
    }

    /// Attaches selection callback invoked when user clicks a mosaic tile.
    pub fn connect_item_selected<F: Fn(usize) + 'static>(&self, callback: F) {
        *self.on_item_selected.borrow_mut() = Some(Box::new(callback));
    }

    /// Populates the mosaic grid with items and marks the active item selected.
    pub fn populate(
        &self,
        paths: &[PathBuf],
        active_index: usize,
        thumbnail_cache: Option<&Arc<ThumbnailCache>>,
    ) {
        // Clear previous children
        while let Some(child) = self.flow_box.first_child() {
            self.flow_box.remove(&child);
        }

        for (i, path) in paths.iter().enumerate() {
            let item_box = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Vertical)
                .spacing(6)
                .halign(gtk4::Align::Center)
                .valign(gtk4::Align::Center)
                .css_classes(["quicklook-mosaic-item"])
                .build();

            if i == active_index {
                item_box.add_css_class("selected");
            }

            // Thumbnail Picture / Image
            let picture = gtk4::Picture::builder()
                .can_shrink(true)
                .content_fit(gtk4::ContentFit::Contain)
                .width_request(Self::THUMBNAIL_SIZE)
                .height_request(Self::THUMBNAIL_SIZE)
                .halign(gtk4::Align::Center)
                .build();

            Self::load_thumbnail_for_tile(&picture, path, thumbnail_cache);

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            let label = gtk4::Label::builder()
                .label(file_name)
                .ellipsize(gtk4::pango::EllipsizeMode::Middle)
                .max_width_chars(12)
                .css_classes(["quicklook-counter"])
                .halign(gtk4::Align::Center)
                .build();

            item_box.append(&picture);
            item_box.append(&label);
            self.flow_box.append(&item_box);
        }

        if let Ok(idx) = i32::try_from(active_index)
            && let Some(child) = self.flow_box.child_at_index(idx)
        {
            self.flow_box.select_child(&child);
        }
    }

    /// Loads a cached or generated thumbnail, falling back to file icon.
    fn load_thumbnail_for_tile(
        picture: &gtk4::Picture,
        path: &Path,
        thumbnail_cache: Option<&Arc<ThumbnailCache>>,
    ) {
        if let Some(cache) = thumbnail_cache
            && let Ok(thumb_path) = cache.get_or_generate(path, ThumbnailSize::Normal, None)
            && thumb_path.is_file()
        {
            let file = gio::File::for_path(&thumb_path);
            picture.set_file(Some(&file));
            return;
        }

        // Direct image loading fast-path for image formats
        let is_image = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg"
                )
            });

        if is_image && path.is_file() {
            let file = gio::File::for_path(path);
            picture.set_file(Some(&file));
        }
    }

    /// Wires child activation signal to callback and dismisses popover.
    fn wire_signals(&self) {
        let pop_weak = self.popover.downgrade();
        let cb_rc = Rc::clone(&self.on_item_selected);

        self.flow_box.connect_child_activated(move |_, child| {
            let idx = child.index();
            if idx >= 0 {
                if let Some(pop) = pop_weak.upgrade() {
                    pop.popdown();
                }
                if let Some(ref cb) = *cb_rc.borrow()
                    && let Ok(u_idx) = usize::try_from(idx)
                {
                    cb(u_idx);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_carousel_navigation_and_boundaries() {
        let mut carousel = QuickLookCarousel::new();
        assert!(carousel.is_empty());
        assert_eq!(carousel.len(), 0);
        assert_eq!(carousel.current_path(), None);
        assert_eq!(carousel.next(), None);
        assert_eq!(carousel.previous(), None);
        assert_eq!(carousel.jump_to(0), None);

        let files = vec![
            PathBuf::from("/tmp/file1.png"),
            PathBuf::from("/tmp/file2.rs"),
            PathBuf::from("/tmp/file3.pdf"),
        ];

        carousel.set_files(files.clone(), 0);
        assert_eq!(carousel.len(), 3);
        assert_eq!(carousel.current_index(), 0);
        assert_eq!(carousel.current_path(), Some(&files[0]));

        // Forward cycling
        assert_eq!(carousel.next(), Some(&files[1]));
        assert_eq!(carousel.current_index(), 1);
        assert_eq!(carousel.next(), Some(&files[2]));
        assert_eq!(carousel.current_index(), 2);
        // Cycle wraps back to 0
        assert_eq!(carousel.next(), Some(&files[0]));
        assert_eq!(carousel.current_index(), 0);

        // Backward cycling
        assert_eq!(carousel.previous(), Some(&files[2]));
        assert_eq!(carousel.current_index(), 2);
        assert_eq!(carousel.previous(), Some(&files[1]));
        assert_eq!(carousel.current_index(), 1);
        assert_eq!(carousel.previous(), Some(&files[0]));
        assert_eq!(carousel.current_index(), 0);
        // Cycle wraps back to end
        assert_eq!(carousel.previous(), Some(&files[2]));
        assert_eq!(carousel.current_index(), 2);

        // Direct jumping
        assert_eq!(carousel.jump_to(1), Some(&files[1]));
        assert_eq!(carousel.current_index(), 1);
        assert_eq!(carousel.jump_to(5), None); // Out of bounds
        assert_eq!(carousel.current_index(), 1); // Unchanged

        // Clamping initial index
        carousel.set_files(files, 99);
        assert_eq!(carousel.current_index(), 2); // Clamped to len - 1
    }

    #[test]
    fn test_carousel_single_file() {
        let mut carousel = QuickLookCarousel::new();
        let files = vec![PathBuf::from("/tmp/single.txt")];
        carousel.set_files(files.clone(), 0);

        assert_eq!(carousel.len(), 1);
        assert_eq!(carousel.current_path(), Some(&files[0]));
        // Cycling with single item stays on same item
        assert_eq!(carousel.next(), Some(&files[0]));
        assert_eq!(carousel.current_index(), 0);
        assert_eq!(carousel.previous(), Some(&files[0]));
        assert_eq!(carousel.current_index(), 0);
    }
}
