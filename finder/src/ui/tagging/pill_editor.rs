//! Interactive tag pill editor with autocomplete suggestions from SQLite tag index.

use std::cell::RefCell;
use std::rc::Rc;

use gdk4::Key;
use gtk4::prelude::*;
use shared::db::Database;
use shared::models::{FileTag, TagColor};

use super::swatches::ColorSwatchBar;

/// Splits comma-separated input into individual trimmed tag token names.
#[must_use]
pub fn tokenize_tag_input(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Filters a list of [`FileTag`] records matching a case-insensitive prefix.
#[must_use]
pub fn filter_autocomplete_tags(tags: &[FileTag], prefix: &str) -> Vec<FileTag> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let lower = trimmed.to_lowercase();
    tags.iter()
        .filter(|t| t.name.to_lowercase().starts_with(&lower))
        .cloned()
        .collect()
}

/// Callback invoked when a tag suggestion is chosen.
pub type TagSelectedCallback = Box<dyn Fn(FileTag) + 'static>;
/// Callback invoked when a tag is added.
pub type TagAddedCallback = Box<dyn Fn(FileTag) + 'static>;
/// Callback invoked when a tag is removed.
pub type TagRemovedCallback = Box<dyn Fn(String) + 'static>;
/// Callback invoked on escape key.
pub type EscapeCallback = Box<dyn Fn() + 'static>;

/// Queries SQLite database for tag suggestions starting with `prefix`.
#[must_use]
pub fn query_autocomplete_from_db(conn: &rusqlite::Connection, prefix: &str) -> Vec<FileTag> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let pattern = format!("{trimmed}%");

    let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT tag_name, tag_color \
         FROM tag_cache \
         WHERE tag_name LIKE ?1 \
         ORDER BY tag_name ASC \
         LIMIT 10;",
    ) else {
        return Vec::new();
    };

    let Ok(rows) = stmt.query_map(rusqlite::params![pattern], |row| {
        let name: String = row.get(0)?;
        let color_str: Option<String> = row.get(1)?;
        let color = color_str
            .as_deref()
            .and_then(TagColor::from_name)
            .or_else(|| TagColor::from_name(&name));
        Ok(FileTag::new(name, color))
    }) else {
        return Vec::new();
    };

    let mut suggestions = Vec::new();
    for row in rows.flatten() {
        suggestions.push(row);
    }

    // Also include canonical macOS colors if matching prefix
    for color in TagColor::ALL {
        if color
            .display_name()
            .to_lowercase()
            .starts_with(&trimmed.to_lowercase())
            && !suggestions
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(color.display_name()))
        {
            suggestions.push(FileTag::new(color.display_name(), Some(color)));
        }
    }

    suggestions
}

/// Data entry representation of a tag pill within the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagPillEntry {
    /// Tag payload model.
    pub tag: FileTag,
    /// Indicates whether the tag is present on only a subset of selected files.
    pub is_partial: bool,
}

/// Autocomplete popover displaying live suggestion rows.
#[derive(Clone)]
pub struct TagAutocompletePopover {
    /// Root GTK popover.
    pub popover: gtk4::Popover,
    /// Inner list box hosting suggestion rows.
    pub list_box: gtk4::ListBox,
    items: Rc<RefCell<Vec<FileTag>>>,
    selected_index: Rc<RefCell<Option<usize>>>,
    on_selected: Rc<RefCell<Option<TagSelectedCallback>>>,
}

impl TagAutocompletePopover {
    /// Constructs a new [`TagAutocompletePopover`] parented to `parent`.
    #[must_use]
    pub fn new<W: IsA<gtk4::Widget>>(parent: &W) -> Self {
        let popover = gtk4::Popover::builder()
            .autohide(true)
            .has_arrow(false)
            .position(gtk4::PositionType::Bottom)
            .css_classes(["tag-autocomplete-popover"])
            .build();
        popover.set_parent(parent);

        let list_box = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::Single)
            .build();

        let scrolled = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .max_content_height(160)
            .propagate_natural_height(true)
            .child(&list_box)
            .build();

        popover.set_child(Some(&scrolled));

        let items = Rc::new(RefCell::new(Vec::new()));
        let selected_index = Rc::new(RefCell::new(None));
        let on_selected: Rc<RefCell<Option<TagSelectedCallback>>> = Rc::new(RefCell::new(None));

        let popover_self = Self {
            popover,
            list_box: list_box.clone(),
            items: items.clone(),
            selected_index,
            on_selected: on_selected.clone(),
        };

