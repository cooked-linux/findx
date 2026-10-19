use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gtk4::prelude::*;
use libadwaita::prelude::*;

use super::columns::ColumnContainer;
use super::header_bar::FinderHeaderBar;
use super::inspector::GetInfoDialog;
use super::list::HierarchicalListViewWidget;
use super::operations::{OperationsQueuePopover, TransferProgressDialog};
use super::path_bar::PathBarWidget;
use super::sidebar::SidebarWidget;
use super::theme::{apply_theme_classes, install_finder_theme};
use super::view_mode::ViewMode;
use crate::breadcrumbs::FinderPreferences;
use crate::coordinator::NavigationCoordinator;
use crate::history::NavigationHistory;
use crate::sidebar::config::SidebarConfig;
use crate::sidebar::tags::TagFilterEvent;

/// Actions triggered by global window keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKeyAction {
    /// Navigate back in directory history (`Alt+Left`).
    HistoryBack,
    /// Navigate forward in directory history (`Alt+Right`).
    HistoryForward,
    /// Switch active view mode (`Ctrl+1`, `Ctrl+2`, `Ctrl+3`).
    SwitchView(ViewMode),
    /// Focus the header bar search entry (`Ctrl+F`).
    FocusSearch,
    /// Toggle left sidebar navigation panel visibility (`Ctrl+Alt+S` or `Cmd+Alt+S`).
    ToggleSidebar,
    /// Toggle bottom breadcrumbs path bar visibility (`Ctrl+Alt+P` or `Cmd+Alt+P`).
    TogglePathBar,
    /// Undo last operation (`Ctrl+Z`).
    Undo,
    /// Redo last undone operation (`Ctrl+Shift+Z` or `Ctrl+Y`).
    Redo,
    /// Copy selected items (`Ctrl+C`).
    Copy,
    /// Cut selected items (`Ctrl+X`).
    Cut,
    /// Paste items from clipboard (`Ctrl+V`).
    Paste,
    /// Duplicate selected items (`Ctrl+D`).
    Duplicate,
    /// Move selected items to trash (`Delete` or `BackSpace`).
    Trash,
    /// Delete selected items permanently without trash (`Shift+Delete`).
    PermanentDelete,
    /// Toggle folder size calculation preference (`Ctrl+J`).
    ToggleCalculateAllSizes,
    /// Open Get Info dialog (`Ctrl+I` or `Cmd+I`).
    GetInfo,
    /// Open persistent Quick Inspector (`Ctrl+Alt+I` or `Cmd+Alt+I`).
    QuickInspector,
}

