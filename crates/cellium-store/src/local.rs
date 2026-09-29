use cellium_core::{Workbook, WorkbookId};
use libsql::{Builder, Connection};

use crate::{
    LibraryRepository, StoreError, WorkbookLibraryEntry, WorkbookListFilter, WorkbookRepository,
};

pub struct LocalWorkbookRepository {
    connection: Connection,
}

impl LocalWorkbookRepository {
    pub async fn in_memory() -> Result<Self, StoreError> {
        let database = Builder::new_local(":memory:").build().await?;
        let connection = database.connect()?;
        let repository = Self { connection };
        repository.migrate().await?;
        Ok(repository)
    }

    pub async fn open(path: &str) -> Result<Self, StoreError> {
        let database = Builder::new_local(path).build().await?;
        let connection = database.connect()?;
        let repository = Self { connection };
        repository.migrate().await?;
        Ok(repository)
    }

    async fn migrate(&self) -> Result<(), StoreError> {
        self.connection
            .execute(
                "CREATE TABLE IF NOT EXISTS workbooks (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    json TEXT NOT NULL
                )",
                (),
            )
            .await?;
        self.connection
            .execute(
                "CREATE TABLE IF NOT EXISTS library_workbooks (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    last_opened_at INTEGER NOT NULL,
                    starred INTEGER NOT NULL DEFAULT 0
                )",
                (),
            )
            .await?;
        self.connection
            .execute(
                "CREATE INDEX IF NOT EXISTS idx_library_workbooks_recent
                 ON library_workbooks (last_opened_at DESC)",
                (),
            )
            .await?;
        Ok(())
    }
}

impl WorkbookRepository for LocalWorkbookRepository {
    async fn save_workbook(&self, workbook: &Workbook) -> Result<(), StoreError> {
        let json = serde_json::to_string(workbook)?;
        self.connection
            .execute(
                "INSERT INTO workbooks (id, name, json) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, json = excluded.json",
                (workbook.id.0 as i64, workbook.name.as_str(), json),
            )
            .await?;
        Ok(())
    }

    async fn load_workbook(&self, id: WorkbookId) -> Result<Workbook, StoreError> {
        let mut rows = self
            .connection
            .query("SELECT json FROM workbooks WHERE id = ?1", [id.0 as i64])
            .await?;
        let Some(row) = rows.next().await? else {
            return Err(StoreError::NotFound(id));
        };
        let json = row.get::<String>(0)?;
        Ok(serde_json::from_str(&json)?)
    }
}

impl LibraryRepository for LocalWorkbookRepository {
    async fn upsert_workbook(&self, workbook: &WorkbookLibraryEntry) -> Result<(), StoreError> {
        self.connection
            .execute(
                "INSERT INTO library_workbooks
                    (id, name, file_path, created_at, updated_at, last_opened_at, starred)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    file_path = excluded.file_path,
                    updated_at = excluded.updated_at,
                    last_opened_at = excluded.last_opened_at,
                    starred = excluded.starred",
                (
                    workbook.id.0 as i64,
                    workbook.name.as_str(),
                    workbook.file_path.as_str(),
                    workbook.created_at,
                    workbook.updated_at,
                    workbook.last_opened_at,
                    i64::from(workbook.starred),
                ),
            )
            .await?;
        Ok(())
    }

