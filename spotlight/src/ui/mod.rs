//! Spotlight user interface components.

pub mod calc_card;
pub mod confirm_chip;
pub mod dict_card;
pub mod empty_state;
pub mod file_row;
pub mod no_results;
pub mod search_bar;
pub mod theme;
pub mod window;

pub use calc_card::{CalcHeroCardWidget, MathRepresentation};
pub use confirm_chip::{CONFIRM_TIMEOUT_SECS, ConfirmChipWidget, ConfirmState, ConfirmationStatus};
pub use dict_card::DictCardWidget;
pub use empty_state::{EmptyStateSelection, EmptyStateWidget, fetch_suggested_apps};
pub use file_row::{FileChipKind, FileRowWidget};
pub use no_results::{NoResultsState, NoResultsWidget};

pub use search_bar::{KeyAction, SearchBarWidget, compute_ghost_completion, map_key_to_action};

pub use theme::{SPOTLIGHT_CSS, apply_theme_classes, init_theme};
pub use window::{
    ANIMATION_DURATION_MS, ANIMATION_DURATION_US, FOOTER_HINT_HEIGHT, FOOTER_HINT_SHORTCUTS,
    HEADER_HEIGHT, INITIAL_HEIGHT, INITIAL_WIDTH, MAX_HEIGHT, OverlayState, ROW_HEIGHT,
    SpotlightWindow, build_footer_hint_box, calculate_target_height, calculate_top_margin,
    ease_out_cubic, init_for_window, interpolate_height,
};
