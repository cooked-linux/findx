//! Preview loading and coordinator submodules.

pub mod loader;

pub use loader::{
    FileMetadataSummary, MAX_CODE_LINES, MAX_CODE_PREVIEW_SIZE, MAX_LINE_LENGTH, PreviewData,
    PreviewKind, PreviewLoader, TRUNCATION_NOTICE, format_breadcrumbs, format_human_bytes,
    format_kind_display, format_timestamp, highlight_code_lines, inspect_metadata,
    read_capped_lines, render_pdf_page_1,
};
