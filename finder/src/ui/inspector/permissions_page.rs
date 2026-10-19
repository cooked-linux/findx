//! "Open With" and "Sharing & Permissions" property groups.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita::prelude::*;
use shared::mime::{AppHandlerInfo, get_open_with_candidates, set_default_app};

use crate::metadata::aggregate::MultiSelectionSummary;
use crate::metadata::types::DeepMetadata;

/// Converts privilege dropdown index (0: Read & Write, 1: Read only, 2: Write only, 3: No access) to 3-bit mode.
#[must_use]
pub const fn privilege_to_bits(index: u32, is_dir_or_exec: bool) -> u32 {
    let base = match index {
        0 => 0o6,
        1 => 0o4,
        2 => 0o2,
        _ => 0o0,
    };
    if is_dir_or_exec && base != 0 {
        base | 0o1
    } else {
        base
    }
}

/// Converts 3-bit mode to privilege dropdown index.
#[must_use]
pub const fn bits_to_privilege(bits: u32) -> u32 {
    let masked = bits & 0o6;
    match masked {
        0o6 => 0,
        0o4 => 1,
        0o2 => 2,
        _ => 3,
    }
}

/// "Open With" property group for default application selection and association updates.
#[derive(Clone)]
pub struct OpenWithGroup {
    /// Root libadwaita preferences group.
    pub group: libadwaita::PreferencesGroup,
    combo_row: libadwaita::ComboRow,
    change_all_btn: gtk4::Button,
    candidates: Rc<RefCell<Vec<AppHandlerInfo>>>,
    current_mime: Rc<RefCell<String>>,
    current_paths: Rc<RefCell<Vec<PathBuf>>>,
    updating_combo: Rc<RefCell<bool>>,
}

impl OpenWithGroup {
    /// Constructs a new [`OpenWithGroup`].
    #[must_use]
    pub fn new() -> Self {
        let group = libadwaita::PreferencesGroup::builder()
            .title("Open With")
            .build();

        let combo_row = libadwaita::ComboRow::builder().title("Application").build();

        let change_all_btn = gtk4::Button::builder()
            .label("Change All...")
            .valign(gtk4::Align::Center)
            .css_classes(["suggested-action"])
            .build();

        let action_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .halign(gtk4::Align::End)
            .margin_top(6)
            .build();
        action_box.append(&change_all_btn);

        group.add(&combo_row);
        group.add(&action_box);

        let candidates: Rc<RefCell<Vec<AppHandlerInfo>>> = Rc::new(RefCell::new(Vec::new()));
        let current_mime = Rc::new(RefCell::new(String::new()));
        let current_paths: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let updating_combo = Rc::new(RefCell::new(false));

        // Wire "Change All..." button
        let candidates_clone = Rc::clone(&candidates);
        let mime_clone = Rc::clone(&current_mime);
        let combo_clone = combo_row.clone();

        change_all_btn.connect_clicked(move |btn| {
            let selected_idx = combo_clone.selected() as usize;
            let list = candidates_clone.borrow();
            let Some(app) = list.get(selected_idx) else {
                return;
            };

            let app_name = app.display_name.clone();
            let desktop_id = app.desktop_id.clone();
            let mime = mime_clone.borrow().clone();

            let root_win = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());

            let dialog = libadwaita::MessageDialog::builder()
                .heading("Change All Similar Documents?")
                .body(format!(
                    "Are you sure you want to change all similar documents to open with the application \"{app_name}\"?"
                ))
                .build();
            if let Some(ref win) = root_win {
                dialog.set_transient_for(Some(win));
            }
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("confirm", "Change All");
            dialog.set_response_appearance("confirm", libadwaita::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("confirm"));
            dialog.set_close_response("cancel");

            dialog.choose(None as Option<&gio::Cancellable>, move |resp| {
                if resp.as_str() == "confirm" {
                    let _ = set_default_app(&mime, &desktop_id);
                }
            });
        });

