//! Formula parsing, dependency extraction, and scalar evaluation.

mod ast;
mod dependencies;
mod error;
mod evaluator;
mod lookup;
mod parser;

pub use ast::{BinaryOp, Expr, UnaryOp, Value};
pub use dependencies::{dependencies, detect_cycles};
pub use error::FormulaError;
pub use evaluator::evaluate;
pub use lookup::CellLookup;
pub use parser::parse_formula;
