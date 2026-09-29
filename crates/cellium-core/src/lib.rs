//! Domain model for Cellium workbooks.
//!
//! This crate intentionally avoids UI, rendering, database, and async runtime dependencies.

mod commands;
mod error;
mod ids;
mod model;
mod serde_map;

pub use commands::{CommandHistory, CommandUndo, WorkbookCommand};
pub use error::CoreError;
pub use ids::{CellId, CellRef, ColumnId, SheetId, TableId, WorkbookId};
pub use model::{
    Cell, CellValue, ComputedColumn, ConnectedTable, FilterExpr, FilterOperator, SavedView, Sheet,
    SortDirection, SortSpec, SourceKind, TableColumn, TableViewState, ViewLayout, Workbook,
};
