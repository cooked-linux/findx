//! Navigation history stack tracking back/forward transitions for Finder.

use std::path::{Path, PathBuf};

/// Bidirectional history stack bounded to a maximum depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationHistory {
    back_stack: Vec<PathBuf>,
    forward_stack: Vec<PathBuf>,
    current: PathBuf,
    max_depth: usize,
}

impl NavigationHistory {
    /// Creates a new navigation history initialized to the given path.
    #[must_use]
    pub fn new(initial_path: impl AsRef<Path>) -> Self {
        Self {
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            current: initial_path.as_ref().to_path_buf(),
            max_depth: 100,
        }
    }

    /// Returns the current active path.
    #[must_use]
    pub fn current_path(&self) -> &Path {
        &self.current
    }

    /// Returns `true` if backward navigation is possible.
    #[must_use]
    pub const fn can_go_back(&self) -> bool {
        !self.back_stack.is_empty()
    }

    /// Returns `true` if forward navigation is possible.
    #[must_use]
    pub const fn can_go_forward(&self) -> bool {
        !self.forward_stack.is_empty()
    }

    /// Navigates to a new target path, pushing the current path onto `back_stack`,
    /// clearing `forward_stack`, and enforcing bounded capacity.
    pub fn navigate_to(&mut self, target: impl AsRef<Path>) {
        let target = target.as_ref().to_path_buf();
        if target == self.current {
            return;
        }
        self.back_stack
            .push(std::mem::replace(&mut self.current, target));
        if self.back_stack.len() > self.max_depth {
            self.back_stack.remove(0);
        }
        self.forward_stack.clear();
    }

    /// Navigates back one step in history, returning the new current path if available.
    pub fn go_back(&mut self) -> Option<PathBuf> {
        if let Some(prev) = self.back_stack.pop() {
            self.forward_stack
                .push(std::mem::replace(&mut self.current, prev.clone()));
            Some(prev)
        } else {
            None
        }
    }

    /// Navigates forward one step in history, returning the new current path if available.
    pub fn go_forward(&mut self) -> Option<PathBuf> {
        if let Some(next) = self.forward_stack.pop() {
            self.back_stack
                .push(std::mem::replace(&mut self.current, next.clone()));
            Some(next)
        } else {
            None
        }
    }

    /// Returns the back stack entries.
    #[must_use]
    pub fn back_stack(&self) -> &[PathBuf] {
        &self.back_stack
    }

    /// Returns the forward stack entries.
    #[must_use]
    pub fn forward_stack(&self) -> &[PathBuf] {
        &self.forward_stack
    }

    /// Returns the configured maximum history depth.
    #[must_use]
    pub const fn max_depth(&self) -> usize {
        self.max_depth
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_history_initial_state() {
        let history = NavigationHistory::new("/home/user");
        assert_eq!(history.current_path(), Path::new("/home/user"));
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
        assert_eq!(history.max_depth(), 100);
    }

    #[test]
    fn test_navigation_history_linear_transitions() {
        let mut history = NavigationHistory::new("/home/user");
        history.navigate_to("/home/user/Documents");
        assert_eq!(history.current_path(), Path::new("/home/user/Documents"));
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        history.navigate_to("/home/user/Downloads");
        assert_eq!(history.current_path(), Path::new("/home/user/Downloads"));
        assert_eq!(history.back_stack().len(), 2);

        // Go back
        let back1 = history.go_back();
        assert_eq!(back1, Some(PathBuf::from("/home/user/Documents")));
        assert_eq!(history.current_path(), Path::new("/home/user/Documents"));
        assert!(history.can_go_back());
        assert!(history.can_go_forward());

        // Go back again
        let back2 = history.go_back();
        assert_eq!(back2, Some(PathBuf::from("/home/user")));
        assert_eq!(history.current_path(), Path::new("/home/user"));
        assert!(!history.can_go_back());
        assert!(history.can_go_forward());

        // Back when empty
        assert_eq!(history.go_back(), None);

        // Go forward
        let fwd1 = history.go_forward();
        assert_eq!(fwd1, Some(PathBuf::from("/home/user/Documents")));
        assert_eq!(history.current_path(), Path::new("/home/user/Documents"));
        assert!(history.can_go_back());
        assert!(history.can_go_forward());

        // Go forward again
        let fwd2 = history.go_forward();
        assert_eq!(fwd2, Some(PathBuf::from("/home/user/Downloads")));
        assert_eq!(history.current_path(), Path::new("/home/user/Downloads"));
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        // Forward when empty
        assert_eq!(history.go_forward(), None);
    }

    #[test]
    fn test_navigation_history_clears_forward_on_new_navigation() {
        let mut history = NavigationHistory::new("/home/user");
        history.navigate_to("/home/user/Documents");
        history.navigate_to("/home/user/Downloads");
        assert_eq!(
            history.go_back(),
            Some(PathBuf::from("/home/user/Documents"))
        );
        assert!(history.can_go_forward());

        // Navigate to new branch
        history.navigate_to("/home/user/Music");
        assert_eq!(history.current_path(), Path::new("/home/user/Music"));
        assert!(!history.can_go_forward());
        assert!(history.can_go_back());
        assert_eq!(history.forward_stack().len(), 0);
    }

    #[test]
    fn test_navigation_history_noop_on_current_path() {
        let mut history = NavigationHistory::new("/home/user");
        history.navigate_to("/home/user");
        assert_eq!(history.current_path(), Path::new("/home/user"));
        assert!(!history.can_go_back());
        assert_eq!(history.back_stack().len(), 0);
    }

    #[test]
    fn test_navigation_history_max_depth_cap() {
        let mut history = NavigationHistory::new("/step/0");
        for i in 1..=120 {
            history.navigate_to(format!("/step/{i}"));
        }
        assert_eq!(history.back_stack().len(), 100);
        assert_eq!(history.back_stack()[0], PathBuf::from("/step/20"));
        assert_eq!(history.current_path(), Path::new("/step/120"));
    }
}