    async fn list_workbooks(
        &self,
        filter: WorkbookListFilter,
        limit: u32,
    ) -> Result<Vec<WorkbookLibraryEntry>, StoreError> {
        let query = match filter {
            WorkbookListFilter::Recent => {
                "SELECT id, name, file_path, created_at, updated_at, last_opened_at, starred
                 FROM library_workbooks
                 ORDER BY last_opened_at DESC, updated_at DESC
                 LIMIT ?1"
            }
            WorkbookListFilter::Starred => {
                "SELECT id, name, file_path, created_at, updated_at, last_opened_at, starred
                 FROM library_workbooks
                 WHERE starred = 1
                 ORDER BY last_opened_at DESC, updated_at DESC
                 LIMIT ?1"
            }
            WorkbookListFilter::All => {
                "SELECT id, name, file_path, created_at, updated_at, last_opened_at, starred
                 FROM library_workbooks
                 ORDER BY name COLLATE NOCASE ASC
                 LIMIT ?1"
            }
        };
        let mut rows = self.connection.query(query, [i64::from(limit)]).await?;
        let mut workbooks = Vec::new();
        while let Some(row) = rows.next().await? {
            workbooks.push(WorkbookLibraryEntry {
                id: WorkbookId(row.get::<i64>(0)? as u64),
                name: row.get::<String>(1)?,
                file_path: row.get::<String>(2)?,
                created_at: row.get::<i64>(3)?,
                updated_at: row.get::<i64>(4)?,
                last_opened_at: row.get::<i64>(5)?,
                starred: row.get::<i64>(6)? != 0,
            });
        }
        Ok(workbooks)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cellium_core::{
        CellRef, CellValue, ColumnId, ComputedColumn, ConnectedTable, FilterExpr, FilterOperator,
        SavedView, Sheet, SheetId, SortDirection, SortSpec, SourceKind, TableColumn, TableId,
        TableViewState, ViewLayout,
    };

    use super::*;

    #[tokio::test]
    async fn local_repository_round_trips_workbook() {
        let repository = LocalWorkbookRepository::in_memory().await.unwrap();
        let mut workbook = Workbook::new(WorkbookId(1), "Book");
        workbook.add_sheet(Sheet::new(SheetId(2), "Sheet"));

        repository.save_workbook(&workbook).await.unwrap();
        let loaded = repository.load_workbook(WorkbookId(1)).await.unwrap();

        assert_eq!(loaded.name, "Book");
    }

    #[tokio::test]
    async fn local_repository_restores_workbook_editable_state_and_table_metadata() {
        let repository = LocalWorkbookRepository::in_memory().await.unwrap();
        let mut workbook = Workbook::new(WorkbookId(1), "Book");
        let sheet_id = SheetId(2);
        let mut sheet = Sheet::new(sheet_id, "Sheet");
        let table = ConnectedTable {
            id: TableId(3),
            name: "orders".to_string(),
            source_path: "/tmp/orders.parquet".to_string(),
            source_kind: SourceKind::Parquet,
            anchor: CellRef::new(4, 2),
            columns: vec![TableColumn {
                id: ColumnId(4),
                name: "amount".to_string(),
                data_type: "DOUBLE".to_string(),
            }],
            row_count: Some(50_000_000),
        };
        sheet.set_cell(
            CellRef::new(1, 1),
            CellValue::Formula("=SUM(B2:B10)".to_string()),
        );
        sheet.add_connected_table(table);
        sheet.add_computed_column(ComputedColumn {
            id: ColumnId(5),
            table_id: TableId(3),
            name: "gross".to_string(),
            formula: "amount * 1.2".to_string(),
        });
        sheet.add_saved_view(SavedView {
            name: "High value".to_string(),
            table_id: TableId(3),
            state: TableViewState {
                filters: vec![FilterExpr {
                    column: "amount".to_string(),
                    operator: FilterOperator::GreaterThan,
                    value: Some("100".to_string()),
                }],
                sorts: vec![SortSpec {
                    column: "amount".to_string(),
                    direction: SortDirection::Descending,
                }],
            },
            layout: ViewLayout {
                frozen_rows: 1,
                frozen_columns: 1,
                hidden_columns: vec![ColumnId(4)],
                column_order: vec![ColumnId(5), ColumnId(4)],
                column_widths: BTreeMap::from([(ColumnId(4), 180)]),
            },
        });
        workbook.add_sheet(sheet);

        repository.save_workbook(&workbook).await.unwrap();
        let loaded = repository.load_workbook(WorkbookId(1)).await.unwrap();

        assert_eq!(
            loaded.sheet(sheet_id).unwrap(),
            workbook.sheet(sheet_id).unwrap()
        );
    }

    #[tokio::test]
    async fn library_repository_lists_recent_and_starred_workbooks() {
        let repository = LocalWorkbookRepository::in_memory().await.unwrap();
        repository
            .upsert_workbook(&WorkbookLibraryEntry {
                id: WorkbookId(1),
                name: "Older".to_string(),
                file_path: "/tmp/older.cellium".to_string(),
                created_at: 10,
                updated_at: 10,
                last_opened_at: 20,
                starred: false,
            })
            .await
            .unwrap();
        repository
            .upsert_workbook(&WorkbookLibraryEntry {
                id: WorkbookId(2),
                name: "Recent".to_string(),
                file_path: "/tmp/recent.cellium".to_string(),
                created_at: 10,
                updated_at: 30,
                last_opened_at: 40,
                starred: true,
            })
            .await
            .unwrap();

        let recent = repository
            .list_workbooks(WorkbookListFilter::Recent, 10)
            .await
            .unwrap();

        assert_eq!(
            recent.first().map(|workbook| workbook.name.as_str()),
            Some("Recent")
        );
        assert_eq!(
            repository
                .list_workbooks(WorkbookListFilter::Starred, 10)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
