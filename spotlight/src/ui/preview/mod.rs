//! Split-pane preview and metadata inspector UI components.

pub mod code_view;
pub mod image_view;
pub mod meta_table;
pub mod pane;

pub use code_view::CodePreviewWidget;
pub use image_view::{ImagePreviewWidget, THUMB_HEIGHT, THUMB_WIDTH};
pub use meta_table::{MetaRow, MetaTableWidget};
pub use pane::PreviewPaneWidget;
