//! Reusable context menu and drag-and-drop attachment for file and directory rows.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gdk4::prelude::*;
use gtk4::prelude::*;
use shared::models::operation::{BatchOperation, OperationKind};

use crate::breadcrumbs::parse_uri_list;
use crate::coordinator::NavigationCoordinator;

/// Attaches a right-click context menu (Cut, Copy, Paste, Duplicate, Move to Trash, Get Info)
/// to a file or directory row widget.
pub fn attach_row_context_menu(
    row: &gtk4::ListBoxRow,
    path: &Path,
    is_directory: bool,
    coordinator: &Rc<NavigationCoordinator>,
) {
    let click_gesture = gtk4::GestureClick::new();
    click_gesture.set_button(gdk4::BUTTON_SECONDARY);

    let row_weak = row.downgrade();
    let coord = Rc::clone(coordinator);
    let item_path = path.to_path_buf();

    click_gesture.connect_pressed(move |_gesture, _n_press, _x, _y| {
        let Some(row_widget) = row_weak.upgrade() else {
            return;
        };

        coord.set_selection(Some(&item_path));

        let popover = gtk4::Popover::builder()
            .has_arrow(true)
            .css_classes(["context-menu-popover"])
            .build();
        popover.set_parent(&row_widget);

        let menu_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(2)
            .margin_start(4)
            .margin_end(4)
            .margin_top(4)
            .margin_bottom(4)
            .build();

        // 1. Cut
        let c_cut = Rc::clone(&coord);
        let p_cut = item_path.clone();
        menu_box.append(&create_menu_button("Cut", "Ctrl+X", &popover, move || {
            c_cut.cut_paths(vec![p_cut.clone()]);
        }));

        // 2. Copy
        let c_copy = Rc::clone(&coord);
        let p_copy = item_path.clone();
        menu_box.append(&create_menu_button("Copy", "Ctrl+C", &popover, move || {
            c_copy.copy_paths(vec![p_copy.clone()]);
        }));

        // 3. Paste
        let c_paste = Rc::clone(&coord);
        let p_paste = if is_directory {
            item_path.clone()
        } else {
            item_path
                .parent()
                .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
        };
        menu_box.append(&create_menu_button(
            "Paste",
            "Ctrl+V",
            &popover,
            move || {
                let _ = c_paste.paste_paths_to(&p_paste);
            },
        ));

        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // 4. Duplicate
        let c_dup = Rc::clone(&coord);
        let p_dup = item_path.clone();
        menu_box.append(&create_menu_button(
            "Duplicate",
            "Ctrl+D",
            &popover,
            move || {
                let _ = c_dup.duplicate_paths(vec![p_dup.clone()]);
            },
        ));

        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // 5. Move to Trash
        let c_trash = Rc::clone(&coord);
        let p_trash = item_path.clone();
        menu_box.append(&create_menu_button(
            "Move to Trash",
            "Delete",
            &popover,
            move || {
                let _ = c_trash.trash_paths(vec![p_trash.clone()]);
            },
        ));

        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // 6. Get Info
        menu_box.append(&create_menu_button("Get Info", "Ctrl+I", &popover, || {}));

        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // 7. Color Swatches and Tags
        let tagging_section = crate::ui::tagging::create_tags_context_menu_widget(
            std::slice::from_ref(&item_path),
            &row_widget,
        );
        menu_box.append(&tagging_section);

        popover.set_child(Some(&menu_box));
        popover.connect_closed(|p| {
            p.unparent();
        });
        popover.popup();
    });

    row.add_controller(click_gesture);
}

/// Attaches a GTK4 [`DropTarget`] to a directory row widget, determining whether source and
/// target are on the same filesystem and scheduling `Move` or `Copy` operations.
pub fn attach_directory_drop_target(
    row: &gtk4::ListBoxRow,
    target_dir: &Path,
    coordinator: &Rc<NavigationCoordinator>,
) {
    let drop_target = gtk4::DropTarget::new(
        glib::Type::STRING,
        gdk4::DragAction::COPY | gdk4::DragAction::MOVE,
    );

    let row_weak = row.downgrade();
    drop_target.connect_enter(move |_dt, _x, _y| {
        if let Some(r) = row_weak.upgrade() {
            r.add_css_class("drop-target");
        }
        gdk4::DragAction::COPY
    });

    let row_weak_leave = row.downgrade();
    drop_target.connect_leave(move |_dt| {
        if let Some(r) = row_weak_leave.upgrade() {
            r.remove_css_class("drop-target");
        }
    });

    let row_weak_drop = row.downgrade();
    let coord = Rc::clone(coordinator);
    let dest_path = target_dir.to_path_buf();

    drop_target.connect_drop(move |_dt, value, _x, _y| {
        if let Some(r) = row_weak_drop.upgrade() {
            r.remove_css_class("drop-target");
        }

        if let Ok(raw_str) = value.get::<String>() {
            let sources = parse_uri_list(&raw_str);
            if sources.is_empty() {
                return false;
            }

            let is_same_fs = sources
                .first()
                .and_then(|src| shared::fs::is_same_filesystem(src, &dest_path).ok())
                .unwrap_or(false);

            let kind = if is_same_fs {
                OperationKind::Move
            } else {
                OperationKind::Copy
            };

            let op_id = format!(
                "dnd-{}-{}",
                if is_same_fs { "move" } else { "copy" },
                uuid::Uuid::new_v4()
            );
            let op = BatchOperation::new(op_id, kind, sources, Some(dest_path.clone()));
            coord.operations_queue.enqueue_operation(op);
            return true;
        }

        false
    });

    row.add_controller(drop_target);
}

fn create_menu_button<F: Fn() + 'static>(
    title: &str,
    shortcut: &str,
    popover: &gtk4::Popover,
    on_click: F,
) -> gtk4::Button {
    let btn = gtk4::Button::builder()
        .has_frame(false)
        .css_classes(["flat", "context-menu-item"])
        .build();

    let hbox = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(24)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let title_label = gtk4::Label::builder()
        .label(title)
        .halign(gtk4::Align::Start)
        .hexpand(true)
        .build();

    let accel_label = gtk4::Label::builder()
        .label(shortcut)
        .halign(gtk4::Align::End)
        .css_classes(["dim-label"])
        .build();

    hbox.append(&title_label);
    hbox.append(&accel_label);
    btn.set_child(Some(&hbox));

    let popover_clone = popover.clone();
    btn.connect_clicked(move |_| {
        popover_clone.popdown();
        on_click();
    });

    btn
}
