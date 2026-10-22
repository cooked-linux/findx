//! Smart Folders and Saved Search engine.
//!
//! Provides the criteria AST ([`types`]), a compiler translating boolean rules into
//! in-memory predicates and FTS5 queries ([`compiler`]), a hybrid execution engine
//! ([`engine`]), inotify-driven live subscriptions ([`live`]), and XDG-compliant
//! `.savedSearch` persistence ([`storage`]).

pub mod builder;
pub mod compiler;
pub mod engine;
pub mod error;
pub mod live;
pub mod storage;
pub mod types;

pub use builder::CriteriaBuilder;
pub use compiler::{
    CompiledQuery, compile_definition, is_known_kind, resolve_active_scopes, size_value_to_bytes,
    validate_slice,
};
pub use engine::{QueryId, SmartFolderEngine, SmartFolderResults, file_item_from_path};
pub use error::{Result, SmartFolderError};
pub use live::SmartFolderSubscription;
pub use storage::{
    SAVED_SEARCH_EXTENSION, delete_saved_search, ensure_storage_dir, is_saved_search_path,
    list_saved_searches, load_smart_folder, resolve_storage_dir, sanitize_smart_folder_name,
    save_smart_folder,
};
pub use types::{
    BooleanMode, CriteriaAttribute, CriteriaGroup, CriteriaOperator, CriteriaSlice, CriteriaValue,
    SmartFolderDefinition,
};
