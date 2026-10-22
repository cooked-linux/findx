//! GTK4 smart folder criteria builder bar displayed beneath the header controls.

mod row;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;

use crate::smart_folders::{
    BooleanMode, CriteriaBuilder, CriteriaGroup, SmartFolderDefinition, resolve_storage_dir,
    save_smart_folder,
};

/// Default label rendered on the compound-rule add button.
const ADD_LABEL: &str = "[+]";
/// Label rendered while the Option/Alt modifier is held.
const COMPOUND_LABEL: &str = "[...]";

type ChangedCallback = Rc<dyn Fn()>;
type SavedCallback = Rc<dyn Fn(PathBuf, SmartFolderDefinition)>;
type ClosedCallback = Rc<dyn Fn()>;

/// Criteria builder bar with attribute/operator/value rows and nested boolean groups.
#[derive(Clone)]
pub struct SmartFolderBar {
    /// Root vertical container mounted above the file views.
    pub container: gtk4::Box,
    rules_box: gtk4::Box,
    title_label: gtk4::Label,
    count_label: gtk4::Label,
    warning_label: gtk4::Label,
    add_btn: gtk4::Button,
    save_btn: gtk4::Button,
    close_btn: gtk4::Button,
    builder: Rc<RefCell<CriteriaBuilder>>,
    alt_down: Rc<Cell<bool>>,
    edit_path: Rc<RefCell<Option<PathBuf>>>,
    on_changed: Rc<RefCell<Option<ChangedCallback>>>,
    on_saved: Rc<RefCell<Option<SavedCallback>>>,
    on_closed: Rc<RefCell<Option<ClosedCallback>>>,
    row_warnings: Rc<RefCell<Vec<(usize, usize, gtk4::Label)>>>,
}

impl SmartFolderBar {
    /// Builds a hidden criteria bar ready to edit a smart folder definition.
    #[must_use]
    pub fn new() -> Self {
        let container = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(10)
            .margin_end(10)
            .css_classes(["smart-folder-bar", "toolbar"])
            .visible(false)
            .build();

        let header = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .build();

        let add_btn = gtk4::Button::builder()
            .label(ADD_LABEL)
            .tooltip_text("Add criteria rule (hold Option for compound group)")
            .css_classes(["flat"])
            .build();
        let title_label = gtk4::Label::builder()
            .label("Smart Folder")
            .halign(gtk4::Align::Start)
            .css_classes(["heading"])
            .build();
        let warning_label = gtk4::Label::builder()
            .halign(gtk4::Align::Start)
            .css_classes(["caption", "error"])
            .visible(false)
            .build();
        let count_label = gtk4::Label::builder()
            .label("0 items")
            .css_classes(["caption", "dim-label"])
            .build();
        let save_btn = gtk4::Button::builder()
            .label("Save")
            .tooltip_text("Save this smart folder")
            .css_classes(["suggested-action"])
            .build();
        let close_btn = gtk4::Button::builder()
            .label("✕")
            .tooltip_text("Close smart folder")
            .css_classes(["flat"])
            .build();

        let spacer = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .build();

        header.append(&add_btn);
        header.append(&title_label);
        header.append(&warning_label);
        header.append(&spacer);
        header.append(&count_label);
        header.append(&save_btn);
        header.append(&close_btn);

        let rules_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .build();

        container.append(&header);
        container.append(&rules_box);

        let bar = Self {
            container,
            rules_box,
            title_label,
            count_label,
            warning_label,
            add_btn,
            save_btn,
            close_btn,
            builder: Rc::new(RefCell::new(CriteriaBuilder::new("Untitled Smart Folder"))),
            alt_down: Rc::new(Cell::new(false)),
            edit_path: Rc::new(RefCell::new(None)),
            on_changed: Rc::new(RefCell::new(None)),
            on_saved: Rc::new(RefCell::new(None)),
            on_closed: Rc::new(RefCell::new(None)),
            row_warnings: Rc::new(RefCell::new(Vec::new())),
        };

        bar.wire_buttons();
        bar
    }

