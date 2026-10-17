//! Resizable divider separator handles with auto-fit and uniform Alt-drag resizing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::prelude::*;

use crate::views::columns::models::{MAX_COLUMN_WIDTH, MIN_COLUMN_WIDTH};

/// Callback triggered when a single column is dragged to a new width `(column_index, new_width)`.
pub type ColumnResizeCallback = Box<dyn Fn(usize, i32)>;
/// Callback triggered when Alt-drag uniformly resizes all columns to `new_width`.
pub type UniformResizeCallback = Box<dyn Fn(i32)>;
/// Callback triggered on double-click to snap column width to content `(column_index)`.
pub type AutoFitCallback = Box<dyn Fn(usize)>;

struct DividerInner {
    column_index: usize,
    initial_width: Cell<i32>,
    is_alt_drag: Cell<bool>,
    on_resize: RefCell<Option<ColumnResizeCallback>>,
    on_uniform_resize: RefCell<Option<UniformResizeCallback>>,
    on_auto_fit: RefCell<Option<AutoFitCallback>>,
}

impl std::fmt::Debug for DividerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DividerInner")
            .field("column_index", &self.column_index)
            .field("initial_width", &self.initial_width)
            .field("is_alt_drag", &self.is_alt_drag)
            .finish_non_exhaustive()
    }
}

/// Divider handle placed between columns supporting dynamic drag and double-click auto-fit.
#[derive(Debug, Clone)]
pub struct ColumnDividerWidget {
    /// Root divider visual widget.
    pub widget: gtk4::Box,
    inner: Rc<DividerInner>,
}

impl ColumnDividerWidget {
    /// Constructs a new [`ColumnDividerWidget`] separating the column at `column_index`.
    #[must_use]
    pub fn new(column_index: usize, current_width: i32) -> Self {
        let widget = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .css_classes(["column-divider"])
            .width_request(5)
            .vexpand(true)
            .build();

        // Col-resize cursor
        widget.set_cursor_from_name(Some("col-resize"));

        let inner = Rc::new(DividerInner {
            column_index,
            initial_width: Cell::new(current_width),
            is_alt_drag: Cell::new(false),
            on_resize: RefCell::new(None),
            on_uniform_resize: RefCell::new(None),
            on_auto_fit: RefCell::new(None),
        });

        let divider = Self { widget, inner };
        divider.wire_gestures();

        divider
    }

    /// Updates the baseline initial width recorded for subsequent drag deltas.
    pub fn set_current_width(&self, width: i32) {
        self.inner.initial_width.set(width);
    }

    /// Registers a callback triggered when resizing the left column.
    pub fn connect_resize<F: Fn(usize, i32) + 'static>(&self, callback: F) {
        *self.inner.on_resize.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback triggered when Alt/Option-dragging to resize all columns uniformly.
    pub fn connect_uniform_resize<F: Fn(i32) + 'static>(&self, callback: F) {
        *self.inner.on_uniform_resize.borrow_mut() = Some(Box::new(callback));
    }

    /// Registers a callback triggered when double-clicking to auto-fit content.
    pub fn connect_auto_fit<F: Fn(usize) + 'static>(&self, callback: F) {
        *self.inner.on_auto_fit.borrow_mut() = Some(Box::new(callback));
    }

    fn wire_gestures(&self) {
        let drag = gtk4::GestureDrag::new();
        let inner_drag = Rc::clone(&self.inner);
        let widget_drag = self.widget.clone();

        drag.connect_drag_begin(move |gesture, _x, _y| {
            let state = gesture.current_event_state();
            let is_alt = state.contains(gdk4::ModifierType::ALT_MASK);
            inner_drag.is_alt_drag.set(is_alt);
            widget_drag.add_css_class("dragging");
        });

        let inner_update = Rc::clone(&self.inner);
        drag.connect_drag_update(move |gesture, offset_x, _offset_y| {
            let state = gesture.current_event_state();
            let is_alt =
                state.contains(gdk4::ModifierType::ALT_MASK) || inner_update.is_alt_drag.get();
            #[allow(clippy::cast_possible_truncation)]
            let delta = offset_x as i32;
            let start = inner_update.initial_width.get();
            let new_width = (start + delta).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);

            if is_alt {
                if let Some(ref cb) = *inner_update.on_uniform_resize.borrow() {
                    cb(new_width);
                }
            } else if let Some(ref cb) = *inner_update.on_resize.borrow() {
                cb(inner_update.column_index, new_width);
            }
        });

        let inner_end = Rc::clone(&self.inner);
        let widget_end = self.widget.clone();
        drag.connect_drag_end(move |_gesture, offset_x, _offset_y| {
            widget_end.remove_css_class("dragging");
            #[allow(clippy::cast_possible_truncation)]
            let delta = offset_x as i32;
            let start = inner_end.initial_width.get();
            let final_width = (start + delta).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
            inner_end.initial_width.set(final_width);
        });

        self.widget.add_controller(drag);

        let click = gtk4::GestureClick::new();
        let inner_click = Rc::clone(&self.inner);
        click.connect_pressed(move |_gesture, n_press, _x, _y| {
            if n_press == 2
                && let Some(ref cb) = *inner_click.on_auto_fit.borrow()
            {
                cb(inner_click.column_index);
            }
        });

        self.widget.add_controller(click);
    }
}
