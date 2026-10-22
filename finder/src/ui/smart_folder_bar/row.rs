//! Individual criteria slice row widgets for the smart folder builder bar.

use std::cell::Cell;
use std::rc::Rc;

use gtk4::prelude::*;

use super::SmartFolderBar;
use crate::smart_folders::{
    CriteriaAttribute, CriteriaOperator, CriteriaSlice, size_value_to_bytes,
};

const ATTRIBUTES: [CriteriaAttribute; 6] = [
    CriteriaAttribute::Kind,
    CriteriaAttribute::Name,
    CriteriaAttribute::DateModified,
    CriteriaAttribute::DateCreated,
    CriteriaAttribute::Tag,
    CriteriaAttribute::Size,
];

const KIND_OPTIONS: [&str; 13] = [
    "Any",
    "Documents",
    "Images",
    "PDF",
    "Audio",
    "Video",
    "Source Code",
    "Archives",
    "Presentations",
    "Spreadsheets",
    "Folders",
    "Applications",
    "Fonts",
];

const SIZE_UNITS: [(&str, i64); 3] = [
    ("KB", 1024),
    ("MB", 1024 * 1024),
    ("GB", 1024 * 1024 * 1024),
];

const NAME_OPERATORS: [CriteriaOperator; 4] = [
    CriteriaOperator::Contains,
    CriteriaOperator::Equals,
    CriteriaOperator::StartsWith,
    CriteriaOperator::EndsWith,
];

const SIZE_OPERATORS: [CriteriaOperator; 3] = [
    CriteriaOperator::GreaterThan,
    CriteriaOperator::LessThan,
    CriteriaOperator::Equals,
];

fn index_to_u32(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

fn indent(depth: usize) -> i32 {
    i32::try_from(depth.saturating_mul(18)).unwrap_or(i32::MAX)
}

fn count_to_f64(value: i64) -> f64 {
    f64::from(i32::try_from(value).unwrap_or(i32::MAX))
}

fn attribute_index(attribute: CriteriaAttribute) -> u32 {
    ATTRIBUTES
        .iter()
        .position(|candidate| *candidate == attribute)
        .map_or(0, index_to_u32)
}

fn attribute_at(index: u32) -> CriteriaAttribute {
    ATTRIBUTES
        .get(index as usize)
        .copied()
        .unwrap_or(CriteriaAttribute::Kind)
}

const fn operators_for(attribute: CriteriaAttribute) -> &'static [CriteriaOperator] {
    match attribute {
        CriteriaAttribute::Kind => &[CriteriaOperator::Equals],
        CriteriaAttribute::Name => &NAME_OPERATORS,
        CriteriaAttribute::DateModified | CriteriaAttribute::DateCreated => {
            &[CriteriaOperator::WithinLastDays]
        }
        CriteriaAttribute::Tag => &[CriteriaOperator::Equals, CriteriaOperator::Contains],
        CriteriaAttribute::Size => &SIZE_OPERATORS,
    }
}

/// Builds one rule row for a slice at `group_index`/`slice_index`.
pub(super) fn build_rule_row(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
    depth: usize,
    show_remove: bool,
) -> gtk4::Box {
    let row = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .margin_start(indent(depth))
        .build();

    row.append(&build_attribute_dropdown(
        bar,
        group_index,
        slice_index,
        slice,
    ));
    row.append(&build_operator_dropdown(
        bar,
        group_index,
        slice_index,
        slice,
    ));

    let value = build_value_widget(bar, group_index, slice_index, slice);
    value.set_hexpand(true);
    row.append(&value);

    let warning = gtk4::Label::builder()
        .label(slice.warning_message.as_deref().unwrap_or(""))
        .css_classes(["caption", "error"])
        .visible(!slice.is_valid)
        .build();
    row.append(&warning);
    bar.row_warnings
        .borrow_mut()
        .push((group_index, slice_index, warning));

    if show_remove {
        let remove = gtk4::Button::builder()
            .label("[-]")
            .tooltip_text("Remove this rule")
            .css_classes(["flat"])
            .build();
        let bar = bar.clone();
        remove.connect_clicked(move |_| {
            bar.remove_slice(group_index, slice_index, depth);
        });
        row.append(&remove);
    }

    row
}

fn build_attribute_dropdown(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
) -> gtk4::DropDown {
    let labels: Vec<&str> = ATTRIBUTES.iter().map(CriteriaAttribute::label).collect();
    let dropdown = gtk4::DropDown::from_strings(&labels);
    dropdown.set_selected(attribute_index(slice.attribute));
    dropdown.set_tooltip_text(Some("File attribute"));

    let bar = bar.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        let attribute = attribute_at(dropdown.selected());
        bar.builder
            .borrow_mut()
            .set_slice_attribute(group_index, slice_index, attribute);
        bar.rebuild_and_notify();
    });
    dropdown
}

fn build_operator_dropdown(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
) -> gtk4::DropDown {
    let operators = operators_for(slice.attribute);
    let labels: Vec<&str> = operators.iter().map(CriteriaOperator::label).collect();
    let dropdown = gtk4::DropDown::from_strings(&labels);
    let selected = operators
        .iter()
        .position(|operator| *operator == slice.operator)
        .map_or(0, index_to_u32);
    dropdown.set_selected(selected);
    dropdown.set_tooltip_text(Some("Comparison operator"));

    let operators = operators.to_vec();
    let bar = bar.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        let Some(operator) = operators.get(dropdown.selected() as usize).copied() else {
            return;
        };
        bar.builder
            .borrow_mut()
            .set_slice_operator(group_index, slice_index, operator);
        bar.rebuild_and_notify();
    });
    dropdown
}

