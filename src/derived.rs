//! Formula-based derived parameters (T6).
//!
//! FlowJo-style derived parameters are arithmetic combinations of measured
//! channels: ratios, differences, log/linear combinations, etc. This module
//! provides a small expression AST, a string parser, dependency extraction and
//! per-event evaluation for such formulas.
//!
//! Supported syntax:
//!
//! * numbers (decimal literals),
//! * parameter references (numeric identifiers, e.g. `3` or `P3`),
//! * binary operators `+`, `-`, `*`, `/` (with parentheses),
//! * unary minus,
//! * functions: `log`, `log10`, `ln`, `exp`, `sqrt`, `abs`, `min`, `max`,
//!   `ratio` (a / b) and `difference` (a - b).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Why a derived-parameter expression could not be used.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DerivedError {
    #[error("invalid derived-parameter expression: {0}")]
    Parse(String),
    #[error("unknown function '{0}' in derived parameter")]
    UnknownFunction(String),
    #[error("division by zero in derived parameter")]
    DivisionByZero,
}

/// Expression AST for a derived parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Number(f64),
    /// Parameter reference by zero-based parameter index.
    Param(usize),
    /// Binary arithmetic operation.
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    /// Unary or named function application.
    Func {
        name: String,
        args: Vec<Expr>,
    },
    /// Named column reference resolved against a table row's columns.
    Column(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Named functions supported by derived parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    Log,
    Log10,
    Ln,
    Exp,
    Sqrt,
    Abs,
}

/// A formula-based derived parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedParameter {
    pub id: String,
    pub name: String,
    pub expression: String,
}

