//! Finder desktop file manager application and library crate.

pub mod app;
pub mod archive;
pub mod bookmarks;
pub mod breadcrumbs;
pub mod coordinator;
pub mod error;
pub mod grouping;
pub mod history;
pub mod metadata;
pub mod operations;
pub mod quicklook;
pub mod search;
pub mod service;
pub mod sidebar;
pub mod smart_folders;
pub mod sorting;
pub mod ui;
pub mod undo;
pub mod views;

pub use app::{FINDER_APP_ID, FinderApp, FinderCliArgs};
pub use archive::{
    ArchiveCompressionJob, ArchiveEntryItem, ArchiveError, ArchiveFormat, ArchiveManifest,
    ArchiveProgress, ArchiveWriter, compress_selection,
};
pub use bookmarks::{BookmarkEntry, default_places, load_user_bookmarks, save_user_bookmarks};
pub use breadcrumbs::{BreadcrumbDisplayItem, BreadcrumbLineage, PathSegment, layout_segments};
pub use coordinator::{
    NavigationCoordinator, PathChangedCallback, RenameRequestedCallback, SelectionChangedCallback,
};
pub use error::{FinderError, Result};
pub use metadata::{
    DeepMetadata, ExtendedAttributesService, MetadataError, MultiSelectionSummary,
    aggregate_metadata, extract_deep_metadata,
};
pub use operations::{
    ActiveTask, BatchOperation, OperationError, OperationEvent, OperationsQueue, ProgressReport,
};
pub use quicklook::{PreviewKind, PreviewPayload, QuickLookEngine, QuickLookError};
pub use search::{
    AttributeFilter, DateConstraint, ParsedQuery, ScopeBarWidget, SearchError, SearchMode,
    SearchResultItem, SearchScope,
};
pub use service::{
    DbusServiceHandle, FileManagerAction, FileManagerService, export_file_manager_service,
};
pub use undo::{FileOperationRecord, UndoManager, UndoOperationType, UndoTransaction};
pub use views::columns::{ColumnEntry, ColumnKind, ColumnViewModel};
pub use views::list::{ExpandedNodesCache, LruWatchPool, TreeListViewModel, TreeNode};
