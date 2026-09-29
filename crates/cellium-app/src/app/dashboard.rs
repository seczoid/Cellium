use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use cellium_core::{CellRef, SheetId, TableId, TableViewState, ViewLayout, WorkbookId};
use cellium_data::{
    AsyncDataEngine, ConnectedTableRecord, DataEngine, SavedViewRecord, SchemaField, SheetRecord,
    SparseCellRecord, TableSchema, WorkbookRecord,
};
use cellium_store::{LibraryRepository, WorkbookLibraryEntry, WorkbookListFilter};
use cellium_ui::{DashboardHoverTarget, DashboardSection, dashboard_first_visible_workbook_id};

use crate::{
    ids::{unique_id, unix_seconds},
    labels::dashboard_workbook,
    paths::default_workbook_path,
};

use super::{ActiveTable, App, AppScreen, DashboardState, WorkbookRegistration, pick_data_file};

impl App {
    pub(super) fn load_dashboard_state(&self) -> Result<DashboardState> {
        self.load_dashboard_state_for(DashboardSection::Home)
    }

    pub(super) fn load_dashboard_state_for(
        &self,
        section: DashboardSection,
    ) -> Result<DashboardState> {
        let filter = match section {
            DashboardSection::Home => WorkbookListFilter::Recent,
            DashboardSection::Starred => WorkbookListFilter::Starred,
            DashboardSection::Workbooks => WorkbookListFilter::All,
        };
        let (search_query, search_focused) = self.state.as_ref().map_or_else(
            || (String::new(), false),
            |state| {
                (
                    state.dashboard.search_query.clone(),
                    state.dashboard.search_focused,
                )
            },
        );
        let workbooks = self
            .runtime
            .block_on(self.library_repository.list_workbooks(filter, 10))
            .context("failed to list workbooks")?;
        Ok(DashboardState {
            active_section: section,
            workbooks: workbooks.into_iter().map(dashboard_workbook).collect(),
            search_query,
            search_focused,
            hovered: None,
            status: "Ready".to_string(),
        })
    }

    pub(super) fn refresh_dashboard(&mut self, section: DashboardSection) {
        let next = self.load_dashboard_state_for(section);
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match next {
            Ok(dashboard) => state.dashboard = dashboard,
            Err(error) => {
                tracing::warn!(%error, "failed to refresh dashboard");
                state.dashboard.active_section = section;
                state.dashboard.status =
                    "Couldn't load recent workbooks. Try reopening Cellium.".to_string();
            }
        }
        state.request_redraw();
    }

    pub(super) fn handle_dashboard_target(&mut self, target: DashboardHoverTarget) {
        match target {
            DashboardHoverTarget::NewWorkbook | DashboardHoverTarget::CreateBlankWorkbook => {
                self.create_blank_workbook();
            }
            DashboardHoverTarget::FileUpload => {
                if let Some(path) = pick_data_file() {
                    self.open_path_in_new_window_or_report(&path);
                }
            }
            DashboardHoverTarget::Search => {
                if let Some(state) = self.state.as_mut() {
                    state.dashboard.search_focused = true;
                    state.request_redraw();
                }
            }
            DashboardHoverTarget::Sidebar(section) => self.refresh_dashboard(section),
            DashboardHoverTarget::ViewAll => self.refresh_dashboard(DashboardSection::Workbooks),
            DashboardHoverTarget::WorkbookRow(id) => self.open_dashboard_workbook(id),
        }
    }

    fn open_dashboard_workbook(&mut self, id: u64) {
        let workbook = self.state.as_ref().and_then(|state| {
            state
                .dashboard
                .workbooks
                .iter()
                .find(|workbook| workbook.id == id)
                .cloned()
        });
        let Some(workbook) = workbook else {
            if let Some(state) = self.state.as_mut() {
                state.dashboard.status = "Workbook is no longer visible in this view.".to_string();
                state.request_redraw();
            }
            return;
        };
        self.open_path_in_new_window_or_report(Path::new(&workbook.file_path));
    }

