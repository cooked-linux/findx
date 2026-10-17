//! Hero card presentation widget for calculation and dimensional conversion results.
//!
//! Replicates the 68px typographic hero card styling from the design system,
//! mapping [`CalcResult`] variants into signature `.card`, `.cx`, `.cv`, and `.ck`
//! layouts, with one-key clipboard copying (`Enter` / `Ctrl+C`) and format cycling (`Tab`).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use gdk4::prelude::*;
use gtk4::prelude::*;
use shared::calc::CalcResult;

/// Format representations for integer-like mathematical calculation results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MathRepresentation {
    /// Standard base-10 decimal notation (e.g. `255`).
    #[default]
    Decimal,
    /// Uppercase hexadecimal notation with `0x` prefix (e.g. `0xFF`).
    Hexadecimal,
    /// Binary notation with `0b` prefix (e.g. `0b11111111`).
    Binary,
    /// Scientific exponential notation with Unicode superscripts (e.g. `2.55 × 10²`).
    Scientific,
}

impl MathRepresentation {
    /// Returns the next representation in cyclic order.
    #[must_use]
    pub const fn next(self, is_integer: bool) -> Self {
        if is_integer {
            match self {
                Self::Decimal => Self::Hexadecimal,
                Self::Hexadecimal => Self::Binary,
                Self::Binary => Self::Scientific,
                Self::Scientific => Self::Decimal,
            }
        } else {
            match self {
                Self::Decimal => Self::Scientific,
                Self::Scientific | Self::Hexadecimal | Self::Binary => Self::Decimal,
            }
        }
    }
}

type DismissCallback = Box<dyn Fn()>;
type CopyCallback = Box<dyn Fn(&str)>;

/// Signature Spotlight Hero Card widget presenting calculated or converted values.
#[derive(Clone)]
pub struct CalcHeroCardWidget {
    /// Root horizontal card container with `.card` CSS class.
    pub container: gtk4::Box,
    /// Expression or source input confirmation label with `.cx` CSS class.
    pub expr_label: gtk4::Label,
    /// Prominent 68px value display label with `.cv` CSS class.
    pub value_label: gtk4::Label,
    /// Subtle category kind label with `.ck` CSS class ("Calculator" or "Conversion").
    pub kind_label: gtk4::Label,
    calc_result: Rc<RefCell<CalcResult>>,
    current_rep: Rc<Cell<MathRepresentation>>,
    dismiss_cbs: Rc<RefCell<Vec<DismissCallback>>>,
    copy_cbs: Rc<RefCell<Vec<CopyCallback>>>,
}

impl fmt::Debug for CalcHeroCardWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CalcHeroCardWidget")
            .field("container", &self.container)
            .field("expr_label", &self.expr_label)
            .field("value_label", &self.value_label)
            .field("kind_label", &self.kind_label)
            .field("current_rep", &self.current_rep.get())
            .finish_non_exhaustive()
    }
}

impl CalcHeroCardWidget {
    /// Constructs a new [`CalcHeroCardWidget`] populated from a [`CalcResult`].
    #[must_use]
    pub fn new(calc_result: &CalcResult) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        container.add_css_class("card");
        container.set_focusable(true);

        let left_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        left_box.set_hexpand(true);
        left_box.set_valign(gtk4::Align::Center);

        let expr_label = gtk4::Label::new(None);
        expr_label.add_css_class("cx");
        expr_label.set_xalign(0.0);

        let value_label = gtk4::Label::new(None);
        value_label.add_css_class("cv");
        value_label.set_xalign(0.0);

        left_box.append(&expr_label);
        left_box.append(&value_label);

        let kind_label = gtk4::Label::new(None);
        kind_label.add_css_class("ck");
        kind_label.set_valign(gtk4::Align::End);
        kind_label.set_halign(gtk4::Align::End);

        container.append(&left_box);
        container.append(&kind_label);

        let calc_result_rc = Rc::new(RefCell::new(calc_result.clone()));
        let current_rep = Rc::new(Cell::new(MathRepresentation::Decimal));
        let dismiss_cbs = Rc::new(RefCell::new(Vec::new()));
        let copy_cbs = Rc::new(RefCell::new(Vec::new()));

        let widget = Self {
            container,
            expr_label,
            value_label,
            kind_label,
            calc_result: calc_result_rc,
            current_rep,
            dismiss_cbs,
            copy_cbs,
        };

        widget.refresh_display();
        widget.setup_key_controller();

