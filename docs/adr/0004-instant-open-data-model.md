# ADR 0004: Instant Open Data Model

## Decision

Cellium treats "instant open" as a first-paint guarantee, not a promise that every byte of every file has already been indexed.

Parquet and Arrow can be genuinely close to instant because schema, row groups, and offsets are available without scanning the whole file. CSV cannot provide the same guarantee because it has no footer, row count, schema, or row offsets.

## V1 Behavior

- Parquet and Arrow should render the shell and first visible window from bounded metadata/data reads.
- CSV should render the shell and first visible window quickly, then continue background work.
- The UI thread must never block on CSV parsing, DuckDB scans, cache warming, or row-count discovery.
- If the user scrolls beyond data currently available to the grid snapshot, the renderer keeps the shell responsive and fills cells on a later frame.

## CSV Strategy

V1 uses a lazy DuckDB view for first paint, then materializes the source into the `.cellium` DuckDB file in the background. The app switches to the materialized table only after the background job finishes.

The background materialization must run off the UI thread. Tokio is used for orchestration, but DuckDB work is blocking CPU/file work, so the job should run with `tokio::task::spawn_blocking` and use its own DuckDB connection:

```rust
tokio::task::spawn_blocking(move || {
    let conn = duckdb::Connection::open(&cellium_path)?;
    conn.execute_batch("
        CREATE TABLE trip_data AS
        SELECT
            row_number() OVER () AS cellium_row_id,
            *
        FROM read_csv_auto('/path/to/file.csv');
    ")?;
    Ok::<_, duckdb::Error>(())
});
```

The winit event loop receives an app event when materialization finishes and swaps the grid from the lazy view to the durable table. If the workbook is closed or the import is superseded, the app may safely ignore the completion event.

Future CSV random-access work should add a row-offset index:

- scan bytes in a background task;
- record row-group or newline offsets;
- keep the scrollbar approximate until row count is known;
- answer visible-window requests from bounded seek-and-parse chunks where possible;
- persist the index or a converted Parquet/DuckDB representation for subsequent opens.

## Consequences

- "Instant" means the grid appears and remains interactive quickly.
- CSV far-scroll is allowed to show an honest loading gap while indexing or cache warming catches up.
- The data engine must keep visible-window work bounded and asynchronous.
- Full CSV parsing before first paint is not allowed.
- Lazy external views are not durable edit targets. Edits during lazy preview must either be disabled or stored as sparse pending overrides until materialization completes.