        Self {
            group,
            combo_row,
            change_all_btn,
            candidates,
            current_mime,
            current_paths,
            updating_combo,
        }
    }

    /// Binds single node MIME type applications.
    pub fn bind_single(&self, meta: &DeepMetadata) {
        *self.current_paths.borrow_mut() = vec![meta.path.clone()];
        (*self.current_mime.borrow_mut()).clone_from(&meta.mime_type);

        if meta.is_directory {
            self.group.set_visible(false);
            return;
        }

        let app_list = get_open_with_candidates(&[&meta.mime_type]).unwrap_or_default();
        self.populate_candidates(app_list);
        self.group.set_visible(true);
    }

    /// Binds multi-selection nodes.
    pub fn bind_multi(&self, paths: &[PathBuf]) {
        *self.current_paths.borrow_mut() = paths.to_vec();
        *self.current_mime.borrow_mut() = String::new();
        self.group.set_visible(false);
    }

    fn populate_candidates(&self, app_list: Vec<AppHandlerInfo>) {
        *self.updating_combo.borrow_mut() = true;

        let string_list = gtk4::StringList::new(&[]);
        let mut default_idx = 0;

        for (idx, app) in app_list.iter().enumerate() {
            let label = if app.is_default {
                default_idx = idx;
                format!("{} (default)", app.display_name)
            } else {
                app.display_name.clone()
            };
            string_list.append(&label);
        }

        if app_list.is_empty() {
            string_list.append("None Available");
            self.change_all_btn.set_sensitive(false);
        } else {
            self.change_all_btn.set_sensitive(true);
        }

        *self.candidates.borrow_mut() = app_list;
        self.combo_row.set_model(Some(&string_list));
        #[allow(clippy::cast_possible_truncation)]
        self.combo_row.set_selected(default_idx as u32);

        *self.updating_combo.borrow_mut() = false;
    }
}

impl Default for OpenWithGroup {
    fn default() -> Self {
        Self::new()
    }
}

/// Privilege dropdown item model.
fn privilege_string_list() -> gtk4::StringList {
    gtk4::StringList::new(&["Read & Write", "Read only", "Write only", "No access"])
}

/// "Sharing & Permissions" property group.
#[derive(Clone)]
pub struct SharingPermissionsGroup {
    /// Root libadwaita preferences group.
    pub group: libadwaita::PreferencesGroup,
    owner_row: libadwaita::ComboRow,
    group_row: libadwaita::ComboRow,
    others_row: libadwaita::ComboRow,
    octal_label: gtk4::Label,
    padlock_btn: gtk4::Button,
    apply_enclosed_btn: gtk4::Button,
    current_paths: Rc<RefCell<Vec<PathBuf>>>,
    current_mode: Rc<RefCell<u32>>,
    is_directory: Rc<RefCell<bool>>,
    updating_perms: Rc<RefCell<bool>>,
    is_unlocked: Rc<RefCell<bool>>,
}

