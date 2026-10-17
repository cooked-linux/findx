//! Multi-key natural numerical sorting module for directory views.

pub mod comparator;

pub use comparator::{
    SortColumn, SortCriteria, SortDirection, compare_file_items, sort_file_items,
};