impl DerivedParameter {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        expression: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            expression: expression.into(),
        }
    }

    /// Parse the expression string into an AST.
    pub fn parse(&self) -> Result<Expr, DerivedError> {
        let tokens = tokenize(&self.expression)?;
        let mut parser = Parser { tokens, pos: 0 };
        let expr = parser.parse_expr()?;
        parser.expect_end()?;
        Ok(expr)
    }

    /// Collect the parameter indices referenced by the expression.
    pub fn dependencies(&self, expr: &Expr) -> Vec<usize> {
        let mut seen = BTreeSet::new();
        collect_params(expr, &mut seen);
        seen.into_iter().collect()
    }

    /// Collect named column references used by the expression
    /// (deduplicated, in order of first use).
    pub fn column_references(&self, expr: &Expr) -> Vec<String> {
        let mut seen = BTreeSet::new();
        collect_columns(expr, &mut seen);
        seen.into_iter().collect()
    }

    /// Evaluate the expression against a single event row.
    ///
    /// `values` holds the parameter values for one event; parameter references
    /// index into it. Returns `None` for division by zero or non-finite results.
    pub fn evaluate(&self, values: &[f64]) -> Option<f64> {
        self.evaluate_with_columns(values, &[])
    }

    /// Evaluate the expression against a parameter-value row, resolving
    /// named column references (e.g. table columns) via `columns`.
    pub fn evaluate_with_columns(&self, values: &[f64], columns: &[(String, f64)]) -> Option<f64> {
        let expr = self.parse().ok()?;
        let value = eval_expr(&expr, values, columns)?;
        value.is_finite().then_some(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, DerivedError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c.is_ascii_digit() {
            let mut text = String::new();
            while chars
                .peek()
                .map(|c: &char| c.is_ascii_digit())
                .unwrap_or(false)
            {
                text.push(chars.next().unwrap());
            }
            if chars.peek().map(|c: &char| *c == '.').unwrap_or(false) {
                chars.next();
                let mut frac = String::new();
                while chars
                    .peek()
                    .map(|c: &char| c.is_ascii_digit())
                    .unwrap_or(false)
                {
                    frac.push(chars.next().unwrap());
                }
                text.push('.');
                text.push_str(&frac);
            }
            let value: f64 = text
                .parse()
                .map_err(|_| DerivedError::Parse(format!("bad number '{text}'")))?;
            tokens.push(Token::Number(value));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let mut text = String::new();
            while chars
                .peek()
                .map(|c: &char| c.is_ascii_alphabetic() || c.is_ascii_digit() || *c == '_')
                .unwrap_or(false)
            {
                text.push(chars.next().unwrap());
            }
            tokens.push(Token::Ident(text));
            continue;
        }
        let token = match c {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '(' => Token::LParen,
            ')' => Token::RParen,
            ',' => Token::Comma,
            _ => return Err(DerivedError::Parse(format!("unexpected character '{c}'"))),
        };
        chars.next();
        tokens.push(token);
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn expect_end(&self) -> Result<(), DerivedError> {
        if self.pos != self.tokens.len() {
            return Err(DerivedError::Parse(
                "trailing tokens in expression".to_string(),
            ));
        }
        Ok(())
    }

    fn parse_expr(&mut self) -> Result<Expr, DerivedError> {
        let mut lhs = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.next();
                    let rhs = self.parse_term()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Token::Minus) => {
                    self.next();
                    let rhs = self.parse_term()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Sub,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> Result<Expr, DerivedError> {
        let mut lhs = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.next();
                    let rhs = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Mul,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Token::Slash) => {
                    self.next();
                    let rhs = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Div,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, DerivedError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.next();
                let operand = self.parse_unary()?;
                Ok(Expr::Func {
                    name: "negate".to_string(),
                    args: vec![operand],
                })
            }
            Some(Token::Plus) => {
                self.next();
                self.parse_unary()
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, DerivedError> {
        match self.next() {
            Some(Token::Number(value)) => Ok(Expr::Number(value)),
            Some(Token::Ident(name)) => {
                if self.peek() == Some(&Token::LParen) {
                    self.next();
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::RParen) {
                        args.push(self.parse_expr()?);
                        while self.peek() == Some(&Token::Comma) {
                            self.next();
                            args.push(self.parse_expr()?);
                        }
                    }
                    if self.next() != Some(Token::RParen) {
                        return Err(DerivedError::Parse(format!(
                            "expected ')' in call to {name}"
                        )));
                    }
                    let known = matches!(
                        name.as_str(),
                        "log"
                            | "log10"
                            | "ln"
                            | "exp"
                            | "sqrt"
                            | "abs"
                            | "min"
                            | "max"
                            | "ratio"
                            | "difference"
                            | "negate"
                    );
                    if !known {
                        return Err(DerivedError::UnknownFunction(name.clone()));
                    }
                    Ok(Expr::Func { name, args })
                } else if let Some(index) = parse_param_ref(&name) {
                    Ok(Expr::Param(index))
                } else {
                    // Not a parameter reference: treat it as a named column
                    // reference (e.g. a FlowJo-style marker such as "FITC").
                    Ok(Expr::Column(name.clone()))
                }
            }
            Some(Token::LParen) => {
                let expr = self.parse_expr()?;
                if self.next() != Some(Token::RParen) {
                    return Err(DerivedError::Parse("expected ')'".to_string()));
                }
                Ok(expr)
            }
            Some(token) => Err(DerivedError::Parse(format!("unexpected token {token:?}"))),
            None => Err(DerivedError::Parse(
                "unexpected end of expression".to_string(),
            )),
        }
    }
}

/// Accept parameter references such as `3`, `P3` or `p12` and return the
/// zero-based parameter index.
fn parse_param_ref(name: &str) -> Option<usize> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let digits = name.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let index = digits.parse::<usize>().ok()?;
    Some(index.saturating_sub(1))
}

fn collect_params(expr: &Expr, out: &mut BTreeSet<usize>) {
    match expr {
        Expr::Number(_) => {}
        Expr::Param(index) => {
            out.insert(*index);
        }
        Expr::Column(_) => {}
        Expr::BinOp { lhs, rhs, .. } => {
            collect_params(lhs, out);
            collect_params(rhs, out);
        }
        Expr::Func { args, .. } => {
            for arg in args {
                collect_params(arg, out);
            }
        }
    }
}

fn collect_columns(expr: &Expr, out: &mut BTreeSet<String>) {
    match expr {
        Expr::Number(_) => {}
        Expr::Param(_) => {}
        Expr::Column(name) => {
            out.insert(name.clone());
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_columns(lhs, out);
            collect_columns(rhs, out);
        }
        Expr::Func { args, .. } => {
            for arg in args {
                collect_columns(arg, out);
            }
        }
    }
}

/// Resolve a named column reference against a row's column values.
fn resolve_column(name: &str, columns: &[(String, f64)]) -> Option<f64> {
    columns
        .iter()
        .find(|(c, _)| c.trim().eq_ignore_ascii_case(name.trim()))
        .map(|(_, v)| *v)
}

fn eval_expr(expr: &Expr, values: &[f64], columns: &[(String, f64)]) -> Option<f64> {
    match expr {
        Expr::Number(value) => Some(*value),
        Expr::Param(index) => values.get(*index).copied(),
        Expr::Column(name) => resolve_column(name, columns),
        Expr::BinOp { op, lhs, rhs } => {
            let lhs = eval_expr(lhs, values, columns)?;
            let rhs = eval_expr(rhs, values, columns)?;
            match op {
                BinOp::Add => Some(lhs + rhs),
                BinOp::Sub => Some(lhs - rhs),
                BinOp::Mul => Some(lhs * rhs),
                BinOp::Div => {
                    if rhs == 0.0 {
                        None
                    } else {
                        Some(lhs / rhs)
                    }
                }
            }
        }
        Expr::Func { name, args } => {
            if args.len() < 2
                && matches!(
                    name.as_str(),
                    "ratio" | "difference" | "min" | "max" | "negate"
                )
            {
                return None;
            }
            let mut nums = [0.0f64; 2];
            let mut count = 0usize;
            for arg in args {
                if let Some(value) = eval_expr(arg, values, columns) {
                    if count < 2 {
                        nums[count] = value;
                    }
                    count += 1;
                }
            }
            if count != args.len() {
                return None;
            }
            match name.as_str() {
                "log" => Some(nums[0].ln()),
                "log10" => Some(nums[0].ln() / std::f64::consts::LN_10),
                "ln" => Some(nums[0].ln()),
                "exp" => Some(nums[0].exp()),
                "sqrt" => Some(nums[0].sqrt()),
                "abs" => Some(nums[0].abs()),
                "min" => Some(nums[0].min(nums[1])),
                "max" => Some(nums[0].max(nums[1])),
                "ratio" => Some(nums[0] / nums[1]),
                "difference" => Some(nums[0] - nums[1]),
                "negate" => Some(-nums[0]),
                _ => None,
            }
        }
    }
}

/// Evaluate a derived parameter against a parameter-value row.
pub fn evaluate_derived(derived: &DerivedParameter, values: &[f64]) -> Option<f64> {
    derived.evaluate(values)
}

/// Evaluate a derived parameter against a parameter-value row that may
/// resolve named column references (table formula columns).
pub fn evaluate_derived_with_columns(
    derived: &DerivedParameter,
    values: &[f64],
    columns: &[(String, f64)],
) -> Option<f64> {
    derived.evaluate_with_columns(values, columns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn derived(expression: &str) -> DerivedParameter {
        DerivedParameter::new("d", "Derived", expression)
    }

    #[test]
    fn arithmetic_expression_evaluates() {
        let d = derived("2 * P1 + P2");
        assert_eq!(
            d.parse().unwrap(),
            Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::BinOp {
                    op: BinOp::Mul,
                    lhs: Box::new(Expr::Number(2.0)),
                    rhs: Box::new(Expr::Param(0)),
                }),
                rhs: Box::new(Expr::Param(1)),
            }
        );
        assert_eq!(d.evaluate(&[3.0, 4.0]), Some(10.0));
    }

    #[test]
    fn parameter_references_use_zero_based_indices() {
        let d = derived("P2 / P1");
        assert_eq!(d.dependencies(&d.parse().unwrap()), vec![0, 1]);
        assert_eq!(d.evaluate(&[6.0, 3.0]), Some(0.5));
        assert_eq!(d.evaluate(&[0.0, 3.0]), None); // divisor is zero
    }

    #[test]
    fn functions_evaluate() {
        let d = derived("log10(P1) + log10(P2)");
        assert_eq!(d.evaluate(&[100.0, 100.0]), Some(4.0));
        let d = derived("sqrt(abs(P1 - 9))");
        assert_eq!(d.evaluate(&[25.0]), Some(4.0));
        let d = derived("min(P1, P2)");
        assert_eq!(d.evaluate(&[3.0, 9.0]), Some(3.0));
        let d = derived("ratio(P1, P2)");
        assert_eq!(d.evaluate(&[10.0, 2.0]), Some(5.0));
        let d = derived("difference(P1, P2)");
        assert_eq!(d.evaluate(&[10.0, 4.0]), Some(6.0));
    }

    #[test]
    fn column_references_resolve_against_table_columns() {
        let d = DerivedParameter::new("c", "CFU", "FITC / PE");
        let expr = d.parse().unwrap();
        assert_eq!(d.column_references(&expr), vec!["FITC", "PE"]);
        let columns = vec![("FITC".to_string(), 10.0), ("PE".to_string(), 2.0)];
        assert_eq!(d.evaluate_with_columns(&[0.0, 0.0], &columns), Some(5.0));
        // Unresolvable columns yield None rather than a silent default.
        assert_eq!(d.evaluate_with_columns(&[0.0, 0.0], &[]), None);
    }

    #[test]
    fn formula_may_mix_parameter_and_column_references() {
        let d = DerivedParameter::new("c", "Ratio", "FITC / P2");
        let columns = vec![("FITC".to_string(), 10.0)];
        // P2 is the second (0-based index 1) parameter; FITC is a column.
        assert_eq!(d.evaluate_with_columns(&[3.0, 2.0], &columns), Some(5.0));
        // Without the column context the parameter path still works.
        assert_eq!(d.evaluate(&[3.0, 2.0]), None);
    }

    #[test]
    fn parameter_only_expressions_ignore_columns() {
        let d = DerivedParameter::new("d", "Derived", "2 * P1 + P2");
        let columns = vec![("FITC".to_string(), 100.0)];
        assert_eq!(d.evaluate_with_columns(&[3.0, 4.0], &columns), Some(10.0));
    }

    #[test]
    fn invalid_expressions_are_rejected() {
        assert!(derived("1 +").parse().is_err());
        assert!(derived("(1 + 2").parse().is_err());
        assert!(derived("1 2").parse().is_err());
        assert!(derived("foo(P1)").parse().is_err());
        assert!(derived("").parse().is_err());
    }

    #[test]
    fn dependencies_are_unique_and_ordered() {
        let d = derived("P2 + P1 * P2");
        assert_eq!(d.dependencies(&d.parse().unwrap()), vec![0, 1]);
    }
}
