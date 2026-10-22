//! Central Navigation Coordinator arbitrating navigation, view modes, and selections across Finder UI components.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gtk4::prelude::*;

use crate::history::NavigationHistory;
use crate::operations::{FolderSizeManager, OperationsQueue};
use crate::quicklook::QuickLookEngine;
use crate::smart_folders::{
    CriteriaGroup, Result as SmartFolderResult, SmartFolderDefinition, SmartFolderEngine,
    is_saved_search_path, load_smart_folder,
};
use crate::ui::inspector::GetInfoDialog;
use crate::ui::quicklook::QuickLookWindow;
use crate::ui::view_mode::ViewMode;
use crate::undo::{UndoError, UndoManager, UndoTransaction};
use shared::models::operation::{BatchOperation, OperationKind};

/// Callback invoked when directory path changes.
pub type PathChangedCallback = Box<dyn Fn(&Path)>;

/// Callback invoked when the active selection changes.
pub type SelectionChangedCallback = Box<dyn Fn(Option<&Path>)>;

/// Clipboard buffer state tracking files copied or cut by user action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardContent {
    /// Files copied to clipboard (will duplicate on paste).
    Copy(Vec<PathBuf>),
    /// Files cut to clipboard (will move and clear clipboard on paste).
    Cut(Vec<PathBuf>),
}

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
    /// All currently selected filesystem paths supporting multi-selection.
    pub active_selection_paths: RefCell<Vec<PathBuf>>,
    /// Headless preview generation engine.
    pub quicklook_engine: Arc<QuickLookEngine>,
    /// Active floating Quick Look preview window instance, if any.
    pub quicklook_window: RefCell<Option<QuickLookWindow>>,
    /// Active floating persistent inspector dialog instance, if any.
    pub persistent_inspector: RefCell<Option<GetInfoDialog>>,
    /// Background operations queue for batch copy, move, trash, and delete tasks.
    pub operations_queue: Arc<OperationsQueue>,
    /// Transactional undo and redo manager.
    pub undo_manager: Arc<Mutex<UndoManager>>,
    /// Asynchronous folder size and item count calculator.
    pub folder_size_manager: Arc<FolderSizeManager>,
    /// Internal clipboard buffer storing copied or cut paths.
    pub clipboard: RefCell<Option<ClipboardContent>>,
    /// Smart folder definition currently displayed by the virtual view, if any.
    pub active_smart_folder: RefCell<Option<SmartFolderDefinition>>,
    /// Hybrid FTS5/filesystem execution engine shared across smart folder views.
    pub smart_folder_engine: Arc<SmartFolderEngine>,
}

impl std::fmt::Debug for NavigationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationCoordinator")
            .field("history", &self.history)
            .field("current_mode", &self.current_mode)
            .field("active_selection", &self.active_selection)
            .field("active_selection_paths", &self.active_selection_paths)
            .field(
                "quicklook_window_active",
                &self.quicklook_window.borrow().is_some(),
            )
            .field(
                "persistent_inspector_active",
                &self.persistent_inspector.borrow().is_some(),
            )
            .field("operations_queue", &self.operations_queue)
            .field("undo_manager", &self.undo_manager)
            .field("folder_size_manager", &self.folder_size_manager)
            .field("clipboard", &self.clipboard)
            .field("active_smart_folder", &self.active_smart_folder)
            .field(
                "on_path_changed_count",
                &self.on_path_changed.borrow().len(),
            )
            .field(
                "on_selection_changed_count",
                &self.on_selection_changed.borrow().len(),
            )
            .finish_non_exhaustive()
    }
}

impl NavigationCoordinator {
    /// Constructs a new [`NavigationCoordinator`] with custom managers.
    #[must_use]
    pub fn with_managers(
        initial_path: impl AsRef<Path>,
        operations_queue: Arc<OperationsQueue>,
        undo_manager: Arc<Mutex<UndoManager>>,
        folder_size_manager: Arc<FolderSizeManager>,
    ) -> Self {
        Self {
            history: RefCell::new(NavigationHistory::new(initial_path)),
            current_mode: RefCell::new(ViewMode::Columns),
            on_path_changed: RefCell::new(Vec::new()),
            on_selection_changed: RefCell::new(Vec::new()),
            active_selection: RefCell::new(None),
            active_selection_paths: RefCell::new(Vec::new()),
            quicklook_engine: Arc::new(QuickLookEngine::new()),
            quicklook_window: RefCell::new(None),
            persistent_inspector: RefCell::new(None),
            operations_queue,
            undo_manager,
            folder_size_manager,
            clipboard: RefCell::new(None),
            active_smart_folder: RefCell::new(None),
            smart_folder_engine: Arc::new(SmartFolderEngine::open_default()),
        }
    }

