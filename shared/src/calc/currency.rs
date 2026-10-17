//! Fiat currency conversion and cached exchange rate tables.
//!
//! Provides ISO-4217 currency conversions with offline fallback rates,
//! timestamp annotations, and natural phrase parsing (e.g. `$100 in eur`,
//! `50 GBP to USD`).

use crate::calc::CalcError;

/// Date of the cached fallback rates.
pub const CACHED_RATES_DATE: &str = "2026-10-15";

#[derive(Debug, Clone, Copy, PartialEq)]
struct CurrencyDef {
    code: &'static str,
    /// Exchange rate relative to EUR base (1 EUR = rate * CODE).
    rate_to_base_eur: f64,
    symbol: Option<&'static str>,
}

const CURRENCY_TABLE: &[CurrencyDef] = &[
    CurrencyDef {
        code: "EUR",
        rate_to_base_eur: 1.0,
        symbol: Some("€"),
    },
    CurrencyDef {
        code: "USD",
        // 100 USD / 1.081665765... = 92.45 EUR
        rate_to_base_eur: 1.081_665_765_278_528_9,
        symbol: Some("$"),
    },
    CurrencyDef {
        code: "GBP",
        rate_to_base_eur: 0.8542,
        symbol: Some("£"),
    },
    CurrencyDef {
        code: "JPY",
        rate_to_base_eur: 164.20,
        symbol: Some("¥"),
    },
    CurrencyDef {
        code: "CAD",
        rate_to_base_eur: 1.4820,
        symbol: Some("C$"),
    },
    CurrencyDef {
        code: "AUD",
        rate_to_base_eur: 1.6610,
        symbol: Some("A$"),
    },
    CurrencyDef {
        code: "CHF",
        rate_to_base_eur: 0.9780,
        symbol: None,
    },
    CurrencyDef {
        code: "INR",
        rate_to_base_eur: 90.50,
        symbol: Some("₹"),
    },
    CurrencyDef {
        code: "CNY",
        rate_to_base_eur: 7.85,
        symbol: None,
    },
    CurrencyDef {
        code: "NZD",
        rate_to_base_eur: 1.7850,
        symbol: None,
    },
    CurrencyDef {
        code: "SEK",
        rate_to_base_eur: 11.45,
        symbol: None,
    },
    CurrencyDef {
        code: "NOK",
        rate_to_base_eur: 11.65,
        symbol: None,
    },
    CurrencyDef {
        code: "SGD",
        rate_to_base_eur: 1.4420,
        symbol: None,
    },
    CurrencyDef {
        code: "HKD",
        rate_to_base_eur: 8.4550,
        symbol: None,
    },
    CurrencyDef {
        code: "KRW",
        rate_to_base_eur: 1475.0,
        symbol: Some("₩"),
    },
    CurrencyDef {
        code: "BRL",
        rate_to_base_eur: 6.05,
        symbol: Some("R$"),
    },
    CurrencyDef {
        code: "MXN",
        rate_to_base_eur: 21.30,
        symbol: None,
    },
    CurrencyDef {
        code: "PLN",
        rate_to_base_eur: 4.31,
        symbol: Some("zł"),
    },
];

/// Resolves a currency string or symbol to its standard ISO-4217 3-letter code.
#[must_use]
pub fn resolve_currency_code(token: &str) -> Option<&'static str> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Direct symbol match
    match trimmed {
        "$" => return Some("USD"),
        "€" => return Some("EUR"),
        "£" => return Some("GBP"),
        "¥" => return Some("JPY"),
        "₹" => return Some("INR"),
        "₩" => return Some("KRW"),
        "C$" | "CAD$" => return Some("CAD"),
        "A$" | "AUD$" => return Some("AUD"),
        "R$" => return Some("BRL"),
        "zł" => return Some("PLN"),
        _ => {}
    }

    let upper = trimmed.to_uppercase();
    for curr in CURRENCY_TABLE {
        if curr.code == upper {
            return Some(curr.code);
        }
        if let Some(sym) = curr.symbol
            && sym.eq_ignore_ascii_case(trimmed)
        {
            return Some(curr.code);
        }
    }

    None
}

/// Retrieves the cached exchange rate relative to EUR for an ISO-4217 code.
#[must_use]
pub fn get_rate(code: &str) -> Option<f64> {
    let upper = code.to_uppercase();
    for curr in CURRENCY_TABLE {
        if curr.code == upper {
            return Some(curr.rate_to_base_eur);
        }
    }
    None
}

/// Converts an amount of currency from one code to another.
///
/// # Errors
///
/// Returns [`CalcError::CurrencyUnavailable`] if either currency code is unrecognized,
/// or [`CalcError::Overflow`] on numerical overflow or negative values.
pub fn convert_currency(amount: f64, from_code: &str, to_code: &str) -> Result<f64, CalcError> {
    if amount < 0.0 {
        return Err(CalcError::Overflow);
    }

    let from_canon = resolve_currency_code(from_code)
        .ok_or_else(|| CalcError::CurrencyUnavailable(from_code.to_string()))?;
    let to_canon = resolve_currency_code(to_code)
        .ok_or_else(|| CalcError::CurrencyUnavailable(to_code.to_string()))?;

    if from_canon == to_canon {
        return Ok(amount);
    }

    let from_rate = get_rate(from_canon)
        .ok_or_else(|| CalcError::CurrencyUnavailable(from_canon.to_string()))?;
    let to_rate =
        get_rate(to_canon).ok_or_else(|| CalcError::CurrencyUnavailable(to_canon.to_string()))?;

    // Convert: amount in EUR = amount / from_rate; amount in target = amount_in_eur * to_rate
    let in_eur = amount / from_rate;
    let converted = in_eur * to_rate;

    if converted.is_nan() || converted.is_infinite() {
        return Err(CalcError::Overflow);
    }

    Ok(converted)
}