    /// Registers a callback invoked after any criteria mutation.
    pub fn connect_changed<F: Fn() + 'static>(&self, callback: F) {
        *self.on_changed.borrow_mut() = Some(Rc::new(callback));
    }

    /// Registers a callback invoked after a successful save.
    pub fn connect_saved<F: Fn(PathBuf, SmartFolderDefinition) + 'static>(&self, callback: F) {
        *self.on_saved.borrow_mut() = Some(Rc::new(callback));
    }

    /// Registers a callback invoked when the user closes the bar.
    pub fn connect_closed<F: Fn() + 'static>(&self, callback: F) {
        *self.on_closed.borrow_mut() = Some(Rc::new(callback));
    }

    /// Returns whether the bar is currently mounted and visible.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.container.is_visible()
    }

    /// Returns the current draft definition from the builder model.
    #[must_use]
    pub fn definition(&self) -> SmartFolderDefinition {
        self.builder.borrow().definition().clone()
    }

    /// Returns the persisted path of the smart folder being edited, if any.
    #[must_use]
    pub fn edit_path(&self) -> Option<PathBuf> {
        self.edit_path.borrow().clone()
    }

    /// Displays an existing smart folder definition in the bar.
    pub fn show_definition(&self, definition: SmartFolderDefinition, edit_path: Option<PathBuf>) {
        self.title_label.set_text(&definition.name);
        *self.edit_path.borrow_mut() = edit_path;
        *self.builder.borrow_mut() = CriteriaBuilder::from_definition(definition);
        self.container.set_visible(true);
        self.rebuild_rows();
    }

    /// Displays a brand-new unsaved definition in the bar.
    pub fn show_draft(&self, definition: SmartFolderDefinition) {
        self.title_label.set_text("New Smart Folder");
        *self.edit_path.borrow_mut() = None;
        *self.builder.borrow_mut() = CriteriaBuilder::from_definition(definition);
        self.container.set_visible(true);
        self.rebuild_rows();
    }

    /// Hides the bar without discarding the current model.
    pub fn hide(&self) {
        self.container.set_visible(false);
    }

    /// Updates the live result count badge.
    pub fn set_count(&self, count: usize) {
        let noun = if count == 1 { "item" } else { "items" };
        self.count_label.set_text(&format!("{count} {noun}"));
    }

    /// Morphs the add button between `[+]` and `[...]` while Option/Alt is held.
    pub fn set_alt_down(&self, down: bool) {
        self.alt_down.set(down);
        self.add_btn
            .set_label(if down { COMPOUND_LABEL } else { ADD_LABEL });
        self.add_btn.set_tooltip_text(Some(if down {
            "Add compound criteria rule (Option)"
        } else {
            "Add criteria rule (hold Option for compound group)"
        }));
    }

    fn wire_buttons(&self) {
        let bar = self.clone();
        self.add_btn.connect_clicked(move |_| {
            if bar.alt_down.get() {
                bar.builder.borrow_mut().add_subgroup(0);
            } else {
                bar.builder.borrow_mut().add_slice(0);
            }
            bar.rebuild_and_notify();
        });

        let bar = self.clone();
        self.save_btn.connect_clicked(move |_| {
            bar.prompt_save();
        });

        let bar = self.clone();
        self.close_btn.connect_clicked(move |_| {
            bar.hide();
            let callback = bar.on_closed.borrow().clone();
            if let Some(callback) = callback {
                callback();
            }
        });
    }

    fn rebuild_and_notify(&self) {
        self.rebuild_rows();
        self.notify_changed();
    }

    fn notify_changed(&self) {
        self.update_warning_labels();
        let callback = self.on_changed.borrow().clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    fn rebuild_rows(&self) {
        self.row_warnings.borrow_mut().clear();
        clear_box_children(&self.rules_box);

        let root = self.builder.borrow().definition().root_criteria.clone();
        let mut next_index = 0_usize;
        self.append_group(&root, 0, &mut next_index);
        self.update_warning_labels();
    }

    fn append_group(&self, group: &CriteriaGroup, depth: usize, next_index: &mut usize) {
        let group_index = *next_index;
        *next_index += 1;

        let header = self.build_group_header(group_index, group, depth);
        self.rules_box.append(&header);

        for (slice_index, slice) in group.slices.iter().enumerate() {
            let show_remove = group.slices.len() > 1 || depth > 0;
            let row =
                row::build_rule_row(self, group_index, slice_index, slice, depth, show_remove);
            self.rules_box.append(&row);
        }

        for subgroup in &group.subgroups {
            self.append_group(subgroup, depth + 1, next_index);
        }
    }

    fn build_group_header(
        &self,
        group_index: usize,
        group: &CriteriaGroup,
        depth: usize,
    ) -> gtk4::Box {
        let header = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(6)
            .margin_start(indent(depth))
            .build();

        let prefix = gtk4::Label::builder()
            .label(if depth == 0 { "Match" } else { "•" })
            .css_classes(["caption", "dim-label"])
            .build();
        header.append(&prefix);

        let modes = ["all", "any", "none"];
        let mode_dropdown = gtk4::DropDown::from_strings(&modes);
        mode_dropdown.set_selected(boolean_mode_index(group.mode));
        mode_dropdown.set_tooltip_text(Some("Boolean combinator for this rule group"));
        let bar = self.clone();
        mode_dropdown.connect_selected_notify(move |dropdown| {
            let mode = boolean_mode_at(dropdown.selected());
            bar.builder.borrow_mut().set_boolean_mode(group_index, mode);
            bar.notify_changed();
        });
        header.append(&mode_dropdown);

        let of_label = gtk4::Label::builder()
            .label("of the following rules")
            .css_classes(["caption", "dim-label"])
            .build();
        header.append(&of_label);

        let spacer = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .build();
        header.append(&spacer);

        if depth > 0 {
            let add_rule = gtk4::Button::builder()
                .label("+")
                .tooltip_text("Add a rule to this group")
                .css_classes(["flat"])
                .build();
            let bar = self.clone();
            add_rule.connect_clicked(move |_| {
                bar.builder.borrow_mut().add_slice(group_index);
                bar.rebuild_and_notify();
            });
            header.append(&add_rule);

            let add_group = gtk4::Button::builder()
                .label("...")
                .tooltip_text("Add nested compound group")
                .css_classes(["flat"])
                .build();
            let bar = self.clone();
            add_group.connect_clicked(move |_| {
                bar.builder.borrow_mut().add_subgroup(group_index);
                bar.rebuild_and_notify();
            });
            header.append(&add_group);

            let remove_group = gtk4::Button::builder()
                .label("−")
                .tooltip_text("Remove this compound group")
                .css_classes(["flat"])
                .build();
            let bar = self.clone();
            remove_group.connect_clicked(move |_| {
                bar.builder.borrow_mut().remove_subgroup(group_index);
                bar.rebuild_and_notify();
            });
            header.append(&remove_group);
        }

        header
    }

    fn remove_slice(&self, group_index: usize, slice_index: usize, depth: usize) {
        self.builder
            .borrow_mut()
            .remove_slice(group_index, slice_index);
        let is_root_empty = depth == 0
            && self
                .builder
                .borrow()
                .group_at(0)
                .is_some_and(|group| group.slices.is_empty());
        if is_root_empty {
            self.builder.borrow_mut().add_slice(0);
        }
        self.rebuild_and_notify();
    }

    fn update_warning_labels(&self) {
        let builder = self.builder.borrow();
        let warnings = builder.warnings();
        if warnings.is_empty() {
            self.warning_label.set_visible(false);
            self.warning_label.set_text("");
        } else {
            self.warning_label
                .set_text(&format!("⚠ {}", warnings.join(" · ")));
            self.warning_label.set_visible(true);
        }

        for (group_index, slice_index, label) in self.row_warnings.borrow().iter() {
            if let Some(slice) = builder.slice_at(*group_index, *slice_index) {
                if slice.is_valid {
                    label.set_visible(false);
                    label.set_text("");
                } else {
                    label.set_text(slice.warning_message.as_deref().unwrap_or("invalid rule"));
                    label.set_visible(true);
                }
            }
        }
    }

    fn prompt_save(&self) {
        let parent = self
            .container
            .root()
            .and_then(|root| root.downcast::<gtk4::Window>().ok());
        let dialog = gtk4::Window::builder()
            .title("Save Smart Folder")
            .modal(true)
            .default_width(380)
            .resizable(false)
            .build();
        if let Some(parent) = parent.as_ref() {
            dialog.set_transient_for(Some(parent));
            dialog.set_application(parent.application().as_ref());
        }

        let content = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(10)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();

        let label = gtk4::Label::builder()
            .label("Name")
            .halign(gtk4::Align::Start)
            .build();
        let entry = gtk4::Entry::builder()
            .text(self.builder.borrow().definition().name.as_str())
            .activates_default(true)
            .build();

        let actions = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk4::Align::End)
            .build();
        let cancel = gtk4::Button::with_label("Cancel");
        let save = gtk4::Button::builder()
            .label("Save")
            .css_classes(["suggested-action"])
            .build();

        actions.append(&cancel);
        actions.append(&save);
        content.append(&label);
        content.append(&entry);
        content.append(&actions);
        dialog.set_child(Some(&content));

        let dialog_weak = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });

        let bar = self.clone();
        let dialog_weak = dialog.downgrade();
        let entry_for_save = entry.clone();
        save.connect_clicked(move |_| {
            let name = entry_for_save.text().to_string();
            bar.save_with_name(&name);
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });

        dialog.present();
        entry.grab_focus();
        entry.select_region(0, -1);
    }

    fn save_with_name(&self, name: &str) {
        let trimmed = name.trim();
        let mut definition = self.builder.borrow().definition().clone();
        definition.name = if trimmed.is_empty() {
            "Untitled Smart Folder".to_string()
        } else {
            trimmed.to_string()
        };
        definition.touch();

        match save_smart_folder(&resolve_storage_dir(), &definition) {
            Ok(path) => {
                {
                    let mut builder = self.builder.borrow_mut();
                    builder.definition_mut().name.clone_from(&definition.name);
                    builder.definition_mut().modified_at = definition.modified_at;
                }
                self.title_label.set_text(&definition.name);
                *self.edit_path.borrow_mut() = Some(path.clone());
                self.warning_label.set_visible(false);
                let callback = self.on_saved.borrow().clone();
                if let Some(callback) = callback {
                    callback(path, definition);
                }
            }
            Err(error) => {
                tracing::warn!("Failed to save smart folder: {error}");
                self.warning_label
                    .set_text(&format!("⚠ Save failed: {error}"));
                self.warning_label.set_visible(true);
            }
        }
    }
}

impl std::fmt::Debug for SmartFolderBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartFolderBar")
            .field("visible", &self.container.is_visible())
            .field("edit_path", &self.edit_path.borrow())
            .finish_non_exhaustive()
    }
}

impl Default for SmartFolderBar {
    fn default() -> Self {
        Self::new()
    }
}

const fn boolean_mode_index(mode: BooleanMode) -> u32 {
    match mode {
        BooleanMode::All => 0,
        BooleanMode::Any => 1,
        BooleanMode::None => 2,
    }
}

const fn boolean_mode_at(index: u32) -> BooleanMode {
    match index {
        1 => BooleanMode::Any,
        2 => BooleanMode::None,
        _ => BooleanMode::All,
    }
}

fn indent(depth: usize) -> i32 {
    i32::try_from(depth.saturating_mul(18)).unwrap_or(i32::MAX)
}

fn clear_box_children(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// Debounce duration applied before re-evaluating live smart folder criteria.
pub const CRITERIA_DEBOUNCE: Duration = Duration::from_millis(150);