    /// Constructs a new [`NavigationCoordinator`] initialized at `initial_path` with default managers.
    #[must_use]
    pub fn new(initial_path: impl AsRef<Path>) -> Self {
        let operations_queue = Arc::new(OperationsQueue::default());
        let undo_manager = Arc::new(Mutex::new(UndoManager::default()));
        operations_queue.attach_undo_manager(Arc::clone(&undo_manager));

        let folder_size_manager = Arc::new(FolderSizeManager::default_manager_fallback());

        Self::with_managers(
            initial_path,
            operations_queue,
            undo_manager,
            folder_size_manager,
        )
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

    /// Returns the smart folder currently displayed, if any.
    #[must_use]
    pub fn active_smart_folder(&self) -> Option<SmartFolderDefinition> {
        self.active_smart_folder.borrow().clone()
    }

    /// Replaces the active smart folder definition.
    pub fn set_active_smart_folder(&self, definition: Option<SmartFolderDefinition>) {
        *self.active_smart_folder.borrow_mut() = definition;
    }

    /// Loads a persisted `.savedSearch` file and marks it as the active smart folder.
    ///
    /// # Errors
    ///
    /// Returns [`crate::smart_folders::SmartFolderError`] when the file is missing,
    /// malformed, or uses an unsupported schema version.
    pub fn open_smart_folder_file(&self, path: &Path) -> SmartFolderResult<()> {
        let definition = load_smart_folder(path)?;
        *self.active_smart_folder.borrow_mut() = Some(definition);
        Ok(())
    }

    /// Creates a draft smart folder scoped to the current directory and activates it.
    pub fn open_new_smart_folder(&self) -> SmartFolderDefinition {
        let current = self.current_path();
        let scopes = if current.is_dir() {
            vec![current]
        } else {
            Vec::new()
        };
        let mut definition =
            SmartFolderDefinition::new("Untitled Smart Folder", CriteriaGroup::match_all());
        definition.scopes = scopes;
        *self.active_smart_folder.borrow_mut() = Some(definition.clone());
        definition
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

        if !is_saved_search_path(target_path) {
            *self.active_smart_folder.borrow_mut() = None;
        }
        self.history.borrow_mut().navigate_to(target_path);
        *self.active_selection.borrow_mut() = None;
        self.active_selection_paths.borrow_mut().clear();

        self.dispatch_path_changed(target_path);
        self.dispatch_selection_changed(None);
    }

    /// Navigates back one step in history if available, dispatching path & selection notifications.
    pub fn go_back(&self) -> Option<PathBuf> {
        let prev = self.history.borrow_mut().go_back()?;
        *self.active_selection.borrow_mut() = None;
        self.active_selection_paths.borrow_mut().clear();

        self.dispatch_path_changed(&prev);
        self.dispatch_selection_changed(None);
        Some(prev)
    }

    /// Navigates forward one step in history if available, dispatching path & selection notifications.
    pub fn go_forward(&self) -> Option<PathBuf> {
        let next = self.history.borrow_mut().go_forward()?;
        *self.active_selection.borrow_mut() = None;
        self.active_selection_paths.borrow_mut().clear();

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
        *self.active_selection_paths.borrow_mut() =
            item.map_or_else(Vec::new, |p| vec![p.to_path_buf()]);
        self.dispatch_selection_changed(item);
    }

    /// Returns all currently selected items in the active view.
    #[must_use]
    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let paths = self.active_selection_paths.borrow().clone();
        if !paths.is_empty() {
            paths
        } else if let Some(sel) = self.active_selection.borrow().clone() {
            vec![sel]
        } else {
            Vec::new()
        }
    }

    /// Updates the selection buffer with multiple selected paths and dispatches selection notifications.
    pub fn set_selected_paths(&self, paths: Vec<PathBuf>) {
        *self.active_selection.borrow_mut() = paths.first().cloned();
        *self.active_selection_paths.borrow_mut() = paths;
        let sel_ref = self.active_selection.borrow();
        self.dispatch_selection_changed(sel_ref.as_deref());
    }

    /// Toggles the floating Quick Look preview window.
    pub fn toggle_quicklook(&self, parent: Option<&impl IsA<gtk4::Window>>) {
        let mut ql_slot = self.quicklook_window.borrow_mut();
        if let Some(ql) = ql_slot.as_ref()
            && ql.window.is_visible()
        {
            ql.dismiss();
            return;
        }

        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }

