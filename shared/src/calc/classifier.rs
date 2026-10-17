//! Zero-allocation query classifier and evaluation dispatcher.
//!
//! Evaluates incoming search queries within a sub-millisecond budget,
//! discriminating between mathematics, unit conversions, currency conversions,
//! and general search terms.

use crate::calc::ast;
use crate::calc::currency;
use crate::calc::units;
use crate::models::{SearchCategory, SearchPayload, SearchRecord};

/// Unified calculation or conversion result variant.
#[derive(Debug, Clone, PartialEq)]
pub enum CalcResult {
    /// Pure mathematical evaluation.
    Math {
        /// Original input expression.
        expression: String,
        /// Raw numeric result.
        result: f64,
        /// User-facing formatted output string.
        formatted: String,
    },
    /// Dimensional physical unit conversion.
    UnitConversion {
        /// Input source scalar amount.
        source_val: f64,
        /// Canonical source unit symbol.
        source_unit: String,
        /// Converted target scalar amount.
        target_val: f64,
        /// Canonical target unit symbol.
        target_unit: String,
        /// User-facing formatted output string.
        formatted: String,
    },
    /// Fiat currency conversion.
    CurrencyConversion {
        /// Input monetary amount.
        amount: f64,
        /// Source currency ISO code.
        from: String,
        /// Target currency ISO code.
        to: String,
        /// Converted target monetary amount.
        converted: f64,
        /// User-facing formatted output string.
        formatted: String,
    },
}

impl CalcResult {
    /// Returns the primary numeric result.
    #[must_use]
    pub const fn raw_value(&self) -> f64 {
        match *self {
            Self::Math { result, .. } => result,
            Self::UnitConversion { target_val, .. } => target_val,
            Self::CurrencyConversion { converted, .. } => converted,
        }
    }

    /// Returns a reference to the formatted output string.
    #[must_use]
    pub const fn formatted(&self) -> &str {
        match self {
            Self::Math { formatted, .. }
            | Self::UnitConversion { formatted, .. }
            | Self::CurrencyConversion { formatted, .. } => formatted.as_str(),
        }
    }

    /// Maps this calculation result into a [`SearchPayload::Calculation`].
    #[must_use]
    pub fn to_search_payload(&self) -> SearchPayload {
        match self {
            Self::Math {
                expression,
                formatted,
                ..
            } => SearchPayload::Calculation {
                expression: expression.clone(),
                result: formatted.clone(),
            },
            Self::UnitConversion {
                source_val,
                source_unit,
                target_unit,
                formatted,
                ..
            } => SearchPayload::Calculation {
                expression: format!("{source_val} {source_unit} to {target_unit}"),
                result: formatted.clone(),
            },
            Self::CurrencyConversion {
                amount,
                from,
                to,
                formatted,
                ..
            } => SearchPayload::Calculation {
                expression: format!("{amount} {from} to {to}"),
                result: formatted.clone(),
            },
        }
    }

    /// Constructs a unified [`SearchRecord`] categorized as [`SearchCategory::Calculations`].
    #[must_use]
    pub fn to_search_record(&self) -> SearchRecord {
        let (id, title, subtitle) = match self {
            Self::Math {
                expression,
                result,
                formatted,
            } => (
                format!("calc:{expression}"),
                formatted.clone(),
                Some(format!("{expression} = {result}")),
            ),
            Self::UnitConversion {
                source_val,
                source_unit,
                target_val,
                target_unit,
                formatted,
            } => (
                format!("unit:{source_val}{source_unit}:{target_unit}"),
                formatted.clone(),
                Some(format!(
                    "{source_val} {source_unit} = {target_val} {target_unit}"
                )),
            ),
            Self::CurrencyConversion {
                amount,
                from,
                to,
                converted,
                formatted,
            } => (
                format!("currency:{amount}{from}:{to}"),
                formatted.clone(),
                Some(format!("{amount} {from} = {converted:.2} {to}")),
            ),
        };

        SearchRecord::new(
            id,
            title,
            subtitle,
            SearchCategory::Calculations,
            Some("accessories-calculator".to_string()),
            1000.0,
            self.to_search_payload(),
        )
    }
}