/// Maps keyboard shortcut combinations to corresponding [`WindowKeyAction`] variants.
#[must_use]
pub fn map_key_to_window_action(
    keyval: gdk4::Key,
    state: gdk4::ModifierType,
) -> Option<WindowKeyAction> {
    let alt = state.contains(gdk4::ModifierType::ALT_MASK);
    let ctrl = state.contains(gdk4::ModifierType::CONTROL_MASK);
    let shift = state.contains(gdk4::ModifierType::SHIFT_MASK);

    if alt && !ctrl && !shift {
        if keyval == gdk4::Key::Left {
            return Some(WindowKeyAction::HistoryBack);
        }
        if keyval == gdk4::Key::Right {
            return Some(WindowKeyAction::HistoryForward);
        }
    }

    if ctrl && shift && !alt && (keyval == gdk4::Key::z || keyval == gdk4::Key::Z) {
        return Some(WindowKeyAction::Redo);
    }

    if shift && !ctrl && !alt && (keyval == gdk4::Key::Delete || keyval == gdk4::Key::KP_Delete) {
        return Some(WindowKeyAction::PermanentDelete);
    }

    if ctrl && !alt && !shift {
        if keyval == gdk4::Key::_1 || keyval == gdk4::Key::KP_1 {
            return Some(WindowKeyAction::SwitchView(ViewMode::Icons));
        }
        if keyval == gdk4::Key::_2 || keyval == gdk4::Key::KP_2 {
            return Some(WindowKeyAction::SwitchView(ViewMode::List));
        }
        if keyval == gdk4::Key::_3 || keyval == gdk4::Key::KP_3 {
            return Some(WindowKeyAction::SwitchView(ViewMode::Columns));
        }
        if keyval == gdk4::Key::f || keyval == gdk4::Key::F {
            return Some(WindowKeyAction::FocusSearch);
        }
        if keyval == gdk4::Key::z || keyval == gdk4::Key::Z {
            return Some(WindowKeyAction::Undo);
        }
        if keyval == gdk4::Key::y || keyval == gdk4::Key::Y {
            return Some(WindowKeyAction::Redo);
        }
        if keyval == gdk4::Key::c || keyval == gdk4::Key::C {
            return Some(WindowKeyAction::Copy);
        }
        if keyval == gdk4::Key::x || keyval == gdk4::Key::X {
            return Some(WindowKeyAction::Cut);
        }
        if keyval == gdk4::Key::v || keyval == gdk4::Key::V {
            return Some(WindowKeyAction::Paste);
        }
        if keyval == gdk4::Key::d || keyval == gdk4::Key::D {
            return Some(WindowKeyAction::Duplicate);
        }
        if keyval == gdk4::Key::j || keyval == gdk4::Key::J {
            return Some(WindowKeyAction::ToggleCalculateAllSizes);
        }
        if keyval == gdk4::Key::i || keyval == gdk4::Key::I {
            return Some(WindowKeyAction::GetInfo);
        }
    }

    if !ctrl
        && !alt
        && !shift
        && (keyval == gdk4::Key::Delete
            || keyval == gdk4::Key::KP_Delete
            || keyval == gdk4::Key::BackSpace)
    {
        return Some(WindowKeyAction::Trash);
    }

    let super_key = state.contains(gdk4::ModifierType::SUPER_MASK)
        || state.contains(gdk4::ModifierType::META_MASK);

    if (ctrl || super_key) && alt {
        if keyval == gdk4::Key::s || keyval == gdk4::Key::S {
            return Some(WindowKeyAction::ToggleSidebar);
        }
        if keyval == gdk4::Key::p || keyval == gdk4::Key::P {
            return Some(WindowKeyAction::TogglePathBar);
        }
        if keyval == gdk4::Key::i || keyval == gdk4::Key::I {
            return Some(WindowKeyAction::QuickInspector);
        }
    }

    if super_key && !ctrl && !alt && !shift && (keyval == gdk4::Key::i || keyval == gdk4::Key::I) {
        return Some(WindowKeyAction::GetInfo);
    }

    None
}

/// Primary Finder window shell wrapping `adw::ApplicationWindow`.
#[derive(Debug, Clone)]
pub struct FinderWindow {
    /// Top-level libadwaita application window.
    pub window: libadwaita::ApplicationWindow,
    /// 52px unified header bar component.
    pub header_bar: FinderHeaderBar,
    /// Central horizontal split paned dividing sidebar and main content.
    pub paned: gtk4::Paned,
    /// Left sidebar navigation widget.
    pub sidebar: SidebarWidget,
    /// Left sidebar container compartment.
    pub sidebar_box: gtk4::Box,
    /// Main directory view compartment.
    pub main_view_box: gtk4::Box,
    /// View page switcher stack (Icons, List, Columns).
    pub stack: gtk4::Stack,
    /// Cascading Miller columns container.
    pub column_container: ColumnContainer,
    /// Hierarchical tabular list view widget.
    pub list_view: HierarchicalListViewWidget,
    /// Bottom breadcrumbs path bar widget (28px height).
    pub path_bar: PathBarWidget,
    /// Bottom breadcrumbs dock container (28px height).
    pub breadcrumbs_box: gtk4::Box,
    /// Label displaying active path breadcrumbs.
    pub breadcrumbs_label: gtk4::Label,
    /// Central navigation coordinator synchronizing sidebar, views, header bar, and path bar.
    pub coordinator: Rc<NavigationCoordinator>,
    /// Navigation history stack tracking forward/backward traversal.
    pub history: Rc<RefCell<NavigationHistory>>,
    /// Current active view mode.
    pub view_mode: Rc<RefCell<ViewMode>>,
    /// Floating operations queue popover attached to the header bar queue button.
    pub queue_popover: OperationsQueuePopover,
    /// Active floating transfer progress dialogs keyed by operation id.
    pub active_dialogs: Rc<RefCell<HashMap<String, TransferProgressDialog>>>,
}

