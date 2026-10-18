//! Headless deep metadata inspection engine and extended attributes service.

pub mod aggregate;
pub mod error;
pub mod extract;
pub mod tags_sync;
pub mod types;
pub mod xattr;

pub use aggregate::{MultiSelectionSummary, aggregate_metadata};
pub use error::{MetadataError, Result};
pub use extract::extract_deep_metadata;
pub use tags_sync::{
    DatabasePool, init_file_tags_table, query_files_by_color, query_files_by_tag, remove_file_tags,
    sync_file_tags,
};
pub use types::{
    DeepMetadata, ExtendedAttrsMeta, ImageExifMeta, MediaStreamMeta, PdfDocMeta, PermissionsInfo,
};
pub use xattr::ExtendedAttributesService;