        widget
    }

    /// Sets up the keyboard event controller for Tab cycling and Enter/Ctrl+C copying.
    fn setup_key_controller(&self) {
        let key_controller = gtk4::EventControllerKey::new();
        let widget_clone = self.clone();

        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if widget_clone.handle_key_press(key, modifiers) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });

        self.container.add_controller(key_controller);
    }

    /// Handles keyboard navigation and copy shortcut events.
    ///
    /// Returns `true` if the key press was handled.
    #[must_use]
    pub fn handle_key_press(&self, key: gdk4::Key, modifiers: gdk4::ModifierType) -> bool {
        // Tab / ISO Left Tab: Cycle through alternative representations
        if key == gdk4::Key::Tab || key == gdk4::Key::ISO_Left_Tab {
            let _ = self.cycle_representation();
            return true;
        }

        // Enter: Copy to clipboard and dismiss
        if key == gdk4::Key::Return || key == gdk4::Key::KP_Enter {
            self.copy_and_dismiss();
            return true;
        }

        // Ctrl+C / Cmd+C / Super+C: Copy to clipboard and dismiss
        let ctrl_or_cmd = modifiers.contains(gdk4::ModifierType::CONTROL_MASK)
            || modifiers.contains(gdk4::ModifierType::SUPER_MASK)
            || modifiers.contains(gdk4::ModifierType::META_MASK);

        if (key == gdk4::Key::c || key == gdk4::Key::C) && ctrl_or_cmd {
            self.copy_and_dismiss();
            return true;
        }

        false
    }

    /// Updates the card to display a new [`CalcResult`].
    pub fn set_result(&self, calc_result: &CalcResult) {
        *self.calc_result.borrow_mut() = calc_result.clone();
        self.current_rep.set(MathRepresentation::Decimal);
        self.refresh_display();
    }

    /// Re-renders all labels based on the active [`CalcResult`] and [`MathRepresentation`].
    pub fn refresh_display(&self) {
        let res = self.calc_result.borrow().clone();
        match res {
            CalcResult::Math {
                expression,
                result,
                formatted,
            } => {
                let pretty_expr = format_math_expression(&expression);
                self.expr_label.set_text(&pretty_expr);

                let is_integer = is_integer_like(result);
                let val_str = format_math_representation(
                    result,
                    &formatted,
                    self.current_rep.get(),
                    is_integer,
                );
                self.value_label.set_text(&val_str);
                self.kind_label.set_text("Calculator");
            }
            CalcResult::UnitConversion {
                source_val,
                source_unit,
                target_val,
                target_unit,
                formatted,
            } => {
                let src_text = format_unit_source(source_val, &source_unit);
                self.expr_label.set_text(&src_text);

                let target_text = format_unit_target(target_val, &target_unit, &formatted);
                self.value_label.set_text(&target_text);
                self.kind_label.set_text("Conversion");
            }
            CalcResult::CurrencyConversion {
                amount,
                from,
                to,
                converted,
                ..
            } => {
                let src_text = format_currency_source(amount, &from);
                self.expr_label.set_text(&src_text);

                let conv_text = format_currency_target(converted, &to);
                self.value_label.set_text(&conv_text);
                self.kind_label.set_text("Conversion");
            }
        }
    }

    /// Cycles to the next representation format and updates the display in-place.
    ///
    /// Returns the newly active representation string in `.cv`.
    #[must_use]
    pub fn cycle_representation(&self) -> String {
        let res = self.calc_result.borrow().clone();
        if let CalcResult::Math {
            result, formatted, ..
        } = res
        {
            let is_integer = is_integer_like(result);
            let next_rep = self.current_rep.get().next(is_integer);
            self.current_rep.set(next_rep);

            let new_text = format_math_representation(result, &formatted, next_rep, is_integer);
            self.value_label.set_text(&new_text);
            new_text
        } else {
            self.current_formatted_value()
        }
    }

    /// Returns the currently displayed text in the main `.cv` label.
    #[must_use]
    pub fn current_formatted_value(&self) -> String {
        self.value_label.text().to_string()
    }

    /// Returns the currently active [`MathRepresentation`].
    #[must_use]
    pub fn current_representation(&self) -> MathRepresentation {
        self.current_rep.get()
    }

    /// Copies the currently displayed formatted string to the system clipboard.
    ///
    /// Returns `true` if a clipboard surface was available.
    #[must_use]
    pub fn copy_to_clipboard(&self) -> bool {
        let text = self.current_formatted_value();
        for cb in &*self.copy_cbs.borrow() {
            cb(&text);
        }

        gdk4::Display::default().is_some_and(|display| {
            display.clipboard().set_text(&text);
            true
        })
    }

    /// Copies the value to the clipboard and invokes all registered dismissal callbacks.
    pub fn copy_and_dismiss(&self) {
        let _ = self.copy_to_clipboard();
        for cb in &*self.dismiss_cbs.borrow() {
            cb();
        }
    }

    /// Registers a listener callback invoked when overlay dismissal is requested.
    pub fn connect_dismiss<F: Fn() + 'static>(&self, callback: F) {
        self.dismiss_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Registers a listener callback invoked when text is copied to clipboard.
    pub fn connect_copied<F: Fn(&str) + 'static>(&self, callback: F) {
        self.copy_cbs.borrow_mut().push(Box::new(callback));
    }
}

