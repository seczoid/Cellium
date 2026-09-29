# ADR 0001: Workspace Boundaries

## Decision

Cellium is a Cargo workspace from the first serious architecture pass. Each crate owns one architectural boundary: app startup, domain model, data engine, formula engine, UI state, renderer, persistence, or search.

## Rationale

The app has several heavy dependency zones. Keeping `wgpu`, DuckDB, formula/domain code, UI state, and rendering code in separate crates prevents accidental coupling and keeps tests focused.

## Rules

- `cellium-core` contains no UI, renderer, storage, data-engine, or async-runtime dependencies.
- `cellium-render` owns `wgpu` and `cosmic-text`.
- `cellium-data` owns DuckDB, Arrow, Parquet, and file sniffing.
- `cellium-store` owns workbook persistence repository traits and implementations.
- New persistence work should target the DuckDB-backed `.cellium` file model from ADR 0005. The current `libsql` backend is transitional legacy, not the product direction.
- `cellium-app` is the only crate that may use `anyhow` for top-level startup errors.
