use std::path::PathBuf;

use cellium_core::{TableViewState, ViewLayout};
use serde::{Deserialize, Serialize};

pub const ROW_ID_COLUMN: &str = "cellium_row_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataFileKind {
    Csv,
    Parquet,
    Arrow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableSchema {
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub table_name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportSummary {
    pub table_name: String,
    pub source_kind: DataFileKind,
    pub schema: TableSchema,
    pub row_count: Option<u64>,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleQuery {
    pub table_name: String,
    pub offset: u64,
    pub limit: u32,
    pub projection: Vec<String>,
    pub available_columns: Vec<String>,
    pub view: TableViewState,
    pub include_row_id: bool,
    pub row_id_range: Option<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkbookRecord {
    pub id: u64,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetRecord {
    pub id: u64,
    pub workbook_id: u64,
    pub name: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectedTableRecord {
    pub id: u64,
    pub sheet_id: u64,
    pub name: String,
    pub physical_table_name: String,
    pub source_path: Option<String>,
    pub source_kind: Option<DataFileKind>,
    pub anchor_row: u64,
    pub anchor_col: u64,
    pub row_count: Option<u64>,
    pub schema: TableSchema,
    pub materialized: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SparseCellRecord {
    pub sheet_id: u64,
    pub row_index: u64,
    pub col_index: u64,
    pub value: String,
    pub formula: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedViewRecord {
    pub id: u64,
    pub sheet_id: u64,
    pub name: String,
    pub table_id: Option<u64>,
    pub state: TableViewState,
    pub layout: ViewLayout,
    pub created_at: i64,
    pub updated_at: i64,
}
