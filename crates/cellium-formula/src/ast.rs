use cellium_core::{CellRef, CellValue};

use crate::FormulaError;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Text(String),
    Cell(CellRef),
    Range(CellRef, CellRef),
    TableColumn(String),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Function {
        name: String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Empty,
}

impl Value {
    pub(crate) fn as_number(&self) -> Result<f64, FormulaError> {
        match self {
            Self::Number(value) => Ok(*value),
            Self::Bool(value) => Ok(if *value { 1.0 } else { 0.0 }),
            Self::Empty => Ok(0.0),
            Self::Text(_) => Err(FormulaError::NotANumber),
        }
    }
}

impl From<CellValue> for Value {
    fn from(value: CellValue) -> Self {
        match value {
            CellValue::Empty => Self::Empty,
            CellValue::Text(value) | CellValue::Formula(value) | CellValue::Error(value) => {
                Self::Text(value)
            }
            CellValue::Number(value) => Self::Number(value),
            CellValue::Bool(value) => Self::Bool(value),
        }
    }
}
