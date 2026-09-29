use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cellium_core::{
    FilterExpr, FilterOperator, SortDirection, SortSpec, TableViewState, ViewLayout,
};
use cellium_data::{
    AsyncDataEngine, ConnectedTableRecord, DataFileKind, ImportRequest, SavedViewRecord,
    VisibleQuery,
};
use cellium_ui::{GridSnapshot, VisibleWindow};
use tracing::{error, info, warn};

use crate::{
    ids::unix_seconds,
    labels::{display_name, format_error_chain, projected_columns, table_name_for_path},
};

use super::{
    ActiveTable, App, AppScreen, ImportResult, MaterializeResult, QueryRequest, QueryWindowPolicy,
    RowCountResult, active_table_extents, display_header_row_count, displayed_cell_text,
    snapshot_has_prerender_margin, snapshot_should_replace_current, stop_scroll_inertia,
    take_matching_pending_row_count, update_snapshot_row_count,
};

impl App {
    pub(super) fn prepare_for_import(&mut self, path: &Path) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.screen = AppScreen::Workbook;
        state.active_table = None;
        state.ui.table_view = TableViewState::default();
        state.hovered_column_header = None;
        state.ui.chrome.hovered = None;
        state.pending_query = None;
        state.pending_row_count = None;
        state.active_import_path = Some(path.to_path_buf());
        state.latest_requested_generation = state.next_query_generation;
        state.next_query_generation = state.next_query_generation.saturating_add(1);
        state.ui.clear_snapshot();
        state.ui.edited_cells.clear();
        state.ui.cancel_cell_edit();
        state.pending_cell_writes.clear();
        state.renderer.window().set_ime_allowed(false);
        state.last_cell_click = None;
        state
            .ui
            .set_status(format!("Importing {}...", display_name(path)));
        state.request_redraw();
    }

    pub(super) fn handle_import_result(&mut self, result: Result<ImportResult>) {
        match result {
            Ok(result) => {
                info!(
                    table = %result.summary.table_name,
                    source = %result.source_name,
                    "file import completed"
                );
                let schema_fields = result.summary.schema.fields.clone();
                let mut active_table = ActiveTable {
                    table_name: result.summary.table_name.clone(),
                    source_name: result.source_name,
                    source_path: result.summary.source_path,
                    source_kind: result.summary.source_kind,
                    source_row_count: result.summary.row_count,
                    row_count: result.summary.row_count,
                    columns: result
                        .summary
                        .schema
                        .fields
                        .into_iter()
                        .map(|field| field.name)
                        .collect(),
                    view: TableViewState::default(),
                    is_materialized: result.summary.row_count.is_some(),
                    workbook_path: None,
                    sheet_id: cellium_core::SheetId(1),
                    table_id: cellium_core::TableId(1),
                };
                let should_materialize = matches!(
                    active_table.source_kind,
                    DataFileKind::Csv | DataFileKind::Parquet
                ) && !active_table.is_materialized;
                if let Some(state) = self.state.as_mut() {
                    if let Some(row_count) =
                        take_matching_pending_row_count(&mut state.pending_row_count, &active_table)
                    {
                        active_table.row_count = Some(row_count);
                    }
                    let status = active_table.row_count.map_or_else(
                        || format!("Loaded {}", active_table.source_name),
                        |row_count| {
                            format!(
                                "Loaded {} rows from {}",
                                row_count, active_table.source_name
                            )
                        },
                    );
                    state.active_table = Some(active_table.clone());
                    state.ui.table_view = active_table.view.clone();
                    stop_scroll_inertia(state);
                    state.ui.viewport.scroll_y_px = 0.0;
                    state.ui.viewport.scroll_x_px = 0.0;
                    state.ui.set_status(status);
                }
                match self.register_imported_workbook(&active_table, &schema_fields) {
                    Ok(registration) => {
                        self.active_workbook_path = Some(registration.path.clone());
                        active_table.workbook_path = Some(registration.path);
                        active_table.sheet_id = registration.sheet_id;
                        active_table.table_id = registration.table_id;
                        info!(
                            workbook_id = registration.workbook_id.0,
                            "registered DuckDB-backed workbook"
                        );
                    }
                    Err(error) => {
                        warn!(%error, "failed to register imported workbook in dashboard library");
                    }
                }
                if let Some(state) = self.state.as_mut() {
                    state.active_table = Some(active_table.clone());
                }
                self.schedule_visible_window_query();
                if should_materialize && active_table.workbook_path.is_some() {
                    self.start_materialize(active_table);
                }
            }
            Err(error) => {
                let error = format_error_chain(&error);
                error!(%error, "file import failed");
                if let Some(state) = self.state.as_mut() {
                    state.ui.set_status(format!("Import failed: {error}"));
                    state.request_redraw();
                }
            }
        }
    }

    pub(super) fn handle_row_count_result(&mut self, result: Result<RowCountResult>) {
        match result {
            Ok(result) => {
                info!(
                    table = %result.logical_table_name,
                    source = %result.source_path.display(),
                    row_count = result.row_count,
                    "exact row count completed"
                );
                let Some(state) = self.state.as_mut() else {
                    return;
                };
                if state
                    .active_import_path
                    .as_ref()
                    .is_none_or(|path| *path != result.source_path)
                {
                    return;
                }
                let Some(active_table) = state.active_table.as_mut() else {
                    state.pending_row_count = Some(result);
                    return;
                };
                if active_table.table_name != result.logical_table_name
                    || active_table.source_path != result.source_path
                {
                    return;
                }
                active_table.source_row_count = Some(result.row_count);
                if active_table.view.filters.is_empty() {
                    active_table.row_count = Some(result.row_count);
                }
                let (row_count, column_count) = active_table_extents(Some(active_table));
                state.ui.clamp_to_table(row_count, column_count);
                update_snapshot_row_count(&mut state.ui, active_table);
                state.ui.set_status(format!(
                    "Loaded {} rows from {}",
                    result.row_count, active_table.source_name
                ));
                state.request_redraw();
            }
            Err(error) => {
                let error = format_error_chain(&error);
                error!(%error, "exact row count failed");
            }
        }
    }

    pub(super) fn handle_materialize_result(&mut self, result: Result<MaterializeResult>) {
        match result {
            Ok(result) => {
                let import = result.import;
                info!(
                    table = %import.summary.table_name,
                    source = %import.source_name,
                    database = %result.database_path.display(),
                    "file materialization completed"
                );
                let refreshed_engine = match AsyncDataEngine::open(&result.database_path)
                    .context("failed to refresh DuckDB session after materialization")
                {
                    Ok(engine) => engine,
                    Err(error) => {
                        let error = format_error_chain(&error);
                        error!(%error, "failed to switch to materialized data");
                        return;
                    }
                };
                let Some(state) = self.state.as_mut() else {
                    return;
                };
                let Some(active_table) = state.active_table.as_mut() else {
                    return;
                };
                if active_table.table_name != import.logical_table_name {
                    return;
                }
                if active_table.source_path != import.summary.source_path {
                    return;
                }
                active_table.table_name = import.summary.table_name;
                active_table.source_row_count = import.summary.row_count;
                if active_table.view.filters.is_empty() {
                    active_table.row_count = import.summary.row_count;
                }
                active_table.columns = import
                    .summary
                    .schema
                    .fields
                    .into_iter()
                    .map(|field| field.name)
                    .collect();
                active_table.is_materialized = true;
                let status = active_table.row_count.map_or_else(
                    || format!("Loaded {}", active_table.source_name),
                    |row_count| {
                        format!(
                            "Loaded {} rows from {}",
                            row_count, active_table.source_name
                        )
                    },
                );
                state.ui.set_status(status);
                update_snapshot_row_count(&mut state.ui, active_table);
                state.ui.clear_snapshot();
                state.request_redraw();
                self.data_engine = refreshed_engine;
                self.persist_active_table_view();
                self.schedule_visible_window_query();
                if self
                    .state
                    .as_ref()
                    .and_then(|state| state.active_table.as_ref())
                    .is_some_and(|table| !table.view.filters.is_empty())
                {
                    self.start_active_view_count();
                }
            }
            Err(error) => {
                let error = format_error_chain(&error);
                error!(%error, "file materialization failed");
            }
        }
    }

    pub(super) fn handle_query_result(&mut self, generation: u64, result: Result<GridSnapshot>) {
        let mut next_request = None;
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.query_in_flight = false;
        match result {
            Ok(snapshot) => {
                let current_window = state.ui.viewport.visible_window().ok();
                if snapshot_should_replace_current(
                    &snapshot,
                    current_window.as_ref(),
                    generation,
                    state.latest_requested_generation,
                ) {
                    state.ui.set_snapshot(snapshot);
                }
            }
            Err(error) => {
                let error = format_error_chain(&error);
                error!(%error, "visible window query failed");
                if generation == state.latest_requested_generation {
                    state.ui.set_status(format!("Query failed: {error}"));
                }
            }
        }
        if let Some(request) = state.pending_query.take() {
            state.query_in_flight = true;
            next_request = Some(request);
        }
        state.request_redraw();
        if let Some(request) = next_request {
            self.spawn_visible_window_query(request);
        }
    }

    pub(super) fn schedule_visible_window_query(&mut self) {
        self.schedule_visible_window_query_with_policy(QueryWindowPolicy::Prefetch);
    }

    pub(super) fn schedule_visible_window_query_visible_only(&mut self) {
        self.schedule_visible_window_query_with_policy(QueryWindowPolicy::VisibleOnly);
    }

    fn schedule_visible_window_query_with_policy(&mut self, policy: QueryWindowPolicy) {
        let mut request_to_spawn = None;
        {
            let Some(state) = self.state.as_mut() else {
                return;
            };
            let Some(table) = state.active_table.clone() else {
                state.request_redraw();
                return;
            };
            let Ok(visible_window) = state.ui.viewport.visible_window() else {
                return;
            };
            state.request_redraw();
            if state.ui.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot_satisfies_policy(snapshot, &visible_window, policy)
            }) {
                return;
            }
            let request = QueryRequest {
                table,
                visible_window,
                policy,
                generation: state.next_query_generation,
            };
            state.next_query_generation = state.next_query_generation.saturating_add(1);
            state.latest_requested_generation = request.generation;
            if state.query_in_flight {
                state.pending_query = Some(request);
            } else {
                state.query_in_flight = true;
                request_to_spawn = Some(request);
            }
        }
        if let Some(request) = request_to_spawn {
            self.spawn_visible_window_query(request);
        }
    }

    pub(super) fn cycle_active_column_sort(&mut self) {
        let next_view = {
            let Some(state) = self.state.as_ref() else {
                return;
            };
            let Some(table) = state.active_table.as_ref() else {
                return;
            };
            let column_index = state.ui.selection.active.column.saturating_sub(1) as usize;
            let Some(column) = table.columns.get(column_index) else {
                return;
            };
            cycle_sort_view(&table.view, column, state.modifiers.shift_key())
        };
        self.apply_table_view(next_view);
    }

    pub(super) fn filter_to_active_cell(&mut self) {
        let next_view = {
            let Some(state) = self.state.as_ref() else {
                return;
            };
            let Some(table) = state.active_table.as_ref() else {
                return;
            };
            let cell = state.ui.selection.active.clone();
            if cell.row <= display_header_row_count(table) {
                return;
            }
            let column_index = cell.column.saturating_sub(1) as usize;
            let Some(column) = table.columns.get(column_index) else {
                return;
            };
            let Some(snapshot) = state.ui.snapshot.as_ref() else {
                return;
            };
            let display_row = u64::from(cell.row.saturating_sub(1));
            if display_row < snapshot.start_row
                || cell.column.saturating_sub(1) < snapshot.start_column
            {
                return;
            }
            let snapshot_row = display_row.saturating_sub(snapshot.start_row) as usize;
            let snapshot_column = cell
                .column
                .saturating_sub(1)
                .saturating_sub(snapshot.start_column) as usize;
            let Some(row) = snapshot.rows.get(snapshot_row) else {
                return;
            };
            if row.get(snapshot_column).is_none() {
                return;
            }
            let value = displayed_cell_text(state, &cell);
            filter_to_value_view(&table.view, column, &value)
        };
        self.apply_table_view(next_view);
    }

    pub(super) fn clear_active_table_view(&mut self) {
        self.apply_table_view(TableViewState::default());
    }

    fn apply_table_view(&mut self, view: TableViewState) {
        let should_count = !view.filters.is_empty();
        {
            let Some(state) = self.state.as_mut() else {
                return;
            };
            let Some(table) = state.active_table.as_mut() else {
                return;
            };
            if table.view == view {
                return;
            }
            table.view = view.clone();
            if !should_count {
                table.row_count = table.source_row_count;
            }
            state.ui.table_view = view;
            state.ui.viewport.scroll_y_px = 0.0;
            state.ui.clear_snapshot();
            state.pending_query = None;
            state.latest_requested_generation = state.next_query_generation;
            state.next_query_generation = state.next_query_generation.saturating_add(1);
            state.view_generation = state.view_generation.saturating_add(1);
            let (row_count, column_count) = active_table_extents(state.active_table.as_ref());
            state.ui.clamp_to_table(row_count, column_count);
            state.ui.set_status(if should_count {
                "Applying filter..."
            } else if state.ui.table_view.sorts.is_empty() {
                "Default view"
            } else {
                "Sorted view"
            });
            state.request_redraw();
        }
        self.persist_active_table_view();
        self.schedule_visible_window_query();
        if should_count {
            self.start_active_view_count();
        }
    }

    pub(super) fn persist_active_table_view(&self) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Some(table) = state.active_table.as_ref() else {
            return;
        };
        if !table.is_materialized {
            return;
        }
        let now = unix_seconds();
        let view = SavedViewRecord {
            id: table.table_id.0,
            sheet_id: table.sheet_id.0,
            name: "Default".to_string(),
            table_id: Some(table.table_id.0),
            state: table.view.clone(),
            layout: ViewLayout::default(),
            created_at: now,
            updated_at: now,
        };
        let engine = self.data_engine.clone();
        self.runtime.spawn(async move {
            if let Err(error) = engine.upsert_saved_view(view).await {
                warn!(%error, "failed to persist active table view");
            }
        });
    }

    pub(super) fn start_active_view_count(&self) {
        let Some(state) = self.state.as_ref() else {
            return;
        };
        let Some(table) = state.active_table.as_ref() else {
            return;
        };
        let generation = state.view_generation;
        let table_name = table.table_name.clone();
        let columns = table.columns.clone();
        let view = table.view.clone();
        let engine = if table.is_materialized {
            table
                .workbook_path
                .as_ref()
                .and_then(|path| AsyncDataEngine::open(path).ok())
                .unwrap_or_else(|| self.data_engine.clone())
        } else {
            self.data_engine.clone()
        };
        let proxy = self.proxy.clone();
        self.runtime.spawn(async move {
            let result = engine
                .view_row_count(table_name.clone(), columns, view.clone())
                .await
                .map_err(anyhow::Error::from);
            if proxy
                .send_event(super::AppEvent::ViewCounted {
                    generation,
                    table_name,
                    view,
                    result,
                })
                .is_err()
            {
                warn!("failed to send filtered row count to event loop");
            }
        });
    }

    pub(super) fn handle_view_count_result(
        &mut self,
        generation: u64,
        table_name: &str,
        view: &TableViewState,
        result: Result<u64>,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let Some(table) = state.active_table.as_mut() else {
            return;
        };
        if generation != state.view_generation
            || table.table_name != table_name
            || table.view != *view
        {
            return;
        }
        match result {
            Ok(filtered_count) => {
                table.row_count = Some(filtered_count);
                let (display_row_count, column_count) = active_table_extents(Some(table));
                state.ui.clamp_to_table(display_row_count, column_count);
                update_snapshot_row_count(&mut state.ui, table);
                state
                    .ui
                    .set_status(format!("Filtered to {filtered_count} rows"));
            }
            Err(error) => {
                error!(%error, "filtered row count failed");
                state.ui.set_status("Filter applied; row count unavailable");
            }
        }
        state.request_redraw();
    }
}

