use cellium_core::CellRef;

use crate::{BinaryOp, Expr, FormulaError, UnaryOp};

pub fn parse_formula(input: &str) -> Result<Expr, FormulaError> {
    let input = input.trim_start_matches('=');
    let mut parser = Parser::new(input);
    let expr = parser.parse_comparison()?;
    parser.skip_ws();
    if parser.is_eof() {
        Ok(expr)
    } else {
        Err(FormulaError::TrailingInput(parser.position))
    }
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Parser<'a> {
    const fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn parse_comparison(&mut self) -> Result<Expr, FormulaError> {
        let mut expr = self.parse_additive()?;
        loop {
            self.skip_ws();
            let op = if self.consume(">=") {
                Some(BinaryOp::GreaterEqual)
            } else if self.consume("<=") {
                Some(BinaryOp::LessEqual)
            } else if self.consume("<>") || self.consume("!=") {
                Some(BinaryOp::NotEqual)
            } else if self.consume("=") {
                Some(BinaryOp::Equal)
            } else if self.consume(">") {
                Some(BinaryOp::Greater)
            } else if self.consume("<") {
                Some(BinaryOp::Less)
            } else {
                None
            };
            let Some(op) = op else {
                return Ok(expr);
            };
            let right = self.parse_additive()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
    }

    fn parse_additive(&mut self) -> Result<Expr, FormulaError> {
        let mut expr = self.parse_multiplicative()?;
        loop {
            self.skip_ws();
            let op = if self.consume("+") {
                Some(BinaryOp::Add)
            } else if self.consume("-") {
                Some(BinaryOp::Subtract)
            } else {
                None
            };
            let Some(op) = op else {
                return Ok(expr);
            };
            let right = self.parse_multiplicative()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, FormulaError> {
        let mut expr = self.parse_unary()?;
        loop {
            self.skip_ws();
            let op = if self.consume("*") {
                Some(BinaryOp::Multiply)
            } else if self.consume("/") {
                Some(BinaryOp::Divide)
            } else {
                None
            };
            let Some(op) = op else {
                return Ok(expr);
            };
            let right = self.parse_unary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, FormulaError> {
        self.skip_ws();
        if self.consume("-") {
            Ok(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(self.parse_unary()?),
            })
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        self.skip_ws();
        if self.consume("(") {
            let expr = self.parse_comparison()?;
            self.expect(")")?;
            return Ok(expr);
        }
        if self.peek() == Some('"') {
            return self.parse_text();
        }
        if self.peek().is_some_and(|char| char.is_ascii_digit()) {
            return self.parse_number();
        }
        if self.peek().is_some_and(|char| char.is_ascii_alphabetic()) {
            return self.parse_symbol();
        }
        Err(FormulaError::Expected {
            expected: "expression",
            position: self.position,
        })
    }

    fn parse_text(&mut self) -> Result<Expr, FormulaError> {
        self.expect("\"")?;
        let start = self.position;
        while !self.is_eof() && self.peek() != Some('"') {
            self.position += 1;
        }
        let text = self.input[start..self.position].to_string();
        self.expect("\"")?;
        Ok(Expr::Text(text))
    }

    fn parse_number(&mut self) -> Result<Expr, FormulaError> {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|char| char.is_ascii_digit() || char == '.')
        {
            self.position += 1;
        }
        let number = self.input[start..self.position]
            .parse::<f64>()
            .map_err(|_| FormulaError::Expected {
                expected: "number",
                position: start,
            })?;
        Ok(Expr::Number(number))
    }

    fn parse_symbol(&mut self) -> Result<Expr, FormulaError> {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|char| char.is_ascii_alphanumeric() || char == '_')
        {
            self.position += 1;
        }
        let symbol = &self.input[start..self.position];
        self.skip_ws();
        if self.consume("(") {
            let mut args = Vec::new();
            self.skip_ws();
            if !self.consume(")") {
                loop {
                    args.push(self.parse_comparison()?);
                    self.skip_ws();
                    if self.consume(")") {
                        break;
                    }
                    self.expect(",")?;
                }
            }
            return Ok(Expr::Function {
                name: symbol.to_string(),
                args,
            });
        }
        if self.consume("[") {
            let column_start = self.position;
            while !self.is_eof() && self.peek() != Some(']') {
                self.position += 1;
            }
            let column = self.input[column_start..self.position].to_string();
            self.expect("]")?;
            return Ok(Expr::TableColumn(column));
        }
        let cell = parse_cell_ref(symbol).ok_or(FormulaError::Expected {
            expected: "cell reference",
            position: start,
        })?;
        if self.consume(":") {
            let range_start = self.position;
            while self.peek().is_some_and(|char| char.is_ascii_alphanumeric()) {
                self.position += 1;
            }
            let end = parse_cell_ref(&self.input[range_start..self.position]).ok_or(
                FormulaError::Expected {
                    expected: "cell reference",
                    position: range_start,
                },
            )?;
            Ok(Expr::Range(cell, end))
        } else {
            Ok(Expr::Cell(cell))
        }
    }

    fn expect(&mut self, token: &'static str) -> Result<(), FormulaError> {
        self.skip_ws();
        if self.consume(token) {
            Ok(())
        } else {
            Err(FormulaError::Expected {
                expected: token,
                position: self.position,
            })
        }
    }

    fn consume(&mut self, token: &str) -> bool {
        if self.input[self.position..].starts_with(token) {
            self.position += token.len();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.position >= self.input.len()
    }
}

fn parse_cell_ref(input: &str) -> Option<CellRef> {
    let split = input
        .char_indices()
        .find(|(_, char)| char.is_ascii_digit())
        .map(|(index, _)| index)?;
    let (letters, digits) = input.split_at(split);
    if letters.is_empty() || digits.is_empty() || !digits.chars().all(|char| char.is_ascii_digit())
    {
        return None;
    }
    let column = letters.chars().try_fold(0_u32, |acc, char| {
        if !char.is_ascii_alphabetic() {
            return None;
        }
        let value = char.to_ascii_uppercase() as u32 - 'A' as u32 + 1;
        Some(acc.saturating_mul(26).saturating_add(value))
    })?;
    let row = digits.parse::<u32>().ok()?;
    Some(CellRef::new(row, column))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependencies;

    #[test]
    fn parse_formula_recognizes_range_dependencies() {
        let expr = parse_formula("=SUM(A1:B2)").unwrap();

        assert_eq!(dependencies(&expr).len(), 4);
    }
}
