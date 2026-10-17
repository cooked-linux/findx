//! Main Finder application window shell layout and keyboard shortcut dispatch.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::prelude::*;

use super::columns::ColumnContainer;
use super::header_bar::FinderHeaderBar;
use super::list::HierarchicalListViewWidget;
use super::sidebar::SidebarWidget;
use super::theme::{apply_theme_classes, install_finder_theme};
use super::view_mode::ViewMode;
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
}

/// Maps keyboard shortcut combinations to corresponding [`WindowKeyAction`] variants.
#[must_use]
pub fn map_key_to_window_action(
    keyval: gdk4::Key,
    state: gdk4::ModifierType,
) -> Option<WindowKeyAction> {
    let alt = state.contains(gdk4::ModifierType::ALT_MASK);
    let ctrl = state.contains(gdk4::ModifierType::CONTROL_MASK);

    if alt && !ctrl {
        if keyval == gdk4::Key::Left {
            return Some(WindowKeyAction::HistoryBack);
        }
        if keyval == gdk4::Key::Right {
            return Some(WindowKeyAction::HistoryForward);
        }
    }

    if ctrl && !alt {
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
    }

    let super_key = state.contains(gdk4::ModifierType::SUPER_MASK)
        || state.contains(gdk4::ModifierType::META_MASK);

    if (ctrl || super_key) && alt && (keyval == gdk4::Key::s || keyval == gdk4::Key::S) {
        return Some(WindowKeyAction::ToggleSidebar);
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
    /// Bottom breadcrumbs dock container (28px height).
    pub breadcrumbs_box: gtk4::Box,
    /// Label displaying active path breadcrumbs.
    pub breadcrumbs_label: gtk4::Label,
    /// Navigation history stack tracking forward/backward traversal.
    pub history: Rc<RefCell<NavigationHistory>>,
    /// Current active view mode.
    pub view_mode: Rc<RefCell<ViewMode>>,
}

struct LayoutCompartments {
    paned: gtk4::Paned,
    sidebar: SidebarWidget,
    sidebar_box: gtk4::Box,
    main_view_box: gtk4::Box,
    stack: gtk4::Stack,
    column_container: ColumnContainer,
    list_view: HierarchicalListViewWidget,
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
        install_finder_theme();
        let initial_path_buf = initial_path.as_ref().to_path_buf();

        let window = Self::build_window(app);
        let header_bar = FinderHeaderBar::new();

        let layout = Self::build_layout_compartments(&initial_path_buf);

        let root_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(0)
            .build();
        root_box.append(&header_bar.container);
        root_box.append(&layout.paned);
        window.set_content(Some(&root_box));

        let history = Rc::new(RefCell::new(NavigationHistory::new(&initial_path_buf)));
        let view_mode = Rc::new(RefCell::new(ViewMode::Columns));

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
            breadcrumbs_box: layout.breadcrumbs_box,
            breadcrumbs_label: layout.breadcrumbs_label,
            history,
            view_mode,
        };

        finder_win.sync_path_ui(&initial_path_buf);
        finder_win.wire_signals();
        finder_win.wire_keyboard_shortcuts();

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

        let (breadcrumbs_box, breadcrumbs_label) = Self::build_breadcrumbs_dock(initial_path);
        main_view_box.append(&breadcrumbs_box);

        paned.set_end_child(Some(&main_view_box));
        LayoutCompartments {
            paned,
            sidebar,
            sidebar_box,
            main_view_box,
            stack,
            column_container,
            list_view,
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

    fn build_breadcrumbs_dock(initial_path: &Path) -> (gtk4::Box, gtk4::Label) {
        let breadcrumbs_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(6)
            .valign(gtk4::Align::Center)
            .css_classes(["finder-breadcrumbs"])
            .build();

        let breadcrumbs_label = gtk4::Label::builder()
            .label(initial_path.to_string_lossy().as_ref())
            .halign(gtk4::Align::Start)
            .css_classes(["breadcrumbs-label"])
            .build();
        breadcrumbs_box.append(&breadcrumbs_label);
        (breadcrumbs_box, breadcrumbs_label)
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
        }
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

    /// Handles filtering by tag emitted from the sidebar.
    pub fn handle_tag_filter(&self, event: TagFilterEvent) {
        match event {
            TagFilterEvent::Selected { tag_name, color: _ } => {
                self.header_bar
                    .set_title(&format!("Tag: {tag_name}"), Some("tag-symbolic"));
                self.breadcrumbs_label
                    .set_text(&format!("Tags › {tag_name}"));
            }
            TagFilterEvent::Cleared => {
                let curr = self.history.borrow().current_path().to_path_buf();
                self.sync_path_ui(&curr);
            }
        }
    }

    /// Navigates to a new directory path.
    pub fn navigate_to(&self, target: impl AsRef<Path>) {
        let target_buf = target.as_ref().to_path_buf();
        self.history.borrow_mut().navigate_to(&target_buf);
        self.column_container.navigate_root(&target_buf);
        self.list_view.navigate_root(&target_buf);
        self.sync_path_ui(&target_buf);
    }

    /// Navigates back one step in history.
    pub fn go_back(&self) {
        let prev = self.history.borrow_mut().go_back();
        if let Some(target) = prev {
            self.column_container.navigate_root(&target);
            self.list_view.navigate_root(&target);
            self.sync_path_ui(&target);
        }
    }

    /// Navigates forward one step in history.
    pub fn go_forward(&self) {
        let next = self.history.borrow_mut().go_forward();
        if let Some(target) = next {
            self.column_container.navigate_root(&target);
            self.list_view.navigate_root(&target);
            self.sync_path_ui(&target);
        }
    }

    /// Switches the active view mode (Icons, List, Columns).
    pub fn set_view_mode(&self, mode: ViewMode) {
        *self.view_mode.borrow_mut() = mode;
        self.stack.set_visible_child_name(mode.as_str());
        self.header_bar.set_view_mode(mode);
    }

    /// Synchronizes header bar title, history buttons, and breadcrumb path.
    fn sync_path_ui(&self, path: &Path) {
        let title = path.file_name().and_then(|n| n.to_str()).unwrap_or("/");
        self.header_bar.set_title(title, Some("folder-symbolic"));
        self.breadcrumbs_label
            .set_text(path.to_string_lossy().as_ref());

        let (can_back, can_forward) = {
            let hist = self.history.borrow();
            (hist.can_go_back(), hist.can_go_forward())
        };
        self.header_bar
            .update_history_buttons(can_back, can_forward);
        self.sidebar.set_active_path(path);
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
            win.sync_path_ui(path);
        });

        let win = self.clone();
        self.list_view.connect_path_changed(move |path| {
            win.sync_path_ui(path);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_key_to_window_action() {
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

        // Unhandled key
        assert_eq!(
            map_key_to_window_action(gdk4::Key::space, gdk4::ModifierType::empty()),
            None
        );
    }
}