        let on_selected_clone = on_selected;
        let items_clone = items;
        let popover_weak = popover_self.popover.downgrade();

        list_box.connect_row_activated(move |_lb, row| {
            let idx = usize::try_from(row.index()).unwrap_or(0);
            if let Some(tag) = items_clone.borrow().get(idx).cloned()
                && let Some(ref cb) = *on_selected_clone.borrow()
            {
                cb(tag);
            }
            if let Some(p) = popover_weak.upgrade() {
                p.popdown();
            }
        });

        popover_self
    }

    /// Registers a callback triggered when a suggestion is selected.
    pub fn connect_selected<F: Fn(FileTag) + 'static>(&self, callback: F) {
        *self.on_selected.borrow_mut() = Some(Box::new(callback));
    }

    /// Updates popover suggestion rows.
    pub fn set_suggestions(&self, suggestions: Vec<FileTag>) {
        while let Some(child) = self.list_box.first_child() {
            self.list_box.remove(&child);
        }

        if suggestions.is_empty() {
            *self.items.borrow_mut() = Vec::new();
            *self.selected_index.borrow_mut() = None;
            self.popover.popdown();
            return;
        }

        for (idx, tag) in suggestions.iter().enumerate() {
            let row = gtk4::ListBoxRow::builder()
                .css_classes(["tag-autocomplete-row"])
                .build();

            let hbox = gtk4::Box::builder()
                .orientation(gtk4::Orientation::Horizontal)
                .spacing(8)
                .margin_start(4)
                .margin_end(4)
                .margin_top(2)
                .margin_bottom(2)
                .build();

            let dot = gtk4::Box::builder().css_classes(["tag-badge-dot"]).build();
            if let Some(color) = tag.color {
                dot.add_css_class(ColorSwatchBar::color_css_class(color));
            } else {
                dot.add_css_class("tag-color-gray");
            }
            hbox.append(&dot);

            let lbl = gtk4::Label::builder()
                .label(&tag.name)
                .halign(gtk4::Align::Start)
                .hexpand(true)
                .build();
            hbox.append(&lbl);

            row.set_child(Some(&hbox));
            self.list_box.append(&row);

            if idx == 0 {
                self.list_box.select_row(Some(&row));
                *self.selected_index.borrow_mut() = Some(0);
            }
        }

        *self.items.borrow_mut() = suggestions;
        self.popover.popup();
    }

    /// Selects the next suggestion downwards.
    pub fn move_down(&self) {
        let total = self.items.borrow().len();
        if total == 0 {
            return;
        }
        let current = self.selected_index.borrow().unwrap_or(0);
        let next = (current + 1).min(total.saturating_sub(1));
        self.select_index(next);
    }

    /// Selects the next suggestion upwards.
    pub fn move_up(&self) {
        let total = self.items.borrow().len();
        if total == 0 {
            return;
        }
        let current = self.selected_index.borrow().unwrap_or(0);
        let next = current.saturating_sub(1);
        self.select_index(next);
    }

    /// Activates and commits the currently selected suggestion if any.
    #[must_use]
    pub fn activate_current_selection(&self) -> bool {
        let Some(idx) = *self.selected_index.borrow() else {
            return false;
        };
        let Some(tag) = self.items.borrow().get(idx).cloned() else {
            return false;
        };

        if let Some(ref cb) = *self.on_selected.borrow() {
            cb(tag);
        }
        self.popover.popdown();
        true
    }

    fn select_index(&self, index: usize) {
        if let Ok(idx_i32) = i32::try_from(index)
            && let Some(row) = self.list_box.row_at_index(idx_i32)
        {
            self.list_box.select_row(Some(&row));
            *self.selected_index.borrow_mut() = Some(index);
        }
    }
}

/// Interactive capsule pill editor widget for adding and removing tags.
#[derive(Clone)]
pub struct TagPillEditor {
    /// Root vertical box enclosing the flow box and styling borders.
    pub container: gtk4::Box,
    /// Flow box organizing pills and the inline entry field.
    pub flow_box: gtk4::FlowBox,
    /// Inline text entry for creating new tags.
    pub entry: gtk4::Entry,
    /// Autocomplete popover suggestion dropdown.
    pub autocomplete: TagAutocompletePopover,
    tags: Rc<RefCell<Vec<TagPillEntry>>>,
    on_tag_added: Rc<RefCell<Vec<TagAddedCallback>>>,
    on_tag_removed: Rc<RefCell<Vec<TagRemovedCallback>>>,
    on_escape: Rc<RefCell<Vec<EscapeCallback>>>,
}