    pub(super) fn open_first_visible_dashboard_workbook(&mut self) {
        let workbook_id = self.state.as_ref().and_then(|state| {
            dashboard_first_visible_workbook_id(
                &state.dashboard.workbooks,
                state.dashboard.active_section,
                &state.dashboard.search_query,
            )
        });

        if let Some(workbook_id) = workbook_id {
            self.open_dashboard_workbook(workbook_id);
            return;
        }

        if let Some(state) = self.state.as_mut() {
            state.dashboard.status = "No matching workbooks.".to_string();
            state.request_redraw();
        }
    }

    pub(super) fn open_workbook_path(&mut self, path: PathBuf) -> Result<()> {
        self.flush_pending_cell_writes();
        let engine = DataEngine::open(&path).context("failed to open Cellium workbook")?;
        let sheet = engine
            .first_sheet()
            .context("failed to load workbook sheets")?
            .context("workbook has no sheets")?;
        let mut active_table = engine
            .connected_tables_for_sheet(sheet.id)
            .context("failed to load connected tables")?
            .into_iter()
            .find(|table| table.materialized)
            .map(|table| active_table_from_record(&path, table))
            .transpose()?;
        if let Some(table) = active_table.as_mut()
            && let Some(view) = engine
                .saved_views_for_sheet(sheet.id)
                .context("failed to load saved table views")?
                .into_iter()
                .find(|view| view.table_id == Some(table.table_id.0))
        {
            table.view = view.state;
        }
        let sparse_cells = engine
            .sparse_cells_for_sheet(sheet.id)
            .context("failed to load sparse cells")?;
        let sparse_cells = if let Some(table) = active_table.as_ref() {
            migrate_legacy_table_edits(&engine, table, sparse_cells)?
        } else {
            sparse_cells
        };
        let edited_cells = sparse_cells_to_ui_map(sparse_cells)?;
        let async_engine =
            AsyncDataEngine::open(&path).context("failed to open workbook query engine")?;
        self.data_engine = async_engine;
        self.active_workbook_path = Some(path.clone());
        let should_query = active_table.is_some();
        let should_count = active_table
            .as_ref()
            .is_some_and(|table| !table.view.filters.is_empty());
        let status = workbook_open_status(&path, active_table.as_ref());
        let Some(state) = self.state.as_mut() else {
            return Ok(());
        };
        state.screen = AppScreen::Workbook;
        state.active_table = active_table;
        state.ui.table_view = state
            .active_table
            .as_ref()
            .map_or_else(TableViewState::default, |table| table.view.clone());
        state.pending_query = None;
        state.pending_row_count = None;
        state.active_import_path = None;
        state.query_in_flight = false;
        state.needs_visible_window_query = false;
        state.ui.active_sheet = SheetId(sheet.id);
        state.ui.viewport.scroll_y_px = 0.0;
        state.ui.viewport.scroll_x_px = 0.0;
        state.ui.clear_snapshot();
        state.ui.edited_cells = edited_cells;
        state.ui.cancel_cell_edit();
        state.ui.set_status(status);
        state.pending_cell_writes.clear();
        state.renderer.window().set_ime_allowed(false);
        state.last_cell_click = None;
        state.request_redraw();

        if should_query {
            self.schedule_visible_window_query();
        }
        if should_count {
            self.start_active_view_count();
        }
        Ok(())
    }

