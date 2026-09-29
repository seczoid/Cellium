use std::{fs::File, path::Path};

use arrow::{compute::cast, datatypes::DataType, ipc::reader::FileReader as ArrowFileReader};
use cellium_core::TableViewState;
use duckdb::{Connection, params_from_iter, types::Value};

use crate::{
    ArrowWindow, ConnectedTableRecord, DataError, DataFileKind, ImportRequest, ImportSummary,
    ROW_ID_COLUMN, SavedViewRecord, SchemaField, SheetRecord, SparseCellRecord, TableSchema,
    VisibleQuery, WorkbookRecord, sniff_file_kind,
    sql::{
        compile_view_filter, compile_view_sort, quote_sql_identifier, sql_string_literal,
        validate_identifier, value_to_grid_string,
    },
    window::string_array_value,
    workbook::{
        i64_from_u64, option_i64_from_u64, source_kind_from_label, source_kind_label, u64_from_i64,
    },
};

pub struct DataEngine {
    connection: Connection,
}

impl DataEngine {
    pub fn in_memory() -> Result<Self, DataError> {
        let engine = Self {
            connection: Connection::open_in_memory()?,
        };
        engine.initialize_workbook_schema()?;
        Ok(engine)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, DataError> {
        let engine = Self {
            connection: Connection::open(path.as_ref())?,
        };
        engine.initialize_workbook_schema()?;
        Ok(engine)
    }

    pub fn initialize_workbook_schema(&self) -> Result<(), DataError> {
        self.connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS cellium_workbooks (
                id BIGINT PRIMARY KEY,
                name VARCHAR NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cellium_sheets (
                id BIGINT PRIMARY KEY,
                workbook_id BIGINT NOT NULL,
                name VARCHAR NOT NULL,
                position BIGINT NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cellium_tables (
                id BIGINT PRIMARY KEY,
                sheet_id BIGINT NOT NULL,
                name VARCHAR NOT NULL,
                physical_table_name VARCHAR NOT NULL UNIQUE,
                source_path VARCHAR,
                source_kind VARCHAR,
                anchor_row BIGINT NOT NULL,
                anchor_col BIGINT NOT NULL,
                row_count BIGINT,
                schema_json VARCHAR NOT NULL,
                materialized BOOLEAN NOT NULL DEFAULT false,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cellium_cells (
                sheet_id BIGINT NOT NULL,
                row_index BIGINT NOT NULL,
                col_index BIGINT NOT NULL,
                value VARCHAR,
                formula VARCHAR,
                updated_at BIGINT NOT NULL,
                PRIMARY KEY (sheet_id, row_index, col_index)
            );

            CREATE TABLE IF NOT EXISTS cellium_computed_columns (
                id BIGINT PRIMARY KEY,
                table_id BIGINT NOT NULL,
                name VARCHAR NOT NULL,
                formula VARCHAR NOT NULL,
                position BIGINT NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cellium_saved_views (
                id BIGINT PRIMARY KEY,
                sheet_id BIGINT NOT NULL,
                name VARCHAR NOT NULL,
                table_id BIGINT,
                filter_sql VARCHAR,
                sort_sql VARCHAR,
                view_state_json VARCHAR NOT NULL DEFAULT '{}',
                layout_json VARCHAR NOT NULL DEFAULT '{}',
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                UNIQUE (sheet_id, name)
            );
            ",
        )?;
        self.connection.execute_batch(
            "ALTER TABLE cellium_saved_views
             ADD COLUMN IF NOT EXISTS view_state_json VARCHAR",
        )?;
        Ok(())
    }

    pub fn upsert_workbook(&self, workbook: &WorkbookRecord) -> Result<(), DataError> {
        self.connection.execute(
            "INSERT INTO cellium_workbooks (id, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                updated_at = excluded.updated_at",
            (
                i64_from_u64(workbook.id)?,
                workbook.name.as_str(),
                workbook.created_at,
                workbook.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn upsert_sheet(&self, sheet: &SheetRecord) -> Result<(), DataError> {
        self.connection.execute(
            "INSERT INTO cellium_sheets
                (id, workbook_id, name, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                workbook_id = excluded.workbook_id,
                name = excluded.name,
                position = excluded.position,
                updated_at = excluded.updated_at",
            (
                i64_from_u64(sheet.id)?,
                i64_from_u64(sheet.workbook_id)?,
                sheet.name.as_str(),
                sheet.position,
                sheet.created_at,
                sheet.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn upsert_connected_table(&self, table: &ConnectedTableRecord) -> Result<(), DataError> {
        let schema_json = serde_json::to_string(&table.schema)?;
        self.connection.execute(
            "INSERT INTO cellium_tables
                (id, sheet_id, name, physical_table_name, source_path, source_kind,
                 anchor_row, anchor_col, row_count, schema_json, materialized,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                sheet_id = excluded.sheet_id,
                name = excluded.name,
                physical_table_name = excluded.physical_table_name,
                source_path = excluded.source_path,
                source_kind = excluded.source_kind,
                anchor_row = excluded.anchor_row,
                anchor_col = excluded.anchor_col,
                row_count = excluded.row_count,
                schema_json = excluded.schema_json,
                materialized = excluded.materialized,
                updated_at = excluded.updated_at",
            (
                i64_from_u64(table.id)?,
                i64_from_u64(table.sheet_id)?,
                table.name.as_str(),
                table.physical_table_name.as_str(),
                table.source_path.as_deref(),
                table.source_kind.map(source_kind_label),
                i64_from_u64(table.anchor_row)?,
                i64_from_u64(table.anchor_col)?,
                option_i64_from_u64(table.row_count)?,
                schema_json,
                table.materialized,
                table.created_at,
                table.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn upsert_sparse_cell(&self, cell: &SparseCellRecord) -> Result<(), DataError> {
        let sheet_id = i64_from_u64(cell.sheet_id)?;
        let row_index = i64_from_u64(cell.row_index)?;
        let col_index = i64_from_u64(cell.col_index)?;
        if cell.value.is_empty() && cell.formula.as_deref().is_none_or(str::is_empty) {
            self.connection.execute(
                "DELETE FROM cellium_cells
                 WHERE sheet_id = ?1 AND row_index = ?2 AND col_index = ?3",
                (sheet_id, row_index, col_index),
            )?;
            return Ok(());
        }

        self.connection.execute(
            "INSERT INTO cellium_cells
                (sheet_id, row_index, col_index, value, formula, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(sheet_id, row_index, col_index) DO UPDATE SET
                value = excluded.value,
                formula = excluded.formula,
                updated_at = excluded.updated_at",
            (
                sheet_id,
                row_index,
                col_index,
                cell.value.as_str(),
                cell.formula.as_deref(),
                cell.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn sparse_cell(
        &self,
        sheet_id: u64,
        row_index: u64,
        col_index: u64,
    ) -> Result<Option<SparseCellRecord>, DataError> {
        let mut statement = self.connection.prepare(
            "SELECT value, formula, updated_at
             FROM cellium_cells
             WHERE sheet_id = ?1 AND row_index = ?2 AND col_index = ?3",
        )?;
        let mut rows = statement.query((
            i64_from_u64(sheet_id)?,
            i64_from_u64(row_index)?,
            i64_from_u64(col_index)?,
        ))?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(SparseCellRecord {
            sheet_id,
            row_index,
            col_index,
            value: row.get::<_, String>(0)?,
            formula: row.get::<_, Option<String>>(1)?,
            updated_at: row.get::<_, i64>(2)?,
        }))
    }

    pub fn first_sheet(&self) -> Result<Option<SheetRecord>, DataError> {
        let mut statement = self.connection.prepare(
            "SELECT id, workbook_id, name, position, created_at, updated_at
             FROM cellium_sheets
             ORDER BY position ASC, id ASC
             LIMIT 1",
        )?;
        let mut rows = statement.query([])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(SheetRecord {
            id: u64_from_i64(row.get::<_, i64>(0)?)?,
            workbook_id: u64_from_i64(row.get::<_, i64>(1)?)?,
            name: row.get::<_, String>(2)?,
            position: row.get::<_, i64>(3)?,
            created_at: row.get::<_, i64>(4)?,
            updated_at: row.get::<_, i64>(5)?,
        }))
    }

    pub fn connected_tables_for_sheet(
        &self,
        sheet_id: u64,
    ) -> Result<Vec<ConnectedTableRecord>, DataError> {
        let mut statement = self.connection.prepare(
            "SELECT id, sheet_id, name, physical_table_name, source_path, source_kind,
                    anchor_row, anchor_col, row_count, schema_json, materialized,
                    created_at, updated_at
             FROM cellium_tables
             WHERE sheet_id = ?1
             ORDER BY id ASC",
        )?;
        let mut rows = statement.query([i64_from_u64(sheet_id)?])?;
        let mut tables = Vec::new();
        while let Some(row) = rows.next()? {
            let source_kind = row
                .get::<_, Option<String>>(5)?
                .map(source_kind_from_label)
                .transpose()?;
            let schema_json = row.get::<_, String>(9)?;
            let schema = serde_json::from_str::<TableSchema>(&schema_json)?;
            tables.push(ConnectedTableRecord {
                id: u64_from_i64(row.get::<_, i64>(0)?)?,
                sheet_id: u64_from_i64(row.get::<_, i64>(1)?)?,
                name: row.get::<_, String>(2)?,
                physical_table_name: row.get::<_, String>(3)?,
                source_path: row.get::<_, Option<String>>(4)?,
                source_kind,
                anchor_row: u64_from_i64(row.get::<_, i64>(6)?)?,
                anchor_col: u64_from_i64(row.get::<_, i64>(7)?)?,
                row_count: row
                    .get::<_, Option<i64>>(8)?
                    .map(u64_from_i64)
                    .transpose()?,
                schema,
                materialized: row.get::<_, bool>(10)?,
                created_at: row.get::<_, i64>(11)?,
                updated_at: row.get::<_, i64>(12)?,
            });
        }
        Ok(tables)
    }

    pub fn sparse_cells_for_sheet(
        &self,
        sheet_id: u64,
    ) -> Result<Vec<SparseCellRecord>, DataError> {
        let mut statement = self.connection.prepare(
            "SELECT sheet_id, row_index, col_index, value, formula, updated_at
             FROM cellium_cells
             WHERE sheet_id = ?1
             ORDER BY row_index ASC, col_index ASC",
        )?;
        let mut rows = statement.query([i64_from_u64(sheet_id)?])?;
        let mut cells = Vec::new();
        while let Some(row) = rows.next()? {
            cells.push(SparseCellRecord {
                sheet_id: u64_from_i64(row.get::<_, i64>(0)?)?,
                row_index: u64_from_i64(row.get::<_, i64>(1)?)?,
                col_index: u64_from_i64(row.get::<_, i64>(2)?)?,
                value: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                formula: row.get::<_, Option<String>>(4)?,
                updated_at: row.get::<_, i64>(5)?,
            });
        }
        Ok(cells)
    }

    pub fn upsert_saved_view(&self, view: &SavedViewRecord) -> Result<(), DataError> {
        let state_json = serde_json::to_string(&view.state)?;
        let layout_json = serde_json::to_string(&view.layout)?;
        self.connection.execute(
            "INSERT INTO cellium_saved_views
                (id, sheet_id, name, table_id, filter_sql, sort_sql, view_state_json,
                 layout_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8)
             ON CONFLICT(sheet_id, name) DO UPDATE SET
                table_id = excluded.table_id,
                filter_sql = NULL,
                sort_sql = NULL,
                view_state_json = excluded.view_state_json,
                layout_json = excluded.layout_json,
                updated_at = excluded.updated_at",
            (
                i64_from_u64(view.id)?,
                i64_from_u64(view.sheet_id)?,
                view.name.as_str(),
                option_i64_from_u64(view.table_id)?,
                state_json,
                layout_json,
                view.created_at,
                view.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn saved_views_for_sheet(&self, sheet_id: u64) -> Result<Vec<SavedViewRecord>, DataError> {
        let mut statement = self.connection.prepare(
            "SELECT id, sheet_id, name, table_id, COALESCE(view_state_json, '{}'), layout_json,
                    created_at, updated_at
             FROM cellium_saved_views
             WHERE sheet_id = ?1
             ORDER BY updated_at DESC, name ASC",
        )?;
        let mut rows = statement.query([i64_from_u64(sheet_id)?])?;
        let mut views = Vec::new();
        while let Some(row) = rows.next()? {
            views.push(SavedViewRecord {
                id: u64_from_i64(row.get::<_, i64>(0)?)?,
                sheet_id: u64_from_i64(row.get::<_, i64>(1)?)?,
                name: row.get::<_, String>(2)?,
                table_id: row
                    .get::<_, Option<i64>>(3)?
                    .map(u64_from_i64)
                    .transpose()?,
                state: serde_json::from_str(&row.get::<_, String>(4)?)?,
                layout: serde_json::from_str(&row.get::<_, String>(5)?)?,
                created_at: row.get::<_, i64>(6)?,
                updated_at: row.get::<_, i64>(7)?,
            });
        }
        Ok(views)
    }

    pub fn update_table_cell(
        &self,
        table_name: &str,
        available_columns: &[String],
        row_id: u64,
        column: &str,
        value: &str,
    ) -> Result<(), DataError> {
        let table_name = validate_identifier(table_name)?;
        if !available_columns
            .iter()
            .any(|candidate| candidate == column)
        {
            return Err(DataError::UnknownColumn(column.to_string()));
        }
        let column = quote_sql_identifier(column);
        let row_id_column = quote_sql_identifier(ROW_ID_COLUMN);
        let updated = self.connection.execute(
            &format!(
                "UPDATE {table_name}
                 SET {column} = ?1
                 WHERE {row_id_column} = ?2"
            ),
            (value, i64_from_u64(row_id)?),
        )?;
        if updated == 0 {
            return Err(DataError::RowNotFound {
                table_name: table_name.to_string(),
                row_id,
            });
        }
        Ok(())
    }

    pub fn view_row_count(
        &self,
        table_name: &str,
        available_columns: &[String],
        view: &TableViewState,
    ) -> Result<u64, DataError> {
        let table_name = validate_identifier(table_name)?;
        let filter = compile_view_filter(view, available_columns, None)?;
        let mut sql = format!("SELECT COUNT(*) FROM {table_name}");
        if let Some(filter) = filter {
            sql.push_str(" WHERE ");
            sql.push_str(&filter);
        }
        self.connection
            .query_row(&sql, [], |row| row.get::<_, u64>(0))
            .map_err(DataError::from)
    }

    pub fn register_file(&self, table_name: &str, path: &Path) -> Result<TableSchema, DataError> {
        let summary = self.import_file(ImportRequest {
            table_name: table_name.to_string(),
            path: path.to_path_buf(),
        })?;
        Ok(summary.schema)
    }

    pub fn import_file(&self, request: ImportRequest) -> Result<ImportSummary, DataError> {
        let table_name = validate_identifier(&request.table_name)?;
        let path = request.path;
        let kind = sniff_file_kind(&path)?;
        let path_text = path.to_str().ok_or(DataError::NonUtf8Path)?;
        let path_sql = sql_string_literal(path_text);
        match kind {
            DataFileKind::Csv => {
                self.connection.execute(
                    &format!(
                        "CREATE OR REPLACE VIEW {table_name} AS SELECT * FROM read_csv_auto({path_sql}, all_varchar = true)"
                    ),
                    [],
                )?;
            }
            DataFileKind::Parquet => {
                self.connection.execute(
                    &format!(
                        "CREATE OR REPLACE VIEW {table_name} AS SELECT * FROM read_parquet({path_sql})"
                    ),
                    [],
                )?;
            }
            DataFileKind::Arrow => {
                self.import_arrow_ipc_as_text(table_name, &path)?;
            }
        }
        Ok(ImportSummary {
            table_name: table_name.to_string(),
            source_kind: kind,
            schema: self.schema(table_name)?,
            source_path: path,
            row_count: match kind {
                DataFileKind::Csv | DataFileKind::Parquet => None,
                DataFileKind::Arrow => self.row_count(table_name)?,
            },
        })
    }

    pub fn schema(&self, table_name: &str) -> Result<TableSchema, DataError> {
        let table_name = validate_identifier(table_name)?;
        let mut statement = self
            .connection
            .prepare(&format!("DESCRIBE SELECT * FROM {table_name} LIMIT 0"))?;
        let rows = statement.query_map([], |row| {
            Ok(SchemaField {
                name: row.get::<_, String>(0)?,
                data_type: row.get::<_, String>(1)?,
            })
        })?;
        let mut fields = Vec::new();
        for row in rows {
            let field = row?;
            if field.name != ROW_ID_COLUMN {
                fields.push(field);
            }
        }
        Ok(TableSchema { fields })
    }

    pub fn visible_window(&self, query: &VisibleQuery) -> Result<ArrowWindow, DataError> {
        let table_name = validate_identifier(&query.table_name)?;
        let visible_projection = if query.projection.is_empty() {
            query
                .available_columns
                .iter()
                .map(|name| quote_sql_identifier(name))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            query
                .projection
                .iter()
                .map(|name| quote_sql_identifier(name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let projection = if query.include_row_id {
            if visible_projection.is_empty() {
                quote_sql_identifier(ROW_ID_COLUMN)
            } else {
                format!(
                    "{}, {visible_projection}",
                    quote_sql_identifier(ROW_ID_COLUMN)
                )
            }
        } else {
            visible_projection
        };
        let mut sql = format!("SELECT {projection} FROM {table_name}");
        if let Some(filter) =
            compile_view_filter(&query.view, &query.available_columns, query.row_id_range)?
        {
            sql.push_str(" WHERE ");
            sql.push_str(&filter);
        }
        if let Some(sort) = compile_view_sort(
            &query.view.sorts,
            &query.available_columns,
            query.include_row_id,
        )? {
            sql.push_str(" ORDER BY ");
            sql.push_str(&sort);
        }
        sql.push_str(&format!(" LIMIT {} OFFSET {}", query.limit, query.offset));

        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query([])?;
        let mut names: Vec<String> = rows
            .as_ref()
            .map(duckdb::Statement::column_names)
            .unwrap_or_default()
            .iter()
            .map(ToString::to_string)
            .collect();
        if query.include_row_id && !names.is_empty() {
            names.remove(0);
        }
        let mut values = vec![Vec::<String>::new(); names.len()];
        let mut row_ids = Vec::new();
        while let Some(row) = rows.next()? {
            if query.include_row_id {
                row_ids.push(Some(u64_from_i64(row.get::<_, i64>(0)?)?));
            } else {
                row_ids.push(None);
            }
            for (index, column) in values.iter_mut().enumerate() {
                let value = row.get::<_, Value>(index + usize::from(query.include_row_id))?;
                column.push(value_to_grid_string(&value));
            }
        }

        ArrowWindow::from_columns(names, values, query.offset, row_ids)
    }

    pub fn row_count(&self, table_name: &str) -> Result<Option<u64>, DataError> {
        let table_name = validate_identifier(table_name)?;
        let count = self.connection.query_row(
            &format!("SELECT COUNT(*) FROM {table_name}"),
            [],
            |row| row.get::<_, u64>(0),
        )?;
        Ok(Some(count))
    }

    pub(crate) fn materialize_source(
        &mut self,
        request: ImportRequest,
    ) -> Result<ImportSummary, DataError> {
        let table_name = validate_identifier(&request.table_name)?;
        let path = request.path;
        let kind = sniff_file_kind(&path)?;
        let path_text = path.to_str().ok_or(DataError::NonUtf8Path)?;
        let path_sql = sql_string_literal(path_text);
        match kind {
            DataFileKind::Csv => {
                self.connection.execute(
                    &format!(
                        "CREATE OR REPLACE TABLE {table_name} AS SELECT row_number() OVER () AS {ROW_ID_COLUMN}, * FROM read_csv_auto({path_sql}, all_varchar = true)"
                    ),
                    [],
                )?;
            }
            DataFileKind::Parquet => {
                self.connection.execute(
                    &format!(
                        "CREATE OR REPLACE TABLE {table_name} AS SELECT row_number() OVER () AS {ROW_ID_COLUMN}, * FROM read_parquet({path_sql})"
                    ),
                    [],
                )?;
            }
            DataFileKind::Arrow => {
                self.import_arrow_ipc_as_text(table_name, &path)?;
            }
        }
        Ok(ImportSummary {
            table_name: table_name.to_string(),
            source_kind: kind,
            schema: self.schema(table_name)?,
            source_path: path,
            row_count: self.row_count(table_name)?,
        })
    }

    fn import_arrow_ipc_as_text(&self, table_name: &str, path: &Path) -> Result<(), DataError> {
        let file = File::open(path)?;
        let reader = ArrowFileReader::try_new(file, None)?;
        let fields = reader
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().to_string())
            .collect::<Vec<_>>();
        let column_defs = fields
            .iter()
            .map(|field| validate_identifier(field).map(|field| format!("{field} TEXT")))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        self.connection
            .execute(&format!("DROP TABLE IF EXISTS {table_name}"), [])?;
        let column_defs = if column_defs.is_empty() {
            format!("{ROW_ID_COLUMN} BIGINT")
        } else {
            format!("{ROW_ID_COLUMN} BIGINT, {column_defs}")
        };
        self.connection
            .execute(&format!("CREATE TABLE {table_name} ({column_defs})"), [])?;

        let placeholders = (1..=fields.len() + 1)
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let insert_sql = format!("INSERT INTO {table_name} VALUES ({placeholders})");
        let mut statement = self.connection.prepare(&insert_sql)?;
        let mut next_row_id = 1_i64;
        for batch in reader {
            let batch = batch?;
            let utf8_columns = batch
                .columns()
                .iter()
                .map(|column| cast(column, &DataType::Utf8))
                .collect::<Result<Vec<_>, _>>()?;
            for row in 0..batch.num_rows() {
                let mut row_values = Vec::with_capacity(utf8_columns.len() + 1);
                row_values.push(Value::BigInt(next_row_id));
                row_values.extend(utf8_columns.iter().map(|column| {
                    match string_array_value(column, row) {
                        Some(value) => Value::Text(value),
                        None => Value::Null,
                    }
                }));
                statement.execute(params_from_iter(row_values.iter()))?;
                next_row_id = next_row_id.saturating_add(1);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write, path::PathBuf, sync::Arc, time::SystemTime};

    use arrow::{
        array::{ArrayRef, Float64Array, Int64Array},
        ipc::writer::FileWriter,
        record_batch::RecordBatch,
    };
    use cellium_core::{
        FilterExpr, FilterOperator, SortDirection, SortSpec, TableViewState, ViewLayout,
    };
    use parquet::arrow::ArrowWriter;

    use super::*;

    #[test]
    fn duckdb_registers_csv_without_counting_all_rows() {
        let directory = temp_fixture_dir("csv");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();
        let engine = DataEngine::in_memory().unwrap();

        let summary = engine
            .import_file(ImportRequest {
                table_name: "orders".to_string(),
                path,
            })
            .unwrap();
        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "orders".to_string(),
                offset: 0,
                limit: 2,
                projection: vec!["id".to_string(), "amount".to_string()],
                available_columns: vec!["id".to_string(), "amount".to_string()],
                view: TableViewState {
                    filters: Vec::new(),
                    sorts: vec![SortSpec {
                        column: "id".to_string(),
                        direction: SortDirection::Ascending,
                    }],
                },
                include_row_id: false,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(
            (
                summary.row_count,
                window.batch.num_rows(),
                window.batch.num_columns()
            ),
            (None, 2, 2)
        );
    }

    #[test]
    fn duckdb_imports_mixed_type_csv_as_text() {
        let directory = temp_fixture_dir("csv-mixed");
        let path = directory.join("mixed.csv");
        std::fs::write(&path, "flag\ntrue\n???\n").unwrap();
        let engine = DataEngine::in_memory().unwrap();

        let summary = engine
            .import_file(ImportRequest {
                table_name: "mixed".to_string(),
                path,
            })
            .unwrap();
        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "mixed".to_string(),
                offset: 0,
                limit: 2,
                projection: vec!["flag".to_string()],
                available_columns: vec!["flag".to_string()],
                view: TableViewState::default(),
                include_row_id: false,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(
            (summary.row_count, window.rows_as_strings()),
            (
                None,
                vec![vec!["true".to_string()], vec!["???".to_string()]]
            )
        );
    }

    #[test]
    fn duckdb_projection_quotes_dotted_column_names() {
        let directory = temp_fixture_dir("csv-dotted-columns");
        let path = directory.join("contacts.csv");
        std::fs::write(&path, "id,phone.phones_enricher.carrier_name\n1,Verizon\n").unwrap();
        let engine = DataEngine::in_memory().unwrap();

        engine
            .import_file(ImportRequest {
                table_name: "contacts".to_string(),
                path,
            })
            .unwrap();
        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "contacts".to_string(),
                offset: 0,
                limit: 1,
                projection: vec!["phone.phones_enricher.carrier_name".to_string()],
                available_columns: vec![
                    "id".to_string(),
                    "phone.phones_enricher.carrier_name".to_string(),
                ],
                view: TableViewState::default(),
                include_row_id: false,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(window.rows_as_strings(), vec![vec!["Verizon".to_string()]]);
    }

    #[test]
    fn duckdb_materializes_lazy_csv_to_requested_table() {
        let directory = temp_fixture_dir("csv-materialized");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();
        let mut engine = DataEngine::in_memory().unwrap();

        engine
            .import_file(ImportRequest {
                table_name: "orders_view".to_string(),
                path: path.clone(),
            })
            .unwrap();
        let summary = engine
            .materialize_source(ImportRequest {
                table_name: "orders_cache".to_string(),
                path,
            })
            .unwrap();

        assert_eq!(summary.row_count, Some(2));
    }

    #[test]
    fn duckdb_materialization_adds_stable_internal_row_id() {
        let directory = temp_fixture_dir("csv-materialized-row-id");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();
        let mut engine = DataEngine::in_memory().unwrap();

        let summary = engine
            .materialize_source(ImportRequest {
                table_name: "orders_cache".to_string(),
                path,
            })
            .unwrap();
        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "orders_cache".to_string(),
                offset: 0,
                limit: 2,
                projection: vec!["id".to_string()],
                available_columns: vec!["id".to_string(), "amount".to_string()],
                view: TableViewState::default(),
                include_row_id: true,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(
            (
                summary.schema.fields[0].name.as_str(),
                window.rows_as_strings(),
                window.row_ids
            ),
            (
                "id",
                vec![vec!["1".to_string()], vec!["2".to_string()]],
                vec![Some(1), Some(2)]
            )
        );
    }

    #[test]
    fn duckdb_typed_view_filters_sorts_and_preserves_row_ids() {
        let directory = temp_fixture_dir("typed-view");
        let path = directory.join("contacts.csv");
        std::fs::write(&path, "name,city\nAlice,Boston\nBob,Boise\nCara,Austin\n").unwrap();
        let mut engine = DataEngine::in_memory().unwrap();
        engine
            .materialize_source(ImportRequest {
                table_name: "contacts".to_string(),
                path,
            })
            .unwrap();

        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "contacts".to_string(),
                offset: 0,
                limit: 10,
                projection: vec!["name".to_string(), "city".to_string()],
                available_columns: vec!["name".to_string(), "city".to_string()],
                view: TableViewState {
                    filters: vec![FilterExpr {
                        column: "city".to_string(),
                        operator: FilterOperator::StartsWith,
                        value: Some("bo".to_string()),
                    }],
                    sorts: vec![SortSpec {
                        column: "name".to_string(),
                        direction: SortDirection::Descending,
                    }],
                },
                include_row_id: true,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(
            (window.rows_as_strings(), window.row_ids),
            (
                vec![
                    vec!["Bob".to_string(), "Boise".to_string()],
                    vec!["Alice".to_string(), "Boston".to_string()],
                ],
                vec![Some(2), Some(1)],
            )
        );
    }

    #[test]
    fn duckdb_view_row_count_is_exact_for_typed_filter() {
        let directory = temp_fixture_dir("typed-view-count");
        let path = directory.join("contacts.csv");
        std::fs::write(&path, "name,status\nA,active\nB,inactive\nC,active\n").unwrap();
        let mut engine = DataEngine::in_memory().unwrap();
        engine
            .materialize_source(ImportRequest {
                table_name: "contacts".to_string(),
                path,
            })
            .unwrap();
        let view = TableViewState {
            filters: vec![FilterExpr {
                column: "status".to_string(),
                operator: FilterOperator::Equals,
                value: Some("active".to_string()),
            }],
            sorts: Vec::new(),
        };

        let count = engine
            .view_row_count(
                "contacts",
                &["name".to_string(), "status".to_string()],
                &view,
            )
            .unwrap();

        assert_eq!(count, 2);
    }

    #[test]
    fn duckdb_table_cell_update_targets_stable_row_id() {
        let directory = temp_fixture_dir("stable-edit");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();
        let mut engine = DataEngine::in_memory().unwrap();
        engine
            .materialize_source(ImportRequest {
                table_name: "orders".to_string(),
                path,
            })
            .unwrap();

        engine
            .update_table_cell(
                "orders",
                &["id".to_string(), "amount".to_string()],
                2,
                "amount",
                "99.0",
            )
            .unwrap();
        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "orders".to_string(),
                offset: 0,
                limit: 1,
                projection: vec!["amount".to_string()],
                available_columns: vec!["id".to_string(), "amount".to_string()],
                view: TableViewState::default(),
                include_row_id: true,
                row_id_range: Some((2, 2)),
            })
            .unwrap();

        assert_eq!(
            (window.rows_as_strings(), window.row_ids),
            (vec![vec!["99.0".to_string()]], vec![Some(2)])
        );
    }

    #[test]
    fn duckdb_saved_view_round_trips_typed_state() {
        let engine = DataEngine::in_memory().unwrap();
        let view = SavedViewRecord {
            id: 1,
            sheet_id: 2,
            name: "Default".to_string(),
            table_id: Some(3),
            state: TableViewState {
                filters: vec![FilterExpr {
                    column: "status".to_string(),
                    operator: FilterOperator::Equals,
                    value: Some("active".to_string()),
                }],
                sorts: vec![SortSpec {
                    column: "revenue".to_string(),
                    direction: SortDirection::Descending,
                }],
            },
            layout: ViewLayout::default(),
            created_at: 10,
            updated_at: 11,
        };

        engine.upsert_saved_view(&view).unwrap();
        let loaded = engine.saved_views_for_sheet(2).unwrap();

        assert_eq!(loaded, vec![view]);
    }

    #[test]
    fn duckdb_reopens_with_stable_edit_and_saved_view() {
        let directory = temp_fixture_dir("reopen-view-edit");
        let source_path = directory.join("contacts.csv");
        let workbook_path = directory.join("contacts.cellium");
        std::fs::write(&source_path, "name,status\nAlice,inactive\nBob,active\n").unwrap();
        let view = TableViewState {
            filters: vec![FilterExpr {
                column: "status".to_string(),
                operator: FilterOperator::Equals,
                value: Some("active".to_string()),
            }],
            sorts: vec![SortSpec {
                column: "name".to_string(),
                direction: SortDirection::Ascending,
            }],
        };

        {
            let mut engine = DataEngine::open(&workbook_path).unwrap();
            engine
                .materialize_source(ImportRequest {
                    table_name: "contacts".to_string(),
                    path: source_path,
                })
                .unwrap();
            engine
                .update_table_cell(
                    "contacts",
                    &["name".to_string(), "status".to_string()],
                    1,
                    "status",
                    "active",
                )
                .unwrap();
            engine
                .upsert_saved_view(&SavedViewRecord {
                    id: 1,
                    sheet_id: 1,
                    name: "Default".to_string(),
                    table_id: Some(1),
                    state: view,
                    layout: ViewLayout::default(),
                    created_at: 10,
                    updated_at: 11,
                })
                .unwrap();
        }

        let engine = DataEngine::open(&workbook_path).unwrap();
        let restored_view = engine.saved_views_for_sheet(1).unwrap().remove(0).state;
        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "contacts".to_string(),
                offset: 0,
                limit: 10,
                projection: vec!["name".to_string(), "status".to_string()],
                available_columns: vec!["name".to_string(), "status".to_string()],
                view: restored_view,
                include_row_id: true,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(
            (window.rows_as_strings(), window.row_ids),
            (
                vec![
                    vec!["Alice".to_string(), "active".to_string()],
                    vec!["Bob".to_string(), "active".to_string()],
                ],
                vec![Some(1), Some(2)],
            )
        );
    }

    #[test]
    fn duckdb_workbook_schema_persists_sparse_cells() {
        let engine = DataEngine::in_memory().unwrap();

        engine
            .upsert_workbook(&WorkbookRecord {
                id: 1,
                name: "Book".to_string(),
                created_at: 10,
                updated_at: 10,
            })
            .unwrap();
        engine
            .upsert_sheet(&SheetRecord {
                id: 1,
                workbook_id: 1,
                name: "Sheet 1".to_string(),
                position: 0,
                created_at: 10,
                updated_at: 10,
            })
            .unwrap();
        engine
            .upsert_sparse_cell(&SparseCellRecord {
                sheet_id: 1,
                row_index: 4,
                col_index: 2,
                value: "hello".to_string(),
                formula: None,
                updated_at: 11,
            })
            .unwrap();

        assert_eq!(
            engine.sparse_cell(1, 4, 2).unwrap().map(|cell| cell.value),
            Some("hello".to_string())
        );
    }

    #[test]
    fn duckdb_sparse_empty_cell_deletes_record() {
        let engine = DataEngine::in_memory().unwrap();
        let populated = SparseCellRecord {
            sheet_id: 1,
            row_index: 1,
            col_index: 1,
            value: "temporary".to_string(),
            formula: None,
            updated_at: 10,
        };
        engine.upsert_sparse_cell(&populated).unwrap();

        engine
            .upsert_sparse_cell(&SparseCellRecord {
                value: String::new(),
                updated_at: 11,
                ..populated
            })
            .unwrap();

        assert_eq!(engine.sparse_cell(1, 1, 1).unwrap(), None);
    }

    #[test]
    fn duckdb_workbook_metadata_loads_sheet_table_and_sparse_cells() {
        let engine = DataEngine::in_memory().unwrap();
        engine
            .upsert_workbook(&WorkbookRecord {
                id: 7,
                name: "Imported".to_string(),
                created_at: 10,
                updated_at: 10,
            })
            .unwrap();
        engine
            .upsert_sheet(&SheetRecord {
                id: 11,
                workbook_id: 7,
                name: "Sheet 1".to_string(),
                position: 0,
                created_at: 10,
                updated_at: 10,
            })
            .unwrap();
        engine
            .upsert_connected_table(&ConnectedTableRecord {
                id: 13,
                sheet_id: 11,
                name: "orders.csv".to_string(),
                physical_table_name: "import_orders".to_string(),
                source_path: Some("/tmp/orders.csv".to_string()),
                source_kind: Some(DataFileKind::Csv),
                anchor_row: 1,
                anchor_col: 1,
                row_count: Some(2),
                schema: TableSchema {
                    fields: vec![SchemaField {
                        name: "id".to_string(),
                        data_type: "VARCHAR".to_string(),
                    }],
                },
                materialized: true,
                created_at: 10,
                updated_at: 11,
            })
            .unwrap();
        engine
            .upsert_sparse_cell(&SparseCellRecord {
                sheet_id: 11,
                row_index: 2,
                col_index: 1,
                value: "edited".to_string(),
                formula: None,
                updated_at: 12,
            })
            .unwrap();

        assert_eq!(
            (
                engine.first_sheet().unwrap().map(|sheet| sheet.id),
                engine
                    .connected_tables_for_sheet(11)
                    .unwrap()
                    .first()
                    .map(|table| (table.id, table.physical_table_name.clone())),
                engine
                    .sparse_cells_for_sheet(11)
                    .unwrap()
                    .first()
                    .map(|cell| (cell.row_index, cell.col_index, cell.value.clone())),
            ),
            (
                Some(11),
                Some((13, "import_orders".to_string())),
                Some((2, 1, "edited".to_string())),
            )
        );
    }

    #[test]
    fn duckdb_file_backed_engine_persists_imports() {
        let directory = temp_fixture_dir("file-backed");
        let db_path = directory.join("session.duckdb");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();

        {
            let engine = DataEngine::open(&db_path).unwrap();
            engine
                .import_file(ImportRequest {
                    table_name: "orders".to_string(),
                    path,
                })
                .unwrap();
        }

        let engine = DataEngine::open(&db_path).unwrap();
        assert_eq!(engine.row_count("orders").unwrap(), Some(2));
    }

    #[test]
    fn arrow_window_returns_row_major_strings() {
        let directory = temp_fixture_dir("grid-rows");
        let path = directory.join("orders.csv");
        std::fs::write(&path, "id,amount\n1,12.5\n2,25.0\n").unwrap();
        let engine = DataEngine::in_memory().unwrap();
        engine
            .import_file(ImportRequest {
                table_name: "orders_grid".to_string(),
                path,
            })
            .unwrap();

        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "orders_grid".to_string(),
                offset: 0,
                limit: 1,
                projection: vec!["id".to_string(), "amount".to_string()],
                available_columns: vec!["id".to_string(), "amount".to_string()],
                view: TableViewState {
                    filters: Vec::new(),
                    sorts: vec![SortSpec {
                        column: "id".to_string(),
                        direction: SortDirection::Ascending,
                    }],
                },
                include_row_id: false,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(
            (window.column_names(), window.rows_as_strings()),
            (
                vec!["id".to_string(), "amount".to_string()],
                vec![vec!["1".to_string(), "12.5".to_string()]]
            )
        );
    }

    #[test]
    fn duckdb_registers_parquet_fixture_without_counting_all_rows() {
        let directory = temp_fixture_dir("parquet");
        let path = directory.join("orders.parquet");
        let batch = orders_batch();
        let file = File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(file, batch.schema(), None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();
        let engine = DataEngine::in_memory().unwrap();

        let summary = engine
            .import_file(ImportRequest {
                table_name: "orders_parquet".to_string(),
                path,
            })
            .unwrap();

        assert_eq!(summary.row_count, None);
    }

    #[test]
    fn arrow_ipc_fixture_is_materialized_as_duckdb_table() {
        let directory = temp_fixture_dir("arrow");
        let path = directory.join("orders.arrow");
        let batch = orders_batch();
        let mut file = File::create(&path).unwrap();
        let mut writer = FileWriter::try_new(&mut file, batch.schema_ref()).unwrap();
        writer.write(&batch).unwrap();
        writer.finish().unwrap();
        file.flush().unwrap();
        let engine = DataEngine::in_memory().unwrap();

        let summary = engine
            .import_file(ImportRequest {
                table_name: "orders_arrow".to_string(),
                path,
            })
            .unwrap();
        let window = engine
            .visible_window(&VisibleQuery {
                table_name: "orders_arrow".to_string(),
                offset: 0,
                limit: 1,
                projection: vec!["id".to_string(), "amount".to_string()],
                available_columns: vec!["id".to_string(), "amount".to_string()],
                view: TableViewState {
                    filters: Vec::new(),
                    sorts: vec![SortSpec {
                        column: "id".to_string(),
                        direction: SortDirection::Ascending,
                    }],
                },
                include_row_id: false,
                row_id_range: None,
            })
            .unwrap();

        assert_eq!(
            (
                summary.source_kind,
                summary.row_count,
                window.batch.num_rows()
            ),
            (DataFileKind::Arrow, Some(2), 1)
        );
    }

    fn orders_batch() -> RecordBatch {
        RecordBatch::try_from_iter(vec![
            ("id", Arc::new(Int64Array::from(vec![1, 2])) as ArrayRef),
            (
                "amount",
                Arc::new(Float64Array::from(vec![12.5, 25.0])) as ArrayRef,
            ),
        ])
        .unwrap()
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
