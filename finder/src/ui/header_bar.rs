//! Unified Finder header bar widget hosting navigation, view switching, and search.

use gtk4::prelude::*;

use super::view_mode::ViewMode;

/// Unified header bar component conforming to the Finder design system.
#[derive(Debug, Clone)]
pub struct FinderHeaderBar {
    /// Top-level horizontal container for the header bar (52px fixed height).
    pub container: gtk4::Box,
    /// History navigate back button.
    pub back_btn: gtk4::Button,
    /// History navigate forward button.
    pub forward_btn: gtk4::Button,
    /// Title container housing directory icon and name.
    pub title_box: gtk4::Box,
    /// Title directory/folder icon.
    pub title_icon: gtk4::Image,
    /// Title directory/folder label.
    pub title_label: gtk4::Label,
    /// Segmented container grouping the 3 view switcher buttons.
    pub view_switcher: gtk4::Box,
    /// Switch to icon grid view button.
    pub icons_btn: gtk4::Button,
    /// Switch to list view button.
    pub list_btn: gtk4::Button,
    /// Switch to column view button.
    pub columns_btn: gtk4::Button,
    /// Toggle group-by arrangement button.
    pub group_btn: gtk4::Button,
    /// Toggle quick look file preview button.
    pub quick_look_btn: gtk4::Button,
    /// Open file inspector / get info button.
    pub get_info_btn: gtk4::Button,
    /// View background file transfer queue button.
    pub queue_btn: gtk4::Button,
    /// Search input field with auto-expansion on focus.
    pub search_entry: gtk4::SearchEntry,
}

impl Default for FinderHeaderBar {
    fn default() -> Self {
        Self::new()
    }
}

impl FinderHeaderBar {
    /// Instantiates and wires a new [`FinderHeaderBar`] widget hierarchy.
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(10)
            .valign(gtk4::Align::Center)
            .css_classes(["finder-headerbar"])
            .build();

        let (nav_box, back_btn, forward_btn) = Self::build_nav_controls();
        container.append(&nav_box);

        let (title_box, title_icon, title_label) = Self::build_title_box();
        container.append(&title_box);

        let (view_switcher, icons_btn, list_btn, columns_btn) = Self::build_view_switcher();
        container.append(&view_switcher);

        let (group_btn, quick_look_btn, get_info_btn, queue_btn) = Self::build_action_buttons();
        container.append(&group_btn);
        container.append(&quick_look_btn);
        container.append(&get_info_btn);
        container.append(&queue_btn);

        let spacer = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .build();
        container.append(&spacer);

        let search_entry = Self::build_search_entry();
        container.append(&search_entry);

        let bar = Self {
            container,
            back_btn,
            forward_btn,
            title_box,
            title_icon,
            title_label,
            view_switcher,
            icons_btn,
            list_btn,
            columns_btn,
            group_btn,
            quick_look_btn,
            get_info_btn,
            queue_btn,
            search_entry,
        };