pub(super) fn cycle_sort_view(
    view: &TableViewState,
    column: &str,
    additive: bool,
) -> TableViewState {
    let mut next = view.clone();
    let current = next
        .sorts
        .iter()
        .find(|sort| sort.column == column)
        .map(|sort| sort.direction);
    if !additive {
        next.sorts.clear();
    } else {
        next.sorts.retain(|sort| sort.column != column);
    }
    let direction = match current {
        None => Some(SortDirection::Ascending),
        Some(SortDirection::Ascending) => Some(SortDirection::Descending),
        Some(SortDirection::Descending) => None,
    };
    if let Some(direction) = direction {
        next.sorts.push(SortSpec {
            column: column.to_string(),
            direction,
        });
    }
    next
}

pub(super) fn filter_to_value_view(
    view: &TableViewState,
    column: &str,
    value: &str,
) -> TableViewState {
    let filter = FilterExpr {
        column: column.to_string(),
        operator: if value.is_empty() {
            FilterOperator::IsEmpty
        } else {
            FilterOperator::Equals
        },
        value: (!value.is_empty()).then(|| value.to_string()),
    };
    let mut next = view.clone();
    let already_active = next.filters.iter().any(|existing| existing == &filter);
    next.filters.retain(|existing| existing.column != column);
    if !already_active {
        next.filters.push(filter);
    }
    next
}