struct LayoutCompartments {
    paned: gtk4::Paned,
    sidebar: SidebarWidget,
    sidebar_box: gtk4::Box,
    main_view_box: gtk4::Box,
    stack: gtk4::Stack,
    column_container: ColumnContainer,
    list_view: HierarchicalListViewWidget,
    path_bar: PathBarWidget,
    breadcrumbs_box: gtk4::Box,
    breadcrumbs_label: gtk4::Label,
}

impl FinderWindow {
    /// Constructs a new [`FinderWindow`] with default home path.
    #[must_use]
    pub fn new(app: Option<&gtk4::Application>) -> Self {
        let initial_path = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
        Self::with_initial_path(app, initial_path)
    }

    /// Constructs a new [`FinderWindow`] with a specific initial directory path.
    #[must_use]
    pub fn with_initial_path(
        app: Option<&gtk4::Application>,
        initial_path: impl AsRef<Path>,
    ) -> Self {
        let coordinator = Rc::new(NavigationCoordinator::new(initial_path.as_ref()));
        Self::with_coordinator(app, coordinator)
    }

    /// Constructs a new [`FinderWindow`] with a pre-configured [`NavigationCoordinator`].
    #[must_use]
    pub fn with_coordinator(
        app: Option<&gtk4::Application>,
        coordinator: Rc<NavigationCoordinator>,
    ) -> Self {
        install_finder_theme();
        let initial_path_buf = coordinator.current_path();

        let window = Self::build_window(app);
        let header_bar = FinderHeaderBar::new();

        let layout = Self::build_layout_compartments(&initial_path_buf);
        layout.column_container.set_coordinator(&coordinator);
        layout.list_view.set_coordinator(&coordinator);

        let root_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(0)
            .build();
        root_box.append(&header_bar.container);
        root_box.append(&layout.paned);
        window.set_content(Some(&root_box));

        let history = Rc::new(RefCell::new(NavigationHistory::new(&initial_path_buf)));
        let view_mode = Rc::new(RefCell::new(coordinator.view_mode()));

        let queue_popover =
            OperationsQueuePopover::attach(&header_bar.queue_btn, &coordinator.operations_queue);
        let active_dialogs = Rc::new(RefCell::new(HashMap::new()));

        let finder_win = Self {
            window,
            header_bar,
            paned: layout.paned,
            sidebar: layout.sidebar,
            sidebar_box: layout.sidebar_box,
            main_view_box: layout.main_view_box,
            stack: layout.stack,
            column_container: layout.column_container,
            list_view: layout.list_view,
            path_bar: layout.path_bar,
            breadcrumbs_box: layout.breadcrumbs_box,
            breadcrumbs_label: layout.breadcrumbs_label,
            coordinator,
            history,
            view_mode,
            queue_popover,
            active_dialogs,
        };

        finder_win.sync_path_ui(&initial_path_buf);
        finder_win.wire_coordinator();
        finder_win.wire_signals();
        finder_win.wire_keyboard_shortcuts();
        finder_win.wire_operations_ui();

        finder_win
    }

    fn build_window(app: Option<&gtk4::Application>) -> libadwaita::ApplicationWindow {
        let mut builder = libadwaita::ApplicationWindow::builder()
            .title("Finder")
            .default_width(1080)
            .default_height(680)
            .css_classes(["finder-window"]);

        if let Some(app) = app {
            builder = builder.application(app);
        }

        let window = builder.build();
        window.set_size_request(720, 480);

        let style_manager = libadwaita::StyleManager::default();
        apply_theme_classes(&window, style_manager.is_dark());

        let win_weak = window.downgrade();
        style_manager.connect_dark_notify(move |mgr| {
            if let Some(win) = win_weak.upgrade() {
                apply_theme_classes(&win, mgr.is_dark());
            }
        });

        window
    }