fn build_value_widget(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
) -> gtk4::Box {
    match slice.attribute {
        CriteriaAttribute::Kind => build_kind_value(bar, group_index, slice_index, slice),
        CriteriaAttribute::Name | CriteriaAttribute::Tag => {
            build_text_value(bar, group_index, slice_index, slice)
        }
        CriteriaAttribute::DateModified | CriteriaAttribute::DateCreated => {
            build_date_value(bar, group_index, slice_index, slice)
        }
        CriteriaAttribute::Size => build_size_value(bar, group_index, slice_index, slice),
    }
}

fn build_kind_value(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
) -> gtk4::Box {
    let container = value_container();
    let dropdown = gtk4::DropDown::from_strings(&KIND_OPTIONS);
    let current = slice.value.as_text().unwrap_or("Any");
    let selected = KIND_OPTIONS
        .iter()
        .position(|option| option.eq_ignore_ascii_case(current))
        .map_or(0, index_to_u32);
    dropdown.set_selected(selected);
    dropdown.set_hexpand(true);

    let bar = bar.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        let option = KIND_OPTIONS
            .get(dropdown.selected() as usize)
            .copied()
            .unwrap_or("Any");
        bar.builder
            .borrow_mut()
            .set_slice_text(group_index, slice_index, option);
        bar.notify_changed();
    });

    container.append(&dropdown);
    container
}

fn build_text_value(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
) -> gtk4::Box {
    let container = value_container();
    let entry = gtk4::Entry::builder()
        .text(slice.value.as_text().unwrap_or_default())
        .placeholder_text(if slice.attribute == CriteriaAttribute::Tag {
            "Tag name or color..."
        } else {
            "File name contains..."
        })
        .hexpand(true)
        .build();

    let bar = bar.clone();
    entry.connect_changed(move |entry| {
        bar.builder
            .borrow_mut()
            .set_slice_text(group_index, slice_index, entry.text().as_str());
        bar.notify_changed();
    });

    container.append(&entry);
    container
}

fn build_date_value(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
) -> gtk4::Box {
    let container = value_container();
    let days = slice.value.as_days().unwrap_or(7);
    let spin = gtk4::SpinButton::with_range(0.0, 36_500.0, 1.0);
    spin.set_value(f64::from(days));
    spin.set_hexpand(true);

    let bar = bar.clone();
    spin.connect_value_changed(move |spin| {
        let value = u32::try_from(spin.value_as_int().clamp(0, 36_500)).unwrap_or(0);
        bar.builder
            .borrow_mut()
            .set_slice_days(group_index, slice_index, value);
        bar.notify_changed();
    });

    container.append(&spin);
    container.append(
        &gtk4::Label::builder()
            .label("days")
            .css_classes(["caption", "dim-label"])
            .build(),
    );
    container
}

fn build_size_value(
    bar: &SmartFolderBar,
    group_index: usize,
    slice_index: usize,
    slice: &CriteriaSlice,
) -> gtk4::Box {
    let container = value_container();
    let bytes = slice.value.as_i64().unwrap_or(0).max(0);
    let unit_index = if bytes >= SIZE_UNITS[2].1 {
        2
    } else {
        usize::from(bytes >= SIZE_UNITS[1].1)
    };
    let multiplier = SIZE_UNITS[unit_index].1;
    let display = if bytes == 0 {
        0.0
    } else {
        count_to_f64((bytes / multiplier).max(1))
    };

    let spin = gtk4::SpinButton::with_range(0.0, 1_000_000.0, 1.0);
    spin.set_value(display);
    spin.set_hexpand(true);

    let unit_labels: Vec<&str> = SIZE_UNITS.iter().map(|(label, _)| *label).collect();
    let unit_dropdown = gtk4::DropDown::from_strings(&unit_labels);
    unit_dropdown.set_selected(index_to_u32(unit_index));

    let updating = Rc::new(Cell::new(false));

    let bar_for_spin = bar.clone();
    let updating_for_spin = Rc::clone(&updating);
    let unit_for_spin = unit_dropdown.clone();
    spin.connect_value_changed(move |spin| {
        if updating_for_spin.get() {
            return;
        }
        let index = unit_for_spin.selected() as usize;
        let multiplier = SIZE_UNITS.get(index).map_or(1024, |(_, factor)| *factor);
        let value = size_value_to_bytes(i64::from(spin.value_as_int().max(0)), multiplier);
        bar_for_spin
            .builder
            .borrow_mut()
            .set_slice_size_bytes(group_index, slice_index, value);
        bar_for_spin.notify_changed();
    });

    let bar_for_unit = bar.clone();
    let updating_for_unit = Rc::clone(&updating);
    let spin_for_unit = spin.clone();
    let current_unit = Rc::new(Cell::new(unit_index));
    unit_dropdown.connect_selected_notify(move |dropdown| {
        let index = dropdown.selected() as usize;
        let multiplier = SIZE_UNITS.get(index).map_or(1024, |(_, factor)| *factor);
        let previous = SIZE_UNITS
            .get(current_unit.get())
            .map_or(1024, |(_, factor)| *factor);
        let bytes = size_value_to_bytes(i64::from(spin_for_unit.value_as_int().max(0)), previous);
        let display = if bytes == 0 {
            0.0
        } else {
            count_to_f64((bytes / multiplier).max(1))
        };
        updating_for_unit.set(true);
        spin_for_unit.set_value(display);
        updating_for_unit.set(false);
        current_unit.set(index);
        bar_for_unit
            .builder
            .borrow_mut()
            .set_slice_size_bytes(group_index, slice_index, bytes);
        bar_for_unit.notify_changed();
    });

    container.append(&spin);
    container.append(&unit_dropdown);
    container
}

fn value_container() -> gtk4::Box {
    gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(4)
        .build()
}