        bar.wire_internal_view_switcher();
        bar
    }

    fn build_nav_controls() -> (gtk4::Box, gtk4::Button, gtk4::Button) {
        let nav_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(0)
            .css_classes(["nav-group"])
            .build();

        let back_btn = gtk4::Button::builder()
            .label("‹")
            .tooltip_text("Back (Alt+Left)")
            .sensitive(false)
            .css_classes(["nav-btn"])
            .build();

        let forward_btn = gtk4::Button::builder()
            .label("›")
            .tooltip_text("Forward (Alt+Right)")
            .sensitive(false)
            .css_classes(["nav-btn"])
            .build();

        nav_box.append(&back_btn);
        nav_box.append(&forward_btn);
        (nav_box, back_btn, forward_btn)
    }

    fn build_title_box() -> (gtk4::Box, gtk4::Image, gtk4::Label) {
        let title_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk4::Align::Center)
            .build();

        let title_icon = gtk4::Image::builder()
            .icon_name("folder-symbolic")
            .pixel_size(16)
            .build();

        let title_label = gtk4::Label::builder()
            .label("Home")
            .css_classes(["title-label"])
            .build();

        title_box.append(&title_icon);
        title_box.append(&title_label);
        (title_box, title_icon, title_label)
    }

    fn build_view_switcher() -> (gtk4::Box, gtk4::Button, gtk4::Button, gtk4::Button) {
        let view_switcher = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(0)
            .css_classes(["segmented-control"])
            .margin_start(12)
            .build();

        let icons_btn = gtk4::Button::builder()
            .label(ViewMode::Icons.as_str())
            .tooltip_text("Icon View (Ctrl+1)")
            .css_classes(["segmented-btn"])
            .build();

        let list_btn = gtk4::Button::builder()
            .label(ViewMode::List.as_str())
            .tooltip_text("List View (Ctrl+2)")
            .css_classes(["segmented-btn"])
            .build();

        let columns_btn = gtk4::Button::builder()
            .label(ViewMode::Columns.as_str())
            .tooltip_text("Column View (Ctrl+3)")
            .css_classes(["segmented-btn", "active"])
            .build();

        view_switcher.append(&icons_btn);
        view_switcher.append(&list_btn);
        view_switcher.append(&columns_btn);
        (view_switcher, icons_btn, list_btn, columns_btn)
    }

    fn build_action_buttons() -> (gtk4::Button, gtk4::Button, gtk4::Button, gtk4::Button) {
        let group_btn = gtk4::Button::builder()
            .label("Group By")
            .tooltip_text("Group items")
            .css_classes(["action-btn"])
            .build();

        let quick_look_btn = gtk4::Button::builder()
            .icon_name("eye-symbolic")
            .tooltip_text("Quick Look Preview (Space)")
            .css_classes(["action-btn"])
            .build();

        let get_info_btn = gtk4::Button::builder()
            .label("Get Info")
            .tooltip_text("Get Info (Ctrl+I)")
            .css_classes(["action-btn"])
            .build();

        let queue_btn = gtk4::Button::builder()
            .label("Queue")
            .tooltip_text("File Operations Queue")
            .css_classes(["action-btn"])
            .build();

        (group_btn, quick_look_btn, get_info_btn, queue_btn)
    }

    fn build_search_entry() -> gtk4::SearchEntry {
        gtk4::SearchEntry::builder()
            .placeholder_text("Search")
            .css_classes(["finder-search"])
            .halign(gtk4::Align::End)
            .valign(gtk4::Align::Center)
            .build()
    }

    /// Internal wiring ensuring view switcher buttons toggle active CSS classes.
    fn wire_internal_view_switcher(&self) {
        let icons = self.icons_btn.clone();
        let list = self.list_btn.clone();
        let columns = self.columns_btn.clone();

        let i_ref = icons.clone();
        let l_ref = list.clone();
        let c_ref = columns.clone();
        icons.connect_clicked(move |_| {
            i_ref.add_css_class("active");
            l_ref.remove_css_class("active");
            c_ref.remove_css_class("active");
        });

        let i_ref = icons.clone();
        let l_ref = list.clone();
        let c_ref = columns.clone();
        list.connect_clicked(move |_| {
            i_ref.remove_css_class("active");
            l_ref.add_css_class("active");
            c_ref.remove_css_class("active");
        });

        let i_ref = icons;
        let l_ref = list;
        let c_ref = columns.clone();
        columns.connect_clicked(move |_| {
            i_ref.remove_css_class("active");
            l_ref.remove_css_class("active");
            c_ref.add_css_class("active");
        });
    }

    /// Updates the current title display and optional folder icon.
    pub fn set_title(&self, title: &str, icon_name: Option<&str>) {
        self.title_label.set_text(title);
        if let Some(icon) = icon_name {
            self.title_icon.set_icon_name(Some(icon));
        }
    }

    /// Updates sensitive states for back and forward history buttons.
    pub fn update_history_buttons(&self, can_back: bool, can_forward: bool) {
        self.back_btn.set_sensitive(can_back);
        self.forward_btn.set_sensitive(can_forward);
    }

    /// Synchronizes the active segmented button with the specified [`ViewMode`].
    pub fn set_view_mode(&self, mode: ViewMode) {
        self.icons_btn.remove_css_class("active");
        self.list_btn.remove_css_class("active");
        self.columns_btn.remove_css_class("active");

        match mode {
            ViewMode::Icons => self.icons_btn.add_css_class("active"),
            ViewMode::List => self.list_btn.add_css_class("active"),
            ViewMode::Columns => self.columns_btn.add_css_class("active"),
        }
    }

    /// Connects a handler triggered when the view mode changes via switcher button clicks.
    pub fn connect_view_mode_changed<F: Fn(ViewMode) + 'static>(&self, callback: F) {
        use std::rc::Rc;
        let cb = Rc::new(callback);

        let cb_icons = Rc::clone(&cb);
        self.icons_btn.connect_clicked(move |_| {
            cb_icons(ViewMode::Icons);
        });

        let cb_list = Rc::clone(&cb);
        self.list_btn.connect_clicked(move |_| {
            cb_list(ViewMode::List);
        });

        let cb_columns = cb;
        self.columns_btn.connect_clicked(move |_| {
            cb_columns(ViewMode::Columns);
        });
    }

    /// Connects a handler invoked on backward navigation request.
    pub fn connect_back<F: Fn() + 'static>(&self, callback: F) {
        self.back_btn.connect_clicked(move |_| {
            callback();
        });
    }

    /// Connects a handler invoked on forward navigation request.
    pub fn connect_forward<F: Fn() + 'static>(&self, callback: F) {
        self.forward_btn.connect_clicked(move |_| {
            callback();
        });
    }

    /// Connects a handler invoked whenever the search text changes.
    pub fn connect_search_changed<F: Fn(&str) + 'static>(&self, callback: F) {
        self.search_entry.connect_search_changed(move |entry| {
            callback(entry.text().as_str());
        });
    }

    /// Connects a handler invoked when the search field is activated (Enter key).
    pub fn connect_search_activate<F: Fn(&str) + 'static>(&self, callback: F) {
        self.search_entry.connect_activate(move |entry| {
            callback(entry.text().as_str());
        });
    }

    /// Connects a handler invoked when the search is stopped (Escape key).
    pub fn connect_stop_search<F: Fn() + 'static>(&self, callback: F) {
        self.search_entry.connect_stop_search(move |_| {
            callback();
        });
    }

    /// Connects a handler invoked when the Quick Look button is clicked.
    pub fn connect_quicklook<F: Fn() + 'static>(&self, callback: F) {
        self.quick_look_btn.connect_clicked(move |_| {
            callback();
        });
    }

    /// Connects a handler invoked when the Get Info button is clicked.
    pub fn connect_get_info<F: Fn() + 'static>(&self, callback: F) {
        self.get_info_btn.connect_clicked(move |_| {
            callback();
        });
    }
}