    fn build_layout_compartments(initial_path: &Path) -> LayoutCompartments {
        let cfg = SidebarConfig::load();

        let paned = gtk4::Paned::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .vexpand(true)
            .position(cfg.width)
            .shrink_start_child(false)
            .resize_start_child(false)
            .shrink_end_child(false)
            .resize_end_child(true)
            .build();

        let sidebar = SidebarWidget::new();

        let sidebar_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .width_request(cfg.width)
            .visible(cfg.is_visible)
            .css_classes(["finder-sidebar"])
            .build();
        sidebar_box.append(&sidebar.container);
        paned.set_start_child(Some(&sidebar_box));

        paned.connect_position_notify(|p| {
            let pos = p.position();
            if pos >= 140 {
                let mut c = SidebarConfig::load();
                c.width = pos;
                let _ = c.save();
            }
        });

        let main_view_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .build();

        let (stack, column_container, list_view) = Self::build_view_stack(initial_path);
        main_view_box.append(&stack);

        let path_bar = PathBarWidget::new();
        let prefs = FinderPreferences::load();
        path_bar.container.set_visible(prefs.pathbar_visible);
        path_bar.set_path(initial_path, None);
        main_view_box.append(&path_bar.container);

        let breadcrumbs_box = path_bar.container.clone();
        let breadcrumbs_label = gtk4::Label::builder()
            .label(initial_path.to_string_lossy().as_ref())
            .halign(gtk4::Align::Start)
            .css_classes(["breadcrumbs-label"])
            .build();

        paned.set_end_child(Some(&main_view_box));
        LayoutCompartments {
            paned,
            sidebar,
            sidebar_box,
            main_view_box,
            stack,
            column_container,
            list_view,
            path_bar,
            breadcrumbs_box,
            breadcrumbs_label,
        }
    }

    fn build_view_stack(
        initial_path: &Path,
    ) -> (gtk4::Stack, ColumnContainer, HierarchicalListViewWidget) {
        let stack = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .hexpand(true)
            .vexpand(true)
            .build();

        let icons_page = Self::build_placeholder_page("Icons View (Placeholder)");
        let list_view = HierarchicalListViewWidget::new(initial_path);
        let column_container = ColumnContainer::new(initial_path);

        stack.add_named(&icons_page, Some(ViewMode::Icons.as_str()));
        stack.add_named(&list_view.container, Some(ViewMode::List.as_str()));
        stack.add_named(
            &column_container.scrolled_window,
            Some(ViewMode::Columns.as_str()),
        );
        stack.set_visible_child_name(ViewMode::Columns.as_str());
        (stack, column_container, list_view)
    }

    fn build_placeholder_page(text: &'static str) -> gtk4::Box {
        let page = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .valign(gtk4::Align::Center)
            .halign(gtk4::Align::Center)
            .build();
        page.append(&gtk4::Label::new(Some(text)));
        page
    }

    /// Presents the Finder window.
    pub fn present(&self) {
        self.window.present();
    }

    /// Handles an abstract [`WindowKeyAction`].
    pub fn handle_action(&self, action: WindowKeyAction) {
        match action {
            WindowKeyAction::HistoryBack => self.go_back(),
            WindowKeyAction::HistoryForward => self.go_forward(),
            WindowKeyAction::SwitchView(mode) => self.set_view_mode(mode),
            WindowKeyAction::FocusSearch => {
                self.header_bar.search_entry.grab_focus();
            }
            WindowKeyAction::ToggleSidebar => {
                self.toggle_sidebar();
            }
            WindowKeyAction::TogglePathBar => {
                self.toggle_path_bar();
            }
            WindowKeyAction::Undo => {
                let _ = self.coordinator.undo();
            }
            WindowKeyAction::Redo => {
                let _ = self.coordinator.redo();
            }
            WindowKeyAction::Copy => {
                self.coordinator.copy_selection();
            }
            WindowKeyAction::Cut => {
                self.coordinator.cut_selection();
            }
            WindowKeyAction::Paste => {
                self.coordinator.paste_to_current_directory();
            }
            WindowKeyAction::Duplicate => {
                self.coordinator.duplicate_selection();
            }
            WindowKeyAction::Trash => {
                self.coordinator.trash_selection();
            }
            WindowKeyAction::PermanentDelete => {
                self.coordinator.delete_selection_permanently(true);
            }
            WindowKeyAction::ToggleCalculateAllSizes => {
                self.toggle_calculate_all_sizes();
            }
            WindowKeyAction::GetInfo => {
                let _ = self.open_get_info();
            }
            WindowKeyAction::QuickInspector => {
                let _ = self.open_quick_inspector();
            }
        }
    }

