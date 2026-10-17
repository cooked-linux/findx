//! Mathematical expression tokenizer, AST, and evaluator.
//!
//! Evaluates arithmetic, powers, and trigonometric/logarithmic functions
//! with overflow and division-by-zero protection. Trailing or incomplete
//! expressions return `Ok(None)` to allow seamless as-you-type evaluation.

use crate::calc::CalcError;
use std::f64::consts::{E, PI};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
}

#[derive(Debug, PartialEq, Eq)]
enum InternalError {
    Incomplete,
    Calc(CalcError),
}

impl From<CalcError> for InternalError {
    fn from(err: CalcError) -> Self {
        Self::Calc(err)
    }
}

const fn check_valid(val: f64) -> Result<f64, InternalError> {
    if val.is_nan() || val.is_infinite() {
        Err(InternalError::Calc(CalcError::Overflow))
    } else {
        Ok(val)
    }
}

fn require_arg_count(name: &str, args: &[f64], expected: usize) -> Result<(), InternalError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(InternalError::Calc(CalcError::Syntax(format!(
            "{name} expects {expected} argument(s), got {}",
            args.len()
        ))))
    }
}

struct Tokenizer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
        }
    }

    fn tokenize(&mut self) -> Result<Vec<Token>, InternalError> {
        let mut tokens = Vec::new();

        while let Some(&ch) = self.chars.peek() {
            if ch.is_whitespace() {
                self.chars.next();
                continue;
            }

            match ch {
                '+' => {
                    self.chars.next();
                    tokens.push(Token::Plus);
                }
                '-' => {
                    self.chars.next();
                    tokens.push(Token::Minus);
                }
                '*' | '×' | '·' => {
                    self.chars.next();
                    if self.chars.peek() == Some(&'*') {
                        self.chars.next();
                        tokens.push(Token::Caret);
                    } else {
                        tokens.push(Token::Star);
                    }
                }
                '/' | '÷' => {
                    self.chars.next();
                    tokens.push(Token::Slash);
                }
                '%' => {
                    self.chars.next();
                    tokens.push(Token::Percent);
                }
                '^' => {
                    self.chars.next();
                    tokens.push(Token::Caret);
                }
                '(' => {
                    self.chars.next();
                    tokens.push(Token::LParen);
                }
                ')' => {
                    self.chars.next();
                    tokens.push(Token::RParen);
                }
                ',' => {
                    self.chars.next();
                    tokens.push(Token::Comma);
                }
                'π' => {
                    self.chars.next();
                    tokens.push(Token::Ident("pi".to_string()));
                }
                '0'..='9' | '.' => {
                    let num = self.read_number()?;
                    tokens.push(Token::Number(num));
                }
                _ if ch.is_alphabetic() || ch == '_' => {
                    let ident = self.read_ident();
                    tokens.push(Token::Ident(ident.to_lowercase()));
                }
                _ => {
                    return Err(InternalError::Calc(CalcError::Syntax(format!(
                        "Unexpected character: '{ch}'"
                    ))));
                }
            }
        }

        Ok(tokens)
    }

    fn read_number(&mut self) -> Result<f64, InternalError> {
        let mut num_str = String::new();
        let mut has_dot = false;
        let mut has_exp = false;

        while let Some(&ch) = self.chars.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.chars.next();
            } else if ch == '.' && !has_dot && !has_exp {
                has_dot = true;
                num_str.push(ch);
                self.chars.next();
            } else if (ch == 'e' || ch == 'E') && !has_exp && !num_str.is_empty() {
                has_exp = true;
                num_str.push(ch);
                self.chars.next();

                if let Some(&sign) = self.chars.peek()
                    && (sign == '+' || sign == '-')
                {
                    num_str.push(sign);
                    self.chars.next();
                }
            } else if ch == ',' {
                // Peek ahead to handle thousands separator (e.g. 1,000) or European decimal (e.g. 2,5)
                #[allow(clippy::unused_peekable)]
                let mut lookahead = self.chars.clone();
                lookahead.next(); // skip ','
                let mut digit_count = 0;
                for c in lookahead {
                    if c.is_ascii_digit() {
                        digit_count += 1;
                    } else {
                        break;
                    }
                }

                if digit_count == 3 && !has_dot {
                    // Thousands separator: consume ',' and continue
                    self.chars.next();
                } else if digit_count > 0 && !has_dot && !has_exp {
                    // European decimal separator: treat as '.'
                    has_dot = true;
                    num_str.push('.');
                    self.chars.next();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if num_str.is_empty() || num_str == "." {
            return Err(InternalError::Calc(CalcError::Syntax(
                "Invalid number literal".to_string(),
            )));
        }

        num_str.parse::<f64>().map_err(|_| {
            InternalError::Calc(CalcError::Syntax(format!("Invalid number: {num_str}")))
        })
    }

    fn read_ident(&mut self) -> String {
        let mut ident = String::new();
        while let Some(&ch) = self.chars.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                ident.push(ch);
                self.chars.next();
            } else {
                break;
            }
        }
        ident
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    const fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next_token(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn match_token(&mut self, expected: &Token) -> bool {
        if let Some(tok) = self.peek()
            && tok == expected
        {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Evaluates the entire token stream into an `f64`.
    fn parse(&mut self) -> Result<f64, InternalError> {
        if self.tokens.is_empty() {
            return Err(InternalError::Incomplete);
        }

        let val = self.parse_additive()?;

        if self.pos < self.tokens.len() {
            return Err(InternalError::Calc(CalcError::Syntax(
                "Unexpected trailing tokens".to_string(),
            )));
        }

        Ok(val)
    }

    /// Precedence 1: Addition and Subtraction (lowest)
    fn parse_additive(&mut self) -> Result<f64, InternalError> {
        let mut lhs = self.parse_multiplicative()?;

        while let Some(tok) = self.peek() {
            match tok {
                Token::Plus => {
                    self.pos += 1;
                    let rhs = self.parse_multiplicative()?;
                    lhs = Self::checked_add(lhs, rhs)?;
                }
                Token::Minus => {
                    self.pos += 1;
                    let rhs = self.parse_multiplicative()?;
                    lhs = Self::checked_sub(lhs, rhs)?;
                }
                _ => break,
            }
        }

        Ok(lhs)
    }

    /// Precedence 2: Multiplication, Division, Modulo
    fn parse_multiplicative(&mut self) -> Result<f64, InternalError> {
        let mut lhs = self.parse_power()?;

        while let Some(tok) = self.peek() {
            match tok {
                Token::Star => {
                    self.pos += 1;
                    let rhs = self.parse_power()?;
                    lhs = Self::checked_mul(lhs, rhs)?;
                }
                Token::Slash => {
                    self.pos += 1;
                    let rhs = self.parse_power()?;
                    lhs = Self::checked_div(lhs, rhs)?;
                }
                Token::Percent => {
                    self.pos += 1;
                    let rhs = self.parse_power()?;
                    lhs = Self::checked_rem(lhs, rhs)?;
                }
                // Implicit multiplication: e.g. 2(3), (2)(3), 2pi
                Token::LParen | Token::Ident(_) => {
                    let rhs = self.parse_power()?;
                    lhs = Self::checked_mul(lhs, rhs)?;
                }
                _ => break,
            }
        }

        Ok(lhs)
    }

    /// Precedence 3: Exponentiation (`^`, right-associative)
    fn parse_power(&mut self) -> Result<f64, InternalError> {
        let lhs = self.parse_unary()?;

        if self.match_token(&Token::Caret) {
            let rhs = self.parse_power()?;
            Self::checked_pow(lhs, rhs)
        } else {
            Ok(lhs)
        }
    }

    /// Precedence 4: Unary Negation (`-x`) and Unary Plus (`+x`)
    fn parse_unary(&mut self) -> Result<f64, InternalError> {
        if self.match_token(&Token::Minus) {
            let val = self.parse_unary()?;
            let res = -val;
            if res.is_nan() || res.is_infinite() {
                return Err(InternalError::Calc(CalcError::Overflow));
            }
            Ok(res)
        } else if self.match_token(&Token::Plus) {
            self.parse_unary()
        } else {
            self.parse_primary()
        }
    }

    /// Precedence 5: Function calls, Parentheses, Literals, Constants (highest)
    fn parse_primary(&mut self) -> Result<f64, InternalError> {
        match self.next_token() {
            Some(Token::Number(num)) => {
                if num.is_nan() || num.is_infinite() {
                    Err(InternalError::Calc(CalcError::Overflow))
                } else {
                    Ok(num)
                }
            }
            Some(Token::LParen) => {
                let inner = self.parse_additive()?;
                if self.match_token(&Token::RParen) {
                    Ok(inner)
                } else if self.pos >= self.tokens.len() {
                    Err(InternalError::Incomplete)
                } else {
                    Err(InternalError::Calc(CalcError::Syntax(
                        "Expected closing parenthesis ')'".to_string(),
                    )))
                }
            }
            Some(Token::Ident(name)) => {
                if self.match_token(&Token::LParen) {
                    // Function call
                    let args = self.parse_function_args()?;
                    Self::eval_function(&name, &args)
                } else {
                    // Constant
                    match name.as_str() {
                        "pi" => Ok(PI),
                        "e" => Ok(E),
                        _ => Err(InternalError::Calc(CalcError::Syntax(format!(
                            "Unknown constant: '{name}'"
                        )))),
                    }
                }
            }
            None => Err(InternalError::Incomplete),
            Some(other) => Err(InternalError::Calc(CalcError::Syntax(format!(
                "Unexpected token: {other:?}"
            )))),
        }
    }

    fn parse_function_args(&mut self) -> Result<Vec<f64>, InternalError> {
        let mut args = Vec::new();

        if self.match_token(&Token::RParen) {
            return Ok(args);
        }

        loop {
            let arg = self.parse_additive()?;
            args.push(arg);

            if self.match_token(&Token::Comma) {
                // Next argument
            } else if self.match_token(&Token::RParen) {
                break;
            } else if self.pos >= self.tokens.len() {
                return Err(InternalError::Incomplete);
            } else {
                return Err(InternalError::Calc(CalcError::Syntax(
                    "Expected ',' or ')' in argument list".to_string(),
                )));
            }
        }

        Ok(args)
    }

    fn eval_function(name: &str, args: &[f64]) -> Result<f64, InternalError> {
        if let Some(res) = Self::eval_misc_math(name, args)? {
            return Ok(res);
        }
        if let Some(res) = Self::eval_log(name, args)? {
            return Ok(res);
        }
        if let Some(res) = Self::eval_trig(name, args)? {
            return Ok(res);
        }
        Err(InternalError::Calc(CalcError::Syntax(format!(
            "Unknown function: '{name}'"
        ))))
    }

    fn eval_misc_math(name: &str, args: &[f64]) -> Result<Option<f64>, InternalError> {
        match name {
            "sqrt" => {
                require_arg_count(name, args, 1)?;
                let x = args[0];
                if x < 0.0 {
                    return Err(InternalError::Calc(CalcError::Overflow));
                }
                check_valid(x.sqrt()).map(Some)
            }
            "cbrt" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].cbrt()).map(Some)
            }
            "abs" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].abs()).map(Some)
            }
            "exp" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].exp()).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn eval_log(name: &str, args: &[f64]) -> Result<Option<f64>, InternalError> {
        match name {
            "ln" => {
                require_arg_count(name, args, 1)?;
                let x = args[0];
                if x <= 0.0 {
                    return Err(InternalError::Calc(CalcError::Overflow));
                }
                check_valid(x.ln()).map(Some)
            }
            "log" => {
                if args.len() == 1 {
                    let x = args[0];
                    if x <= 0.0 {
                        return Err(InternalError::Calc(CalcError::Overflow));
                    }
                    check_valid(x.log10()).map(Some)
                } else if args.len() == 2 {
                    let x = args[0];
                    let base = args[1];
                    if x <= 0.0 || base <= 0.0 || (base - 1.0).abs() < f64::EPSILON {
                        return Err(InternalError::Calc(CalcError::Overflow));
                    }
                    check_valid(x.log(base)).map(Some)
                } else {
                    Err(InternalError::Calc(CalcError::Syntax(
                        "log requires 1 or 2 arguments".to_string(),
                    )))
                }
            }
            "log10" => {
                require_arg_count(name, args, 1)?;
                let x = args[0];
                if x <= 0.0 {
                    return Err(InternalError::Calc(CalcError::Overflow));
                }
                check_valid(x.log10()).map(Some)
            }
            "log2" => {
                require_arg_count(name, args, 1)?;
                let x = args[0];
                if x <= 0.0 {
                    return Err(InternalError::Calc(CalcError::Overflow));
                }
                check_valid(x.log2()).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn eval_trig(name: &str, args: &[f64]) -> Result<Option<f64>, InternalError> {
        match name {
            "sin" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].sin()).map(Some)
            }
            "cos" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].cos()).map(Some)
            }
            "tan" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].tan()).map(Some)
            }
            "asin" => {
                require_arg_count(name, args, 1)?;
                let x = args[0];
                if !(-1.0..=1.0).contains(&x) {
                    return Err(InternalError::Calc(CalcError::Overflow));
                }
                check_valid(x.asin()).map(Some)
            }
            "acos" => {
                require_arg_count(name, args, 1)?;
                let x = args[0];
                if !(-1.0..=1.0).contains(&x) {
                    return Err(InternalError::Calc(CalcError::Overflow));
                }
                check_valid(x.acos()).map(Some)
            }
            "atan" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].atan()).map(Some)
            }
            "atan2" => {
                require_arg_count(name, args, 2)?;
                check_valid(args[0].atan2(args[1])).map(Some)
            }
            "deg" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].to_radians()).map(Some)
            }
            "rad" => {
                require_arg_count(name, args, 1)?;
                check_valid(args[0].to_degrees()).map(Some)
            }
            _ => Ok(None),
        }
    }

    fn checked_add(a: f64, b: f64) -> Result<f64, InternalError> {
        check_valid(a + b)
    }

    fn checked_sub(a: f64, b: f64) -> Result<f64, InternalError> {
        check_valid(a - b)
    }

    fn checked_mul(a: f64, b: f64) -> Result<f64, InternalError> {
        check_valid(a * b)
    }

    fn checked_div(a: f64, b: f64) -> Result<f64, InternalError> {
        if b == 0.0 || b.abs() < f64::MIN_POSITIVE {
            return Err(InternalError::Calc(CalcError::DivisionByZero));
        }
        check_valid(a / b)
    }

    fn checked_rem(a: f64, b: f64) -> Result<f64, InternalError> {
        if b == 0.0 || b.abs() < f64::MIN_POSITIVE {
            return Err(InternalError::Calc(CalcError::DivisionByZero));
        }
        check_valid(a % b)
    }

    fn checked_pow(a: f64, b: f64) -> Result<f64, InternalError> {
        let res = a.powf(b);
        check_valid(res)
    }
}