        if let Some(ql) = ql_slot.as_ref() {
            if let Some(p) = parent {
                ql.window
                    .set_transient_for(Some(p.upcast_ref::<gtk4::Window>()));
            }
            ql.show_preview(paths, 0);
        } else {
            let ql = QuickLookWindow::new(parent, Arc::clone(&self.quicklook_engine));
            ql.show_preview(paths, 0);
            *ql_slot = Some(ql);
        }
    }

    /// Opens the Get Info metadata summary inspector dialog for the active selection.
    pub fn open_get_info(&self, parent: Option<&impl IsA<gtk4::Window>>) -> GetInfoDialog {
        let paths = self.selected_paths();
        let target_paths = if paths.is_empty() {
            vec![self.current_path()]
        } else {
            paths
        };
        GetInfoDialog::open_for_paths(parent, target_paths)
    }

    /// Toggles the floating persistent Quick Inspector window.
    pub fn toggle_persistent_inspector(
        &self,
        parent: Option<&impl IsA<gtk4::Window>>,
    ) -> Option<GetInfoDialog> {
        let mut inspector_slot = self.persistent_inspector.borrow_mut();
        if let Some(dialog) = inspector_slot.as_ref()
            && dialog.window.is_visible()
        {
            dialog.window.close();
            *inspector_slot = None;
            return None;
        }

        let paths = self.selected_paths();
        let target_paths = if paths.is_empty() {
            vec![self.current_path()]
        } else {
            paths
        };
        let dialog = GetInfoDialog::new(crate::ui::inspector::InspectorMode::Persistent, parent);
        dialog.rebind_target(&target_paths);
        dialog.window.present();
        *inspector_slot = Some(dialog.clone());
        Some(dialog)
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

    /// Copies the currently selected item into the internal clipboard.
    pub fn copy_selection(&self) {
        if let Some(path) = self.selection() {
            self.copy_paths(vec![path]);
        }
    }

    /// Cuts the currently selected item into the internal clipboard.
    pub fn cut_selection(&self) {
        if let Some(path) = self.selection() {
            self.cut_paths(vec![path]);
        }
    }

    /// Stores the provided paths into the clipboard with `Copy` intent.
    pub fn copy_paths(&self, paths: Vec<PathBuf>) {
        if !paths.is_empty() {
            *self.clipboard.borrow_mut() = Some(ClipboardContent::Copy(paths));
        }
    }

    /// Stores the provided paths into the clipboard with `Cut` intent.
    pub fn cut_paths(&self, paths: Vec<PathBuf>) {
        if !paths.is_empty() {
            *self.clipboard.borrow_mut() = Some(ClipboardContent::Cut(paths));
        }
    }

    /// Returns a clone of the current clipboard content, if any.
    #[must_use]
    pub fn clipboard(&self) -> Option<ClipboardContent> {
        self.clipboard.borrow().clone()
    }

    /// Clears the clipboard buffer.
    pub fn clear_clipboard(&self) {
        *self.clipboard.borrow_mut() = None;
    }

    /// Pastes clipboard contents into the current directory.
    pub fn paste_to_current_directory(&self) -> Option<String> {
        let current_dir = self.current_path();
        self.paste_paths_to(&current_dir)
    }

    /// Pastes clipboard contents into the specified target directory.
    pub fn paste_paths_to(&self, target_dir: &Path) -> Option<String> {
        let content = self.clipboard.borrow().clone()?;
        match content {
            ClipboardContent::Copy(sources) => {
                if sources.is_empty() {
                    return None;
                }
                let op_id = format!("paste-copy-{}", uuid::Uuid::new_v4());
                let op = BatchOperation::new(
                    op_id,
                    OperationKind::Copy,
                    sources,
                    Some(target_dir.to_path_buf()),
                );
                Some(self.operations_queue.enqueue_operation(op))
            }
            ClipboardContent::Cut(sources) => {
                if sources.is_empty() {
                    return None;
                }
                let op_id = format!("paste-cut-{}", uuid::Uuid::new_v4());
                let op = BatchOperation::new(
                    op_id,
                    OperationKind::Move,
                    sources,
                    Some(target_dir.to_path_buf()),
                );
                *self.clipboard.borrow_mut() = None;
                Some(self.operations_queue.enqueue_operation(op))
            }
        }
    }

    /// Executes the top undo transaction from the undo stack.
    ///
    /// # Errors
    /// Returns [`UndoError`] if the inverse operation execution fails.
    pub fn undo(&self) -> Result<Option<UndoTransaction>, UndoError> {
        let mut guard = self
            .undo_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.undo()
    }

    /// Re-executes the top redo transaction from the redo stack.
    ///
    /// # Errors
    /// Returns [`UndoError`] if forward re-execution fails.
    pub fn redo(&self) -> Result<Option<UndoTransaction>, UndoError> {
        let mut guard = self
            .undo_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.redo()
    }

    /// Returns `true` if an undo operation is available.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        let guard = self
            .undo_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.can_undo()
    }

    /// Returns `true` if a redo operation is available.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        let guard = self
            .undo_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.can_redo()
    }

    /// Returns the UI label for the current top undo transaction.
    #[must_use]
    pub fn undo_label(&self) -> Option<String> {
        let guard = self
            .undo_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.current_undo_label()
    }

    /// Returns the UI label for the current top redo transaction.
    #[must_use]
    pub fn redo_label(&self) -> Option<String> {
        let guard = self
            .undo_manager
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.current_redo_label()
    }

    /// Duplicates the currently selected item.
    pub fn duplicate_selection(&self) -> Option<String> {
        let path = self.selection()?;
        self.duplicate_paths(vec![path])
    }

    /// Duplicates the given paths.
    pub fn duplicate_paths(&self, paths: Vec<PathBuf>) -> Option<String> {
        if paths.is_empty() {
            return None;
        }
        let op_id = format!("duplicate-{}", uuid::Uuid::new_v4());
        let op = BatchOperation::new(op_id, OperationKind::Duplicate, paths, None);
        Some(self.operations_queue.enqueue_operation(op))
    }

    /// Moves the currently selected item to the trash.
    pub fn trash_selection(&self) -> Option<String> {
        let path = self.selection()?;
        self.trash_paths(vec![path])
    }

    /// Moves the given paths to the trash.
    pub fn trash_paths(&self, paths: Vec<PathBuf>) -> Option<String> {
        if paths.is_empty() {
            return None;
        }
        let op_id = format!("trash-{}", uuid::Uuid::new_v4());
        let op = BatchOperation::new(op_id, OperationKind::Trash, paths, None);
        Some(self.operations_queue.enqueue_operation(op))
    }

    /// Permanently deletes the currently selected item if confirmed.
    pub fn delete_selection_permanently(&self, confirm: bool) -> Option<String> {
        let path = self.selection()?;
        self.delete_paths_permanently(vec![path], confirm)
    }

    /// Permanently deletes the given paths if confirmed.
    pub fn delete_paths_permanently(&self, paths: Vec<PathBuf>, confirm: bool) -> Option<String> {
        if !confirm || paths.is_empty() {
            return None;
        }
        let op_id = format!("delete-{}", uuid::Uuid::new_v4());
        let op = BatchOperation::new(op_id, OperationKind::Delete, paths, None);
        Some(self.operations_queue.enqueue_operation(op))
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

    #[tokio::test]
    async fn test_coordinator_clipboard_and_operations() {
        let coordinator = NavigationCoordinator::new("/tmp/test_dir");

        // No initial selection -> copy and cut do nothing
        coordinator.copy_selection();
        assert_eq!(coordinator.clipboard(), None);
        coordinator.cut_selection();
        assert_eq!(coordinator.clipboard(), None);

        // Select item
        let file = PathBuf::from("/tmp/test_dir/file.txt");
        coordinator.set_selection(Some(&file));

        // Copy selection
        coordinator.copy_selection();
        assert_eq!(
            coordinator.clipboard(),
            Some(ClipboardContent::Copy(vec![file.clone()]))
        );

        // Paste copies and retains clipboard
        let task_id = coordinator.paste_to_current_directory();
        assert!(task_id.is_some());
        assert_eq!(
            coordinator.clipboard(),
            Some(ClipboardContent::Copy(vec![file.clone()]))
        );

        // Cut selection
        coordinator.cut_selection();
        assert_eq!(
            coordinator.clipboard(),
            Some(ClipboardContent::Cut(vec![file]))
        );

        // Paste moves and clears clipboard
        let task_id2 = coordinator.paste_to_current_directory();
        assert!(task_id2.is_some());
        assert_eq!(coordinator.clipboard(), None);

        // Duplicate
        let dup_id = coordinator.duplicate_selection();
        assert!(dup_id.is_some());

        // Trash
        let trash_id = coordinator.trash_selection();
        assert!(trash_id.is_some());

        // Delete unconfirmed
        assert_eq!(coordinator.delete_selection_permanently(false), None);

        // Delete confirmed
        let del_id = coordinator.delete_selection_permanently(true);
        assert!(del_id.is_some());

        // Undo & redo initial state
        assert!(!coordinator.can_undo());
        assert!(!coordinator.can_redo());
        assert_eq!(coordinator.undo_label(), None);
        assert_eq!(coordinator.redo_label(), None);
        assert!(coordinator.undo().is_ok());
        assert!(coordinator.redo().is_ok());
    }
}