impl Default for TagPillEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl TagPillEditor {
    /// Constructs a new [`TagPillEditor`].
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .css_classes(["tag-pill-editor-box"])
            .build();

        let flow_box = gtk4::FlowBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .max_children_per_line(30)
            .min_children_per_line(1)
            .column_spacing(4)
            .row_spacing(4)
            .build();
        container.append(&flow_box);

        let entry = gtk4::Entry::builder()
            .placeholder_text("Add a tag...")
            .css_classes(["tag-entry-field"])
            .hexpand(true)
            .build();
        flow_box.append(&entry);

        let autocomplete = TagAutocompletePopover::new(&entry);

        let editor = Self {
            container,
            flow_box,
            entry,
            autocomplete,
            tags: Rc::new(RefCell::new(Vec::new())),
            on_tag_added: Rc::new(RefCell::new(Vec::new())),
            on_tag_removed: Rc::new(RefCell::new(Vec::new())),
            on_escape: Rc::new(RefCell::new(Vec::new())),
        };

        editor.wire_keyboard_and_autocomplete();
        editor
    }

    /// Registers a callback invoked when a new tag is added.
    pub fn connect_tag_added<F: Fn(FileTag) + 'static>(&self, callback: F) {
        self.on_tag_added.borrow_mut().push(Box::new(callback));
    }

    /// Registers a callback invoked when a tag is removed.
    pub fn connect_tag_removed<F: Fn(String) + 'static>(&self, callback: F) {
        self.on_tag_removed.borrow_mut().push(Box::new(callback));
    }

    /// Registers a callback invoked when the user presses Escape.
    pub fn connect_escape<F: Fn() + 'static>(&self, callback: F) {
        self.on_escape.borrow_mut().push(Box::new(callback));
    }

    /// Returns a list of all currently active tag names.
    #[must_use]
    pub fn get_tags(&self) -> Vec<FileTag> {
        self.tags.borrow().iter().map(|e| e.tag.clone()).collect()
    }

    /// Bulk populates tags with support for shared and partially shared tags.
    pub fn set_tags(&self, shared_tags: &[FileTag], partial_tags: &[FileTag]) {
        let mut entries = Vec::new();
        for t in shared_tags {
            entries.push(TagPillEntry {
                tag: t.clone(),
                is_partial: false,
            });
        }
        for t in partial_tags {
            if !entries
                .iter()
                .any(|e| e.tag.name.eq_ignore_ascii_case(&t.name))
            {
                entries.push(TagPillEntry {
                    tag: t.clone(),
                    is_partial: true,
                });
            }
        }
        *self.tags.borrow_mut() = entries;
        self.rebuild_pills();
    }

    /// Adds a single tag to the editor and notifies listeners.
    pub fn add_tag(&self, tag: &FileTag) {
        if self
            .tags
            .borrow()
            .iter()
            .any(|e| e.tag.name.eq_ignore_ascii_case(&tag.name))
        {
            return;
        }

        self.tags.borrow_mut().push(TagPillEntry {
            tag: tag.clone(),
            is_partial: false,
        });
        self.rebuild_pills();

        for cb in self.on_tag_added.borrow().iter() {
            cb(tag.clone());
        }
    }

    /// Removes a tag by name and notifies listeners.
    pub fn remove_tag(&self, tag_name: &str) {
        let mut found = false;
        self.tags.borrow_mut().retain(|e| {
            if e.tag.name.eq_ignore_ascii_case(tag_name) {
                found = true;
                false
            } else {
                true
            }
        });

        if found {
            self.rebuild_pills();
            for cb in self.on_tag_removed.borrow().iter() {
                cb(tag_name.to_string());
            }
        }
    }

    /// Removes the last added tag pill (for Backspace key on empty entry).
    #[must_use]
    pub fn remove_last_tag(&self) -> Option<String> {
        let last = self.tags.borrow_mut().pop()?;
        self.rebuild_pills();
        for cb in self.on_tag_removed.borrow().iter() {
            cb(last.tag.name.clone());
        }
        Some(last.tag.name)
    }

    /// Clears all tag pills and entry text.
    pub fn clear(&self) {
        self.tags.borrow_mut().clear();
        self.entry.set_text("");
        self.rebuild_pills();
    }

    /// Commits the current entry text as one or more tag tokens.
    pub fn commit_entry_text(&self) {
        let text = self.entry.text().to_string();
        let tokens = tokenize_tag_input(&text);
        for token in tokens {
            let tag = FileTag::with_name(token);
            self.add_tag(&tag);
        }
        self.entry.set_text("");
        self.autocomplete.popover.popdown();
    }

    fn rebuild_pills(&self) {
        // Remove all children except the entry field
        while let Some(child) = self.flow_box.first_child() {
            if child.is::<gtk4::FlowBoxChild>()
                && let Some(inner) = child.first_child()
                && inner == self.entry.clone().upcast::<gtk4::Widget>()
            {
                break;
            }
            self.flow_box.remove(&child);
        }

        // Re-append pills
        for entry in self.tags.borrow().iter() {
            let pill_widget = self.create_pill_widget(entry);
            self.flow_box.insert(&pill_widget, -1);
        }

        // Ensure entry is at the end
        if let Some(parent) = self.entry.parent()
            && let Some(fb_child) = parent.downcast_ref::<gtk4::FlowBoxChild>()
        {
            self.flow_box.remove(fb_child);
        }
        self.flow_box.append(&self.entry);
    }

    fn create_pill_widget(&self, entry: &TagPillEntry) -> gtk4::Box {
        let pill_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(4)
            .css_classes(["tag-pill"])
            .build();

        if entry.is_partial {
            pill_box.add_css_class("tag-pill-partial");
        }

        if let Some(color) = entry.tag.color {
            let dot = gtk4::Box::builder()
                .css_classes(["tag-badge-dot", ColorSwatchBar::color_css_class(color)])
                .build();
            pill_box.append(&dot);
        }

        let lbl = gtk4::Label::builder()
            .label(&entry.tag.name)
            .halign(gtk4::Align::Start)
            .build();
        pill_box.append(&lbl);

        let remove_btn = gtk4::Button::builder()
            .label("✕")
            .has_frame(false)
            .tooltip_text("Remove")
            .css_classes(["tag-pill-remove-btn"])
            .build();

        let this = self.clone();
        let name = entry.tag.name.clone();
        remove_btn.connect_clicked(move |_| {
            this.remove_tag(&name);
        });
        pill_box.append(&remove_btn);

        pill_box
    }

    fn wire_keyboard_and_autocomplete(&self) {
        let key_controller = gtk4::EventControllerKey::new();
        let this = self.clone();

        key_controller.connect_key_pressed(move |_ctrl, key, _keycode, _state| match key {
            Key::Return | Key::KP_Enter => {
                if this.autocomplete.popover.is_visible()
                    && this.autocomplete.activate_current_selection()
                {
                    this.entry.set_text("");
                    return glib::Propagation::Stop;
                }
                this.commit_entry_text();
                glib::Propagation::Stop
            }
            Key::comma => {
                this.commit_entry_text();
                glib::Propagation::Stop
            }
            Key::BackSpace => {
                if this.entry.text().is_empty() {
                    let _ = this.remove_last_tag();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
            Key::Escape => {
                this.autocomplete.popover.popdown();
                for cb in this.on_escape.borrow().iter() {
                    cb();
                }
                glib::Propagation::Stop
            }
            Key::Down => {
                if this.autocomplete.popover.is_visible() {
                    this.autocomplete.move_down();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
            Key::Up => {
                if this.autocomplete.popover.is_visible() {
                    this.autocomplete.move_up();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
            Key::Tab => {
                if this.autocomplete.popover.is_visible()
                    && this.autocomplete.activate_current_selection()
                {
                    this.entry.set_text("");
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
            _ => glib::Propagation::Proceed,
        });

        self.entry.add_controller(key_controller);

        let this_ac = self.clone();
        self.autocomplete.connect_selected(move |tag| {
            this_ac.add_tag(&tag);
            this_ac.entry.set_text("");
        });

        let this_change = self.clone();
        self.entry.connect_changed(move |entry| {
            let text = entry.text().to_string();
            if text.trim().is_empty() {
                this_change.autocomplete.popover.popdown();
                return;
            }

            let query = text.split(',').next_back().unwrap_or("").trim();

            if query.is_empty() {
                this_change.autocomplete.popover.popdown();
                return;
            }

            let suggestions = Database::open_default().map_or_else(
                |_| {
                    let canonical: Vec<FileTag> = TagColor::ALL
                        .iter()
                        .map(|c| FileTag::new(c.display_name(), Some(*c)))
                        .collect();
                    filter_autocomplete_tags(&canonical, query)
                },
                |db| query_autocomplete_from_db(&db, query),
            );

            this_change.autocomplete.set_suggestions(suggestions);
        });
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use crate::ui::tagging::GTK_TEST_MUTEX;
    use rusqlite::Connection;

    fn ensure_gtk() -> bool {
        if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err())
        {
            return false;
        }
        true
    }

    #[test]
    fn test_tokenize_tag_input_single_and_multiple() {
        assert_eq!(tokenize_tag_input("Work"), vec!["Work"]);
        assert_eq!(
            tokenize_tag_input("Work, Urgent, Review"),
            vec!["Work", "Urgent", "Review"]
        );
        assert_eq!(
            tokenize_tag_input("  Invoice , , Taxes ,   "),
            vec!["Invoice", "Taxes"]
        );
        assert!(tokenize_tag_input(" , ,   ").is_empty());
        assert!(tokenize_tag_input("").is_empty());
    }

    #[test]
    fn test_filter_autocomplete_tags_prefix_matching() {
        let tags = vec![
            FileTag::new("ProjectA", Some(TagColor::Blue)),
            FileTag::new("ProjectB", Some(TagColor::Green)),
            FileTag::new("Personal", None),
            FileTag::new("Work", Some(TagColor::Red)),
        ];

        let res = filter_autocomplete_tags(&tags, "proj");
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].name, "ProjectA");
        assert_eq!(res[1].name, "ProjectB");

        let res_upper = filter_autocomplete_tags(&tags, "PROJ");
        assert_eq!(res_upper.len(), 2);

        let res_single = filter_autocomplete_tags(&tags, "per");
        assert_eq!(res_single.len(), 1);
        assert_eq!(res_single[0].name, "Personal");

        let res_none = filter_autocomplete_tags(&tags, "xyz");
        assert!(res_none.is_empty());

        let res_empty = filter_autocomplete_tags(&tags, "  ");
        assert!(res_empty.is_empty());
    }

    #[test]
    fn test_sqlite_autocomplete_query() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE tag_cache (
                 path TEXT NOT NULL,
                 tag_name TEXT NOT NULL,
                 tag_color TEXT,
                 mtime INTEGER
             );
             INSERT INTO tag_cache VALUES ('/a', 'Documentation', 'Blue', 100);
             INSERT INTO tag_cache VALUES ('/b', 'Docker', 'Green', 200);
             INSERT INTO tag_cache VALUES ('/c', 'Finance', 'Yellow', 300);
            ",
        )
        .unwrap();

        let suggestions = query_autocomplete_from_db(&conn, "doc");
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].name, "Docker");
        assert_eq!(suggestions[0].color, Some(TagColor::Green));
        assert_eq!(suggestions[1].name, "Documentation");
        assert_eq!(suggestions[1].color, Some(TagColor::Blue));

        let fin = query_autocomplete_from_db(&conn, "fin");
        assert_eq!(fin.len(), 1);
        assert_eq!(fin[0].name, "Finance");
        assert_eq!(fin[0].color, Some(TagColor::Yellow));

        let red = query_autocomplete_from_db(&conn, "re");
        assert!(red.iter().any(|s| s.name == "Red"));
    }

    #[test]
    fn test_tag_pill_editor_state_and_token_commit() {
        let _lock = GTK_TEST_MUTEX.lock().unwrap();
        if !ensure_gtk() {
            return;
        }

        let editor = TagPillEditor::new();
        assert_eq!(editor.get_tags().len(), 0);

        editor.add_tag(&FileTag::new("Work", Some(TagColor::Red)));
        assert_eq!(editor.get_tags().len(), 1);
        assert_eq!(editor.get_tags()[0].name, "Work");

        // Duplicate rejection
        editor.add_tag(&FileTag::new("work", None));
        assert_eq!(editor.get_tags().len(), 1);

        editor.entry.set_text("Urgent, Invoices");
        editor.commit_entry_text();
        assert_eq!(editor.get_tags().len(), 3);
        assert_eq!(editor.entry.text(), "");

        // Removal
        editor.remove_tag("Work");
        assert_eq!(editor.get_tags().len(), 2);
        assert!(!editor.get_tags().iter().any(|t| t.name == "Work"));

        // Remove last tag
        let removed = editor.remove_last_tag();
        assert_eq!(removed.as_deref(), Some("Invoices"));
        assert_eq!(editor.get_tags().len(), 1);

        editor.clear();
        assert_eq!(editor.get_tags().len(), 0);
    }
}