/// Checks whether a floating-point number is safely representable as an integer.
#[must_use]
pub fn is_integer_like(val: f64) -> bool {
    #[allow(clippy::cast_precision_loss)]
    {
        val.is_finite()
            && val.fract().abs() < 1e-9
            && val >= (i64::MIN as f64)
            && val <= (i64::MAX as f64)
    }
}

/// Formats a mathematical value into the specified [`MathRepresentation`].
#[must_use]
pub fn format_math_representation(
    val: f64,
    default_formatted: &str,
    rep: MathRepresentation,
    is_integer: bool,
) -> String {
    match rep {
        MathRepresentation::Decimal => default_formatted.to_string(),
        MathRepresentation::Hexadecimal => {
            if is_integer {
                #[allow(clippy::cast_possible_truncation)]
                let int_val = val.round() as i64;
                if int_val >= 0 {
                    format!("{:#X}", int_val.unsigned_abs())
                } else {
                    format!("-{:#X}", int_val.unsigned_abs())
                }
            } else {
                default_formatted.to_string()
            }
        }
        MathRepresentation::Binary => {
            if is_integer {
                #[allow(clippy::cast_possible_truncation)]
                let int_val = val.round() as i64;
                if int_val >= 0 {
                    format!("{:#b}", int_val.unsigned_abs())
                } else {
                    format!("-{:#b}", int_val.unsigned_abs())
                }
            } else {
                default_formatted.to_string()
            }
        }
        MathRepresentation::Scientific => format_scientific(val),
    }
}

/// Formats a number in scientific exponential notation with Unicode superscripts (e.g. `2.55 × 10²`).
#[must_use]
pub fn format_scientific(val: f64) -> String {
    if !val.is_finite() || val == 0.0 {
        return "0".to_string();
    }

    let sign = if val < 0.0 { "-" } else { "" };
    let abs_val = val.abs();

    #[allow(clippy::cast_possible_truncation)]
    let mut exp = abs_val.log10().floor() as i32;
    let mut mantissa = abs_val / 10f64.powi(exp);

    // Guard against potential IEEE 754 log10 floor inaccuracies
    if mantissa >= 10.0 {
        mantissa /= 10.0;
        exp = exp.saturating_add(1);
    } else if mantissa < 1.0 && exp > i32::MIN {
        mantissa *= 10.0;
        exp = exp.saturating_sub(1);
    }

    let mantissa_raw = format!("{mantissa:.2}");
    let trimmed = mantissa_raw.trim_end_matches('0').trim_end_matches('.');

    let exp_superscript = to_superscript(exp);
    format!("{sign}{trimmed} × 10{exp_superscript}")
}

/// Converts an integer into its Unicode superscript string representation.
#[must_use]
pub fn to_superscript(num: i32) -> String {
    let s = num.to_string();
    let mut out = String::with_capacity(s.len() * 3);
    for ch in s.chars() {
        match ch {
            '-' => out.push('⁻'),
            '0' => out.push('⁰'),
            '1' => out.push('¹'),
            '2' => out.push('²'),
            '3' => out.push('³'),
            '4' => out.push('⁴'),
            '5' => out.push('⁵'),
            '6' => out.push('⁶'),
            '7' => out.push('⁷'),
            '8' => out.push('⁸'),
            '9' => out.push('⁹'),
            _ => out.push(ch),
        }
    }
    out
}

