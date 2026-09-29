use serde::{Deserialize, Serialize};

use crate::{
    CellRef, CellValue, ColumnId, ComputedColumn, ConnectedTable, CoreError, SavedView, Sheet,
    SheetId, TableId, ViewLayout, Workbook,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkbookCommand {
    SetCell {
        sheet_id: SheetId,
        reference: CellRef,
        value: CellValue,
    },
    SetFormula {
        sheet_id: SheetId,
        reference: CellRef,
        formula: String,
    },
    AddComputedColumn {
        sheet_id: SheetId,
        column: ComputedColumn,
    },
    EditComputedColumn {
        sheet_id: SheetId,
        column: ComputedColumn,
    },
    RemoveComputedColumn {
        sheet_id: SheetId,
        column_id: ColumnId,
    },
    AddConnectedTable {
        sheet_id: SheetId,
        table: ConnectedTable,
    },
    RemoveConnectedTable {
        sheet_id: SheetId,
        table_id: TableId,
    },
    AddSheet {
        sheet: Sheet,
    },
    DeleteSheet {
        sheet_id: SheetId,
    },
    RenameSheet {
        sheet_id: SheetId,
        name: String,
    },
    AddSavedView {
        sheet_id: SheetId,
        view: SavedView,
    },
    UpdateSavedView {
        sheet_id: SheetId,
        view: SavedView,
    },
    RemoveSavedView {
        sheet_id: SheetId,
        name: String,
    },
    UpdateViewLayout {
        sheet_id: SheetId,
        view_name: String,
        layout: ViewLayout,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommandUndo {
    SetCell {
        sheet_id: SheetId,
        reference: CellRef,
        previous: CellValue,
    },
    SetFormula {
        sheet_id: SheetId,
        reference: CellRef,
        previous: CellValue,
        formula: String,
    },
    AddComputedColumn {
        sheet_id: SheetId,
        column_id: ColumnId,
    },
    EditComputedColumn {
        sheet_id: SheetId,
        previous: ComputedColumn,
    },
    RemoveComputedColumn {
        sheet_id: SheetId,
        column: ComputedColumn,
    },
    AddConnectedTable {
        sheet_id: SheetId,
        table_id: TableId,
    },
    RemoveConnectedTable {
        sheet_id: SheetId,
        table: ConnectedTable,
    },
    AddSheet {
        sheet_id: SheetId,
    },
    DeleteSheet {
        sheet: Sheet,
    },
    RenameSheet {
        sheet_id: SheetId,
        previous: String,
    },
    AddSavedView {
        sheet_id: SheetId,
        name: String,
    },
    UpdateSavedView {
        sheet_id: SheetId,
        previous: SavedView,
    },
    RemoveSavedView {
        sheet_id: SheetId,
        view: SavedView,
    },
    UpdateViewLayout {
        sheet_id: SheetId,
        view_name: String,
        previous: ViewLayout,
    },
}

impl WorkbookCommand {
    pub fn apply(self, workbook: &mut Workbook) -> Result<CommandUndo, CoreError> {
        match self {
            Self::SetCell {
                sheet_id,
                reference,
                value,
            } => {
                let previous = workbook
                    .sheet_mut(sheet_id)?
                    .set_cell(reference.clone(), value);
                Ok(CommandUndo::SetCell {
                    sheet_id,
                    reference,
                    previous,
                })
            }
            Self::SetFormula {
                sheet_id,
                reference,
                formula,
            } => {
                let previous = workbook
                    .sheet_mut(sheet_id)?
                    .set_cell(reference.clone(), CellValue::Formula(formula.clone()));
                Ok(CommandUndo::SetFormula {
                    sheet_id,
                    reference,
                    previous,
                    formula,
                })
            }
            Self::AddComputedColumn { sheet_id, column } => {
                let sheet = workbook.sheet_mut(sheet_id)?;
                if sheet.computed_column(column.id).is_some() {
                    return Err(CoreError::ComputedColumnAlreadyExists(column.id));
                }
                sheet.add_computed_column(column.clone());
                Ok(CommandUndo::AddComputedColumn {
                    sheet_id,
                    column_id: column.id,
                })
            }
            Self::EditComputedColumn { sheet_id, column } => {
                let column_id = column.id;
                let previous = workbook
                    .sheet_mut(sheet_id)?
                    .replace_computed_column(column)
                    .ok_or(CoreError::ComputedColumnNotFound(column_id))?;
                Ok(CommandUndo::EditComputedColumn { sheet_id, previous })
            }
            Self::RemoveComputedColumn {
                sheet_id,
                column_id,
            } => {
                let column = workbook
                    .sheet_mut(sheet_id)?
                    .remove_computed_column(column_id)
                    .ok_or(CoreError::ComputedColumnNotFound(column_id))?;
                Ok(CommandUndo::RemoveComputedColumn { sheet_id, column })
            }
            Self::AddConnectedTable { sheet_id, table } => {
                let sheet = workbook.sheet_mut(sheet_id)?;
                if sheet.connected_table(table.id).is_some() {
                    return Err(CoreError::TableAlreadyExists(table.id));
                }
                sheet.add_connected_table(table.clone());
                Ok(CommandUndo::AddConnectedTable {
                    sheet_id,
                    table_id: table.id,
                })
            }
            Self::RemoveConnectedTable { sheet_id, table_id } => {
                let table = workbook
                    .sheet_mut(sheet_id)?
                    .remove_connected_table(table_id)
                    .ok_or(CoreError::TableNotFound(table_id))?;
                Ok(CommandUndo::RemoveConnectedTable { sheet_id, table })
            }
            Self::AddSheet { sheet } => {
                if workbook.sheets().any(|existing| existing.id == sheet.id) {
                    return Err(CoreError::SheetAlreadyExists(sheet.id));
                }
                workbook.add_sheet(sheet.clone());
                Ok(CommandUndo::AddSheet { sheet_id: sheet.id })
            }
            Self::DeleteSheet { sheet_id } => {
                let sheet = workbook
                    .remove_sheet(sheet_id)
                    .ok_or(CoreError::SheetNotFound(sheet_id))?;
                Ok(CommandUndo::DeleteSheet { sheet })
            }
            Self::RenameSheet { sheet_id, name } => {
                let sheet = workbook.sheet_mut(sheet_id)?;
                let previous = std::mem::replace(&mut sheet.name, name);
                Ok(CommandUndo::RenameSheet { sheet_id, previous })
            }
            Self::AddSavedView { sheet_id, view } => {
                let sheet = workbook.sheet_mut(sheet_id)?;
                if sheet.saved_view(&view.name).is_some() {
                    return Err(CoreError::SavedViewAlreadyExists(view.name));
                }
                let name = view.name.clone();
                sheet.add_saved_view(view);
                Ok(CommandUndo::AddSavedView { sheet_id, name })
            }
            Self::UpdateSavedView { sheet_id, view } => {
                let name = view.name.clone();
                let previous = workbook
                    .sheet_mut(sheet_id)?
                    .replace_saved_view(view)
                    .ok_or(CoreError::SavedViewNotFound(name))?;
                Ok(CommandUndo::UpdateSavedView { sheet_id, previous })
            }
            Self::RemoveSavedView { sheet_id, name } => {
                let view = workbook
                    .sheet_mut(sheet_id)?
                    .remove_saved_view(&name)
                    .ok_or_else(|| CoreError::SavedViewNotFound(name.clone()))?;
                Ok(CommandUndo::RemoveSavedView { sheet_id, view })
            }
            Self::UpdateViewLayout {
                sheet_id,
                view_name,
                layout,
            } => {
                let view = workbook
                    .sheet_mut(sheet_id)?
                    .saved_view_mut(&view_name)
                    .ok_or_else(|| CoreError::SavedViewNotFound(view_name.clone()))?;
                let previous = std::mem::replace(&mut view.layout, layout);
                Ok(CommandUndo::UpdateViewLayout {
                    sheet_id,
                    view_name,
                    previous,
                })
            }
        }
    }
}

impl CommandUndo {
    pub fn into_redo(self, workbook: &mut Workbook) -> Result<WorkbookCommand, CoreError> {
        match self {
            Self::SetCell {
                sheet_id,
                reference,
                previous,
            } => {
                let current = workbook
                    .sheet_mut(sheet_id)?
                    .set_cell(reference.clone(), previous);
                Ok(WorkbookCommand::SetCell {
                    sheet_id,
                    reference,
                    value: current,
                })
            }
            Self::SetFormula {
                sheet_id,
                reference,
                previous,
                formula,
            } => {
                workbook
                    .sheet_mut(sheet_id)?
                    .set_cell(reference.clone(), previous);
                Ok(WorkbookCommand::SetFormula {
                    sheet_id,
                    reference,
                    formula,
                })
            }
            Self::AddComputedColumn {
                sheet_id,
                column_id,
            } => {
                let column = workbook
                    .sheet_mut(sheet_id)?
                    .remove_computed_column(column_id)
                    .ok_or(CoreError::ComputedColumnNotFound(column_id))?;
                Ok(WorkbookCommand::AddComputedColumn { sheet_id, column })
            }
            Self::EditComputedColumn { sheet_id, previous } => {
                let column_id = previous.id;
                let current = workbook
                    .sheet_mut(sheet_id)?
                    .replace_computed_column(previous)
                    .ok_or(CoreError::ComputedColumnNotFound(column_id))?;
                Ok(WorkbookCommand::EditComputedColumn {
                    sheet_id,
                    column: current,
                })
            }
            Self::RemoveComputedColumn { sheet_id, column } => {
                workbook
                    .sheet_mut(sheet_id)?
                    .add_computed_column(column.clone());
                Ok(WorkbookCommand::RemoveComputedColumn {
                    sheet_id,
                    column_id: column.id,
                })
            }
            Self::AddConnectedTable { sheet_id, table_id } => {
                let table = workbook
                    .sheet_mut(sheet_id)?
                    .remove_connected_table(table_id)
                    .ok_or(CoreError::TableNotFound(table_id))?;
                Ok(WorkbookCommand::AddConnectedTable { sheet_id, table })
            }
            Self::RemoveConnectedTable { sheet_id, table } => {
                workbook
                    .sheet_mut(sheet_id)?
                    .add_connected_table(table.clone());
                Ok(WorkbookCommand::RemoveConnectedTable {
                    sheet_id,
                    table_id: table.id,
                })
            }
            Self::AddSheet { sheet_id } => {
                let sheet = workbook
                    .remove_sheet(sheet_id)
                    .ok_or(CoreError::SheetNotFound(sheet_id))?;
                Ok(WorkbookCommand::AddSheet { sheet })
            }
            Self::DeleteSheet { sheet } => {
                let sheet_id = sheet.id;
                if workbook.add_sheet(sheet).is_some() {
                    return Err(CoreError::SheetAlreadyExists(sheet_id));
                }
                Ok(WorkbookCommand::DeleteSheet { sheet_id })
            }
            Self::RenameSheet { sheet_id, previous } => {
                let sheet = workbook.sheet_mut(sheet_id)?;
                let current = std::mem::replace(&mut sheet.name, previous);
                Ok(WorkbookCommand::RenameSheet {
                    sheet_id,
                    name: current,
                })
            }
            Self::AddSavedView { sheet_id, name } => {
                let view = workbook
                    .sheet_mut(sheet_id)?
                    .remove_saved_view(&name)
                    .ok_or_else(|| CoreError::SavedViewNotFound(name.clone()))?;
                Ok(WorkbookCommand::AddSavedView { sheet_id, view })
            }
            Self::UpdateSavedView { sheet_id, previous } => {
                let name = previous.name.clone();
                let current = workbook
                    .sheet_mut(sheet_id)?
                    .replace_saved_view(previous)
                    .ok_or(CoreError::SavedViewNotFound(name))?;
                Ok(WorkbookCommand::UpdateSavedView {
                    sheet_id,
                    view: current,
                })
            }
            Self::RemoveSavedView { sheet_id, view } => {
                let name = view.name.clone();
                workbook.sheet_mut(sheet_id)?.add_saved_view(view);
                Ok(WorkbookCommand::RemoveSavedView { sheet_id, name })
            }
            Self::UpdateViewLayout {
                sheet_id,
                view_name,
                previous,
            } => {
                let view = workbook
                    .sheet_mut(sheet_id)?
                    .saved_view_mut(&view_name)
                    .ok_or_else(|| CoreError::SavedViewNotFound(view_name.clone()))?;
                let layout = std::mem::replace(&mut view.layout, previous);
                Ok(WorkbookCommand::UpdateViewLayout {
                    sheet_id,
                    view_name,
                    layout,
                })
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct CommandHistory {
    undo_stack: Vec<CommandUndo>,
    redo_stack: Vec<WorkbookCommand>,
}

impl CommandHistory {
    pub fn apply(
        &mut self,
        workbook: &mut Workbook,
        command: WorkbookCommand,
    ) -> Result<(), CoreError> {
        let undo = command.apply(workbook)?;
        self.undo_stack.push(undo);
        self.redo_stack.clear();
        Ok(())
    }

    pub fn undo(&mut self, workbook: &mut Workbook) -> Result<(), CoreError> {
        let undo = self.undo_stack.pop().ok_or(CoreError::NothingToUndo)?;
        let redo = undo.into_redo(workbook)?;
        self.redo_stack.push(redo);
        Ok(())
    }

    pub fn redo(&mut self, workbook: &mut Workbook) -> Result<(), CoreError> {
        let command = self.redo_stack.pop().ok_or(CoreError::NothingToRedo)?;
        let undo = command.apply(workbook)?;
        self.undo_stack.push(undo);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        FilterExpr, FilterOperator, SortDirection, SortSpec, SourceKind, TableColumn,
        TableViewState, WorkbookId,
    };

    fn workbook_with_sheet() -> (Workbook, SheetId) {
        let mut workbook = Workbook::new(WorkbookId(1), "Test");
        let sheet_id = SheetId(2);
        workbook.add_sheet(Sheet::new(sheet_id, "Sheet 1"));
        (workbook, sheet_id)
    }

    fn computed_column(formula: &str) -> ComputedColumn {
        ComputedColumn {
            id: ColumnId(10),
            table_id: TableId(20),
            name: "Total".to_string(),
            formula: formula.to_string(),
        }
    }

    fn saved_view(name: &str) -> SavedView {
        SavedView {
            name: name.to_string(),
            table_id: TableId(20),
            state: TableViewState {
                filters: vec![FilterExpr {
                    column: "amount".to_string(),
                    operator: FilterOperator::GreaterThan,
                    value: Some("10".to_string()),
                }],
                sorts: vec![SortSpec {
                    column: "amount".to_string(),
                    direction: SortDirection::Descending,
                }],
            },
            layout: ViewLayout::default(),
        }
    }

    #[test]
    fn command_history_restores_cell_after_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        let reference = CellRef::new(1, 1);

        history
            .apply(
                &mut workbook,
                WorkbookCommand::SetCell {
                    sheet_id,
                    reference: reference.clone(),
                    value: CellValue::Text("hello".to_string()),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert_eq!(
            workbook.sheet(sheet_id).unwrap().cell(&reference),
            CellValue::Empty
        );
    }

    #[test]
    fn command_history_reapplies_cell_after_redo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        let reference = CellRef::new(1, 1);

        history
            .apply(
                &mut workbook,
                WorkbookCommand::SetCell {
                    sheet_id,
                    reference: reference.clone(),
                    value: CellValue::Number(42.0),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();
        history.redo(&mut workbook).unwrap();

        assert_eq!(
            workbook.sheet(sheet_id).unwrap().cell(&reference),
            CellValue::Number(42.0)
        );
    }

    #[test]
    fn command_history_restores_previous_cell_after_formula_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        let reference = CellRef::new(2, 3);
        workbook
            .sheet_mut(sheet_id)
            .unwrap()
            .set_cell(reference.clone(), CellValue::Number(8.0));

        history
            .apply(
                &mut workbook,
                WorkbookCommand::SetFormula {
                    sheet_id,
                    reference: reference.clone(),
                    formula: "=A1+B1".to_string(),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert_eq!(
            workbook.sheet(sheet_id).unwrap().cell(&reference),
            CellValue::Number(8.0)
        );
    }

    #[test]
    fn command_history_reapplies_formula_after_redo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        let reference = CellRef::new(2, 3);

        history
            .apply(
                &mut workbook,
                WorkbookCommand::SetFormula {
                    sheet_id,
                    reference: reference.clone(),
                    formula: "=A1+B1".to_string(),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();
        history.redo(&mut workbook).unwrap();

        assert_eq!(
            workbook.sheet(sheet_id).unwrap().cell(&reference),
            CellValue::Formula("=A1+B1".to_string())
        );
    }

    #[test]
    fn command_history_restores_computed_column_after_edit_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        workbook
            .sheet_mut(sheet_id)
            .unwrap()
            .add_computed_column(computed_column("amount * 2"));

        history
            .apply(
                &mut workbook,
                WorkbookCommand::EditComputedColumn {
                    sheet_id,
                    column: computed_column("amount * 3"),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert_eq!(
            workbook
                .sheet(sheet_id)
                .unwrap()
                .computed_column(ColumnId(10))
                .unwrap()
                .formula,
            "amount * 2"
        );
    }

    #[test]
    fn command_history_reapplies_computed_column_edit_after_redo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        workbook
            .sheet_mut(sheet_id)
            .unwrap()
            .add_computed_column(computed_column("amount * 2"));

        history
            .apply(
                &mut workbook,
                WorkbookCommand::EditComputedColumn {
                    sheet_id,
                    column: computed_column("amount * 3"),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();
        history.redo(&mut workbook).unwrap();

        assert_eq!(
            workbook
                .sheet(sheet_id)
                .unwrap()
                .computed_column(ColumnId(10))
                .unwrap()
                .formula,
            "amount * 3"
        );
    }

    #[test]
    fn command_history_restores_deleted_sheet_after_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();

        history
            .apply(&mut workbook, WorkbookCommand::DeleteSheet { sheet_id })
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert!(workbook.sheet(sheet_id).is_ok());
    }

    #[test]
    fn command_history_redeletes_sheet_after_redo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();

        history
            .apply(&mut workbook, WorkbookCommand::DeleteSheet { sheet_id })
            .unwrap();
        history.undo(&mut workbook).unwrap();
        history.redo(&mut workbook).unwrap();

        assert_eq!(
            workbook.sheet(sheet_id).unwrap_err(),
            CoreError::SheetNotFound(sheet_id)
        );
    }

    #[test]
    fn command_history_removes_added_saved_view_after_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();

        history
            .apply(
                &mut workbook,
                WorkbookCommand::AddSavedView {
                    sheet_id,
                    view: saved_view("High value"),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert!(
            workbook
                .sheet(sheet_id)
                .unwrap()
                .saved_view("High value")
                .is_none()
        );
    }

    #[test]
    fn command_history_restores_saved_view_update_after_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        workbook
            .sheet_mut(sheet_id)
            .unwrap()
            .add_saved_view(saved_view("High value"));
        let mut updated = saved_view("High value");
        updated.state.filters[0].value = Some("100".to_string());

        history
            .apply(
                &mut workbook,
                WorkbookCommand::UpdateSavedView {
                    sheet_id,
                    view: updated,
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert_eq!(
            workbook
                .sheet(sheet_id)
                .unwrap()
                .saved_view("High value")
                .unwrap()
                .state
                .filters[0]
                .value
                .as_deref(),
            Some("10")
        );
    }

    #[test]
    fn command_history_restores_removed_saved_view_after_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        workbook
            .sheet_mut(sheet_id)
            .unwrap()
            .add_saved_view(saved_view("High value"));

        history
            .apply(
                &mut workbook,
                WorkbookCommand::RemoveSavedView {
                    sheet_id,
                    name: "High value".to_string(),
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert!(
            workbook
                .sheet(sheet_id)
                .unwrap()
                .saved_view("High value")
                .is_some()
        );
    }

    #[test]
    fn command_history_restores_view_layout_after_undo() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let mut history = CommandHistory::default();
        workbook
            .sheet_mut(sheet_id)
            .unwrap()
            .add_saved_view(saved_view("High value"));
        let layout = ViewLayout {
            frozen_rows: 1,
            frozen_columns: 2,
            hidden_columns: vec![ColumnId(10)],
            column_order: vec![ColumnId(11), ColumnId(10)],
            column_widths: BTreeMap::from([(ColumnId(10), 144)]),
        };

        history
            .apply(
                &mut workbook,
                WorkbookCommand::UpdateViewLayout {
                    sheet_id,
                    view_name: "High value".to_string(),
                    layout,
                },
            )
            .unwrap();
        history.undo(&mut workbook).unwrap();

        assert_eq!(
            workbook
                .sheet(sheet_id)
                .unwrap()
                .saved_view("High value")
                .unwrap()
                .layout,
            ViewLayout::default()
        );
    }

    #[test]
    fn command_history_rejects_duplicate_connected_table() {
        let (mut workbook, sheet_id) = workbook_with_sheet();
        let table = ConnectedTable {
            id: TableId(20),
            name: "orders".to_string(),
            source_path: "/tmp/orders.csv".to_string(),
            source_kind: SourceKind::Csv,
            anchor: CellRef::new(1, 1),
            columns: vec![TableColumn {
                id: ColumnId(1),
                name: "amount".to_string(),
                data_type: "TEXT".to_string(),
            }],
            row_count: None,
        };
        workbook
            .sheet_mut(sheet_id)
            .unwrap()
            .add_connected_table(table.clone());

        let error = WorkbookCommand::AddConnectedTable { sheet_id, table }
            .apply(&mut workbook)
            .unwrap_err();

        assert_eq!(error, CoreError::TableAlreadyExists(TableId(20)));
    }
}