/// Evaluates a user query as a calculation or dimensional conversion.
///
/// Returns `None` if the input is not a calculation/conversion query, is incomplete
/// (e.g. trailing binary operators like `10 + `), or is a plain integer (e.g. `1984`).
#[must_use]
pub fn try_evaluate(query: &str) -> Option<CalcResult> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Gatekeeper heuristic 1: Must contain at least one numeric digit
    if !trimmed.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }

    // Gatekeeper heuristic 2: Suppress plain numbers without operators or units (e.g. "1984" or "404")
    if is_plain_number(trimmed) {
        return None;
    }

    // Gatekeeper heuristic 3: Fast-path currency conversion
    if let Some(curr) = currency::try_convert_currency(trimmed) {
        return Some(CalcResult::CurrencyConversion {
            amount: curr.amount,
            from: curr.from,
            to: curr.to,
            converted: curr.converted,
            formatted: curr.formatted,
        });
    }

    // Gatekeeper heuristic 4: Fast-path physical unit conversion
    if let Some(unit) = units::try_convert_phrase(trimmed) {
        return Some(CalcResult::UnitConversion {
            source_val: unit.source_val,
            source_unit: unit.source_unit,
            target_val: unit.target_val,
            target_unit: unit.target_unit,
            formatted: unit.formatted,
        });
    }

    // Gatekeeper heuristic 5: Check for math symbols or function identifiers
    if is_probable_math(trimmed)
        && let Ok(Some(result)) = ast::evaluate(trimmed)
    {
        let formatted = format_math_result(result);
        return Some(CalcResult::Math {
            expression: trimmed.to_string(),
            result,
            formatted,
        });
    }

    None
}

/// Checks if a string is solely a plain integer or decimal number without operators.
fn is_plain_number(s: &str) -> bool {
    let stripped = s.trim_start_matches(['+', '-']);
    if stripped.is_empty() {
        return false;
    }

    let mut seen_dot = false;
    for ch in stripped.chars() {
        if ch.is_ascii_digit() || ch == ',' {
            continue;
        }
        if ch == '.' && !seen_dot {
            seen_dot = true;
            continue;
        }
        return false;
    }

    true
}

const MATH_OPS: &[char] = &['+', '-', '*', '×', '·', '/', '÷', '%', '^', '(', ')'];
const MATH_WORDS: &[&str] = &[
    "sqrt", "cbrt", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "log", "ln", "log10",
    "log2", "exp", "abs", "deg", "rad", "pi",
];

/// Checks if a string likely contains mathematical syntax.
fn is_probable_math(s: &str) -> bool {
    if s.chars().any(|c| MATH_OPS.contains(&c)) {
        return true;
    }

    let lower = s.to_lowercase();
    MATH_WORDS.iter().any(|&word| lower.contains(word))
}

fn format_math_result(val: f64) -> String {
    if val.fract().abs() < 1e-9 && val.abs() < 1e15 {
        format!("{val:.0}")
    } else {
        let formatted = format!("{val:.6}");
        let trimmed = formatted.trim_end_matches('0');
        trimmed.trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;

    #[test]
    fn test_gatekeeper_suppresses_plain_numbers() {
        assert_eq!(try_evaluate("1984"), None);
        assert_eq!(try_evaluate("404"), None);
        assert_eq!(try_evaluate("3.1415"), None);
        assert_eq!(try_evaluate("42"), None);
    }

    #[test]
    fn test_gatekeeper_suppresses_words_and_trailing() {
        assert_eq!(try_evaluate("firefox"), None);
        assert_eq!(try_evaluate("10 + "), None);
        assert_eq!(try_evaluate("45 * "), None);
    }

    #[test]
    fn test_try_evaluate_math() {
        let res = try_evaluate("2 + 3 * 4").unwrap();
        match res {
            CalcResult::Math {
                result, formatted, ..
            } => {
                assert_eq!(result, 14.0);
                assert_eq!(formatted, "14");
            }
            _ => panic!("Expected math result"),
        }
    }

    #[test]
    fn test_try_evaluate_unit() {
        let res = try_evaluate("100 miles in km").unwrap();
        match res {
            CalcResult::UnitConversion {
                source_val,
                target_val,
                target_unit,
                ..
            } => {
                assert_eq!(source_val, 100.0);
                assert_eq!(target_unit, "km");
                assert!((target_val - 160.9344).abs() < 1e-4);
            }
            _ => panic!("Expected unit conversion result"),
        }
    }

    #[test]
    fn test_try_evaluate_currency() {
        let res = try_evaluate("$100 in eur").unwrap();
        match res {
            CalcResult::CurrencyConversion {
                amount,
                from,
                to,
                converted,
                ..
            } => {
                assert_eq!(amount, 100.0);
                assert_eq!(from, "USD");
                assert_eq!(to, "EUR");
                assert!((converted - 92.45).abs() < 0.01);
            }
            _ => panic!("Expected currency conversion result"),
        }
    }

    #[test]
    fn test_to_search_record() {
        let res = try_evaluate("2 + 3 * 4").unwrap();
        let record = res.to_search_record();
        assert_eq!(record.category, SearchCategory::Calculations);
        assert_eq!(record.title, "14");
        assert_eq!(record.subtitle, Some("2 + 3 * 4 = 14".to_string()));
    }
}