/// Formats an arithmetic expression for user display in the `.cx` label.
///
/// Replaces `*` with ` × ` and `/` with ` ÷ `, ensuring clean whitespace around operators.
#[must_use]
pub fn format_math_expression(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        let ch = chars[i];
        match ch {
            '*' => {
                // Ensure spaces around multiplication symbol
                if !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push('×');
                out.push(' ');
                // Skip subsequent space in input if present
                if i + 1 < len && chars[i + 1] == ' ' {
                    i += 1;
                }
            }
            '/' => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push('÷');
                out.push(' ');
                if i + 1 < len && chars[i + 1] == ' ' {
                    i += 1;
                }
            }
            '+' => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push('+');
                out.push(' ');
                if i + 1 < len && chars[i + 1] == ' ' {
                    i += 1;
                }
            }
            '-' => {
                // Keep unary minus tight if at start or immediately following another operator
                let is_unary = out.is_empty()
                    || out.ends_with('(')
                    || out.ends_with("× ")
                    || out.ends_with("÷ ")
                    || out.ends_with("+ ")
                    || out.ends_with("- ");
                if is_unary {
                    out.push('-');
                } else {
                    if !out.ends_with(' ') {
                        out.push(' ');
                    }
                    out.push('-');
                    out.push(' ');
                    if i + 1 < len && chars[i + 1] == ' ' {
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(ch);
            }
        }
        i += 1;
    }

    out.trim().to_string()
}

/// Formats the source quantity for unit conversions in `.cx` (e.g. `10 miles`).
#[must_use]
pub fn format_unit_source(source_val: f64, source_unit: &str) -> String {
    let val_str = if source_val.fract().abs() < 1e-9 {
        format!("{source_val:.0}")
    } else {
        format!("{source_val}")
    };

    let lower = source_unit.to_ascii_lowercase();
    let unit_display = match lower.as_str() {
        "mi" | "mile" | "miles" => {
            if (source_val - 1.0).abs() < 1e-9 {
                "mile"
            } else {
                "miles"
            }
        }
        "km" | "kilometer" | "kilometers" => "km",
        "m" | "meter" | "meters" => "m",
        "cm" => "cm",
        "mm" => "mm",
        "in" | "inch" | "inches" => {
            if (source_val - 1.0).abs() < 1e-9 {
                "inch"
            } else {
                "inches"
            }
        }
        "ft" | "foot" | "feet" => {
            if (source_val - 1.0).abs() < 1e-9 {
                "foot"
            } else {
                "feet"
            }
        }
        _ => source_unit,
    };

    format!("{val_str} {unit_display}")
}

/// Formats the target converted quantity for unit conversions in `.cv` (e.g. `16.09 km`).
#[must_use]
pub fn format_unit_target(target_val: f64, target_unit: &str, raw_formatted: &str) -> String {
    if target_val.fract().abs() < 1e-9 {
        format!("{target_val:.0} {target_unit}")
    } else if target_val.abs() >= 0.01 {
        format!("{target_val:.2} {target_unit}")
    } else {
        raw_formatted.to_string()
    }
}

/// Formats the source monetary amount with ISO code or symbol for currency in `.cx` (e.g. `$100`).
#[must_use]
pub fn format_currency_source(amount: f64, from: &str) -> String {
    let amount_str = if amount.fract().abs() < 1e-9 {
        format!("{amount:.0}")
    } else {
        format!("{amount:.2}")
    };

    match from.to_uppercase().as_str() {
        "USD" => format!("${amount_str}"),
        "EUR" => format!("€{amount_str}"),
        "GBP" => format!("£{amount_str}"),
        "JPY" => format!("¥{amount_str}"),
        "CAD" => format!("C${amount_str}"),
        "AUD" => format!("A${amount_str}"),
        "INR" => format!("₹{amount_str}"),
        _ => format!("{amount_str} {from}"),
    }
}

