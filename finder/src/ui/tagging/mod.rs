//! GTK4 and libadwaita graphical interface components for file tagging and color labels.

pub mod badges;
pub mod menu;
pub mod pill_editor;
pub mod sidebar_filter;
pub mod swatches;
pub mod theme;

pub use badges::{
    TagBadgeCache, TagBadgeView, invalidate_badge_cache, render_column_badge_cluster,
    render_list_view_tags_cell,
};
pub use menu::{
    MultiSelectionSummary, apply_tag_to_paths, create_tags_context_menu_widget,
    remove_tag_from_paths, toggle_color_on_paths,
};
pub use pill_editor::{TagAutocompletePopover, TagPillEditor, TagPillEntry};
pub use sidebar_filter::{
    VIRTUAL_TAGS_SCHEME, is_tag_virtual_path, make_tag_virtual_path, parse_tag_virtual_path,
    query_tagged_files,
};
pub use swatches::{ColorSwatchBar, ColorToggledCallback, SwatchState};
pub use theme::{TAGGING_CSS, install_tagging_theme};