    pub(super) fn create_blank_workbook(&mut self) {
        let now = unix_seconds();
        let id = WorkbookId(unique_id());
        let sheet_id = SheetId(1);
        let name = "Untitled Workbook".to_string();
        let path = default_workbook_path(id, &name);
        let result = (|| -> Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).context("failed to create workbook folder")?;
            }
            let engine = DataEngine::open(&path).context("failed to open workbook file")?;
            engine
                .upsert_workbook(&WorkbookRecord {
                    id: id.0,
                    name: name.clone(),
                    created_at: now,
                    updated_at: now,
                })
                .context("failed to save workbook metadata")?;
            engine
                .upsert_sheet(&SheetRecord {
                    id: sheet_id.0,
                    workbook_id: id.0,
                    name: "Sheet 1".to_string(),
                    position: 0,
                    created_at: now,
                    updated_at: now,
                })
                .context("failed to save sheet metadata")?;
            self.runtime
                .block_on(
                    self.library_repository
                        .upsert_workbook(&WorkbookLibraryEntry {
                            id,
                            name: name.clone(),
                            file_path: path.to_string_lossy().into_owned(),
                            created_at: now,
                            updated_at: now,
                            last_opened_at: now,
                            starred: false,
                        }),
                )
                .context("failed to index workbook")?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.refresh_dashboard(DashboardSection::Home);
                self.open_path_in_new_window_or_report(&path);
            }
            Err(error) => {
                tracing::warn!(%error, "failed to create blank workbook");
                if let Some(state) = self.state.as_mut() {
                    state.dashboard.status =
                        "Couldn't create a blank workbook. Check the folder and try again."
                            .to_string();
                    state.request_redraw();
                }
            }
        }
    }

    pub(super) fn register_imported_workbook(
        &self,
        table: &ActiveTable,
        schema_fields: &[SchemaField],
    ) -> Result<WorkbookRegistration> {
        let now = unix_seconds();
        let workbook_id = WorkbookId(unique_id());
        let sheet_id = SheetId(1);
        let table_id = TableId(1);
        let workbook_name = table.source_name.clone();
        let workbook_path = default_workbook_path(workbook_id, &workbook_name);
        if let Some(parent) = workbook_path.parent() {
            std::fs::create_dir_all(parent).context("failed to create workbook folder")?;
        }

        let engine =
            DataEngine::open(&workbook_path).context("failed to open imported workbook")?;
        engine
            .upsert_workbook(&WorkbookRecord {
                id: workbook_id.0,
                name: workbook_name.clone(),
                created_at: now,
                updated_at: now,
            })
            .context("failed to save imported workbook metadata")?;
        engine
            .upsert_sheet(&SheetRecord {
                id: sheet_id.0,
                workbook_id: workbook_id.0,
                name: "Sheet 1".to_string(),
                position: 0,
                created_at: now,
                updated_at: now,
            })
            .context("failed to save imported sheet metadata")?;
        engine
            .upsert_connected_table(&ConnectedTableRecord {
                id: table_id.0,
                sheet_id: sheet_id.0,
                name: table.source_name.clone(),
                physical_table_name: table.table_name.clone(),
                source_path: Some(table.source_path.to_string_lossy().into_owned()),
                source_kind: Some(table.source_kind),
                anchor_row: 1,
                anchor_col: 1,
                row_count: table.row_count,
                schema: TableSchema {
                    fields: schema_fields.to_vec(),
                },
                materialized: table.is_materialized,
                created_at: now,
                updated_at: now,
            })
            .context("failed to save imported table metadata")?;
        engine
            .upsert_saved_view(&SavedViewRecord {
                id: table_id.0,
                sheet_id: sheet_id.0,
                name: "Default".to_string(),
                table_id: Some(table_id.0),
                state: TableViewState::default(),
                layout: ViewLayout::default(),
                created_at: now,
                updated_at: now,
            })
            .context("failed to save default table view")?;

        self.runtime
            .block_on(
                self.library_repository
                    .upsert_workbook(&WorkbookLibraryEntry {
                        id: workbook_id,
                        name: workbook_name,
                        file_path: workbook_path.to_string_lossy().into_owned(),
                        created_at: now,
                        updated_at: now,
                        last_opened_at: now,
                        starred: false,
                    }),
            )
            .context("failed to index imported workbook")?;
        Ok(WorkbookRegistration {
            workbook_id,
            sheet_id,
            table_id,
            path: workbook_path,
        })
    }
}

