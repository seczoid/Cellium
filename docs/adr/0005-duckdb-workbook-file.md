# ADR 0005: DuckDB Workbook File

## Decision

Cellium's product file format is a DuckDB database file with a `.cellium` extension.

DuckDB owns:

- workbook metadata;
- sheet metadata;
- sparse editable cells;
- formulas and computed-column definitions;
- saved views and layout state;
- imported/materialized tables;
- visible-window queries over workbook-owned data.

Rust coordinates app state, input/editing behavior, formula evaluation, import jobs, and query requests. `wgpu` renders visible grid state only.

## Workbook Schema Shape

Manual sheets are sparse, not dense SQL tables with thousands of empty rows.

```sql
CREATE TABLE cellium_workbooks (...);
CREATE TABLE cellium_sheets (...);
CREATE TABLE cellium_tables (...);

CREATE TABLE cellium_cells (
    sheet_id BIGINT NOT NULL,
    row_index BIGINT NOT NULL,
    col_index BIGINT NOT NULL,
    value VARCHAR,
    formula VARCHAR,
    PRIMARY KEY (sheet_id, row_index, col_index)
);
```

Imported datasets are normal DuckDB tables. CSV materialization should add a stable row id:

```sql
CREATE TABLE trip_data AS
SELECT
    row_number() OVER () AS cellium_row_id,
    *
FROM read_csv_auto('/path/to/yellow_tripdata.csv');
```

## Instant Open Flow

Large CSV import is a two-stage flow:

1. Create a lazy/temporary DuckDB source for first paint.
2. Query the first visible window and keep the UI responsive.
3. Materialize the dataset into the `.cellium` DuckDB file on a blocking worker thread.
4. Send an app event to the main loop when materialization completes.
5. Swap the grid from the lazy source to the durable materialized table.

Tokio provides orchestration and app events. DuckDB import/materialization work must run in `spawn_blocking` with a separate DuckDB connection.

## Consequences

- New metadata persistence should be DuckDB-backed.
- `libsql` is not the long-term local workbook store.
- Polars stays out of the default pipeline unless profiling proves a concrete need.
- Lazy preview sources are not durable edit targets.
- Materialized tables can support persisted edits because they live inside the `.cellium` DuckDB file and have stable row identity.