impl SharingPermissionsGroup {
    /// Constructs a new [`SharingPermissionsGroup`].
    #[must_use]
    pub fn new() -> Self {
        let group = libadwaita::PreferencesGroup::builder()
            .title("Sharing &amp; Permissions")
            .build();

        let owner_row = libadwaita::ComboRow::builder().title("Owner").build();
        owner_row.set_model(Some(&privilege_string_list()));

        let group_row = libadwaita::ComboRow::builder().title("Group").build();
        group_row.set_model(Some(&privilege_string_list()));

        let others_row = libadwaita::ComboRow::builder().title("Everyone").build();
        others_row.set_model(Some(&privilege_string_list()));

        let octal_label = gtk4::Label::builder()
            .label("Octal: 0755")
            .halign(gtk4::Align::Start)
            .css_classes(["dim-label", "caption"])
            .build();

        let padlock_btn = gtk4::Button::builder()
            .icon_name("changes-allow-symbolic")
            .tooltip_text("Click padlock to make changes")
            .valign(gtk4::Align::Center)
            .css_classes(["flat", "inspector-padlock"])
            .build();

        let apply_enclosed_btn = gtk4::Button::builder()
            .label("Apply to enclosed items...")
            .valign(gtk4::Align::Center)
            .sensitive(false)
            .build();

        let bottom_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(8)
            .margin_top(8)
            .build();
        bottom_box.append(&padlock_btn);
        bottom_box.append(&octal_label);

        let spring = gtk4::Box::builder().hexpand(true).build();
        bottom_box.append(&spring);
        bottom_box.append(&apply_enclosed_btn);

        group.add(&owner_row);
        group.add(&group_row);
        group.add(&others_row);
        group.add(&bottom_box);

        let current_paths: Rc<RefCell<Vec<PathBuf>>> = Rc::new(RefCell::new(Vec::new()));
        let current_mode = Rc::new(RefCell::new(0o755));
        let is_directory = Rc::new(RefCell::new(false));
        let updating_perms = Rc::new(RefCell::new(false));
        let is_unlocked = Rc::new(RefCell::new(true));

        let spg = Self {
            group,
            owner_row,
            group_row,
            others_row,
            octal_label,
            padlock_btn,
            apply_enclosed_btn,
            current_paths,
            current_mode,
            is_directory,
            updating_perms,
            is_unlocked,
        };

        spg.wire_signals();
        spg
    }