    /// Opens the modal Get Info dialog for the active selection.
    #[must_use]
    pub fn open_get_info(&self) -> GetInfoDialog {
        let paths = self
            .coordinator
            .selection()
            .map_or_else(|| vec![self.coordinator.current_path()], |sel| vec![sel]);
        GetInfoDialog::open_for_paths(Some(&self.window), paths)
    }

    /// Opens or updates the floating persistent inspector.
    #[must_use]
    pub fn open_quick_inspector(&self) -> GetInfoDialog {
        GetInfoDialog::open_persistent(Some(&self.window), Some(Rc::clone(&self.coordinator)))
    }

    /// Toggles the sidebar visibility and persists the state.
    pub fn toggle_sidebar(&self) {
        let is_visible = self.sidebar_box.is_visible();
        let new_visible = !is_visible;
        self.sidebar_box.set_visible(new_visible);

        let mut cfg = SidebarConfig::load();
        cfg.is_visible = new_visible;
        let _ = cfg.save();
    }

    /// Toggles the bottom path bar visibility and persists the preference.
    pub fn toggle_path_bar(&self) {
        let is_visible = self.path_bar.container.is_visible();
        let new_visible = !is_visible;
        self.path_bar.container.set_visible(new_visible);

        let mut prefs = FinderPreferences::load();
        prefs.pathbar_visible = new_visible;
        let _ = prefs.save();
    }

    /// Toggles the folder size calculation preference and refreshes the directory view.
    pub fn toggle_calculate_all_sizes(&self) {
        let mut prefs = FinderPreferences::load();
        prefs.calculate_all_sizes = !prefs.calculate_all_sizes;
        let _ = prefs.save();
        let curr = self.history.borrow().current_path().to_path_buf();
        if !crate::ui::tagging::sidebar_filter::is_tag_virtual_path(&curr) {
            self.list_view.navigate_root(&curr);
        }
    }

    /// Handles filtering by tag emitted from the sidebar.
    pub fn handle_tag_filter(&self, event: TagFilterEvent) {
        match event {
            TagFilterEvent::Selected { tag_name, color: _ } => {
                let virtual_path =
                    crate::ui::tagging::sidebar_filter::make_tag_virtual_path(&tag_name);
                self.navigate_to(&virtual_path);
            }
            TagFilterEvent::Cleared => {
                let curr = self.history.borrow().current_path().to_path_buf();
                if crate::ui::tagging::sidebar_filter::is_tag_virtual_path(&curr) {
                    if let Ok(home) = std::env::var("HOME") {
                        self.navigate_to(Path::new(&home));
                    }
                } else {
                    self.sync_path_ui(&curr);
                }
            }
        }
    }

    /// Navigates to a new directory path.
    pub fn navigate_to(&self, target: impl AsRef<Path>) {
        let target_buf = target.as_ref().to_path_buf();
        self.history.borrow_mut().navigate_to(&target_buf);
        self.coordinator.navigate_to(&target_buf);
        self.apply_navigation(&target_buf);
    }

    /// Navigates back one step in history.
    pub fn go_back(&self) {
        if let Some(target) = self.coordinator.go_back() {
            self.history.borrow_mut().navigate_to(&target);
            self.apply_navigation(&target);
        }
    }

    /// Navigates forward one step in history.
    pub fn go_forward(&self) {
        if let Some(target) = self.coordinator.go_forward() {
            self.history.borrow_mut().navigate_to(&target);
            self.apply_navigation(&target);
        }
    }

    fn apply_navigation(&self, target: &Path) {
        if crate::ui::tagging::sidebar_filter::is_tag_virtual_path(target)
            && let Some(tag_name) =
                crate::ui::tagging::sidebar_filter::parse_tag_virtual_path(target)
        {
            let items = crate::ui::tagging::sidebar_filter::query_tagged_files(&tag_name);
            self.column_container
                .navigate_virtual_tag(target, items.clone());
            self.list_view.set_custom_items(target, items);
            self.header_bar
                .set_title(&format!("Tag: {tag_name}"), Some("tag-symbolic"));
            self.breadcrumbs_label
                .set_text(&format!("Tags › {tag_name}"));
            self.path_bar.set_tag_filter(&tag_name);
            let can_back = self.coordinator.can_go_back();
            let can_forward = self.coordinator.can_go_forward();
            self.header_bar
                .update_history_buttons(can_back, can_forward);
            return;
        }
        self.column_container.navigate_root(target);
        self.list_view.navigate_root(target);
    }

