//! Headless real-time calculation and dimensional conversion engine.

pub mod ast;
pub mod classifier;
pub mod currency;
pub mod units;

pub use ast::evaluate;
pub use classifier::{CalcResult, try_evaluate};
pub use currency::{
    CACHED_RATES_DATE, CurrencyConversionResult, convert_currency, get_rate, resolve_currency_code,
    try_convert_currency,
};
pub use units::{Dimension, UnitConversionResult, UnitDef, convert, find_unit, try_convert_phrase};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CalcError {
    #[error("Syntax error: {0}")]
    Syntax(String),
    #[error("Division by zero")]
    DivisionByZero,
    #[error("Numerical overflow or undefined result")]
    Overflow,
    #[error("Incompatible dimensional units: cannot convert {0} to {1}")]
    IncompatibleUnits(String, String),
    #[error("Unknown unit symbol: {0}")]
    UnknownUnit(String),
    #[error("Currency rate unavailable for: {0}")]
    CurrencyUnavailable(String),
}
