# ADR 0003: Connected Table Editing Model

## Decision

Imported connected table bodies are read-only while they are lazy external views. Once an imported dataset is materialized into the `.cellium` DuckDB file with a stable `cellium_row_id`, Cellium may persist edits against that durable table model.

Blank/manual spreadsheet sheets remain sparse. Editable spreadsheet cells, formulas, computed columns, saved views, and layout metadata live in DuckDB-owned workbook tables, not in dense `A VARCHAR, B VARCHAR, ...` sheet tables.

## Rationale

This keeps the Row Zero-style fast connected-table mental model while making the durable `.cellium` file feel like a real spreadsheet workbook. Lazy external files are not durable edit targets; materialized DuckDB tables are.

## Consequences

- Undo and redo apply to workbook mutations and persisted table edits that Cellium owns.
- Computed columns are workbook-owned metadata evaluated against connected table rows.
- Sparse cell storage is the default for manual sheets:
  `cellium_cells(sheet_id, row_index, col_index, value, formula)`.
- Imported tables should get a stable `cellium_row_id` during materialization.
- "Explode every imported table cell into sparse cells" is deferred.
