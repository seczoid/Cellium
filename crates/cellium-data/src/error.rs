use thiserror::Error;

#[derive(Debug, Error)]
pub enum DataError {
    #[error("unsupported file extension")]
    UnsupportedFile,
    #[error("file path is not valid UTF-8")]
    NonUtf8Path,
    #[error("invalid SQL identifier `{0}`")]
    InvalidIdentifier(String),
    #[error("column `{0}` does not exist in this table")]
    UnknownColumn(String),
    #[error("filter for column `{0}` requires a value")]
    MissingFilterValue(String),
    #[error("filter value `{0}` is not a finite number")]
    InvalidNumericFilter(String),
    #[error("row `{row_id}` was not found in table `{table_name}`")]
    RowNotFound { table_name: String, row_id: u64 },
    #[error("data engine lock was poisoned")]
    LockPoisoned,
    #[error("blocking data task failed: {0}")]
    BlockingTask(#[from] tokio::task::JoinError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("DuckDB error: {0}")]
    DuckDb(#[from] duckdb::Error),
    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),
    #[error("file metadata reported a negative row count: {0}")]
    NegativeRowCount(i64),
    #[error("row count does not fit in u64")]
    RowCountOverflow,
    #[error("identifier value does not fit in DuckDB BIGINT: {0}")]
    IdOverflow(u64),
    #[error("metadata id must be non-negative: {0}")]
    NegativeMetadataId(i64),
    #[error("unknown source kind `{0}` in workbook metadata")]
    UnknownSourceKind(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