pub(super) async fn import_file(engine: AsyncDataEngine, path: &Path) -> Result<ImportResult> {
    let source_name = display_name(path);
    let summary = engine
        .import_file(ImportRequest {
            table_name: table_name_for_path(path),
            path: path.to_path_buf(),
        })
        .await
        .with_context(|| format!("failed to import {}", path.display()))?;
    Ok(ImportResult {
        summary,
        source_name,
        logical_table_name: table_name_for_path(path),
    })
}

pub(super) async fn query_visible_window(
    engine: AsyncDataEngine,
    table: ActiveTable,
    window: VisibleWindow,
    policy: QueryWindowPolicy,
) -> Result<GridSnapshot> {
    let row_overscan = policy.row_overscan(&table);
    let column_overscan = policy.column_overscan();
    let header_row_count = display_header_row_count(&table);
    let header_rows = u64::from(header_row_count);
    let display_query_start_row = window.start_row.saturating_sub(u64::from(row_overscan));
    let query_start_row = display_query_start_row.saturating_sub(header_rows);
    let query_limit = window
        .row_count
        .saturating_add(row_overscan)
        .saturating_add(row_overscan);
    let query_start_column = window.start_column.saturating_sub(column_overscan);
    let query_column_count = window
        .column_count
        .saturating_add(column_overscan)
        .saturating_add(column_overscan);
    let projection = projected_columns(&table.columns, query_start_column, query_column_count);
    let projected_width = projection.len();
    let query_start_id = query_start_row.saturating_add(1);
    let query_end_id = query_start_row.saturating_add(u64::from(query_limit));
    let row_id_range = (table.is_materialized && table.view.is_default())
        .then_some((query_start_id, query_end_id));
    let arrow_window = engine
        .visible_window(VisibleQuery {
            table_name: table.table_name.clone(),
            offset: if row_id_range.is_some() {
                0
            } else {
                query_start_row
            },
            limit: query_limit,
            projection,
            available_columns: table.columns.clone(),
            view: table.view.clone(),
            include_row_id: table.is_materialized,
            row_id_range,
        })
        .await
        .with_context(|| format!("failed to query {}", table.table_name))?;
    let (rows, row_ids) = display_rows_with_ids(
        &table.columns,
        query_start_column,
        projected_width,
        header_row_count,
        display_query_start_row,
        arrow_window.rows_as_strings(),
        arrow_window.row_ids,
    );

    Ok(GridSnapshot {
        table_name: table.table_name,
        source_name: table.source_name,
        row_count: table.row_count,
        start_row: display_query_start_row,
        start_column: query_start_column,
        header_row_count,
        columns: table.columns,
        rows,
        row_ids,
    })
}