    fn wire_signals(&self) {
        // Wire permission changes
        let weak_self = self.downgrade();
        let on_combo_changed = move || {
            if let Some(this) = weak_self.upgrade() {
                if *this.updating_perms.borrow() {
                    return;
                }
                this.commit_current_permissions();
            }
        };

        let cb1 = on_combo_changed.clone();
        self.owner_row.connect_selected_notify(move |_| cb1());

        let cb2 = on_combo_changed.clone();
        self.group_row.connect_selected_notify(move |_| cb2());

        let cb3 = on_combo_changed;
        self.others_row.connect_selected_notify(move |_| cb3());

        // Wire padlock elevation toggle
        let padlock_clone = self.padlock_btn.clone();
        let unlocked_clone = Rc::clone(&self.is_unlocked);
        let owner_clone = self.owner_row.clone();
        let group_clone = self.group_row.clone();
        let others_clone = self.others_row.clone();
        let paths_lock = Rc::clone(&self.current_paths);
        let mode_lock = Rc::clone(&self.current_mode);

        self.padlock_btn.connect_clicked(move |_btn| {
            let mut unlocked = unlocked_clone.borrow_mut();
            if !*unlocked {
                // Elevate via pkexec
                let paths = paths_lock.borrow();
                let mode = *mode_lock.borrow();
                if let Some(target) = paths.first() {
                    let _ = std::process::Command::new("pkexec")
                        .arg("chmod")
                        .arg(format!("{mode:04o}"))
                        .arg(target)
                        .spawn();
                }
                *unlocked = true;
                padlock_clone.set_icon_name("changes-allow-symbolic");
                owner_clone.set_sensitive(true);
                group_clone.set_sensitive(true);
                others_clone.set_sensitive(true);
            }
        });

        // Wire "Apply to enclosed items..."
        let paths_apply = Rc::clone(&self.current_paths);
        let mode_apply = Rc::clone(&self.current_mode);

        self.apply_enclosed_btn.connect_clicked(move |btn| {
            let paths = paths_apply.borrow().clone();
            let target_mode = *mode_apply.borrow();
            let root_win = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());

            let dialog = libadwaita::MessageDialog::builder()
                .heading("Apply to Enclosed Items?")
                .body("Are you sure you want to apply these permissions to all enclosed items?")
                .build();
            if let Some(ref win) = root_win {
                dialog.set_transient_for(Some(win));
            }
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("confirm", "Apply");
            dialog.set_response_appearance("confirm", libadwaita::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("confirm"));
            dialog.set_close_response("cancel");

            dialog.choose(None as Option<&gio::Cancellable>, move |resp| {
                if resp.as_str() == "confirm" {
                    for dir_path in paths {
                        apply_permissions_recursively(&dir_path, target_mode);
                    }
                }
            });
        });
    }

    fn downgrade(&self) -> WeakSharingPermissionsGroup {
        WeakSharingPermissionsGroup {
            owner_row: self.owner_row.downgrade(),
            group_row: self.group_row.downgrade(),
            others_row: self.others_row.downgrade(),
            octal_label: self.octal_label.downgrade(),
            current_paths: Rc::clone(&self.current_paths),
            current_mode: Rc::clone(&self.current_mode),
            is_directory: Rc::clone(&self.is_directory),
            updating_perms: Rc::clone(&self.updating_perms),
        }
    }

    /// Binds single node permissions.
    pub fn bind_single(&self, meta: &DeepMetadata) {
        *self.current_paths.borrow_mut() = vec![meta.path.clone()];
        *self.is_directory.borrow_mut() = meta.is_directory;
        *self.current_mode.borrow_mut() = meta.permissions.mode & 0o7777;

        let is_writable = meta.permissions.is_user_writable;
        *self.is_unlocked.borrow_mut() = is_writable;
        self.padlock_btn.set_icon_name(if is_writable {
            "changes-allow-symbolic"
        } else {
            "changes-prevent-symbolic"
        });
        self.owner_row.set_sensitive(is_writable);
        self.group_row.set_sensitive(is_writable);
        self.others_row.set_sensitive(is_writable);

        self.owner_row
            .set_title(&format!("Owner ({})", meta.permissions.owner_name));
        self.group_row
            .set_title(&format!("Group ({})", meta.permissions.group_name));
        self.others_row.set_title("Everyone");

        *self.updating_perms.borrow_mut() = true;
        let mode = meta.permissions.mode;
        self.owner_row
            .set_selected(bits_to_privilege((mode >> 6) & 0o7));
        self.group_row
            .set_selected(bits_to_privilege((mode >> 3) & 0o7));
        self.others_row.set_selected(bits_to_privilege(mode & 0o7));

        self.octal_label
            .set_label(&format!("Octal: {:04o}", mode & 0o7777));
        self.apply_enclosed_btn
            .set_sensitive(meta.is_directory && is_writable);
        *self.updating_perms.borrow_mut() = false;
        self.group.set_visible(true);
    }

    /// Binds multi-selection permissions.
    pub fn bind_multi(&self, summary: &MultiSelectionSummary, paths: &[PathBuf]) {
        *self.current_paths.borrow_mut() = paths.to_vec();
        *self.is_directory.borrow_mut() = summary.total_directories > 0;

        self.owner_row.set_title("Owner");
        self.group_row.set_title("Group");
        self.others_row.set_title("Everyone");

        *self.updating_perms.borrow_mut() = true;
        if let Some(mode) = summary.uniform_permissions {
            *self.current_mode.borrow_mut() = mode & 0o7777;
            self.owner_row
                .set_selected(bits_to_privilege((mode >> 6) & 0o7));
            self.group_row
                .set_selected(bits_to_privilege((mode >> 3) & 0o7));
            self.others_row.set_selected(bits_to_privilege(mode & 0o7));
            self.octal_label
                .set_label(&format!("Octal: {:04o}", mode & 0o7777));
        } else {
            self.octal_label.set_label("Octal: Mixed");
        }

        self.apply_enclosed_btn
            .set_sensitive(summary.total_directories > 0);
        *self.updating_perms.borrow_mut() = false;
        self.group.set_visible(true);
    }
}

