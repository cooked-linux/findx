//! `shared` crate: Common foundation and shared primitives across `finder` and `spotlight`.

pub mod db;
pub mod desktop;
pub mod errors;
pub mod frecency;
pub mod fs;
pub mod indexer;
pub mod ipc;
pub mod models;
pub mod thumbnail;

pub use db::{
    Database, FrecencyAggregate, FrecencyEvent, FtsDocument, FtsQueryBuilder, FtsSearchResult,
    Migration, TagCacheEntry, configure_connection, default_db_path,
};

pub use desktop::{
    DesktopApp, DesktopError, calculate_text_score, resolve_icon_path, scan_installed_apps,
};

pub use errors::{DbError, FsError, IpcError, ModelError, Result, SharedError, XattrError};
pub use frecency::{
    CompositeScorer, DEFAULT_FRECENCY_WEIGHT, DEFAULT_HALF_LIFE_SECS, EventKind, FrecencyError,
    FrecencyTracker, RankedCandidate, compute_event_score, rank_and_sort,
};
pub use fs::{
    EnhancedStat, InodeKey, TrashInfo, TrashedItem, XATTR_COLOR, XATTR_COMMENT, XATTR_ORIGIN_URL,
    XATTR_TAGS, clone_file, clone_file_with_progress, empty_trash, get_comment, get_origin_url,
    get_tags, is_same_filesystem, list_trash_items, lstat_path, move_to_trash, natural_cmp,
    parse_trash_info, remove_comment, remove_origin_url, remove_tags, restore_from_trash,
    set_comment, set_origin_url, set_tags, stat_path,
};
pub use indexer::{
    CrawlerConfig, DirectoryCrawler, DiscoveredFile, FileWatcher, FsEvent, IndexPipeline,
    IndexerError, PipelineOp, SearchHit, SearchQueryBuilder, is_ignored_fs_path,
    search_indexed_files,
};

pub use ipc::{
    DEFAULT_TIMEOUT, DeviceEvent, DriveConnectionType, FileManagerClient, FileManagerProxy,
    GlobalShortcutsClient, GlobalShortcutsProxy, Login1Client, Login1ManagerProxy, MountedVolume,
    SessionHandle, ShortcutActivation, UDisks2Monitor, file_uri_to_path, get_session_connection,
    get_system_connection, is_session_bus_available, is_system_bus_available, path_to_file_uri,
    reset_connections_for_test,
};
pub use models::{
    AggregatedSearchResults, BatchOperation, CategorySection, FileItem, FileMetadata, FileTag,
    MimeCategory, OperationKind, OperationState, ProgressReport, SearchCategory, SearchPayload,
    SearchRecord, TagColor, TransferItem,
};
pub use thumbnail::{
    ThumbnailCache, ThumbnailError, ThumbnailMetadata, ThumbnailMetadataRecord, ThumbnailSize,
    apply_exif_orientation, canonical_file_uri, compute_thumbnail_hash, generate_thumbnail_image,
    get_or_generate_thumbnail, read_thumbnail_metadata, write_failed_thumbnail,
    write_thumbnail_png,
};

#[must_use]
pub const fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