/// Formats the target converted monetary amount with symbol for currency in `.cv` (e.g. `€92.13`).
#[must_use]
pub fn format_currency_target(converted: f64, to: &str) -> String {
    let conv_str = format!("{converted:.2}");

    match to.to_uppercase().as_str() {
        "EUR" => format!("€{conv_str}"),
        "USD" => format!("${conv_str}"),
        "GBP" => format!("£{conv_str}"),
        "JPY" => format!("¥{conv_str}"),
        "CAD" => format!("C${conv_str}"),
        "AUD" => format!("A${conv_str}"),
        "INR" => format!("₹{conv_str}"),
        _ => format!("{conv_str} {to}"),
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

    #[test]
    fn test_format_math_expression() {
        assert_eq!(format_math_expression("25*4+10"), "25 × 4 + 10");
        assert_eq!(format_math_expression("25 * 4 + 10"), "25 × 4 + 10");
        assert_eq!(format_math_expression("100 / 4"), "100 ÷ 4");
        assert_eq!(format_math_expression("-5 + 10"), "-5 + 10");
    }

    #[test]
    fn test_format_scientific() {
        assert_eq!(format_scientific(255.0), "2.55 × 10²");
        assert_eq!(format_scientific(1000.0), "1 × 10³");
        assert_eq!(format_scientific(0.0), "0");
        assert_eq!(format_scientific(-255.0), "-2.55 × 10²");
        assert_eq!(format_scientific(0.005), "5 × 10⁻³");
    }

    #[test]
    fn test_to_superscript() {
        assert_eq!(to_superscript(0), "⁰");
        assert_eq!(to_superscript(2), "²");
        assert_eq!(to_superscript(12), "¹²");
        assert_eq!(to_superscript(-3), "⁻³");
    }

    #[test]
    fn test_math_representation_cycling() {
        let val = 255.0;
        let def = "255";
        let is_int = is_integer_like(val);
        assert!(is_int);

        let mut rep = MathRepresentation::Decimal;
        assert_eq!(format_math_representation(val, def, rep, is_int), "255");

        rep = rep.next(is_int);
        assert_eq!(rep, MathRepresentation::Hexadecimal);
        assert_eq!(format_math_representation(val, def, rep, is_int), "0xFF");

        rep = rep.next(is_int);
        assert_eq!(rep, MathRepresentation::Binary);
        assert_eq!(
            format_math_representation(val, def, rep, is_int),
            "0b11111111"
        );

        rep = rep.next(is_int);
        assert_eq!(rep, MathRepresentation::Scientific);
        assert_eq!(
            format_math_representation(val, def, rep, is_int),
            "2.55 × 10²"
        );

        rep = rep.next(is_int);
        assert_eq!(rep, MathRepresentation::Decimal);
        assert_eq!(format_math_representation(val, def, rep, is_int), "255");
    }

    #[test]
    fn test_format_unit_helpers() {
        assert_eq!(format_unit_source(10.0, "mi"), "10 miles");
        assert_eq!(format_unit_source(1.0, "mi"), "1 mile");
        assert_eq!(format_unit_source(5.0, "km"), "5 km");

        assert_eq!(
            format_unit_target(16.09344, "km", "16.09344 km"),
            "16.09 km"
        );
        assert_eq!(format_unit_target(16000.0, "MB", "16000 MB"), "16000 MB");
    }

    #[test]
    fn test_format_currency_helpers() {
        assert_eq!(format_currency_source(100.0, "USD"), "$100");
        assert_eq!(format_currency_source(50.5, "EUR"), "€50.50");

        assert_eq!(format_currency_target(92.13, "EUR"), "€92.13");
        assert_eq!(format_currency_target(125.0, "USD"), "$125.00");
    }

    #[gtk4::test]
    fn test_calc_hero_card_widget_lifecycle() {
        let calc_res = CalcResult::Math {
            expression: "25 * 4 + 10".to_string(),
            result: 110.0,
            formatted: "110".to_string(),
        };

        let card = CalcHeroCardWidget::new(&calc_res);
        assert_eq!(card.expr_label.text(), "25 × 4 + 10");
        assert_eq!(card.value_label.text(), "110");
        assert_eq!(card.kind_label.text(), "Calculator");
        assert_eq!(card.current_formatted_value(), "110");

        // Cycle through representations via Tab simulation
        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "0x6E");
        assert_eq!(
            card.current_representation(),
            MathRepresentation::Hexadecimal
        );

        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "0b1101110");
        assert_eq!(card.current_representation(), MathRepresentation::Binary);

        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "1.1 × 10²");
        assert_eq!(
            card.current_representation(),
            MathRepresentation::Scientific
        );

        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "110");
        assert_eq!(card.current_representation(), MathRepresentation::Decimal);

        // Update to conversion
        let conv_res = CalcResult::UnitConversion {
            source_val: 10.0,
            source_unit: "mi".to_string(),
            target_val: 16.09344,
            target_unit: "km".to_string(),
            formatted: "16.09344 km".to_string(),
        };
        card.set_result(&conv_res);
        assert_eq!(card.expr_label.text(), "10 miles");
        assert_eq!(card.value_label.text(), "16.09 km");
        assert_eq!(card.kind_label.text(), "Conversion");

        // Test dismissal callback on Enter
        let dismissed = Rc::new(Cell::new(false));
        let dismissed_clone = Rc::clone(&dismissed);
        card.connect_dismiss(move || {
            dismissed_clone.set(true);
        });

        let copied = Rc::new(RefCell::new(String::new()));
        let copied_clone = Rc::clone(&copied);
        card.connect_copied(move |text| {
            *copied_clone.borrow_mut() = text.to_string();
        });

        assert!(card.handle_key_press(gdk4::Key::Return, gdk4::ModifierType::empty()));
        assert!(dismissed.get());
        assert_eq!(*copied.borrow(), "16.09 km");
    }
}
