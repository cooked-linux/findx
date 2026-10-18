//! Quick Look engine, sandboxing, and preview generators.

pub mod engine;
pub mod error;
pub mod generator;
pub mod sandbox;
pub mod types;

pub use engine::QuickLookEngine;
pub use error::{QuickLookError, Result};
pub use generator::{
    FallbackGenerator, ImageGenerator, MAX_CODE_LINES, MAX_CODE_PREVIEW_SIZE, MediaGenerator,
    PdfGenerator, PreviewGenerator, SourceCodeGenerator, quicklook_cache_dir,
};
pub use sandbox::{SandboxConfig, has_bubblewrap, run_sandboxed, run_sandboxed_with_rw};
pub use types::{
    CodePreviewData, FallbackCardData, ImagePreviewData, MediaPreviewData, PdfPreviewData,
    PreviewKind, PreviewPayload, PreviewRequest,
};