    /// Returns the currently active view mode.
    #[must_use]
    pub fn view_mode(&self) -> ViewMode {
        self.coordinator.view_mode()
    }

    /// Switches the active view mode (Icons, List, Columns).
    pub fn set_view_mode(&self, mode: ViewMode) {
        self.coordinator.set_view_mode(mode);
        *self.view_mode.borrow_mut() = mode;
        self.stack.set_visible_child_name(mode.as_str());
        self.header_bar.set_view_mode(mode);

        let curr = self.coordinator.current_path();
        if crate::ui::tagging::sidebar_filter::is_tag_virtual_path(&curr) {
            if let Some(tag_name) =
                crate::ui::tagging::sidebar_filter::parse_tag_virtual_path(&curr)
            {
                let items = crate::ui::tagging::sidebar_filter::query_tagged_files(&tag_name);
                match mode {
                    ViewMode::Columns => {
                        self.column_container.navigate_virtual_tag(&curr, items);
                    }
                    ViewMode::List => {
                        self.list_view.set_custom_items(&curr, items);
                    }
                    ViewMode::Icons => {}
                }
            }
        } else {
            match mode {
                ViewMode::Columns => {
                    self.column_container.navigate_root(&curr);
                }
                ViewMode::List => {
                    self.list_view.navigate_root(&curr);
                }
                ViewMode::Icons => {}
            }
        }
    }

    /// Synchronizes header bar title, history buttons, and breadcrumb path.
    fn sync_path_ui(&self, path: &Path) {
        if crate::ui::tagging::sidebar_filter::is_tag_virtual_path(path) {
            let tag_name = crate::ui::tagging::sidebar_filter::parse_tag_virtual_path(path)
                .unwrap_or_default();
            self.header_bar
                .set_title(&format!("Tag: {tag_name}"), Some("tag-symbolic"));
            self.breadcrumbs_label
                .set_text(&format!("Tags › {tag_name}"));
            self.path_bar.set_tag_filter(&tag_name);
            let can_back = self.coordinator.can_go_back();
            let can_forward = self.coordinator.can_go_forward();
            self.header_bar
                .update_history_buttons(can_back, can_forward);
            return;
        }

        let title = path.file_name().and_then(|n| n.to_str()).unwrap_or("/");
        self.header_bar.set_title(title, Some("folder-symbolic"));
        self.breadcrumbs_label
            .set_text(path.to_string_lossy().as_ref());
        self.path_bar
            .set_path(path, self.coordinator.selection().as_deref());

        let can_back = self.coordinator.can_go_back();
        let can_forward = self.coordinator.can_go_forward();
        self.header_bar
            .update_history_buttons(can_back, can_forward);
        self.sidebar.set_active_path(path);
    }

    fn wire_coordinator(&self) {
        let win = self.clone();
        self.coordinator.connect_path_changed(move |path| {
            win.sync_path_ui(path);
        });

        let win = self.clone();
        self.coordinator
            .connect_selection_changed(move |selection| {
                let curr = win.coordinator.current_path();
                win.path_bar.set_path(&curr, selection);
            });
    }

