//! Integration tests for the inline calculator and dimensional unit converter.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::needless_raw_string_hashes
)]

use shared::calc::{CalcError, CalcResult, evaluate, try_evaluate};
use shared::models::{SearchCategory, SearchPayload};

#[test]
fn test_arithmetic_precedence() {
    let res1 = try_evaluate("2 + 3 * 4");
    assert!(res1.is_some());
    if let Some(CalcResult::Math {
        result, formatted, ..
    }) = res1
    {
        assert_eq!(result, 14.0);
        assert_eq!(formatted, "14");
    } else {
        panic!("Expected Math variant");
    }

    let res2 = try_evaluate("(2 + 3) * 4");
    assert!(res2.is_some());
    if let Some(CalcResult::Math {
        result, formatted, ..
    }) = res2
    {
        assert_eq!(result, 20.0);
        assert_eq!(formatted, "20");
    } else {
        panic!("Expected Math variant");
    }

    assert_eq!(evaluate("2 + 3 * 4"), Ok(Some(14.0)));
    assert_eq!(evaluate("(2 + 3) * 4"), Ok(Some(20.0)));
}

#[test]
fn test_powers_and_functions() {
    let res_pow = try_evaluate("2^10");
    assert!(res_pow.is_some());
    if let Some(CalcResult::Math { result, .. }) = res_pow {
        assert_eq!(result, 1024.0);
    } else {
        panic!("Expected Math variant for 2^10");
    }

    let res_sqrt = try_evaluate("sqrt(16)");
    assert!(res_sqrt.is_some());
    if let Some(CalcResult::Math { result, .. }) = res_sqrt {
        assert_eq!(result, 4.0);
    } else {
        panic!("Expected Math variant for sqrt(16)");
    }

    let res_sin = try_evaluate("sin(pi/2)");
    assert!(res_sin.is_some());
    if let Some(CalcResult::Math { result, .. }) = res_sin {
        assert!((result - 1.0).abs() < 1e-10);
    } else {
        panic!("Expected Math variant for sin(pi/2)");
    }

    assert_eq!(evaluate("2^10"), Ok(Some(1024.0)));
    assert_eq!(evaluate("sqrt(16)"), Ok(Some(4.0)));
    let sin_val = evaluate("sin(pi/2)").unwrap().unwrap();
    assert!((sin_val - 1.0).abs() < 1e-10);
}

#[test]
fn test_division_by_zero_yields_graceful_error() {
    assert_eq!(evaluate("10 / 0"), Err(CalcError::DivisionByZero));
    assert_eq!(evaluate("10 % 0"), Err(CalcError::DivisionByZero));
    assert_eq!(evaluate("42 / (5 - 5)"), Err(CalcError::DivisionByZero));

    // Classifier gracefully returns None so other search providers can handle query
    assert_eq!(try_evaluate("10 / 0"), None);
}

#[test]
fn test_trailing_operator_returns_none() {
    assert_eq!(evaluate("10 + "), Ok(None));
    assert_eq!(evaluate("12 * "), Ok(None));
    assert_eq!(evaluate("45 - "), Ok(None));
    assert_eq!(evaluate("2 ^ "), Ok(None));
    assert_eq!(evaluate("sqrt("), Ok(None));

    assert_eq!(try_evaluate("10 + "), None);
    assert_eq!(try_evaluate("12 * "), None);
    assert_eq!(try_evaluate("45 - "), None);
}

#[test]
fn test_unit_conversions() {
    // Length: 100 miles in km
    let length_res = try_evaluate("100 miles in km");
    assert!(length_res.is_some());
    if let Some(CalcResult::UnitConversion {
        source_val,
        source_unit,
        target_val,
        target_unit,
        ..
    }) = length_res
    {
        assert_eq!(source_val, 100.0);
        assert_eq!(source_unit, "mi");
        assert_eq!(target_unit, "km");
        assert!((target_val - 160.9344).abs() < 1e-4);
    } else {
        panic!("Expected UnitConversion for 100 miles in km");
    }

    // Temperature: 32 F to C -> 0 C
    let f_to_c = try_evaluate("32 F to C");
    assert!(f_to_c.is_some());
    if let Some(CalcResult::UnitConversion {
        source_val,
        target_val,
        target_unit,
        ..
    }) = f_to_c
    {
        assert_eq!(source_val, 32.0);
        assert_eq!(target_val, 0.0);
        assert_eq!(target_unit, "C");
    } else {
        panic!("Expected UnitConversion for 32 F to C");
    }

    // Temperature: 100 C to F -> 212 F
    let c_to_f = try_evaluate("100 C to F");
    assert!(c_to_f.is_some());
    if let Some(CalcResult::UnitConversion {
        source_val,
        target_val,
        target_unit,
        ..
    }) = c_to_f
    {
        assert_eq!(source_val, 100.0);
        assert_eq!(target_val, 212.0);
        assert_eq!(target_unit, "F");
    } else {
        panic!("Expected UnitConversion for 100 C to F");
    }

    // Data Storage: 16GB in MB
    let storage_res = try_evaluate("16GB in MB");
    assert!(storage_res.is_some());
    if let Some(CalcResult::UnitConversion {
        source_val,
        target_val,
        target_unit,
        ..
    }) = storage_res
    {
        assert_eq!(source_val, 16.0);
        assert_eq!(target_val, 16000.0);
        assert_eq!(target_unit, "MB");
    } else {
        panic!("Expected UnitConversion for 16GB in MB");
    }
}

#[test]
fn test_currency_conversions() {
    let curr_res = try_evaluate("$100 to eur");
    assert!(curr_res.is_some());
    if let Some(CalcResult::CurrencyConversion {
        amount,
        from,
        to,
        converted,
        formatted,
    }) = curr_res
    {
        assert_eq!(amount, 100.0);
        assert_eq!(from, "USD");
        assert_eq!(to, "EUR");
        assert!((converted - 92.45).abs() < 0.01);
        assert_eq!(formatted, "= 92.45 EUR (Rate cached 2026-10-15)");
    } else {
        panic!("Expected CurrencyConversion for $100 to eur");
    }

    let gbp_res = try_evaluate("50 GBP to USD");
    assert!(gbp_res.is_some());
    if let Some(CalcResult::CurrencyConversion {
        from,
        to,
        converted,
        ..
    }) = gbp_res
    {
        assert_eq!(from, "GBP");
        assert_eq!(to, "USD");
        assert!((converted - 63.31).abs() < 0.05);
    } else {
        panic!("Expected CurrencyConversion for 50 GBP to USD");
    }
}

#[test]
fn test_plain_search_terms_return_none() {
    assert_eq!(try_evaluate("1984"), None);
    assert_eq!(try_evaluate("404"), None);
    assert_eq!(try_evaluate("plain_text"), None);
    assert_eq!(try_evaluate("document.pdf"), None);
    assert_eq!(try_evaluate("findx"), None);
}

#[test]
fn test_search_models_integration() {
    let calc = try_evaluate("100 miles in km").unwrap();
    let record = calc.to_search_record();

    assert_eq!(record.category, SearchCategory::Calculations);
    assert_eq!(record.score, 1000.0);
    assert_eq!(record.icon_name, Some("accessories-calculator".to_string()));

    match record.payload {
        SearchPayload::Calculation { expression, result } => {
            assert_eq!(expression, "100 mi to km");
            assert_eq!(result, "160.9344 km");
        }
        _ => panic!("Expected SearchPayload::Calculation"),
    }
}
