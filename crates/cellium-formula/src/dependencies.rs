use std::collections::{BTreeMap, BTreeSet};

use cellium_core::CellRef;

use crate::{Expr, FormulaError};

pub fn dependencies(expr: &Expr) -> BTreeSet<CellRef> {
    let mut refs = BTreeSet::new();
    collect_dependencies(expr, &mut refs);
    refs
}

pub fn detect_cycles(graph: &BTreeMap<CellRef, BTreeSet<CellRef>>) -> Result<(), FormulaError> {
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for node in graph.keys() {
        visit(node, graph, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit(
    node: &CellRef,
    graph: &BTreeMap<CellRef, BTreeSet<CellRef>>,
    visiting: &mut BTreeSet<CellRef>,
    visited: &mut BTreeSet<CellRef>,
) -> Result<(), FormulaError> {
    if visited.contains(node) {
        return Ok(());
    }
    if !visiting.insert(node.clone()) {
        return Err(FormulaError::Cycle);
    }
    if let Some(children) = graph.get(node) {
        for child in children {
            visit(child, graph, visiting, visited)?;
        }
    }
    visiting.remove(node);
    visited.insert(node.clone());
    Ok(())
}

fn collect_dependencies(expr: &Expr, refs: &mut BTreeSet<CellRef>) {
    match expr {
        Expr::Cell(reference) => {
            refs.insert(reference.clone());
        }
        Expr::Range(start, end) => {
            for row in start.row.min(end.row)..=start.row.max(end.row) {
                for column in start.column.min(end.column)..=start.column.max(end.column) {
                    refs.insert(CellRef::new(row, column));
                }
            }
        }
        Expr::Unary { expr, .. } => collect_dependencies(expr, refs),
        Expr::Binary { left, right, .. } => {
            collect_dependencies(left, refs);
            collect_dependencies(right, refs);
        }
        Expr::Function { args, .. } => {
            for arg in args {
                collect_dependencies(arg, refs);
            }
        }
        Expr::Number(_) | Expr::Text(_) | Expr::TableColumn(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cycles_rejects_recursive_graph() {
        let mut graph = BTreeMap::new();
        graph.insert(CellRef::new(1, 1), BTreeSet::from([CellRef::new(1, 2)]));
        graph.insert(CellRef::new(1, 2), BTreeSet::from([CellRef::new(1, 1)]));

        assert_eq!(detect_cycles(&graph), Err(FormulaError::Cycle));
    }
}
