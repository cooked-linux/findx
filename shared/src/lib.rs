//! `shared` crate: Common foundation and shared primitives across `finder` and `spotlight`.

pub mod errors;

pub use errors::{DbError, FsError, IpcError, ModelError, Result, SharedError, XattrError};

#[must_use]
pub const fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
