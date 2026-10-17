//! Search providers for the Spotlight arbiter.

pub mod app_provider;
pub mod calc_provider;

pub use app_provider::{
    AppLaunchError, ApplicationProvider, compute_ghost_completion, find_default_terminal,
    is_settings_panel, launch_desktop_app, launch_desktop_app_with_token,
    request_xdg_activation_token, resolve_app_icon_path, wrap_in_terminal,
    wrap_in_terminal_emulator,
};
pub use calc_provider::CalculatorProvider;
