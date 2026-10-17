//! Asynchronous filesystem crawler, inotify watcher, text extractor, and search indexer.

pub mod crawler;
pub mod extractor;
pub mod pipeline;
pub mod query;
pub mod watcher;

pub use crawler::{CrawlerConfig, DEFAULT_BLACKLISTED_DIR_NAMES, DirectoryCrawler, DiscoveredFile};
pub use extractor::{
    EXTRACTION_TIMEOUT, MAX_EXTRACTED_CHARS, MAX_INDEX_FILE_SIZE, SUPPORTED_EXTENSIONS,
    extract_file_content, extract_file_content_sync, is_text_extractable,
};
pub use pipeline::{DEFAULT_BATCH_THRESHOLD, DEFAULT_FLUSH_INTERVAL, IndexPipeline, PipelineOp};
pub use query::{SearchHit, SearchQueryBuilder, sanitize_fts5_query, search_indexed_files};
pub use watcher::{DEFAULT_DEBOUNCE_DURATION, FileWatcher, FsEvent, is_ignored_fs_path};

/// Domain error type for filesystem crawling, extraction, watching, and indexing.
#[derive(Debug, thiserror::Error)]
pub enum IndexerError {
    /// SQLite or database-related failures during indexing.
    #[error("Database error in indexing engine: {0}")]
    Db(#[from] rusqlite::Error),

    /// Filesystem directory traversal errors.
    #[error("Filesystem walk error: {0}")]
    Walk(String),

    /// File monitoring / inotify notifications error.
    #[error("Inotify watcher error: {0}")]
    Watcher(String),

    /// Timeout exceeded while extracting text content from a file.
    #[error("Text extraction timeout on file: {0}")]
    ExtractionTimeout(std::path::PathBuf),

    /// Standard I/O errors encountered during operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<notify::Error> for IndexerError {
    fn from(err: notify::Error) -> Self {
        Self::Watcher(err.to_string())
    }
}

impl From<walkdir::Error> for IndexerError {
    fn from(err: walkdir::Error) -> Self {
        Self::Walk(err.to_string())
    }
}
