//! Central Navigation Coordinator arbitrating navigation, view modes, and selections across Finder UI components.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::history::NavigationHistory;
use crate::ui::view_mode::ViewMode;

/// Callback invoked when directory path changes.
pub type PathChangedCallback = Box<dyn Fn(&Path)>;

/// Callback invoked when the active selection changes.
pub type SelectionChangedCallback = Box<dyn Fn(Option<&Path>)>;

/// Central coordinator arbitrating navigation, history, view modes, and item selections.
pub struct NavigationCoordinator {
    /// Bounded navigation history stack tracking back/forward traversal.
    pub history: RefCell<NavigationHistory>,
    /// Currently active view mode (Icons, List, Columns).
    pub current_mode: RefCell<ViewMode>,
    /// Registered callbacks notified when directory path changes.
    pub on_path_changed: RefCell<Vec<PathChangedCallback>>,
    /// Registered callbacks notified when active selected item changes.
    pub on_selection_changed: RefCell<Vec<SelectionChangedCallback>>,
    /// Current selected item or directory leaf, if any.
    pub active_selection: RefCell<Option<PathBuf>>,
}

impl std::fmt::Debug for NavigationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationCoordinator")
            .field("history", &self.history)
            .field("current_mode", &self.current_mode)
            .field("active_selection", &self.active_selection)
            .field(
                "on_path_changed_count",
                &self.on_path_changed.borrow().len(),
            )
            .field(
                "on_selection_changed_count",
                &self.on_selection_changed.borrow().len(),
            )
            .finish()
    }
}

impl NavigationCoordinator {
    /// Constructs a new [`NavigationCoordinator`] initialized at `initial_path`.
    #[must_use]
    pub fn new(initial_path: impl AsRef<Path>) -> Self {
        Self {
            history: RefCell::new(NavigationHistory::new(initial_path)),
            current_mode: RefCell::new(ViewMode::Columns),
            on_path_changed: RefCell::new(Vec::new()),
            on_selection_changed: RefCell::new(Vec::new()),
            active_selection: RefCell::new(None),
        }
    }

    /// Returns the current active directory path.
    #[must_use]
    pub fn current_path(&self) -> PathBuf {
        self.history.borrow().current_path().to_path_buf()
    }

    /// Returns `true` if backward navigation is possible.
    #[must_use]
    pub fn can_go_back(&self) -> bool {
        self.history.borrow().can_go_back()
    }

    /// Returns `true` if forward navigation is possible.
    #[must_use]
    pub fn can_go_forward(&self) -> bool {
        self.history.borrow().can_go_forward()
    }

    /// Returns the current active [`ViewMode`].
    #[must_use]
    pub fn view_mode(&self) -> ViewMode {
        *self.current_mode.borrow()
    }

    /// Sets the active [`ViewMode`].
    pub fn set_view_mode(&self, mode: ViewMode) {
        *self.current_mode.borrow_mut() = mode;
    }

    /// Returns the currently selected item or leaf, if any.
    #[must_use]
    pub fn selection(&self) -> Option<PathBuf> {
        self.active_selection.borrow().clone()
    }