    /// Wires header bar button interactions to window handlers.
    fn wire_signals(&self) {
        let win = self.clone();
        self.header_bar.connect_back(move || {
            win.go_back();
        });

        let win = self.clone();
        self.header_bar.connect_forward(move || {
            win.go_forward();
        });

        let win = self.clone();
        self.header_bar.connect_view_mode_changed(move |mode| {
            win.set_view_mode(mode);
        });

        let win = self.clone();
        self.sidebar.connect_navigate(move |path| {
            win.navigate_to(path);
        });

        let win = self.clone();
        self.sidebar.connect_tag_filter(move |event| {
            win.handle_tag_filter(event);
        });

        let win = self.clone();
        self.column_container.connect_path_changed(move |path| {
            if path.is_dir() {
                win.coordinator.navigate_to(path);
            } else {
                win.coordinator.set_selection(Some(path));
            }
        });

        let win = self.clone();
        self.list_view.connect_path_changed(move |path| {
            if path.is_dir() {
                win.coordinator.navigate_to(path);
            } else {
                win.coordinator.set_selection(Some(path));
            }
        });

        let win = self.clone();
        self.path_bar.connect_navigate(move |path| {
            win.navigate_to(path);
        });

        self.path_bar.connect_open_new_window(|path| {
            let new_win = Self::with_initial_path(None, path);
            new_win.present();
        });
    }

    /// Binds global keyboard shortcuts on the window via [`gtk4::EventControllerKey`].
    fn wire_keyboard_shortcuts(&self) {
        let controller = gtk4::EventControllerKey::new();
        let win = self.clone();

        controller.connect_key_pressed(move |_ctrl, keyval, _code, state| {
            map_key_to_window_action(keyval, state).map_or(glib::Propagation::Proceed, |action| {
                win.handle_action(action);
                glib::Propagation::Stop
            })
        });

        self.window.add_controller(controller);
    }

