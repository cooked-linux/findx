//! Scope bar GTK4 widget providing live search scope toggling, mode selection, and status feedback.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;

use super::scope::ScopeBarState;
use super::types::{SearchMode, SearchScope};

type ScopeChangedCallback = Box<dyn Fn(SearchScope)>;
type ModeChangedCallback = Box<dyn Fn(SearchMode)>;
type SystemFilesChangedCallback = Box<dyn Fn(bool)>;
type ClearClickedCallback = Box<dyn Fn()>;

/// Interactive GTK4 scope bar widget mounted directly beneath the header bar.
#[derive(Clone)]
pub struct ScopeBarWidget {
    /// Root revealer supporting smooth height transition.
    pub revealer: gtk4::Revealer,
    /// Inner horizontal container box styled with CSS class `finder-scope-bar`.
    pub container: gtk4::Box,
    /// Button selecting system-wide search scope.
    pub this_pc_btn: gtk4::Button,
    /// Button selecting current directory search scope.
    pub current_folder_btn: gtk4::Button,
    /// Button selecting filename matching mode.
    pub name_matches_btn: gtk4::Button,
    /// Button selecting deep content search mode.
    pub content_contains_btn: gtk4::Button,
    /// Checkbox toggling hidden / system files inclusion.
    pub system_files_check: gtk4::CheckButton,
    /// Label displaying active result counts or searching state.
    pub status_label: gtk4::Label,
    /// Close / Clear button dismissing search.
    pub clear_btn: gtk4::Button,
    state: Rc<RefCell<ScopeBarState>>,
    on_scope_changed: Rc<RefCell<Option<ScopeChangedCallback>>>,
    on_mode_changed: Rc<RefCell<Option<ModeChangedCallback>>>,
    on_system_files_changed: Rc<RefCell<Option<SystemFilesChangedCallback>>>,
    on_clear_clicked: Rc<RefCell<Option<ClearClickedCallback>>>,
}

impl ScopeBarWidget {
    /// Constructs a new [`ScopeBarWidget`] for the specified initial folder.
    #[must_use]
    pub fn new(initial_folder: PathBuf) -> Self {
        let state = Rc::new(RefCell::new(ScopeBarState::new(initial_folder)));

        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .margin_top(4)
            .margin_bottom(4)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["finder-scope-bar", "toolbar"])
            .build();

        let scope_lbl = gtk4::Label::builder()
            .label("Search:")
            .css_classes(["caption", "dim-label"])
            .build();
        container.append(&scope_lbl);

        // Segmented scope switcher: [Current Folder] | [This PC]
        let scope_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .css_classes(["linked"])
            .build();

        let folder_name = state.borrow().folder_name.clone();
        let current_folder_btn = gtk4::Button::builder()
            .label(format!("\"{folder_name}\""))
            .css_classes(["active"])
            .build();

        let this_pc_btn = gtk4::Button::builder().label("This PC").build();

        scope_box.append(&current_folder_btn);
        scope_box.append(&this_pc_btn);
        container.append(&scope_box);

        let sep1 = gtk4::Separator::new(gtk4::Orientation::Vertical);
        sep1.set_margin_start(4);
        sep1.set_margin_end(4);
        container.append(&sep1);

        // Segmented mode switcher: [Name Matches] | [Content Contains]
        let mode_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .css_classes(["linked"])
            .build();

        let name_matches_btn = gtk4::Button::builder()
            .label("Name Matches")
            .css_classes(["active"])
            .build();

        let content_contains_btn = gtk4::Button::builder().label("Content Contains").build();

        mode_box.append(&name_matches_btn);
        mode_box.append(&content_contains_btn);
        container.append(&mode_box);

        let sep2 = gtk4::Separator::new(gtk4::Orientation::Vertical);
        sep2.set_margin_start(4);
        sep2.set_margin_end(4);
        container.append(&sep2);

        // System files check button
        let system_files_check = gtk4::CheckButton::builder()
            .label("Include system files")
            .active(false)
            .build();
        container.append(&system_files_check);

        // Status label (results count)
        let status_label = gtk4::Label::builder()
            .hexpand(true)
            .halign(gtk4::Align::End)
            .css_classes(["caption", "dim-label"])
            .build();
        container.append(&status_label);

        // Clear / Close button
        let clear_btn = gtk4::Button::builder()
            .label("✕")
            .tooltip_text("Clear search (Esc)")
            .css_classes(["flat", "circular"])
            .halign(gtk4::Align::End)
            .valign(gtk4::Align::Center)
            .build();
        container.append(&clear_btn);

        let revealer = gtk4::Revealer::builder()
            .transition_type(gtk4::RevealerTransitionType::SlideDown)
            .reveal_child(false)
            .child(&container)
            .build();

        let widget = Self {
            revealer,
            container,
            this_pc_btn,
            current_folder_btn,
            name_matches_btn,
            content_contains_btn,
            system_files_check,
            status_label,
            clear_btn,
            state,
            on_scope_changed: Rc::new(RefCell::new(None)),
            on_mode_changed: Rc::new(RefCell::new(None)),
            on_system_files_changed: Rc::new(RefCell::new(None)),
            on_clear_clicked: Rc::new(RefCell::new(None)),
        };