#[cfg(test)]
pub(super) fn display_rows(
    columns: &[String],
    start_column: u32,
    projected_width: usize,
    header_row_count: u32,
    display_start_row: u64,
    data_rows: Vec<Vec<String>>,
) -> Vec<Vec<String>> {
    let row_ids = vec![None; data_rows.len()];
    display_rows_with_ids(
        columns,
        start_column,
        projected_width,
        header_row_count,
        display_start_row,
        data_rows,
        row_ids,
    )
    .0
}

fn display_rows_with_ids(
    columns: &[String],
    start_column: u32,
    projected_width: usize,
    header_row_count: u32,
    display_start_row: u64,
    mut data_rows: Vec<Vec<String>>,
    mut row_ids: Vec<Option<u64>>,
) -> (Vec<Vec<String>>, Vec<Option<u64>>) {
    if header_row_count == 0 || display_start_row != 0 {
        return (data_rows, row_ids);
    }
    let header_row = columns
        .iter()
        .skip(start_column as usize)
        .take(data_rows.first().map_or(projected_width, Vec::len))
        .cloned()
        .collect::<Vec<_>>();
    data_rows.insert(0, header_row);
    row_ids.insert(0, None);
    (data_rows, row_ids)
}

pub(super) fn snapshot_satisfies_policy(
    snapshot: &GridSnapshot,
    window: &VisibleWindow,
    policy: QueryWindowPolicy,
) -> bool {
    match policy {
        QueryWindowPolicy::Prefetch => snapshot_has_prerender_margin(snapshot, window),
        QueryWindowPolicy::VisibleOnly => super::snapshot_covers_window(snapshot, window),
    }
}

pub(super) async fn materialize_file(
    engine: AsyncDataEngine,
    table: ActiveTable,
    database_path: PathBuf,
) -> Result<MaterializeResult> {
    let logical_table_name = table.table_name.clone();
    let summary = engine
        .materialize_source(ImportRequest {
            table_name: table.table_name.clone(),
            path: table.source_path,
        })
        .await
        .with_context(|| format!("failed to materialize {}", table.source_name))?;
    let now = unix_seconds();
    engine
        .upsert_connected_table(ConnectedTableRecord {
            id: table.table_id.0,
            sheet_id: table.sheet_id.0,
            name: table.source_name.clone(),
            physical_table_name: summary.table_name.clone(),
            source_path: Some(summary.source_path.to_string_lossy().into_owned()),
            source_kind: Some(summary.source_kind),
            anchor_row: 1,
            anchor_col: 1,
            row_count: summary.row_count,
            schema: summary.schema.clone(),
            materialized: true,
            created_at: now,
            updated_at: now,
        })
        .await
        .context("failed to update workbook table metadata")?;
    Ok(MaterializeResult {
        import: ImportResult {
            summary,
            source_name: table.source_name,
            logical_table_name,
        },
        database_path,
    })
}