/// Evaluates a mathematical expression string.
///
/// Returns:
/// - `Ok(Some(val))` if the expression is syntactically complete and successfully evaluated.
/// - `Ok(None)` if the expression is incomplete or ends prematurely (e.g. trailing operators like `12 * `),
///   allowing search typing to continue without error popups.
///
/// # Errors
///
/// Returns [`CalcError::DivisionByZero`] on division by zero, [`CalcError::Overflow`] on undefined
/// numeric results or overflows, or [`CalcError::Syntax`] on invalid tokens or syntax.
pub fn evaluate(input: &str) -> Result<Option<f64>, CalcError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut tokenizer = Tokenizer::new(trimmed);
    let tokens = match tokenizer.tokenize() {
        Ok(t) => t,
        Err(InternalError::Incomplete) => return Ok(None),
        Err(InternalError::Calc(err)) => return Err(err),
    };

    if tokens.is_empty() {
        return Ok(None);
    }

    let mut parser = Parser::new(tokens);
    match parser.parse() {
        Ok(val) => Ok(Some(val)),
        Err(InternalError::Incomplete) => Ok(None),
        Err(InternalError::Calc(err)) => Err(err),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::assert_is_empty,
    clippy::suboptimal_flops
)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        assert_eq!(evaluate("2 + 3 * 4"), Ok(Some(14.0)));
        assert_eq!(evaluate("(2 + 3) * 4"), Ok(Some(20.0)));
        assert_eq!(evaluate("10 - 4 - 2"), Ok(Some(4.0)));
        assert_eq!(evaluate("100 / 4 / 5"), Ok(Some(5.0)));
    }

    #[test]
    fn test_powers_and_functions() {
        assert_eq!(evaluate("2^10"), Ok(Some(1024.0)));
        assert_eq!(evaluate("2 ^ 3 ^ 2"), Ok(Some(512.0)));
        assert_eq!(evaluate("sqrt(16)"), Ok(Some(4.0)));
        assert_eq!(evaluate("cbrt(27)"), Ok(Some(3.0)));
        assert_eq!(evaluate("abs(-42)"), Ok(Some(42.0)));
        assert_eq!(evaluate("ln(e)"), Ok(Some(1.0)));
        assert_eq!(evaluate("log10(100)"), Ok(Some(2.0)));
    }

    #[test]
    fn test_trigonometry() {
        let sin_val = evaluate("sin(pi / 2)").unwrap().unwrap();
        assert!((sin_val - 1.0).abs() < 1e-10);

        let sin_deg = evaluate("sin(deg(90))").unwrap().unwrap();
        assert!((sin_deg - 1.0).abs() < 1e-10);

        let cos_val = evaluate("cos(0)").unwrap().unwrap();
        assert!((cos_val - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_division_by_zero() {
        assert_eq!(evaluate("10 / 0"), Err(CalcError::DivisionByZero));
        assert_eq!(evaluate("10 % 0"), Err(CalcError::DivisionByZero));
        assert_eq!(evaluate("5 / (2 - 2)"), Err(CalcError::DivisionByZero));
    }

    #[test]
    fn test_trailing_incomplete_expressions() {
        assert_eq!(evaluate("10 + "), Ok(None));
        assert_eq!(evaluate("12 * "), Ok(None));
        assert_eq!(evaluate("45 - "), Ok(None));
        assert_eq!(evaluate("2 ^ "), Ok(None));
        assert_eq!(evaluate("sqrt("), Ok(None));
        assert_eq!(evaluate("(2 + 3 * "), Ok(None));
    }

    #[test]
    fn test_unary_and_implicit_multiplication() {
        assert_eq!(evaluate("-5 + 10"), Ok(Some(5.0)));
        assert_eq!(evaluate("2(3 + 4)"), Ok(Some(14.0)));
        let pi_val = evaluate("2pi").unwrap().unwrap();
        assert!((pi_val - 2.0 * PI).abs() < 1e-10);
    }
}