    /// Registers a callback notified when the active directory path changes.
    pub fn connect_path_changed<F: Fn(&Path) + 'static>(&self, callback: F) {
        self.on_path_changed.borrow_mut().push(Box::new(callback));
    }

    /// Registers a callback notified when the selected item changes.
    pub fn connect_selection_changed<F: Fn(Option<&Path>) + 'static>(&self, callback: F) {
        self.on_selection_changed
            .borrow_mut()
            .push(Box::new(callback));
    }

    /// Navigates to a new directory path, updating history and dispatching path & selection notifications.
    pub fn navigate_to(&self, target: impl AsRef<Path>) {
        let target_path = target.as_ref();
        let prev_path = self.history.borrow().current_path().to_path_buf();
        if target_path == prev_path {
            return;
        }

        self.history.borrow_mut().navigate_to(target_path);
        *self.active_selection.borrow_mut() = None;

        self.dispatch_path_changed(target_path);
        self.dispatch_selection_changed(None);
    }

    /// Navigates back one step in history if available, dispatching path & selection notifications.
    pub fn go_back(&self) -> Option<PathBuf> {
        let prev = self.history.borrow_mut().go_back()?;
        *self.active_selection.borrow_mut() = None;

        self.dispatch_path_changed(&prev);
        self.dispatch_selection_changed(None);
        Some(prev)
    }

    /// Navigates forward one step in history if available, dispatching path & selection notifications.
    pub fn go_forward(&self) -> Option<PathBuf> {
        let next = self.history.borrow_mut().go_forward()?;
        *self.active_selection.borrow_mut() = None;

        self.dispatch_path_changed(&next);
        self.dispatch_selection_changed(None);
        Some(next)
    }

    /// Updates the active item selection and dispatches selection notifications.
    pub fn set_selection(&self, item: Option<&Path>) {
        let changed = self.active_selection.borrow().as_deref() != item;
        if !changed {
            return;
        }

        *self.active_selection.borrow_mut() = item.map(Path::to_path_buf);
        self.dispatch_selection_changed(item);
    }

    fn dispatch_path_changed(&self, path: &Path) {
        let callbacks = self.on_path_changed.borrow();
        for cb in callbacks.iter() {
            cb(path);
        }
    }

    fn dispatch_selection_changed(&self, selection: Option<&Path>) {
        let callbacks = self.on_selection_changed.borrow();
        for cb in callbacks.iter() {
            cb(selection);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn test_coordinator_initial_state() {
        let coordinator = NavigationCoordinator::new("/tmp");
        assert_eq!(coordinator.current_path(), PathBuf::from("/tmp"));
        assert!(!coordinator.can_go_back());
        assert!(!coordinator.can_go_forward());
        assert_eq!(coordinator.view_mode(), ViewMode::Columns);
        assert_eq!(coordinator.selection(), None);
    }

    #[test]
    fn test_coordinator_navigation_and_callbacks() {
        let coordinator = Rc::new(NavigationCoordinator::new("/home/user"));

        let path_notified = Rc::new(RefCell::new(Vec::new()));
        let p_clone = Rc::clone(&path_notified);
        coordinator.connect_path_changed(move |path| {
            p_clone.borrow_mut().push(path.to_path_buf());
        });

        let sel_notified = Rc::new(RefCell::new(Vec::new()));
        let s_clone = Rc::clone(&sel_notified);
        coordinator.connect_selection_changed(move |sel| {
            sel_notified.borrow_mut().push(sel.map(Path::to_path_buf));
        });

        // Navigate to Documents
        coordinator.navigate_to("/home/user/Documents");
        assert_eq!(
            coordinator.current_path(),
            PathBuf::from("/home/user/Documents")
        );
        assert!(coordinator.can_go_back());
        assert!(!coordinator.can_go_forward());
        assert_eq!(
            *path_notified.borrow(),
            vec![PathBuf::from("/home/user/Documents")]
        );
        assert_eq!(*s_clone.borrow(), vec![None]);

        // Set selection
        coordinator.set_selection(Some(Path::new("/home/user/Documents/report.pdf")));
        assert_eq!(
            coordinator.selection(),
            Some(PathBuf::from("/home/user/Documents/report.pdf"))
        );
        assert_eq!(
            *s_clone.borrow(),
            vec![None, Some(PathBuf::from("/home/user/Documents/report.pdf"))]
        );

        // Go back
        let back = coordinator.go_back();
        assert_eq!(back, Some(PathBuf::from("/home/user")));
        assert_eq!(coordinator.current_path(), PathBuf::from("/home/user"));
        assert!(!coordinator.can_go_back());
        assert!(coordinator.can_go_forward());
        assert_eq!(coordinator.selection(), None);

        // Go forward
        let fwd = coordinator.go_forward();
        assert_eq!(fwd, Some(PathBuf::from("/home/user/Documents")));
        assert_eq!(
            coordinator.current_path(),
            PathBuf::from("/home/user/Documents")
        );
        assert!(coordinator.can_go_back());
        assert!(!coordinator.can_go_forward());

        // View mode switching
        coordinator.set_view_mode(ViewMode::List);
        assert_eq!(coordinator.view_mode(), ViewMode::List);
    }
}
