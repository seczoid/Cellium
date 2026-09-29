use cellium_core::CellRef;

use crate::{BinaryOp, CellLookup, Expr, FormulaError, UnaryOp, Value};

pub fn evaluate(expr: &Expr, cells: &impl CellLookup) -> Result<Value, FormulaError> {
    match expr {
        Expr::Number(value) => Ok(Value::Number(*value)),
        Expr::Text(value) => Ok(Value::Text(value.clone())),
        Expr::Cell(reference) => Ok(Value::from(cells.value_at(reference))),
        Expr::Range(_, _) => Err(FormulaError::NotANumber),
        Expr::TableColumn(name) => Ok(Value::Text(name.clone())),
        Expr::Unary { op, expr } => match op {
            UnaryOp::Negate => Ok(Value::Number(-evaluate(expr, cells)?.as_number()?)),
        },
        Expr::Binary { left, op, right } => {
            let left = evaluate(left, cells)?;
            let right = evaluate(right, cells)?;
            evaluate_binary(left, *op, right)
        }
        Expr::Function { name, args } => evaluate_function(name, args, cells),
    }
}

fn evaluate_binary(left: Value, op: BinaryOp, right: Value) -> Result<Value, FormulaError> {
    match op {
        BinaryOp::Add => Ok(Value::Number(left.as_number()? + right.as_number()?)),
        BinaryOp::Subtract => Ok(Value::Number(left.as_number()? - right.as_number()?)),
        BinaryOp::Multiply => Ok(Value::Number(left.as_number()? * right.as_number()?)),
        BinaryOp::Divide => {
            let divisor = right.as_number()?;
            if divisor == 0.0 {
                return Err(FormulaError::DivisionByZero);
            }
            Ok(Value::Number(left.as_number()? / divisor))
        }
        BinaryOp::Equal => Ok(Value::Bool(left == right)),
        BinaryOp::NotEqual => Ok(Value::Bool(left != right)),
        BinaryOp::Greater => Ok(Value::Bool(left.as_number()? > right.as_number()?)),
        BinaryOp::GreaterEqual => Ok(Value::Bool(left.as_number()? >= right.as_number()?)),
        BinaryOp::Less => Ok(Value::Bool(left.as_number()? < right.as_number()?)),
        BinaryOp::LessEqual => Ok(Value::Bool(left.as_number()? <= right.as_number()?)),
    }
}

fn evaluate_function(
    name: &str,
    args: &[Expr],
    cells: &impl CellLookup,
) -> Result<Value, FormulaError> {
    let name = name.to_ascii_uppercase();
    let values = numeric_args(args, cells)?;
    match name.as_str() {
        "SUM" => Ok(Value::Number(values.iter().sum())),
        "COUNT" => Ok(Value::Number(values.len() as f64)),
        "AVG" => {
            if values.is_empty() {
                Ok(Value::Number(0.0))
            } else {
                Ok(Value::Number(
                    values.iter().sum::<f64>() / values.len() as f64,
                ))
            }
        }
        "MIN" => Ok(Value::Number(
            values.into_iter().fold(f64::INFINITY, f64::min),
        )),
        "MAX" => Ok(Value::Number(
            values.into_iter().fold(f64::NEG_INFINITY, f64::max),
        )),
        "LEN" => {
            let Some(first) = args.first() else {
                return Ok(Value::Number(0.0));
            };
            let value = evaluate(first, cells)?;
            let len = match value {
                Value::Text(value) => value.len(),
                Value::Number(value) => value.to_string().len(),
                Value::Bool(value) => value.to_string().len(),
                Value::Empty => 0,
            };
            Ok(Value::Number(len as f64))
        }
        _ => Err(FormulaError::UnknownFunction(name)),
    }
}

fn numeric_args(args: &[Expr], cells: &impl CellLookup) -> Result<Vec<f64>, FormulaError> {
    let mut values = Vec::new();
    for arg in args {
        match arg {
            Expr::Range(start, end) => {
                for row in start.row.min(end.row)..=start.row.max(end.row) {
                    for column in start.column.min(end.column)..=start.column.max(end.column) {
                        values.push(
                            Value::from(cells.value_at(&CellRef::new(row, column))).as_number()?,
                        );
                    }
                }
            }
            _ => values.push(evaluate(arg, cells)?.as_number()?),
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use cellium_core::{CellRef, CellValue};

    use super::*;
    use crate::parse_formula;

    #[test]
    fn evaluate_returns_arithmetic_result() {
        let expr = parse_formula("=A1 * 2 + 1").unwrap();
        let result = evaluate(&expr, &|reference: &CellRef| {
            if *reference == CellRef::new(1, 1) {
                CellValue::Number(4.0)
            } else {
                CellValue::Empty
            }
        })
        .unwrap();

        assert_eq!(result, Value::Number(9.0));
    }
}
