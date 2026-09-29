use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use cellium_core::TableViewState;
use tokio::task;

use crate::{
    ArrowWindow, ConnectedTableRecord, DataEngine, DataError, ImportRequest, ImportSummary,
    SavedViewRecord, SheetRecord, SparseCellRecord, VisibleQuery, WorkbookRecord,
};

#[derive(Clone)]
pub struct AsyncDataEngine {
    engine: Arc<Mutex<DataEngine>>,
}

impl AsyncDataEngine {
    pub fn in_memory() -> Result<Self, DataError> {
        Ok(Self {
            engine: Arc::new(Mutex::new(DataEngine::in_memory()?)),
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DataError> {
        Ok(Self {
            engine: Arc::new(Mutex::new(DataEngine::open(path)?)),
        })
    }

    pub async fn import_file(&self, request: ImportRequest) -> Result<ImportSummary, DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .import_file(request)
        })
        .await
    }

    pub async fn visible_window(&self, query: VisibleQuery) -> Result<ArrowWindow, DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .visible_window(&query)
        })
        .await
    }

    pub async fn materialize_source(
        &self,
        request: ImportRequest,
    ) -> Result<ImportSummary, DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            let mut guard = engine.lock().map_err(|_| DataError::LockPoisoned)?;
            guard.materialize_source(request)
        })
        .await
    }

    pub async fn upsert_workbook(&self, workbook: WorkbookRecord) -> Result<(), DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .upsert_workbook(&workbook)
        })
        .await
    }

    pub async fn upsert_sheet(&self, sheet: SheetRecord) -> Result<(), DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .upsert_sheet(&sheet)
        })
        .await
    }

    pub async fn upsert_connected_table(
        &self,
        table: ConnectedTableRecord,
    ) -> Result<(), DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .upsert_connected_table(&table)
        })
        .await
    }

    pub async fn upsert_sparse_cell(&self, cell: SparseCellRecord) -> Result<(), DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .upsert_sparse_cell(&cell)
        })
        .await
    }

    pub async fn update_table_cell(
        &self,
        table_name: String,
        available_columns: Vec<String>,
        row_id: u64,
        column: String,
        value: String,
    ) -> Result<(), DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .update_table_cell(&table_name, &available_columns, row_id, &column, &value)
        })
        .await
    }

    pub async fn view_row_count(
        &self,
        table_name: String,
        available_columns: Vec<String>,
        view: TableViewState,
    ) -> Result<u64, DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .view_row_count(&table_name, &available_columns, &view)
        })
        .await
    }

    pub async fn upsert_saved_view(&self, view: SavedViewRecord) -> Result<(), DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .upsert_saved_view(&view)
        })
        .await
    }

    pub async fn saved_views_for_sheet(
        &self,
        sheet_id: u64,
    ) -> Result<Vec<SavedViewRecord>, DataError> {
        let engine = Arc::clone(&self.engine);
        spawn_data_task(move || {
            engine
                .lock()
                .map_err(|_| DataError::LockPoisoned)?
                .saved_views_for_sheet(sheet_id)
        })
        .await
    }
}

pub(crate) async fn spawn_data_task<T>(
    work: impl FnOnce() -> Result<T, DataError> + Send + 'static,
) -> Result<T, DataError>
where
    T: Send + 'static,
{
    task::spawn_blocking(work)
        .await
        .map_err(DataError::BlockingTask)?
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use super::*;

    #[tokio::test]
    async fn async_data_engine_runs_import_and_query_jobs() {
        let directory = temp_fixture_dir("async");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();
        let engine = AsyncDataEngine::in_memory().unwrap();

        engine
            .import_file(ImportRequest {
                table_name: "async_orders".to_string(),
                path,
            })
            .await
            .unwrap();
        let window = engine
            .visible_window(VisibleQuery {
                table_name: "async_orders".to_string(),
                offset: 1,
                limit: 1,
                projection: vec!["id".to_string()],
                available_columns: vec!["id".to_string(), "amount".to_string()],
                view: TableViewState {
                    filters: Vec::new(),
                    sorts: vec![cellium_core::SortSpec {
                        column: "id".to_string(),
                        direction: cellium_core::SortDirection::Ascending,
                    }],
                },
                include_row_id: false,
                row_id_range: None,
            })
            .await
            .unwrap();

        assert_eq!(window.offset, 1);
    }

    fn temp_fixture_dir(label: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("cellium-data-{label}-{}-{now}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }
}
