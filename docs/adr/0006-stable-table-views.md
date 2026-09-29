# ADR 0006: Stable Table Rows And Typed Views

## Decision

Materialized imported rows are identified by `cellium_row_id`. Displayed spreadsheet row numbers are viewport coordinates and are never durable identities for imported records.

Imported-table edits target:

```text
table + cellium_row_id + column
```

Blank-sheet cells continue to use sparse spreadsheet coordinates.

Table filters and sorts are stored as typed `FilterExpr` and `SortSpec` values. Only `cellium-data` may compile those values into quoted DuckDB SQL. UI and workbook code do not construct filter or sort SQL strings.

Each imported table has an autosaved `Default` view. The view is persisted in the `.cellium` workbook and restored when the workbook reopens.

## Query Behavior

- Default materialized views keep the fast `cellium_row_id` range-window query path.
- Sorted or filtered views use stable `ORDER BY` clauses with `cellium_row_id` as the final tie-breaker.
- Visible query results return row IDs separately from user-visible cell values.
- Filtered row counts are exact and run outside the window event loop.

## Editing Behavior

- Lazy external previews remain read-only until materialization completes.
- Materialized table edits update the workbook-owned table by stable row ID.
- Existing coordinate-based imported-table edits are migrated on workbook open where they can be mapped safely.
- An edit that affects the active sort or filter invalidates the visible snapshot and recounts the view.

## Consequences

- Sorting and filtering cannot move an edit onto a different record.
- Saved views are portable domain data instead of executable SQL fragments.
- Deep sorted offsets may require keyset pagination or a row-order cache in a later performance pass.
