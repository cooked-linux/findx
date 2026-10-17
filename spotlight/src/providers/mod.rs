//! Search providers for the Spotlight arbiter.

pub mod app_provider;
pub mod calc_provider;
pub mod dict_provider;
pub mod file_provider;
pub mod system_provider;
pub mod web_provider;

pub use app_provider::{
    AppLaunchError, ApplicationProvider, compute_ghost_completion, find_default_terminal,
    is_settings_panel, launch_desktop_app, launch_desktop_app_with_token,
    request_xdg_activation_token, resolve_app_icon_path, wrap_in_terminal,
    wrap_in_terminal_emulator,
};
pub use calc_provider::CalculatorProvider;
pub use dict_provider::DictionaryProvider;
pub use file_provider::{
    DEFAULT_FILE_LIMIT, FileSearchProvider, copy_file_path_to_clipboard, hit_to_search_record,
    open_file, show_in_file_manager,
};
pub use system_provider::{
    SYSTEM_COMMANDS, SystemActionProvider, SystemCommand, execute_system_action,
    is_destructive_action, match_system_actions, parse_brightness_command, parse_volume_command,
};
pub use web_provider::{SearchEngine, WebSearchProvider, open_browser_url, parse_explicit_prefix};
