use cellium_core::WorkbookId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("libsql error: {0}")]
    Libsql(#[from] libsql::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("workbook `{0:?}` was not found")]
    NotFound(WorkbookId),
}
