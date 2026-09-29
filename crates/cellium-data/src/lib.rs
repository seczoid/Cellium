//! DuckDB-first data access and Arrow-shaped visible windows.

mod async_engine;
mod engine;
mod error;
mod file_kind;
mod row_count;
mod sql;
mod types;
mod window;
mod workbook;

pub use async_engine::AsyncDataEngine;
pub use engine::DataEngine;
pub use error::DataError;
pub use file_kind::sniff_file_kind;
pub use row_count::{exact_file_row_count, exact_file_row_count_blocking};
pub use types::{
    ConnectedTableRecord, DataFileKind, ImportRequest, ImportSummary, ROW_ID_COLUMN,
    SavedViewRecord, SchemaField, SheetRecord, SparseCellRecord, TableSchema, VisibleQuery,
    WorkbookRecord,
};
pub use window::ArrowWindow;
