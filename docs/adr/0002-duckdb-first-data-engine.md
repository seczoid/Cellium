# ADR 0002: DuckDB-First Data Engine

## Decision

DuckDB owns Cellium's V1 import, query, and durable workbook storage path for CSV, Parquet, Arrow-backed datasets, workbook metadata, sparse spreadsheet cells, computed columns, and saved views. Arrow is the boundary between query results and the grid.

## Rationale

DuckDB gives Cellium one local analytical database for parsing large files, storing imported/materialized tables, running SQL, filtering, sorting, projection, and saving workbook state. Arrow batches are a stable data interchange format for visible grid windows.

## Consequences

- UI and workbook crates never call DuckDB directly.
- Visible grid windows are requested through `cellium-data` APIs and returned as Arrow-shaped batches.
- A `.cellium` file is a DuckDB database file.
- No new Polars or libsql dependency paths should be added unless profiling or sync requirements prove they are needed.
