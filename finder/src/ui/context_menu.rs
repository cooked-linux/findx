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
#[allow(clippy::too_many_lines)]
pub fn attach_row_context_menu(
    row: &impl IsA<gtk4::Widget>,
    path: &Path,
    is_directory: bool,
    coordinator: &Rc<NavigationCoordinator>,
) {
    let click_gesture = gtk4::GestureClick::new();
    click_gesture.set_button(gdk4::BUTTON_SECONDARY);

    let row_weak = row.upcast_ref::<gtk4::Widget>().downgrade();
    let coord = Rc::clone(coordinator);
    let item_path = path.to_path_buf();

    click_gesture.connect_pressed(move |_gesture, _n_press, _x, _y| {
        let Some(row_widget) = row_weak.upgrade() else {
            return;
        };

        let selected_paths = coord.selected_paths();
        let target_paths = if selected_paths.contains(&item_path) {
            selected_paths
        } else {
            coord.set_selected_paths(vec![item_path.clone()]);
            vec![item_path.clone()]
        };

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

        // 1. Color Swatches at top
        let swatches = crate::ui::tagging::ColorSwatchBar::new();
        let summary = crate::ui::tagging::MultiSelectionSummary::for_paths(&target_paths);
        swatches.set_active_colors(&summary.shared_colors, &summary.partial_colors);
        let p_swatch = target_paths.clone();
        swatches.connect_color_toggled(move |color, active| {
            crate::ui::tagging::toggle_color_on_paths(&p_swatch, color, active);
        });
        menu_box.append(&swatches.container);

        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // 2. Quick Look "filename" (with Eye icon)
        let c_ql = Rc::clone(&coord);
        let p_ql = target_paths.clone();
        let file_name = item_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Item");
        let ql_label = if target_paths.len() > 1 {
            format!("Quick Look {} Items", target_paths.len())
        } else {
            format!("Quick Look \"{file_name}\"")
        };
        menu_box.append(&create_menu_button_with_icon(
            &ql_label,
            "Space",
            Some("eye-symbolic"),
            &popover,
            move || {
                c_ql.set_selected_paths(p_ql.clone());
                c_ql.toggle_quicklook(None::<&gtk4::Window>);
            },
        ));

        // 3. Get Info (Ctrl+I)
        let c_info = Rc::clone(&coord);
        let p_info = target_paths.clone();
        menu_box.append(&create_menu_button(
            "Get Info",
            "Ctrl+I",
            &popover,
            move || {
                c_info.set_selected_paths(p_info.clone());
                let _ = c_info.open_get_info(None::<&gtk4::Window>);
            },
        ));

        // 4. Tags...
        let row_anchor = row_widget;
        let p_tags = target_paths.clone();
        let pop_tags = popover.clone();
        menu_box.append(&create_menu_button("Tags...", "", &popover, move || {
            pop_tags.popdown();
            let floating_popover = gtk4::Popover::builder()
                .has_arrow(true)
                .autohide(true)
                .css_classes(["tags-editor-popover"])
                .build();
            floating_popover.set_parent(&row_anchor);
            let editor = crate::ui::tagging::TagPillEditor::new();
            let current_summary = crate::ui::tagging::MultiSelectionSummary::for_paths(&p_tags);
            editor.set_tags(&current_summary.shared_tags, &current_summary.partial_tags);

            let paths_add = p_tags.clone();
            editor.connect_tag_added(move |tag| {
                crate::ui::tagging::apply_tag_to_paths(&paths_add, &tag);
            });

            let paths_rem = p_tags.clone();
            editor.connect_tag_removed(move |tag_name| {
                crate::ui::tagging::remove_tag_from_paths(&paths_rem, &tag_name);
            });

            floating_popover.set_child(Some(&editor.container));
            floating_popover.connect_closed(|p| {
                p.unparent();
            });
            floating_popover.popup();
        }));

        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // 5. Cut
        let c_cut = Rc::clone(&coord);
        let p_cut = target_paths.clone();
        menu_box.append(&create_menu_button("Cut", "Ctrl+X", &popover, move || {
            c_cut.cut_paths(p_cut.clone());
        }));

        // 6. Copy
        let c_copy = Rc::clone(&coord);
        let p_copy = target_paths.clone();
        menu_box.append(&create_menu_button("Copy", "Ctrl+C", &popover, move || {
            c_copy.copy_paths(p_copy.clone());
        }));

        // 7. Paste
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

        // 8. Duplicate
        let c_dup = Rc::clone(&coord);
        let p_dup = target_paths.clone();
        menu_box.append(&create_menu_button(
            "Duplicate",
            "Ctrl+D",
            &popover,
            move || {
                let _ = c_dup.duplicate_paths(p_dup.clone());
            },
        ));

        menu_box.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

        // 9. Move to Trash
        let c_trash = Rc::clone(&coord);
        let p_trash = target_paths;
        menu_box.append(&create_menu_button(
            "Move to Trash",
            "Delete",
            &popover,
            move || {
                let _ = c_trash.trash_paths(p_trash.clone());
            },
        ));

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
    row: &impl IsA<gtk4::Widget>,
    target_dir: &Path,
    coordinator: &Rc<NavigationCoordinator>,
) {
    let drop_target = gtk4::DropTarget::new(
        glib::Type::STRING,
        gdk4::DragAction::COPY | gdk4::DragAction::MOVE,
    );

    let row_widget = row.upcast_ref::<gtk4::Widget>();
    let row_weak = row_widget.downgrade();
    drop_target.connect_enter(move |_dt, _x, _y| {
        if let Some(r) = row_weak.upgrade() {
            r.add_css_class("drop-target");
        }
        gdk4::DragAction::COPY
    });

    let row_weak_leave = row_widget.downgrade();
    drop_target.connect_leave(move |_dt| {
        if let Some(r) = row_weak_leave.upgrade() {
            r.remove_css_class("drop-target");
        }
    });

    let row_weak_drop = row_widget.downgrade();
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

fn create_menu_button_with_icon<F: Fn() + 'static>(
    title: &str,
    shortcut: &str,
    icon_name: Option<&str>,
    popover: &gtk4::Popover,
    on_click: F,
) -> gtk4::Button {
    let btn = gtk4::Button::builder()
        .has_frame(false)
        .css_classes(["flat", "context-menu-item"])
        .build();

    let hbox = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    if let Some(icon) = icon_name {
        let img = gtk4::Image::builder()
            .icon_name(icon)
            .pixel_size(16)
            .build();
        hbox.append(&img);
    }

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

fn create_menu_button<F: Fn() + 'static>(
    title: &str,
    shortcut: &str,
    popover: &gtk4::Popover,
    on_click: F,
) -> gtk4::Button {
    create_menu_button_with_icon(title, shortcut, None, popover, on_click)
}
