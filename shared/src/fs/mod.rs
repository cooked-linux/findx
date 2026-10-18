//! Filesystem and POSIX system layer primitives.
//!
//! Provides native Linux filesystem operations: extended attributes (tags, color,
//! comments, origin URL), natural alphanumeric sorting, fast reflink cloning via
//! `copy_file_range`, Freedesktop Trash management, and birth time retrieval via `statx`.

pub mod clone;
pub mod natord;
pub mod stat;
pub mod trash;
pub mod xattr;

pub use crate::models::{FileTag, TagColor};
pub use clone::{clone_file, clone_file_with_progress};
pub use natord::natural_cmp;
pub use stat::{EnhancedStat, InodeKey, is_same_filesystem, lstat_path, stat_path};
pub use trash::{
    TrashInfo, TrashedItem, empty_trash, list_trash_items, move_to_trash, parse_trash_info,
    restore_from_trash, send_to_trash,
};
pub use xattr::{
    XATTR_COLOR, XATTR_COMMENT, XATTR_ORIGIN_URL, XATTR_TAGS, get_comment, get_origin_url,
    get_tags, remove_comment, remove_origin_url, remove_tags, set_comment, set_origin_url,
    set_tags,
};