impl Default for SharingPermissionsGroup {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct WeakSharingPermissionsGroup {
    owner_row: glib::WeakRef<libadwaita::ComboRow>,
    group_row: glib::WeakRef<libadwaita::ComboRow>,
    others_row: glib::WeakRef<libadwaita::ComboRow>,
    octal_label: glib::WeakRef<gtk4::Label>,
    current_paths: Rc<RefCell<Vec<PathBuf>>>,
    current_mode: Rc<RefCell<u32>>,
    is_directory: Rc<RefCell<bool>>,
    updating_perms: Rc<RefCell<bool>>,
}

impl WeakSharingPermissionsGroup {
    fn upgrade(&self) -> Option<SharingPermissionsGroupHandle> {
        let owner = self.owner_row.upgrade()?;
        let group = self.group_row.upgrade()?;
        let others = self.others_row.upgrade()?;
        let octal = self.octal_label.upgrade()?;
        Some(SharingPermissionsGroupHandle {
            owner_row: owner,
            group_row: group,
            others_row: others,
            octal_label: octal,
            current_paths: Rc::clone(&self.current_paths),
            current_mode: Rc::clone(&self.current_mode),
            is_directory: Rc::clone(&self.is_directory),
            updating_perms: Rc::clone(&self.updating_perms),
        })
    }
}

struct SharingPermissionsGroupHandle {
    owner_row: libadwaita::ComboRow,
    group_row: libadwaita::ComboRow,
    others_row: libadwaita::ComboRow,
    octal_label: gtk4::Label,
    current_paths: Rc<RefCell<Vec<PathBuf>>>,
    current_mode: Rc<RefCell<u32>>,
    is_directory: Rc<RefCell<bool>>,
    updating_perms: Rc<RefCell<bool>>,
}

impl SharingPermissionsGroupHandle {
    fn commit_current_permissions(&self) {
        let is_dir = *self.is_directory.borrow();
        let u_bits = privilege_to_bits(self.owner_row.selected(), is_dir);
        let g_bits = privilege_to_bits(self.group_row.selected(), is_dir);
        let o_bits = privilege_to_bits(self.others_row.selected(), is_dir);

        let new_mode = (u_bits << 6) | (g_bits << 3) | o_bits;
        *self.current_mode.borrow_mut() = new_mode;
        self.octal_label
            .set_label(&format!("Octal: {new_mode:04o}"));

        let paths = self.current_paths.borrow();
        for path in paths.iter() {
            let _ = apply_single_mode(path, new_mode);
        }
    }
}

fn apply_single_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    let mut perms = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(mode);
    }
    std::fs::set_permissions(path, perms)
}

fn apply_permissions_recursively(dir_path: &Path, mode: u32) {
    let _ = apply_single_mode(dir_path, mode);
    for ent in walkdir::WalkDir::new(dir_path)
        .min_depth(1)
        .into_iter()
        .flatten()
    {
        let ent_mode = if ent.file_type().is_dir() {
            mode | 0o111
        } else {
            mode & !0o111
        };
        let _ = apply_single_mode(ent.path(), ent_mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_privilege_bit_conversions() {
        // Read & Write (file): 0o6
        assert_eq!(privilege_to_bits(0, false), 0o6);
        // Read & Write (dir): 0o7
        assert_eq!(privilege_to_bits(0, true), 0o7);
        // Read only (file): 0o4
        assert_eq!(privilege_to_bits(1, false), 0o4);
        // Read only (dir): 0o5
        assert_eq!(privilege_to_bits(1, true), 0o5);
        // No access: 0
        assert_eq!(privilege_to_bits(3, false), 0);

        // Roundtrip
        assert_eq!(bits_to_privilege(0o7), 0);
        assert_eq!(bits_to_privilege(0o6), 0);
        assert_eq!(bits_to_privilege(0o5), 1);
        assert_eq!(bits_to_privilege(0o4), 1);
        assert_eq!(bits_to_privilege(0o2), 2);
        assert_eq!(bits_to_privilege(0o0), 3);
    }
}
