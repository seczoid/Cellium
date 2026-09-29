use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum FormulaError {
    #[error("expected {expected} at byte {position}")]
    Expected {
        expected: &'static str,
        position: usize,
    },
    #[error("unexpected trailing input at byte {0}")]
    TrailingInput(usize),
    #[error("unknown function `{0}`")]
    UnknownFunction(String),
    #[error("cycle detected in formula dependencies")]
    Cycle,
    #[error("formula cannot be evaluated as a number")]
    NotANumber,
    #[error("division by zero")]
    DivisionByZero,
}