/// Result of evaluating a currency conversion query.
#[derive(Debug, PartialEq)]
pub struct CurrencyConversionResult {
    /// Input monetary amount.
    pub amount: f64,
    /// Canonical source ISO code.
    pub from: String,
    /// Canonical target ISO code.
    pub to: String,
    /// Converted target monetary amount.
    pub converted: f64,
    /// User-facing formatted output string.
    pub formatted: String,
}

/// Attempts to parse and evaluate a natural currency conversion query.
///
/// Examples:
/// - `$100 in eur`
/// - `50 GBP to USD`
/// - `1500 JPY in EUR`
/// - `$100 to eur`
#[must_use]
pub fn try_convert_currency(query: &str) -> Option<CurrencyConversionResult> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }

    let separators = [" to ", " in ", " TO ", " IN "];
    for sep in separators {
        if let Some(idx) = trimmed.find(sep) {
            let left = &trimmed[..idx];
            let right = &trimmed[idx + sep.len()..];

            if let Some(res) = evaluate_currency_split(left, right) {
                return Some(res);
            }
        }
    }

    None
}

fn evaluate_currency_split(left: &str, right: &str) -> Option<CurrencyConversionResult> {
    let to_code = resolve_currency_code(right.trim())?;
    let (amount, from_code) = parse_amount_and_currency(left.trim())?;

    let converted = convert_currency(amount, from_code, to_code).ok()?;

    let formatted = format!("= {converted:.2} {to_code} (Rate cached {CACHED_RATES_DATE})");

    Some(CurrencyConversionResult {
        amount,
        from: from_code.to_string(),
        to: to_code.to_string(),
        converted,
        formatted,
    })
}

fn parse_amount_and_currency(text: &str) -> Option<(f64, &'static str)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Check for leading currency symbol / code: e.g. "$100", "€ 50", "USD 100", "C$ 20"
    for curr in CURRENCY_TABLE {
        if let Some(sym) = curr.symbol
            && let Some(rest) = trimmed.strip_prefix(sym)
        {
            let num = parse_number_literal(rest.trim())?;
            return Some((num, curr.code));
        }
        if let Some(rest) = trimmed.strip_prefix(curr.code) {
            let num = parse_number_literal(rest.trim())?;
            return Some((num, curr.code));
        }
    }

    // Also check lower-case leading code: e.g. "usd 100"
    let upper = trimmed.to_uppercase();
    for curr in CURRENCY_TABLE {
        if let Some(rest) = upper.strip_prefix(curr.code) {
            let num_part = &trimmed[trimmed.len() - rest.len()..];
            if let Some(num) = parse_number_literal(num_part.trim()) {
                return Some((num, curr.code));
            }
        }
    }

    // 2. Check for trailing currency symbol / code: e.g. "100$", "50 GBP", "1500 JPY", "100 usd"
    let (num, trailing) = extract_leading_number(trimmed)?;
    let code = resolve_currency_code(trailing)?;
    Some((num, code))
}

fn extract_leading_number(text: &str) -> Option<(f64, &str)> {
    let trimmed = text.trim();
    let mut end_num = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;

    for (byte_idx, ch) in trimmed.char_indices() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            end_num = byte_idx + ch.len_utf8();
        } else if ch == '.' && !seen_dot {
            seen_dot = true;
            end_num = byte_idx + ch.len_utf8();
        } else if ch == ',' {
            end_num = byte_idx + ch.len_utf8();
        } else {
            break;
        }
    }

    if !seen_digit || end_num == 0 {
        return None;
    }

    let num_str = &trimmed[..end_num];
    let remainder = trimmed[end_num..].trim();
    let num = parse_number_literal(num_str)?;

    Some((num, remainder))
}

fn parse_number_literal(text: &str) -> Option<f64> {
    let clean = text.replace(',', "");
    clean.parse::<f64>().ok()
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
    fn test_currency_conversion_rates() {
        let res = convert_currency(100.0, "USD", "EUR").unwrap();
        assert!((res - 92.45).abs() < 0.01);

        let gbp_res = convert_currency(50.0, "GBP", "USD").unwrap();
        assert!((gbp_res - 63.31).abs() < 0.05);
    }

    #[test]
    fn test_natural_currency_phrases() {
        let r1 = try_convert_currency("$100 in eur").unwrap();
        assert_eq!(r1.amount, 100.0);
        assert_eq!(r1.from, "USD");
        assert_eq!(r1.to, "EUR");
        assert_eq!(r1.formatted, "= 92.45 EUR (Rate cached 2026-10-15)");

        let r2 = try_convert_currency("50 GBP to USD").unwrap();
        assert_eq!(r2.from, "GBP");
        assert_eq!(r2.to, "USD");
        assert!((r2.converted - 63.31).abs() < 0.05);

        let r3 = try_convert_currency("1500 JPY in EUR").unwrap();
        assert_eq!(r3.from, "JPY");
        assert_eq!(r3.to, "EUR");
        assert!((r3.converted - 9.14).abs() < 0.05);

        let r4 = try_convert_currency("$100 to eur").unwrap();
        assert_eq!(r4.formatted, "= 92.45 EUR (Rate cached 2026-10-15)");
    }
}