        widget.wire_internal_buttons();
        widget
    }

    fn wire_internal_buttons(&self) {
        let cur_folder_btn = self.current_folder_btn.clone();
        let pc_btn = self.this_pc_btn.clone();
        let state_scope = Rc::clone(&self.state);
        let on_scope_cb = Rc::clone(&self.on_scope_changed);

        let cur_folder_ref1 = cur_folder_btn.clone();
        let pc_ref1 = pc_btn.clone();
        let state_scope1 = Rc::clone(&state_scope);
        let scope_cb1 = Rc::clone(&on_scope_cb);
        cur_folder_btn.connect_clicked(move |_| {
            cur_folder_ref1.add_css_class("active");
            pc_ref1.remove_css_class("active");
            state_scope1.borrow_mut().scope = SearchScope::CurrentFolder;
            if let Some(ref cb) = *scope_cb1.borrow() {
                cb(SearchScope::CurrentFolder);
            }
        });

        let cur_folder_ref2 = cur_folder_btn;
        let pc_ref2 = pc_btn.clone();
        let state_scope2 = state_scope;
        let scope_cb2 = on_scope_cb;
        pc_btn.connect_clicked(move |_| {
            pc_ref2.add_css_class("active");
            cur_folder_ref2.remove_css_class("active");
            state_scope2.borrow_mut().scope = SearchScope::ThisPc;
            if let Some(ref cb) = *scope_cb2.borrow() {
                cb(SearchScope::ThisPc);
            }
        });

        let name_btn = self.name_matches_btn.clone();
        let content_btn = self.content_contains_btn.clone();
        let state_mode = Rc::clone(&self.state);
        let on_mode_cb = Rc::clone(&self.on_mode_changed);

        let name_ref1 = name_btn.clone();
        let cnt_ref1 = content_btn.clone();
        let state_mode1 = Rc::clone(&state_mode);
        let mode_cb1 = Rc::clone(&on_mode_cb);
        name_btn.connect_clicked(move |_| {
            name_ref1.add_css_class("active");
            cnt_ref1.remove_css_class("active");
            state_mode1.borrow_mut().mode = SearchMode::NameMatches;
            if let Some(ref cb) = *mode_cb1.borrow() {
                cb(SearchMode::NameMatches);
            }
        });

        let name_ref2 = name_btn;
        let cnt_ref2 = content_btn;
        let state_mode2 = state_mode;
        let mode_cb2 = on_mode_cb;
        cnt_ref2.clone().connect_clicked(move |_| {
            cnt_ref2.add_css_class("active");
            name_ref2.remove_css_class("active");
            state_mode2.borrow_mut().mode = SearchMode::ContentContains;
            if let Some(ref cb) = *mode_cb2.borrow() {
                cb(SearchMode::ContentContains);
            }
        });

        let state_sys = Rc::clone(&self.state);
        let on_sys_cb = Rc::clone(&self.on_system_files_changed);
        self.system_files_check.connect_toggled(move |chk| {
            let active = chk.is_active();
            state_sys.borrow_mut().include_system_files = active;
            if let Some(ref cb) = *on_sys_cb.borrow() {
                cb(active);
            }
        });

        let on_clear = Rc::clone(&self.on_clear_clicked);
        self.clear_btn.connect_clicked(move |_| {
            if let Some(ref cb) = *on_clear.borrow() {
                cb();
            }
        });
    }

    /// Sets whether the scope bar is smoothly revealed or hidden.
    pub fn set_revealed(&self, revealed: bool) {
        self.revealer.set_reveal_child(revealed);
    }

    /// Checks whether the scope bar is currently revealed.
    #[must_use]
    pub fn is_revealed(&self) -> bool {
        self.revealer.reveals_child()
    }

    /// Updates the active folder name displayed on the localized scope button.
    pub fn set_current_folder(&self, path: &Path) {
        self.state.borrow_mut().set_folder(path.to_path_buf());
        let name = &self.state.borrow().folder_name;
        self.current_folder_btn.set_label(&format!("\"{name}\""));
    }

    /// Updates the text shown in the status label.
    pub fn set_status_text(&self, text: &str) {
        self.status_label.set_text(text);
    }

    /// Returns the active [`SearchScope`].
    #[must_use]
    pub fn scope(&self) -> SearchScope {
        self.state.borrow().scope
    }

    /// Returns the active [`SearchMode`].
    #[must_use]
    pub fn mode(&self) -> SearchMode {
        self.state.borrow().mode
    }

    /// Returns whether system/hidden files should be included.
    #[must_use]
    pub fn include_system_files(&self) -> bool {
        self.state.borrow().include_system_files
    }

    /// Connects a handler invoked whenever the search scope changes.
    pub fn connect_scope_changed<F: Fn(SearchScope) + 'static>(&self, callback: F) {
        *self.on_scope_changed.borrow_mut() = Some(Box::new(callback));
    }

    /// Connects a handler invoked whenever the search mode changes.
    pub fn connect_mode_changed<F: Fn(SearchMode) + 'static>(&self, callback: F) {
        *self.on_mode_changed.borrow_mut() = Some(Box::new(callback));
    }

    /// Connects a handler invoked whenever system files inclusion is toggled.
    pub fn connect_system_files_changed<F: Fn(bool) + 'static>(&self, callback: F) {
        *self.on_system_files_changed.borrow_mut() = Some(Box::new(callback));
    }

    /// Connects a handler invoked whenever the clear / close button is clicked.
    pub fn connect_clear_clicked<F: Fn() + 'static>(&self, callback: F) {
        *self.on_clear_clicked.borrow_mut() = Some(Box::new(callback));
    }
}

impl std::fmt::Debug for ScopeBarWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopeBarWidget")
            .field("revealed", &self.is_revealed())
            .field("scope", &self.scope())
            .field("mode", &self.mode())
            .field("include_system_files", &self.include_system_files())
            .finish()
    }
}
