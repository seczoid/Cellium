use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CellRef, ColumnId, CoreError, SheetId, TableId, WorkbookId};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellValue {
    #[default]
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
    Formula(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub reference: CellRef,
    pub value: CellValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    Csv,
    Parquet,
    Arrow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectedTable {
    pub id: TableId,
    pub name: String,
    pub source_path: String,
    pub source_kind: SourceKind,
    pub anchor: CellRef,
    pub columns: Vec<TableColumn>,
    pub row_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumn {
    pub id: ColumnId,
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputedColumn {
    pub id: ColumnId,
    pub table_id: TableId,
    pub name: String,
    pub formula: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewLayout {
    pub frozen_rows: u32,
    pub frozen_columns: u32,
    pub hidden_columns: Vec<ColumnId>,
    pub column_order: Vec<ColumnId>,
    #[serde(default, with = "crate::serde_map")]
    pub column_widths: BTreeMap<ColumnId, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortSpec {
    pub column: String,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOperator {
    Contains,
    Equals,
    NotEquals,
    StartsWith,
    EndsWith,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterExpr {
    pub column: String,
    pub operator: FilterOperator,
    pub value: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableViewState {
    pub filters: Vec<FilterExpr>,
    pub sorts: Vec<SortSpec>,
}

impl TableViewState {
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.filters.is_empty() && self.sorts.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedView {
    pub name: String,
    pub table_id: TableId,
    #[serde(default)]
    pub state: TableViewState,
    #[serde(default)]
    pub layout: ViewLayout,
}

impl SavedView {
    #[must_use]
    pub fn new(name: impl Into<String>, table_id: TableId) -> Self {
        Self {
            name: name.into(),
            table_id,
            state: TableViewState::default(),
            layout: ViewLayout::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub id: SheetId,
    pub name: String,
    #[serde(default, with = "crate::serde_map")]
    cells: BTreeMap<CellRef, CellValue>,
    #[serde(default, with = "crate::serde_map")]
    connected_tables: BTreeMap<TableId, ConnectedTable>,
    #[serde(default, with = "crate::serde_map")]
    computed_columns: BTreeMap<ColumnId, ComputedColumn>,
    #[serde(default)]
    saved_views: Vec<SavedView>,
}

impl Sheet {
    #[must_use]
    pub fn new(id: SheetId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            cells: BTreeMap::new(),
            connected_tables: BTreeMap::new(),
            computed_columns: BTreeMap::new(),
            saved_views: Vec::new(),
        }
    }

    #[must_use]
    pub fn cell(&self, reference: &CellRef) -> CellValue {
        self.cells.get(reference).cloned().unwrap_or_default()
    }

    pub fn set_cell(&mut self, reference: CellRef, value: CellValue) -> CellValue {
        let previous = self.cell(&reference);
        if value == CellValue::Empty {
            self.cells.remove(&reference);
        } else {
            self.cells.insert(reference, value);
        }
        previous
    }

    pub fn add_connected_table(&mut self, table: ConnectedTable) -> Option<ConnectedTable> {
        self.connected_tables.insert(table.id, table)
    }

    pub fn connected_table(&self, table_id: TableId) -> Option<&ConnectedTable> {
        self.connected_tables.get(&table_id)
    }

    pub fn remove_connected_table(&mut self, table_id: TableId) -> Option<ConnectedTable> {
        self.connected_tables.remove(&table_id)
    }

    pub fn add_computed_column(&mut self, column: ComputedColumn) -> Option<ComputedColumn> {
        self.computed_columns.insert(column.id, column)
    }

    pub fn replace_computed_column(&mut self, column: ComputedColumn) -> Option<ComputedColumn> {
        if !self.computed_columns.contains_key(&column.id) {
            return None;
        }
        self.computed_columns.insert(column.id, column)
    }

    pub fn computed_column(&self, column_id: ColumnId) -> Option<&ComputedColumn> {
        self.computed_columns.get(&column_id)
    }

    pub fn remove_computed_column(&mut self, column_id: ColumnId) -> Option<ComputedColumn> {
        self.computed_columns.remove(&column_id)
    }

    pub fn add_saved_view(&mut self, view: SavedView) {
        self.saved_views.push(view);
    }

    pub fn replace_saved_view(&mut self, view: SavedView) -> Option<SavedView> {
        let index = self
            .saved_views
            .iter()
            .position(|saved_view| saved_view.name == view.name)?;
        Some(std::mem::replace(&mut self.saved_views[index], view))
    }

    pub fn remove_saved_view(&mut self, name: &str) -> Option<SavedView> {
        let index = self
            .saved_views
            .iter()
            .position(|saved_view| saved_view.name == name)?;
        Some(self.saved_views.remove(index))
    }

    pub fn saved_view(&self, name: &str) -> Option<&SavedView> {
        self.saved_views
            .iter()
            .find(|saved_view| saved_view.name == name)
    }

    pub fn saved_view_mut(&mut self, name: &str) -> Option<&mut SavedView> {
        self.saved_views
            .iter_mut()
            .find(|saved_view| saved_view.name == name)
    }

    pub fn cells(&self) -> impl Iterator<Item = (&CellRef, &CellValue)> {
        self.cells.iter()
    }

    pub fn connected_tables(&self) -> impl Iterator<Item = &ConnectedTable> {
        self.connected_tables.values()
    }

    pub fn computed_columns(&self) -> impl Iterator<Item = &ComputedColumn> {
        self.computed_columns.values()
    }

    #[must_use]
    pub fn saved_views(&self) -> &[SavedView] {
        &self.saved_views
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workbook {
    pub id: WorkbookId,
    pub name: String,
    #[serde(default, with = "crate::serde_map")]
    sheets: BTreeMap<SheetId, Sheet>,
    next_id: u64,
}

impl Workbook {
    #[must_use]
    pub fn new(id: WorkbookId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            sheets: BTreeMap::new(),
            next_id: id.0.saturating_add(1),
        }
    }

    #[must_use]
    pub fn next_sheet_id(&mut self) -> SheetId {
        let id = SheetId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub fn add_sheet(&mut self, sheet: Sheet) -> Option<Sheet> {
        self.sheets.insert(sheet.id, sheet)
    }

    pub fn remove_sheet(&mut self, sheet_id: SheetId) -> Option<Sheet> {
        self.sheets.remove(&sheet_id)
    }

    pub fn sheet(&self, sheet_id: SheetId) -> Result<&Sheet, CoreError> {
        self.sheets
            .get(&sheet_id)
            .ok_or(CoreError::SheetNotFound(sheet_id))
    }

    pub fn sheet_mut(&mut self, sheet_id: SheetId) -> Result<&mut Sheet, CoreError> {
        self.sheets
            .get_mut(&sheet_id)
            .ok_or(CoreError::SheetNotFound(sheet_id))
    }

    pub fn sheets(&self) -> impl Iterator<Item = &Sheet> {
        self.sheets.values()
    }
}