fn active_table_from_record(
    workbook_path: &Path,
    table: ConnectedTableRecord,
) -> Result<ActiveTable> {
    let source_kind = table
        .source_kind
        .context("connected table is missing source kind")?;
    Ok(ActiveTable {
        table_name: table.physical_table_name,
        source_name: table.name,
        source_path: table
            .source_path
            .map_or_else(|| workbook_path.to_path_buf(), PathBuf::from),
        source_kind,
        source_row_count: table.row_count,
        row_count: table.row_count,
        columns: table
            .schema
            .fields
            .into_iter()
            .map(|field| field.name)
            .collect(),
        view: TableViewState::default(),
        is_materialized: table.materialized,
        workbook_path: Some(workbook_path.to_path_buf()),
        sheet_id: SheetId(table.sheet_id),
        table_id: TableId(table.id),
    })
}

fn sparse_cells_to_ui_map(cells: Vec<SparseCellRecord>) -> Result<BTreeMap<CellRef, String>> {
    cells
        .into_iter()
        .map(|cell| {
            let row = u32::try_from(cell.row_index).context("cell row index is too large")?;
            let column = u32::try_from(cell.col_index).context("cell column index is too large")?;
            Ok((CellRef::new(row, column), cell.value))
        })
        .collect()
}

fn migrate_legacy_table_edits(
    engine: &DataEngine,
    table: &ActiveTable,
    cells: Vec<SparseCellRecord>,
) -> Result<Vec<SparseCellRecord>> {
    if !table.is_materialized {
        return Ok(cells);
    }
    let header_rows = u64::from(super::display_header_row_count(table));
    let mut remaining = Vec::with_capacity(cells.len());
    for cell in cells {
        if cell.formula.is_some() {
            remaining.push(cell);
            continue;
        }
        let column_index = cell.col_index.checked_sub(1).map(|index| index as usize);
        let row_id = cell.row_index.checked_sub(header_rows).filter(|row_id| {
            *row_id > 0
                && table
                    .source_row_count
                    .is_none_or(|row_count| *row_id <= row_count)
        });
        let Some((row_id, column)) =
            row_id.zip(column_index.and_then(|index| table.columns.get(index)))
        else {
            remaining.push(cell);
            continue;
        };
        match engine.update_table_cell(
            &table.table_name,
            &table.columns,
            row_id,
            column,
            &cell.value,
        ) {
            Ok(()) => {}
            Err(cellium_data::DataError::RowNotFound { .. }) => {
                remaining.push(cell);
                continue;
            }
            Err(error) => return Err(error).context("failed to migrate a legacy table edit"),
        }
        engine
            .upsert_sparse_cell(&SparseCellRecord {
                value: String::new(),
                formula: None,
                ..cell
            })
            .context("failed to remove a migrated table edit")?;
    }
    Ok(remaining)
}

fn workbook_open_status(path: &Path, table: Option<&ActiveTable>) -> String {
    table.map_or_else(
        || format!("Opened blank workbook {}", path.display()),
        |table| {
            table.row_count.map_or_else(
                || format!("Opened {}", table.source_name),
                |row_count| format!("Opened {} rows from {}", row_count, table.source_name),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cellium_data::DataFileKind;

    #[test]
    fn active_table_from_record_restores_materialized_table_fields() {
        let table = active_table_from_record(
            Path::new("/tmp/book.cellium"),
            ConnectedTableRecord {
                id: 9,
                sheet_id: 3,
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
            },
        )
        .unwrap();

        assert_eq!(
            (
                table.table_name,
                table.sheet_id.0,
                table.table_id.0,
                table.columns,
                table.is_materialized,
            ),
            (
                "import_orders".to_string(),
                3,
                9,
                vec!["id".to_string()],
                true,
            )
        );
    }

    #[test]
    fn sparse_cells_to_ui_map_restores_cell_references() {
        let cells = sparse_cells_to_ui_map(vec![SparseCellRecord {
            sheet_id: 1,
            row_index: 2,
            col_index: 3,
            value: "edited".to_string(),
            formula: None,
            updated_at: 10,
        }])
        .unwrap();

        assert_eq!(
            cells.get(&CellRef::new(2, 3)).map(String::as_str),
            Some("edited")
        );
    }
}
