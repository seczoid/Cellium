use anyhow::{Context, Result};
use cellium_data::AsyncDataEngine;
use tracing::info;

use crate::paths::session_database_path;

pub fn open_data_engine() -> Result<AsyncDataEngine> {
    let session_database = session_database_path();
    info!(database = %session_database.display(), "opening file-backed DuckDB session");
    AsyncDataEngine::open(&session_database).context("failed to open DuckDB session")
}