    /// Wires operations progress events to update badge and floating transfer dialogs.
    fn wire_operations_ui(&self) {
        let events = self.coordinator.operations_queue.subscribe_events();
        let queue = Arc::clone(&self.coordinator.operations_queue);
        let active_dialogs = Rc::clone(&self.active_dialogs);
        let win_weak = self.window.downgrade();

        glib::spawn_future_local(async move {
            while let Ok(event) = events.recv().await {
                let prefs = FinderPreferences::load();
                if !prefs.show_transfer_dialog {
                    continue;
                }

                match event {
                    crate::operations::OperationEvent::PreflightCompleted {
                        operation_id,
                        manifest,
                    } => {
                        let active_tasks = queue.active_tasks();
                        let task = active_tasks
                            .into_iter()
                            .find(|t| t.operation_id == operation_id);
                        let kind = task
                            .as_ref()
                            .map_or(shared::models::operation::OperationKind::Copy, |t| t.kind);
                        let dest_name = task
                            .as_ref()
                            .and_then(|t| t.operation.destination.as_ref())
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str());

                        let dialog = TransferProgressDialog::new(
                            &operation_id,
                            kind,
                            manifest.total_items,
                            dest_name,
                        );
                        dialog.connect_queue((*queue).clone());
                        if let Some(parent_win) = win_weak.upgrade() {
                            dialog.window.set_transient_for(Some(&parent_win));
                        }
                        dialog.window.present();
                        active_dialogs.borrow_mut().insert(operation_id, dialog);
                    }
                    crate::operations::OperationEvent::Progress(report) => {
                        let dialogs = active_dialogs.borrow();
                        if let Some(dialog) = dialogs.get(&report.operation_id) {
                            dialog.update_progress(&report);
                        }
                    }
                    crate::operations::OperationEvent::Paused { operation_id } => {
                        let dialogs = active_dialogs.borrow();
                        if let Some(dialog) = dialogs.get(&operation_id) {
                            dialog.set_paused(true);
                        }
                    }
                    crate::operations::OperationEvent::Resumed { operation_id } => {
                        let dialogs = active_dialogs.borrow();
                        if let Some(dialog) = dialogs.get(&operation_id) {
                            dialog.set_paused(false);
                        }
                    }
                    crate::operations::OperationEvent::Completed { operation_id } => {
                        if let Some(dialog) = active_dialogs.borrow_mut().remove(&operation_id) {
                            dialog.mark_completed();
                        }
                    }
                    crate::operations::OperationEvent::Failed {
                        operation_id,
                        error,
                    } => {
                        if let Some(dialog) = active_dialogs.borrow_mut().remove(&operation_id) {
                            dialog.mark_failed(&error);
                        }
                    }
                    crate::operations::OperationEvent::Cancelled { operation_id } => {
                        if let Some(dialog) = active_dialogs.borrow_mut().remove(&operation_id) {
                            dialog.window.close();
                        }
                    }
                    _ => {}
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_key_to_window_action_navigation() {
        // Alt + Left -> HistoryBack
        assert_eq!(
            map_key_to_window_action(gdk4::Key::Left, gdk4::ModifierType::ALT_MASK),
            Some(WindowKeyAction::HistoryBack)
        );

        // Alt + Right -> HistoryForward
        assert_eq!(
            map_key_to_window_action(gdk4::Key::Right, gdk4::ModifierType::ALT_MASK),
            Some(WindowKeyAction::HistoryForward)
        );

        // View modes (Ctrl+1, Ctrl+2, Ctrl+3)
        assert_eq!(
            map_key_to_window_action(gdk4::Key::_1, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::SwitchView(ViewMode::Icons))
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::_2, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::SwitchView(ViewMode::List))
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::_3, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::SwitchView(ViewMode::Columns))
        );

        // Focus Search (Ctrl+F, Ctrl+f)
        assert_eq!(
            map_key_to_window_action(gdk4::Key::f, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::FocusSearch)
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::F, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::FocusSearch)
        );

        // Toggle Sidebar (Ctrl+Alt+S, Cmd+Alt+S)
        assert_eq!(
            map_key_to_window_action(
                gdk4::Key::s,
                gdk4::ModifierType::CONTROL_MASK | gdk4::ModifierType::ALT_MASK
            ),
            Some(WindowKeyAction::ToggleSidebar)
        );
        assert_eq!(
            map_key_to_window_action(
                gdk4::Key::S,
                gdk4::ModifierType::SUPER_MASK | gdk4::ModifierType::ALT_MASK
            ),
            Some(WindowKeyAction::ToggleSidebar)
        );

        // Toggle Path Bar (Ctrl+Alt+P, Cmd+Alt+P)
        assert_eq!(
            map_key_to_window_action(
                gdk4::Key::p,
                gdk4::ModifierType::CONTROL_MASK | gdk4::ModifierType::ALT_MASK
            ),
            Some(WindowKeyAction::TogglePathBar)
        );
        assert_eq!(
            map_key_to_window_action(
                gdk4::Key::P,
                gdk4::ModifierType::SUPER_MASK | gdk4::ModifierType::ALT_MASK
            ),
            Some(WindowKeyAction::TogglePathBar)
        );

        // Unhandled key
        assert_eq!(
            map_key_to_window_action(gdk4::Key::space, gdk4::ModifierType::empty()),
            None
        );
    }

    #[test]
    fn test_map_key_to_window_action_file_operations() {
        // Undo (Ctrl+Z)
        assert_eq!(
            map_key_to_window_action(gdk4::Key::z, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::Undo)
        );

        // Redo (Ctrl+Shift+Z, Ctrl+Y)
        assert_eq!(
            map_key_to_window_action(
                gdk4::Key::z,
                gdk4::ModifierType::CONTROL_MASK | gdk4::ModifierType::SHIFT_MASK
            ),
            Some(WindowKeyAction::Redo)
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::y, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::Redo)
        );

        // Copy (Ctrl+C), Cut (Ctrl+X), Paste (Ctrl+V), Duplicate (Ctrl+D)
        assert_eq!(
            map_key_to_window_action(gdk4::Key::c, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::Copy)
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::x, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::Cut)
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::v, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::Paste)
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::d, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::Duplicate)
        );

        // Trash (Delete, BackSpace)
        assert_eq!(
            map_key_to_window_action(gdk4::Key::Delete, gdk4::ModifierType::empty()),
            Some(WindowKeyAction::Trash)
        );
        assert_eq!(
            map_key_to_window_action(gdk4::Key::BackSpace, gdk4::ModifierType::empty()),
            Some(WindowKeyAction::Trash)
        );

        // Permanent Delete (Shift+Delete)
        assert_eq!(
            map_key_to_window_action(gdk4::Key::Delete, gdk4::ModifierType::SHIFT_MASK),
            Some(WindowKeyAction::PermanentDelete)
        );

        // Toggle Calculate All Sizes (Ctrl+J)
        assert_eq!(
            map_key_to_window_action(gdk4::Key::j, gdk4::ModifierType::CONTROL_MASK),
            Some(WindowKeyAction::ToggleCalculateAllSizes)
        );
    }
}
