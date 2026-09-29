use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use arboard::Clipboard;
use cellium_core::{CellRef, SheetId, TableId, TableViewState, WorkbookId};
use cellium_data::{AsyncDataEngine, DataFileKind, ImportSummary, SparseCellRecord};
use cellium_render::{DrawPrimitive, RenderError, Renderer};
use cellium_store::LocalWorkbookRepository;
use cellium_ui::{
    ChromeHoverTarget, ColumnResizeAnimation, DashboardFrame, DashboardHoverTarget,
    DashboardSection, DashboardWorkbook, EditorCaretAnimation, EditorIntent, FORMULA_BAR_HEIGHT,
    FORMULA_BAR_TOP, GRID_ZOOM_STEP, GridSnapshot, RowResizeAnimation, ScrollAxis, ScrollbarLayout,
    SelectionAction, SelectionAnimation, SelectionRange, UiState, VisibleWindow,
    dashboard_hit_test,
};
use tokio::runtime::Runtime;
use tracing::{error, warn};
use tracing_subscriber::EnvFilter;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    keyboard::{Key, ModifiersState, NamedKey},
    window::{CursorIcon, WindowAttributes, WindowId},
};

use crate::{
    constants::*,
    engine::open_data_engine,
    labels::{display_name, table_name_for_path},
    paths::library_index_path,
};

pub fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let initial_path = std::env::args_os().nth(1).map(PathBuf::from);
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let runtime = Runtime::new().context("failed to create tokio runtime")?;
    let mut app = App::new(proxy, runtime, initial_path)?;
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[derive(Debug)]
enum AppEvent {
    Imported(Result<ImportResult>),
    RowCounted(Result<RowCountResult>),
    Materialized(Result<MaterializeResult>),
    CellWritesFinished(Result<()>),
    ViewCounted {
        generation: u64,
        table_name: String,
        view: TableViewState,
        result: Result<u64>,
    },
    Queried {
        generation: u64,
        result: Result<GridSnapshot>,
    },
}

#[derive(Debug)]
struct ImportResult {
    summary: ImportSummary,
    source_name: String,
    logical_table_name: String,
}

#[derive(Debug, Clone)]
struct RowCountResult {
    logical_table_name: String,
    source_path: PathBuf,
    row_count: u64,
}

#[derive(Debug)]
struct MaterializeResult {
    import: ImportResult,
    database_path: PathBuf,
}

#[derive(Debug, Clone)]
struct WorkbookRegistration {
    workbook_id: WorkbookId,
    sheet_id: SheetId,
    table_id: TableId,
    path: PathBuf,
}

struct App {
    state: Option<AppState>,
    proxy: EventLoopProxy<AppEvent>,
    runtime: Runtime,
    data_engine: AsyncDataEngine,
    library_repository: LocalWorkbookRepository,
    initial_path: Option<PathBuf>,
    active_workbook_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppScreen {
    Dashboard,
    Workbook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DashboardKeyAction {
    Ignored,
    Redraw,
    OpenFirstVisibleWorkbook,
}

#[derive(Debug, Clone)]
struct DashboardState {
    active_section: DashboardSection,
    workbooks: Vec<DashboardWorkbook>,
    search_query: String,
    search_focused: bool,
    hovered: Option<DashboardHoverTarget>,
    status: String,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            active_section: DashboardSection::Home,
            workbooks: Vec::new(),
            search_query: String::new(),
            search_focused: false,
            hovered: None,
            status: "Ready".to_string(),
        }
    }
}

struct AppState {
    renderer: Renderer,
    ui: UiState,
    screen: AppScreen,
    dashboard: DashboardState,
    active_table: Option<ActiveTable>,
    cursor_position: Option<PhysicalPosition<f64>>,
    hovered_column_header: Option<u32>,
    scrollbar_drag: Option<ScrollbarDrag>,
    canvas_pan_drag: Option<CanvasPanDrag>,
    column_resize_drag: Option<ColumnResizeDrag>,
    column_resize_animation: Option<ColumnResizeAnimationState>,
    row_resize_drag: Option<RowResizeDrag>,
    row_resize_animation: Option<RowResizeAnimationState>,
    selection_drag: Option<SelectionDrag>,
    selection_animation: Option<SelectionAnimationState>,
    modifiers: ModifiersState,
    query_in_flight: bool,
    pending_query: Option<QueryRequest>,
    pending_row_count: Option<RowCountResult>,
    active_import_path: Option<PathBuf>,
    needs_visible_window_query: bool,
    needs_redraw: bool,
    next_query_generation: u64,
    latest_requested_generation: u64,
    view_generation: u64,
    scroll_velocity_x_px_s: f64,
    scroll_velocity_y_px_s: f64,
    last_frame_time: Instant,
    last_cell_click: Option<CellClick>,
    last_text_commit: Option<TextCommit>,
    clipboard: Option<Clipboard>,
    editor_text_drag: Option<EditorTextDrag>,
    editor_caret_animation: Option<EditorCaretAnimationState>,
    editor_caret_blink_started: Instant,
    pending_cell_writes: Vec<CellEditWrite>,
}

#[derive(Debug, Clone)]
enum CellEditWrite {
    Sparse {
        sheet_id: SheetId,
        cell: CellRef,
        value: String,
        updated_at: i64,
    },
    Table {
        table_name: String,
        available_columns: Vec<String>,
        row_id: u64,
        column: String,
        value: String,
    },
}

#[derive(Debug, Clone, Copy)]
struct ScrollbarDrag {
    axis: ScrollAxis,
    pointer_offset_px: f64,
}

#[derive(Debug, Clone, Copy)]
enum ScrollbarHit {
    Thumb(ScrollbarDrag),
    Track {
        axis: ScrollAxis,
        local_position_px: f64,
    },
}

#[derive(Debug, Clone, Copy)]
struct CanvasPanDrag {
    last_position: PhysicalPosition<f64>,
    last_at: Instant,
    velocity_x_px_s: f64,
    velocity_y_px_s: f64,
}

#[derive(Debug, Clone, Copy)]
struct ColumnResizeDrag {
    column: u32,
    start_x: f64,
    start_width_px: f64,
}

#[derive(Debug, Clone, Copy)]
struct ColumnResizeAnimationState {
    column: u32,
    visual_width_px: f64,
    target_width_px: f64,
    separator_x_px: f64,
    show_separator: bool,
    last_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct RowResizeDrag {
    row: u64,
    start_y: f64,
    start_height_px: f64,
}

#[derive(Debug, Clone, Copy)]
struct RowResizeAnimationState {
    row: u64,
    visual_height_px: f64,
    target_height_px: f64,
    separator_y_px: f64,
    show_separator: bool,
    last_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct SelectionDrag {
    target: SelectionTargetKind,
    last_position: PhysicalPosition<f64>,
    auto_scroll_x_px_s: f64,
    auto_scroll_y_px_s: f64,
}

#[derive(Debug, Clone)]
struct SelectionAnimationState {
    from: SelectionRange,
    to: SelectionRange,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct EditorCaretAnimationState {
    from_buffer: String,
    from_caret: usize,
    to_caret: usize,
    started_at: Instant,
}

#[derive(Debug, Clone)]
struct CellClick {
    cell: CellRef,
    at: Instant,
    count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextCommitSource {
    Keyboard,
    Ime,
}

#[derive(Debug, Clone)]
struct TextCommit {
    text: String,
    source: TextCommitSource,
    at: Instant,
}

#[derive(Debug, Clone, Copy)]
enum EditorTextDrag {
    Character {
        anchor_offset: usize,
    },
    Word {
        anchor_start_offset: usize,
        anchor_end_offset: usize,
    },
}

#[derive(Debug, Clone, Copy)]
enum SelectionTargetKind {
    Cell,
    Row,
    Column,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GridHit {
    Cell(CellRef),
    Row(u32),
    Column(u32),
    Corner,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScrollVelocity {
    x_px_s: f64,
    y_px_s: f64,
}

#[derive(Debug, Clone, Copy)]
struct WheelImpulse {
    velocity: ScrollVelocity,
    horizontal_lines: f32,
    vertical_lines: f32,
    row_height: u32,
    column_width: u32,
    body_width: u32,
    body_height: u32,
}

impl AppState {
    fn request_redraw(&mut self) {
        self.needs_redraw = true;
        self.renderer.window().request_redraw();
    }
}

#[derive(Debug, Clone)]
struct ActiveTable {
    table_name: String,
    source_name: String,
    source_path: PathBuf,
    source_kind: cellium_data::DataFileKind,
    source_row_count: Option<u64>,
    row_count: Option<u64>,
    columns: Vec<String>,
    view: TableViewState,
    is_materialized: bool,
    workbook_path: Option<PathBuf>,
    sheet_id: SheetId,
    table_id: TableId,
}

#[derive(Debug, Clone)]
struct QueryRequest {
    table: ActiveTable,
    visible_window: VisibleWindow,
    policy: QueryWindowPolicy,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryWindowPolicy {
    Prefetch,
    VisibleOnly,
}

impl QueryWindowPolicy {
    fn row_overscan(self, table: &ActiveTable) -> u32 {
        match self {
            Self::Prefetch if table.is_materialized => MATERIALIZED_ROW_OVERSCAN,
            Self::Prefetch => LAZY_ROW_OVERSCAN,
            Self::VisibleOnly => 0,
        }
    }

    fn column_overscan(self) -> u32 {
        match self {
            Self::Prefetch => COLUMN_OVERSCAN,
            Self::VisibleOnly => 0,
        }
    }
}

impl App {
    fn new(
        proxy: EventLoopProxy<AppEvent>,
        runtime: Runtime,
        initial_path: Option<PathBuf>,
    ) -> Result<Self> {
        let data_engine = open_data_engine().context("failed to create data engine")?;
        let library_path = library_index_path().context("failed to create library index path")?;
        let library_repository = runtime
            .block_on(LocalWorkbookRepository::open(
                library_path.to_string_lossy().as_ref(),
            ))
            .context("failed to open Cellium library index")?;
        Ok(Self {
            state: None,
            proxy,
            runtime,
            data_engine,
            library_repository,
            initial_path,
            active_workbook_path: None,
        })
    }

    fn start_import(&mut self, path: PathBuf) {
        self.flush_pending_cell_writes();
        let preview_engine = match open_data_engine()
            .context("failed to create preview DuckDB session for import")
        {
            Ok(engine) => engine,
            Err(error) => {
                if self
                    .proxy
                    .send_event(AppEvent::Imported(Err(error)))
                    .is_err()
                {
                    warn!("failed to send import setup failure to event loop");
                }
                return;
            }
        };
        self.data_engine = preview_engine;
        self.active_workbook_path = None;
        let logical_table_name = table_name_for_path(&path);
        if should_start_exact_row_count(&path) {
            self.start_row_count(logical_table_name, path.clone());
        }
        let proxy = self.proxy.clone();
        let engine = self.data_engine.clone();
        self.runtime.spawn(async move {
            let result = import_file(engine, &path).await;
            if proxy.send_event(AppEvent::Imported(result)).is_err() {
                warn!("failed to send import result to event loop");
            }
        });
    }

    fn start_row_count(&self, logical_table_name: String, source_path: PathBuf) {
        let proxy = self.proxy.clone();
        self.runtime.spawn(async move {
            let path = source_path.clone();
            let result = cellium_data::exact_file_row_count(path)
                .await
                .map(|row_count| RowCountResult {
                    logical_table_name,
                    source_path,
                    row_count,
                })
                .with_context(|| "failed to count rows exactly");
            if proxy.send_event(AppEvent::RowCounted(result)).is_err() {
                warn!("failed to send row count result to event loop");
            }
        });
    }

    fn spawn_visible_window_query(&self, request: QueryRequest) {
        let proxy = self.proxy.clone();
        let engine = self.data_engine.clone();
        self.runtime.spawn(async move {
            let generation = request.generation;
            let result = query_visible_window(
                engine,
                request.table,
                request.visible_window,
                request.policy,
            )
            .await;
            if proxy
                .send_event(AppEvent::Queried { generation, result })
                .is_err()
            {
                warn!("failed to send query result to event loop");
            }
        });
    }

    fn start_materialize(&self, table: ActiveTable) {
        let proxy = self.proxy.clone();
        let Some(database_path) = table.workbook_path.clone() else {
            if proxy
                .send_event(AppEvent::Materialized(Err(anyhow!(
                    "cannot materialize without an active .cellium workbook"
                ))))
                .is_err()
            {
                warn!("failed to send materialize result to event loop");
            }
            return;
        };
        let engine = match AsyncDataEngine::open(&database_path)
            .context("failed to open materialization engine")
        {
            Ok(engine) => engine,
            Err(error) => {
                if proxy
                    .send_event(AppEvent::Materialized(Err(error)))
                    .is_err()
                {
                    warn!("failed to send materialize result to event loop");
                }
                return;
            }
        };
        self.runtime.spawn(async move {
            let result = materialize_file(engine, table, database_path).await;
            if proxy.send_event(AppEvent::Materialized(result)).is_err() {
                warn!("failed to send materialize result to event loop");
            }
        });
    }

    fn flush_pending_cell_writes(&mut self) {
        let writes = {
            let Some(state) = self.state.as_mut() else {
                return;
            };
            if state.pending_cell_writes.is_empty() {
                return;
            }
            if state
                .active_table
                .as_ref()
                .is_some_and(|table| !table.is_materialized)
            {
                return;
            }
            std::mem::take(&mut state.pending_cell_writes)
        };
        let engine = self.data_engine.clone();
        let proxy = self.proxy.clone();
        self.runtime.spawn(async move {
            let result: anyhow::Result<()> = async {
                for write in writes {
                    match write {
                        CellEditWrite::Sparse {
                            sheet_id,
                            cell,
                            value,
                            updated_at,
                        } => {
                            engine
                                .upsert_sparse_cell(SparseCellRecord {
                                    sheet_id: sheet_id.0,
                                    row_index: u64::from(cell.row),
                                    col_index: u64::from(cell.column),
                                    value,
                                    formula: None,
                                    updated_at,
                                })
                                .await?;
                        }
                        CellEditWrite::Table {
                            table_name,
                            available_columns,
                            row_id,
                            column,
                            value,
                        } => {
                            engine
                                .update_table_cell(
                                    table_name,
                                    available_columns,
                                    row_id,
                                    column,
                                    value,
                                )
                                .await?;
                        }
                    }
                }
                Ok(())
            }
            .await;
            if proxy
                .send_event(AppEvent::CellWritesFinished(result))
                .is_err()
            {
                warn!("failed to send cell-write result to event loop");
            }
        });
    }

    fn handle_cell_writes_finished(&mut self, result: Result<()>) {
        match result {
            Ok(()) => {
                let mut should_recount = false;
                if let Some(state) = self.state.as_mut() {
                    if let Some(table) = state.active_table.as_ref() {
                        should_recount = !table.view.filters.is_empty();
                        state.ui.clear_snapshot();
                    }
                    state.ui.set_status("Saved locally");
                    state.request_redraw();
                }
                self.schedule_visible_window_query();
                if should_recount {
                    self.start_active_view_count();
                }
            }
            Err(error) => {
                warn!(%error, "failed to persist cell edit");
                if let Some(state) = self.state.as_mut() {
                    state.ui.set_status("Couldn't save that edit. Try again.");
                    state.request_redraw();
                }
            }
        }
    }

    fn open_path_in_current_window(&mut self, path: PathBuf) {
        if is_cellium_workbook_path(&path) {
            if let Err(error) = self.open_workbook_path(path)
                && let Some(state) = self.state.as_mut()
            {
                state.dashboard.status = format!("Open workbook failed: {error}");
                state
                    .ui
                    .set_status(format!("Open workbook failed: {error}"));
                state.request_redraw();
            }
            return;
        }
        if let Some(state) = self.state.as_mut() {
            state.screen = AppScreen::Workbook;
        }
        self.prepare_for_import(&path);
        self.start_import(path);
    }

    pub(super) fn open_path_in_new_window(&mut self, path: &Path) -> Result<()> {
        self.flush_pending_cell_writes();
        let executable = std::env::current_exe().context("failed to locate Cellium executable")?;
        Command::new(executable)
            .arg(path)
            .spawn()
            .with_context(|| format!("failed to open {} in a new window", path.display()))?;
        if let Some(state) = self.state.as_mut() {
            let status = format!("Opening {} in a new window...", display_name(path));
            state.dashboard.status = status.clone();
            state.ui.set_status(status);
            state.request_redraw();
        }
        Ok(())
    }

    fn open_path_in_new_window_or_report(&mut self, path: &Path) {
        if let Err(error) = self.open_path_in_new_window(path) {
            warn!(%error, "failed to open path in new window");
            if let Some(state) = self.state.as_mut() {
                let status =
                    "Couldn't open that file. Try a CSV, Parquet, Arrow, or Cellium workbook."
                        .to_string();
                state.dashboard.status = status.clone();
                state.ui.set_status(status);
                state.request_redraw();
            }
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        let attributes = WindowAttributes::default()
            .with_title("Cellium")
            .with_inner_size(LogicalSize::new(
                INITIAL_WINDOW_WIDTH,
                INITIAL_WINDOW_HEIGHT,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                error!(%error, "failed to create window");
                event_loop.exit();
                return;
            }
        };

        match pollster::block_on(Renderer::new(window)) {
            Ok(renderer) => {
                let mut ui = UiState::new(cellium_core::SheetId(1));
                let size = renderer.size();
                ui.resize_viewport(size.width, size.height, renderer.scale_factor());
                let dashboard = self.load_dashboard_state().unwrap_or_else(|error| {
                    warn!(%error, "failed to load dashboard library");
                    DashboardState {
                        status: "Couldn't load recent workbooks. Try reopening Cellium."
                            .to_string(),
                        ..DashboardState::default()
                    }
                });
                let screen = if self.initial_path.is_some() {
                    AppScreen::Workbook
                } else {
                    AppScreen::Dashboard
                };
                self.state = Some(AppState {
                    renderer,
                    ui,
                    screen,
                    dashboard,
                    active_table: None,
                    cursor_position: None,
                    hovered_column_header: None,
                    scrollbar_drag: None,
                    canvas_pan_drag: None,
                    column_resize_drag: None,
                    column_resize_animation: None,
                    row_resize_drag: None,
                    row_resize_animation: None,
                    selection_drag: None,
                    selection_animation: None,
                    modifiers: ModifiersState::default(),
                    query_in_flight: false,
                    pending_query: None,
                    pending_row_count: None,
                    active_import_path: None,
                    needs_visible_window_query: false,
                    needs_redraw: true,
                    next_query_generation: 1,
                    latest_requested_generation: 0,
                    view_generation: 0,
                    scroll_velocity_x_px_s: 0.0,
                    scroll_velocity_y_px_s: 0.0,
                    last_frame_time: Instant::now(),
                    last_cell_click: None,
                    last_text_commit: None,
                    clipboard: Clipboard::new().ok(),
                    editor_text_drag: None,
                    editor_caret_animation: None,
                    editor_caret_blink_started: Instant::now(),
                    pending_cell_writes: Vec::new(),
                });
                if let Some(path) = self.initial_path.take() {
                    self.open_path_in_current_window(path);
                } else if let Some(state) = self.state.as_mut() {
                    state.request_redraw();
                }
            }
            Err(error) => {
                error!(%error, "failed to initialize renderer");
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Imported(result) => self.handle_import_result(result),
            AppEvent::RowCounted(result) => self.handle_row_count_result(result),
            AppEvent::Materialized(result) => self.handle_materialize_result(result),
            AppEvent::CellWritesFinished(result) => self.handle_cell_writes_finished(result),
            AppEvent::ViewCounted {
                generation,
                table_name,
                view,
                result,
            } => self.handle_view_count_result(generation, &table_name, &view, result),
            AppEvent::Queried { generation, result } => {
                self.handle_query_result(generation, result);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.flush_pending_cell_writes();

        let needs_query = self
            .state
            .as_ref()
            .is_some_and(|state| state.needs_visible_window_query);
        if needs_query {
            if let Some(state) = self.state.as_mut() {
                state.needs_visible_window_query = false;
            }
            self.schedule_visible_window_query();
        }

        if let Some(state) = self.state.as_mut() {
            let now = Instant::now();
            if update_editor_caret_blink(state, now) {
                state.needs_redraw = true;
            }
            if state.ui.is_editing_cell() {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    next_editor_caret_blink_deadline(state, now),
                ));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }

        if let Some(state) = self.state.as_ref()
            && (state.needs_redraw
                || scroll_inertia_active(state)
                || selection_autoscroll_active(state)
                || selection_animation_active(state, Instant::now())
                || column_resize_animation_active(state)
                || row_resize_animation_active(state)
                || editor_caret_animation_active(state, Instant::now()))
        {
            state.renderer.window().request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if window_id != state.renderer.window().id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => state.request_redraw(),
            WindowEvent::Resized(size) => {
                stop_scroll_inertia(state);
                state.renderer.resize(size);
                state
                    .ui
                    .resize_viewport(size.width, size.height, state.renderer.scale_factor());
                refresh_hover_state(state);
                self.schedule_visible_window_query();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                stop_scroll_inertia(state);
                let size = state.renderer.window().inner_size();
                state.renderer.resize(size);
                state
                    .ui
                    .resize_viewport(size.width, size.height, state.renderer.scale_factor());
                refresh_hover_state(state);
                self.schedule_visible_window_query();
            }
            WindowEvent::DroppedFile(path) => {
                self.open_path_in_new_window_or_report(&path);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.modifiers = modifiers.state();
            }
            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_position = Some(position);
                refresh_hover_state(state);
                if let Some(drag) = state.editor_text_drag {
                    if extend_editor_text_selection_from_position(
                        state,
                        drag,
                        position,
                        state.renderer.scale_factor(),
                    ) {
                        state.request_redraw();
                    }
                } else if let Some(drag) = state.scrollbar_drag {
                    let (row_count, column_count) =
                        active_table_extents(state.active_table.as_ref());
                    if apply_scrollbar_drag(
                        &mut state.ui,
                        drag,
                        position,
                        state.renderer.scale_factor(),
                        row_count,
                        column_count,
                    ) {
                        stop_scroll_inertia(state);
                        state.ui.clamp_to_table(row_count, column_count);
                        self.schedule_visible_window_query_visible_only();
                    } else {
                        state.request_redraw();
                    }
                } else if let Some(drag) = state.column_resize_drag {
                    let before = state.ui.viewport.visible_window().ok();
                    let (row_count, column_count) =
                        active_table_extents(state.active_table.as_ref());
                    apply_column_resize_drag(state, drag, position);
                    state.ui.clamp_to_table(row_count, column_count);
                    if visible_query_window_changed(
                        before.as_ref(),
                        state.ui.viewport.visible_window().ok().as_ref(),
                    ) {
                        self.schedule_visible_window_query();
                    } else {
                        state.request_redraw();
                    }
                } else if let Some(drag) = state.row_resize_drag {
                    let before = state.ui.viewport.visible_window().ok();
                    let (row_count, column_count) =
                        active_table_extents(state.active_table.as_ref());
                    apply_row_resize_drag(state, drag, position);
                    state.ui.clamp_to_table(row_count, column_count);
                    if visible_query_window_changed(
                        before.as_ref(),
                        state.ui.viewport.visible_window().ok().as_ref(),
                    ) {
                        self.schedule_visible_window_query();
                    } else {
                        state.request_redraw();
                    }
                } else if let Some(mut drag) = state.canvas_pan_drag {
                    let before = state.ui.viewport.visible_window().ok();
                    let (row_count, column_count) =
                        active_table_extents(state.active_table.as_ref());
                    if apply_canvas_pan_drag(state, &mut drag, position, Instant::now()) {
                        state.canvas_pan_drag = Some(drag);
                        state.ui.clamp_to_table(row_count, column_count);
                        if visible_query_window_changed(
                            before.as_ref(),
                            state.ui.viewport.visible_window().ok().as_ref(),
                        ) {
                            self.schedule_visible_window_query();
                        } else {
                            state.request_redraw();
                        }
                    }
                } else if let Some(mut drag) = state.selection_drag
                    && apply_selection_drag(state, &mut drag, position)
                {
                    state.selection_drag = Some(drag);
                    refresh_hover_state(state);
                    state.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let Some(position) = state.cursor_position else {
                    return;
                };
                stop_scroll_inertia(state);
                if state.screen == AppScreen::Dashboard {
                    let target = dashboard_hit(state, position);
                    if let Some(target) = target {
                        self.handle_dashboard_target(target);
                    } else if state.dashboard.search_focused {
                        state.dashboard.search_focused = false;
                        state.request_redraw();
                    }
                    return;
                }
                if state.ui.is_editing_cell() && position_is_over_active_editor(state, position) {
                    handle_active_editor_click(state, position, state.renderer.scale_factor());
                    state.request_redraw();
                    return;
                }
                if let Some(drag) = begin_column_resize_drag(state, position) {
                    if state.ui.is_editing_cell() {
                        commit_active_cell_edit(state);
                    }
                    state.column_resize_drag = Some(drag);
                    refresh_hover_state(state);
                    state.request_redraw();
                    return;
                }
                if let Some(drag) = begin_row_resize_drag(state, position) {
                    if state.ui.is_editing_cell() {
                        commit_active_cell_edit(state);
                    }
                    state.row_resize_drag = Some(drag);
                    refresh_hover_state(state);
                    state.request_redraw();
                    return;
                }
                let (row_count, column_count) = active_table_extents(state.active_table.as_ref());
                if let Some(hit) = scrollbar_hit(
                    &state.ui,
                    position,
                    state.renderer.scale_factor(),
                    row_count,
                    column_count,
                ) {
                    if state.ui.is_editing_cell() {
                        commit_active_cell_edit(state);
                    }
                    match hit {
                        ScrollbarHit::Thumb(drag) => {
                            state.scrollbar_drag = Some(drag);
                            refresh_hover_state(state);
                            state.request_redraw();
                        }
                        ScrollbarHit::Track {
                            axis,
                            local_position_px,
                        } => {
                            stop_scroll_inertia(state);
                            page_scrollbar_track(
                                &mut state.ui,
                                axis,
                                local_position_px,
                                row_count,
                                column_count,
                            );
                            state.ui.clamp_to_table(row_count, column_count);
                            self.schedule_visible_window_query();
                        }
                    }
                } else if position_is_over_rect(
                    position,
                    state.renderer.scale_factor(),
                    SORT_BUTTON_LEFT,
                    ACTION_BUTTON_TOP,
                    SORT_BUTTON_RIGHT,
                    ACTION_BUTTON_BOTTOM,
                ) {
                    if state.ui.is_editing_cell() {
                        commit_active_cell_edit(state);
                    }
                    self.cycle_active_column_sort();
                } else if position_is_over_rect(
                    position,
                    state.renderer.scale_factor(),
                    FILTER_BUTTON_LEFT,
                    ACTION_BUTTON_TOP,
                    FILTER_BUTTON_RIGHT,
                    ACTION_BUTTON_BOTTOM,
                ) {
                    if state.ui.is_editing_cell() {
                        commit_active_cell_edit(state);
                    }
                    self.filter_to_active_cell();
                } else if position_is_over_rect(
                    position,
                    state.renderer.scale_factor(),
                    CLEAR_VIEW_BUTTON_LEFT,
                    ACTION_BUTTON_TOP,
                    CLEAR_VIEW_BUTTON_RIGHT,
                    ACTION_BUTTON_BOTTOM,
                ) {
                    if state.ui.is_editing_cell() {
                        commit_active_cell_edit(state);
                    }
                    self.clear_active_table_view();
                } else if position_is_over_open_button(position, state.renderer.scale_factor())
                    && let Some(path) = pick_data_file()
                {
                    if state.ui.is_editing_cell() {
                        commit_active_cell_edit(state);
                    }
                    self.open_path_in_new_window_or_report(&path);
                } else if let Some(hit) =
                    grid_hit(&state.ui, position, state.renderer.scale_factor())
                {
                    if state.ui.is_editing_cell() {
                        let same_edit_cell = matches!(
                            &hit,
                            GridHit::Cell(cell) if state.ui.editing_cell() == Some(cell)
                        );
                        if same_edit_cell {
                            handle_active_editor_click(
                                state,
                                position,
                                state.renderer.scale_factor(),
                            );
                            state.request_redraw();
                            return;
                        }
                        commit_active_cell_edit(state);
                    }
                    if let GridHit::Cell(cell) = &hit {
                        let now = Instant::now();
                        let click_count =
                            next_cell_click_count(state.last_cell_click.as_ref(), cell, now);
                        if click_count >= 3 {
                            begin_cell_edit_with_current_value(state, cell.clone());
                            apply_editor_intent(state, EditorIntent::SelectAll);
                            state.last_cell_click = Some(CellClick {
                                cell: cell.clone(),
                                at: now,
                                count: click_count,
                            });
                            refresh_hover_state(state);
                            state.request_redraw();
                            return;
                        }
                        if click_count == 2 {
                            begin_cell_edit_with_current_value(state, cell.clone());
                            begin_editor_word_drag(state, position, state.renderer.scale_factor());
                            state.last_cell_click = Some(CellClick {
                                cell: cell.clone(),
                                at: now,
                                count: click_count,
                            });
                            refresh_hover_state(state);
                            state.request_redraw();
                            return;
                        }
                        state.last_cell_click = Some(CellClick {
                            cell: cell.clone(),
                            at: now,
                            count: click_count,
                        });
                    } else {
                        state.last_cell_click = None;
                    }
                    let action = selection_action_for_modifiers(state.modifiers);
                    state.selection_drag = apply_selection_hit(state, hit, action, position);
                    refresh_hover_state(state);
                    state.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if state.editor_text_drag.take().is_some() {
                    state.request_redraw();
                }
                if state.scrollbar_drag.take().is_some() {
                    state.needs_visible_window_query = true;
                    state.request_redraw();
                }
                if release_column_resize_drag(state) {
                    state.request_redraw();
                }
                if release_row_resize_drag(state) {
                    state.request_redraw();
                }
                if state.selection_drag.take().is_some() {
                    state.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Middle,
                ..
            } => {
                let Some(position) = state.cursor_position else {
                    return;
                };
                if state.screen != AppScreen::Workbook
                    || !position_is_over_grid_body(state, position)
                {
                    return;
                }
                if state.ui.is_editing_cell() {
                    commit_active_cell_edit(state);
                }
                stop_scroll_inertia(state);
                state.canvas_pan_drag = Some(CanvasPanDrag {
                    last_position: position,
                    last_at: Instant::now(),
                    velocity_x_px_s: 0.0,
                    velocity_y_px_s: 0.0,
                });
                refresh_hover_state(state);
                state.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Middle,
                ..
            } => {
                if let Some(drag) = state.canvas_pan_drag.take() {
                    start_pan_inertia(state, drag);
                    refresh_hover_state(state);
                    state.request_redraw();
                }
            }
            WindowEvent::PinchGesture { delta, phase, .. } => {
                if matches!(phase, TouchPhase::Started | TouchPhase::Moved) {
                    stop_scroll_inertia(state);
                    if apply_pinch_zoom(state, delta) {
                        let (row_count, column_count) =
                            active_table_extents(state.active_table.as_ref());
                        state.ui.clamp_to_table(row_count, column_count);
                        refresh_hover_state(state);
                        self.schedule_visible_window_query();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let zooming = state.modifiers.super_key() || state.modifiers.control_key();
                if zooming {
                    stop_scroll_inertia(state);
                    let zoom_delta = wheel_zoom_delta(&delta);
                    if zoom_delta.abs() > f64::EPSILON {
                        let cursor = state.cursor_position.unwrap_or(PhysicalPosition {
                            x: f64::from(state.renderer.size().width) / 2.0,
                            y: f64::from(state.renderer.size().height) / 2.0,
                        });
                        let table_zoom = state.ui.table_zoom();
                        let zoom_factor = GRID_ZOOM_STEP.powf(zoom_delta.abs());
                        let new_zoom = if zoom_delta > 0.0 {
                            table_zoom * zoom_factor
                        } else {
                            table_zoom / zoom_factor
                        };
                        if state.ui.set_table_zoom_anchored(
                            new_zoom,
                            state.renderer.scale_factor(),
                            cursor.x,
                            cursor.y - f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT),
                        ) {
                            let (row_count, column_count) =
                                active_table_extents(state.active_table.as_ref());
                            state.ui.clamp_to_table(row_count, column_count);
                            refresh_hover_state(state);
                            self.schedule_visible_window_query();
                        }
                    }
                    return;
                }
                match delta {
                    MouseScrollDelta::LineDelta(horizontal, vertical) => {
                        apply_wheel_scroll_impulse(state, horizontal, vertical);
                    }
                    MouseScrollDelta::PixelDelta(position) => {
                        stop_scroll_inertia(state);
                        let before = state.ui.viewport.visible_window().ok();
                        state.ui.scroll_y_pixels(-position.y);
                        state.ui.scroll_x_pixels(-position.x);
                        let (row_count, column_count) =
                            active_table_extents(state.active_table.as_ref());
                        state.ui.clamp_to_table(row_count, column_count);
                        if viewport_needs_query_after_scroll(&state.ui, before.as_ref()) {
                            state.needs_visible_window_query = true;
                        }
                    }
                }
                refresh_hover_state(state);
                state.request_redraw();
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if state.screen == AppScreen::Dashboard && state.dashboard.search_focused {
                    if append_dashboard_search_text(state, &text) {
                        state.request_redraw();
                    }
                    return;
                }
                if state.ui.is_editing_cell()
                    && commit_editor_text(state, text, TextCommitSource::Ime)
                {
                    state.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed() {
                    stop_scroll_inertia(state);
                }
                if event.state.is_pressed() && state.screen == AppScreen::Dashboard {
                    match handle_dashboard_key(state, &event.logical_key, event.text.as_deref()) {
                        DashboardKeyAction::Ignored => {}
                        DashboardKeyAction::Redraw => state.request_redraw(),
                        DashboardKeyAction::OpenFirstVisibleWorkbook => {
                            self.open_first_visible_dashboard_workbook();
                        }
                    }
                    return;
                }
                if event.state.is_pressed() {
                    match handle_cell_editor_key(state, &event.logical_key, event.text.as_deref()) {
                        EditorInputResult::Ignored => {}
                        EditorInputResult::Redraw => {
                            state.request_redraw();
                            return;
                        }
                        EditorInputResult::Query => {
                            self.schedule_visible_window_query();
                            return;
                        }
                    }
                }
                if event.state.is_pressed() && state.ui.is_editing_cell() {
                    return;
                }
                if event.state.is_pressed() && !state.ui.is_editing_cell() {
                    if handle_select_all_shortcut(state, &event.logical_key) {
                        state.request_redraw();
                        return;
                    }
                    if begin_cell_edit_from_printable_key(
                        state,
                        &event.logical_key,
                        event.text.as_deref(),
                    ) {
                        state.request_redraw();
                        return;
                    }
                }
                if event.state.is_pressed()
                    && !state.ui.is_editing_cell()
                    && handle_zoom_shortcut(state, &event.logical_key)
                {
                    let (row_count, column_count) =
                        active_table_extents(state.active_table.as_ref());
                    state.ui.clamp_to_table(row_count, column_count);
                    refresh_hover_state(state);
                    self.schedule_visible_window_query();
                } else if event.state.is_pressed()
                    && let Some(command) = state.ui.route_key(&event.logical_key)
                {
                    match command {
                        cellium_ui::UiCommand::MoveSelection {
                            row_delta,
                            column_delta,
                        } => {
                            let before_window = state.ui.viewport.visible_window().ok();
                            let extend = state.modifiers.shift_key();
                            apply_selection_change(state, |ui| {
                                ui.move_selection_with(row_delta, column_delta, extend);
                            });
                            if state.ui.viewport.visible_window().ok() == before_window {
                                state.request_redraw();
                            } else {
                                self.schedule_visible_window_query();
                            }
                        }
                        cellium_ui::UiCommand::BeginCellEdit => {
                            begin_cell_edit_with_current_value(
                                state,
                                state.ui.selection.active.clone(),
                            );
                            state.selection_animation = None;
                            state.request_redraw();
                        }
                        cellium_ui::UiCommand::CommitEdit => {
                            commit_active_cell_edit(state);
                            state.request_redraw();
                        }
                        cellium_ui::UiCommand::CancelEdit => {
                            cancel_active_cell_edit(state);
                            state.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                state.needs_redraw = false;
                let now = Instant::now();
                let selection_scrolled = advance_selection_autoscroll(state, now);
                let column_resize_animated = advance_column_resize_animation(state, now);
                let row_resize_animated = advance_row_resize_animation(state, now);
                if selection_scrolled || advance_scroll_inertia(state, now) {
                    refresh_hover_state(state);
                }
                let primitives = render_primitives(state);
                match state.renderer.render(&primitives) {
                    Ok(()) | Err(RenderError::Occluded | RenderError::Timeout) => {}
                    Err(RenderError::Lost | RenderError::Outdated | RenderError::Suboptimal) => {
                        state.renderer.resize(state.renderer.size());
                        state.request_redraw();
                    }
                    Err(RenderError::Validation) => {
                        error!("surface validation failed");
                        event_loop.exit();
                    }
                    Err(error) => {
                        error!(%error, "render failed");
                        event_loop.exit();
                    }
                }
                if scroll_inertia_active(state) || selection_autoscroll_active(state) {
                    state.request_redraw();
                }
                if selection_animation_active(state, now)
                    || column_resize_animated
                    || column_resize_animation_active(state)
                    || row_resize_animated
                    || row_resize_animation_active(state)
                    || editor_caret_animation_active(state, now)
                {
                    state.request_redraw();
                }
            }
            _ => {}
        }
    }
}

mod dashboard;
mod data_flow;

use data_flow::*;

fn position_is_over_open_button(position: PhysicalPosition<f64>, scale_factor: f64) -> bool {
    position_is_over_rect(
        position,
        scale_factor,
        OPEN_BUTTON_LEFT,
        OPEN_BUTTON_TOP,
        OPEN_BUTTON_RIGHT,
        OPEN_BUTTON_BOTTOM,
    )
}

fn position_is_over_rect(
    position: PhysicalPosition<f64>,
    scale_factor: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
) -> bool {
    let scale_factor = scale_factor.max(1.0);
    position.x >= left * scale_factor
        && position.x <= right * scale_factor
        && position.y >= top * scale_factor
        && position.y <= bottom * scale_factor
}

fn position_is_over_formula_bar(position: PhysicalPosition<f64>, state: &AppState) -> bool {
    let scale_factor = state.renderer.scale_factor().max(1.0);
    let right = f64::from(state.renderer.size().width) - FORMULA_INPUT_RIGHT_PAD * scale_factor;
    position.x >= FORMULA_CELL_LEFT * scale_factor
        && position.x <= right.max(FORMULA_INPUT_LEFT * scale_factor)
        && position.y >= f64::from(FORMULA_BAR_TOP) * scale_factor
        && position.y <= f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT) * scale_factor
        && (position.x <= FORMULA_CELL_RIGHT * scale_factor
            || position.x >= FORMULA_INPUT_LEFT * scale_factor)
}

fn position_is_over_sheet_tab(position: PhysicalPosition<f64>, state: &AppState) -> bool {
    let scale_factor = state.renderer.scale_factor().max(1.0);
    let height = f64::from(state.renderer.size().height);
    let tab_top = height - f64::from(cellium_ui::SHEET_TAB_HEIGHT) * scale_factor;
    position.x >= SHEET_TAB_LEFT * scale_factor
        && position.x <= SHEET_TAB_RIGHT * scale_factor
        && position.y >= tab_top + SHEET_TAB_TOP_OFFSET * scale_factor
        && position.y <= tab_top + SHEET_TAB_BOTTOM_OFFSET * scale_factor
}

fn pick_data_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open Cellium workbook or data file")
        .add_filter("Cellium workbook", &["cellium"])
        .add_filter("Data files", &["csv", "parquet", "arrow", "ipc", "feather"])
        .add_filter("CSV", &["csv"])
        .add_filter("Parquet", &["parquet"])
        .add_filter("Arrow IPC", &["arrow", "ipc", "feather"])
        .pick_file()
}

fn is_cellium_workbook_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cellium"))
}

fn active_table_extents(table: Option<&ActiveTable>) -> (Option<u64>, usize) {
    (
        table.and_then(display_row_count_for_table),
        table.map_or(0, |table| table.columns.len()),
    )
}

fn display_row_count_for_table(table: &ActiveTable) -> Option<u64> {
    table
        .row_count
        .map(|row_count| row_count.saturating_add(u64::from(display_header_row_count(table))))
}

fn display_header_row_count(table: &ActiveTable) -> u32 {
    u32::from(matches!(table.source_kind, DataFileKind::Csv) && !table.columns.is_empty())
}

mod editor;
use editor::*;

fn apply_pinch_zoom(state: &mut AppState, delta: f64) -> bool {
    if !delta.is_finite() || delta.abs() <= f64::EPSILON {
        return false;
    }
    let cursor = state.cursor_position.unwrap_or(PhysicalPosition {
        x: f64::from(state.renderer.size().width) / 2.0,
        y: f64::from(state.renderer.size().height) / 2.0,
    });
    let zoom_factor = (delta * PINCH_ZOOM_SENSITIVITY).exp();
    state.ui.set_table_zoom_anchored(
        state.ui.table_zoom() * zoom_factor,
        state.renderer.scale_factor(),
        cursor.x,
        cursor.y - f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT),
    )
}

fn selection_action_for_modifiers(modifiers: ModifiersState) -> SelectionAction {
    SelectionAction {
        extend: modifiers.shift_key(),
        additive: modifiers.super_key() || modifiers.control_key(),
    }
}

fn apply_selection_change(state: &mut AppState, change: impl FnOnce(&mut UiState)) -> bool {
    apply_selection_change_inner(state, change)
}

fn apply_selection_change_inner(state: &mut AppState, change: impl FnOnce(&mut UiState)) -> bool {
    let before = selection_animation_frame(state, Instant::now())
        .map(|animation| animation.to)
        .or_else(|| state.ui.selection.ranges.last().cloned());
    change(&mut state.ui);
    let after = state.ui.selection.ranges.last().cloned();
    if before == after {
        return false;
    }

    state.selection_animation = match (before, after) {
        (Some(from), Some(to)) => Some(SelectionAnimationState {
            from,
            to,
            started_at: Instant::now(),
        }),
        _ => None,
    };
    true
}

fn move_selection_with_animation(
    state: &mut AppState,
    row_delta: i32,
    column_delta: i32,
    extend: bool,
) -> bool {
    apply_selection_change_inner(state, |ui| {
        ui.move_selection_with(row_delta, column_delta, extend);
    })
}

fn selection_animation_frame(state: &AppState, now: Instant) -> Option<SelectionAnimation> {
    let animation = state.selection_animation.as_ref()?;
    let duration = Duration::from_millis(SELECTION_ANIMATION_MS);
    let progress = now
        .saturating_duration_since(animation.started_at)
        .as_secs_f32()
        / duration.as_secs_f32();
    (progress < 1.0).then(|| SelectionAnimation {
        from: animation.from.clone(),
        to: animation.to.clone(),
        progress: progress.clamp(0.0, 1.0),
    })
}

fn selection_animation_active(state: &AppState, now: Instant) -> bool {
    selection_animation_frame(state, now).is_some()
}

fn begin_column_resize_drag(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
) -> Option<ColumnResizeDrag> {
    let Some(ChromeHoverTarget::ColumnResize(column)) =
        resize_edge_hover(&state.ui, position, state.renderer.scale_factor())
    else {
        return None;
    };
    let column_index = column.saturating_sub(1);
    let current_width = f64::from(state.ui.viewport.column_width_at(column_index));
    let (separator_x, _) = grid_local_position(position, state.renderer.scale_factor());
    let visual_width = state
        .column_resize_animation
        .as_ref()
        .filter(|animation| animation.column == column_index)
        .map_or(current_width, |animation| animation.visual_width_px);
    state.column_resize_animation = Some(ColumnResizeAnimationState {
        column: column_index,
        visual_width_px: visual_width,
        target_width_px: current_width,
        separator_x_px: separator_x,
        show_separator: true,
        last_at: Instant::now(),
    });
    Some(ColumnResizeDrag {
        column: column_index,
        start_x: position.x,
        start_width_px: current_width,
    })
}

fn apply_column_resize_drag(
    state: &mut AppState,
    drag: ColumnResizeDrag,
    position: PhysicalPosition<f64>,
) {
    let table_scale = state.renderer.scale_factor().max(1.0) * state.ui.table_zoom();
    let min_width = COLUMN_RESIZE_MIN_WIDTH * table_scale;
    let target_width = (drag.start_width_px + position.x - drag.start_x).max(min_width);
    let previous = state.column_resize_animation;
    let visual_width = previous.map_or(drag.start_width_px, |animation| animation.visual_width_px);
    let last_at = previous.map_or_else(Instant::now, |animation| animation.last_at);
    let (separator_x, _) = grid_local_position(position, state.renderer.scale_factor());
    state.ui.set_column_width_px(drag.column, target_width);
    state.column_resize_animation = Some(ColumnResizeAnimationState {
        column: drag.column,
        visual_width_px: visual_width,
        target_width_px: target_width.round(),
        separator_x_px: separator_x,
        show_separator: true,
        last_at,
    });
    state.ui.chrome.hovered = Some(ChromeHoverTarget::ColumnResize(
        drag.column.saturating_add(1),
    ));
}

fn release_column_resize_drag(state: &mut AppState) -> bool {
    if state.column_resize_drag.take().is_none() {
        return false;
    }
    if let Some(animation) = state.column_resize_animation.as_mut() {
        animation.show_separator = false;
    }
    true
}

fn advance_column_resize_animation(state: &mut AppState, now: Instant) -> bool {
    let Some(mut animation) = state.column_resize_animation else {
        return false;
    };
    let dt = now
        .saturating_duration_since(animation.last_at)
        .as_secs_f64()
        .min(MAX_SCROLL_FRAME_DT);
    animation.last_at = now;
    let delta = animation.target_width_px - animation.visual_width_px;
    if dt > 0.0 && delta.abs() > 0.1 {
        let amount = 1.0 - (-COLUMN_RESIZE_LERP_SPEED * dt).exp();
        animation.visual_width_px += delta * amount;
    }
    if (animation.target_width_px - animation.visual_width_px).abs() <= 0.25 {
        animation.visual_width_px = animation.target_width_px;
    }
    let changed = state.column_resize_animation.is_some_and(|previous| {
        (previous.visual_width_px - animation.visual_width_px).abs() > f64::EPSILON
            || previous.show_separator != animation.show_separator
    });
    if !animation.show_separator
        && (animation.target_width_px - animation.visual_width_px).abs() <= f64::EPSILON
    {
        state.column_resize_animation = None;
    } else {
        state.column_resize_animation = Some(animation);
    }
    changed
}

fn column_resize_animation_frame(state: &AppState) -> Option<ColumnResizeAnimation> {
    state
        .column_resize_animation
        .map(|animation| ColumnResizeAnimation {
            column: animation.column,
            visual_column_width_px: animation.visual_width_px as f32,
            separator_x_px: animation.separator_x_px as f32,
            show_separator: animation.show_separator,
        })
}

fn column_resize_animation_active(state: &AppState) -> bool {
    state.column_resize_animation.is_some_and(|animation| {
        (animation.target_width_px - animation.visual_width_px).abs() > 0.25
    })
}

fn begin_row_resize_drag(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
) -> Option<RowResizeDrag> {
    let Some(ChromeHoverTarget::RowResize(row)) =
        resize_edge_hover(&state.ui, position, state.renderer.scale_factor())
    else {
        return None;
    };
    let row_index = u64::from(row.saturating_sub(1));
    let current_height = f64::from(state.ui.viewport.row_height_at(row_index));
    let (_, separator_y) = grid_local_position(position, state.renderer.scale_factor());
    let visual_height = state
        .row_resize_animation
        .as_ref()
        .filter(|animation| animation.row == row_index)
        .map_or(current_height, |animation| animation.visual_height_px);
    state.row_resize_animation = Some(RowResizeAnimationState {
        row: row_index,
        visual_height_px: visual_height,
        target_height_px: current_height,
        separator_y_px: separator_y,
        show_separator: true,
        last_at: Instant::now(),
    });
    Some(RowResizeDrag {
        row: row_index,
        start_y: position.y,
        start_height_px: current_height,
    })
}

fn apply_row_resize_drag(
    state: &mut AppState,
    drag: RowResizeDrag,
    position: PhysicalPosition<f64>,
) {
    let table_scale = state.renderer.scale_factor().max(1.0) * state.ui.table_zoom();
    let min_height = ROW_RESIZE_MIN_HEIGHT * table_scale;
    let target_height = (drag.start_height_px + position.y - drag.start_y).max(min_height);
    let previous = state.row_resize_animation;
    let visual_height =
        previous.map_or(drag.start_height_px, |animation| animation.visual_height_px);
    let last_at = previous.map_or_else(Instant::now, |animation| animation.last_at);
    let (_, separator_y) = grid_local_position(position, state.renderer.scale_factor());
    state.ui.set_row_height_px(drag.row, target_height);
    state.row_resize_animation = Some(RowResizeAnimationState {
        row: drag.row,
        visual_height_px: visual_height,
        target_height_px: target_height.round(),
        separator_y_px: separator_y,
        show_separator: true,
        last_at,
    });
    state.ui.chrome.hovered = Some(ChromeHoverTarget::RowResize(
        u32::try_from(drag.row.saturating_add(1))
            .ok()
            .unwrap_or(u32::MAX),
    ));
}

fn release_row_resize_drag(state: &mut AppState) -> bool {
    if state.row_resize_drag.take().is_none() {
        return false;
    }
    if let Some(animation) = state.row_resize_animation.as_mut() {
        animation.show_separator = false;
    }
    true
}

fn advance_row_resize_animation(state: &mut AppState, now: Instant) -> bool {
    let Some(mut animation) = state.row_resize_animation else {
        return false;
    };
    let dt = now
        .saturating_duration_since(animation.last_at)
        .as_secs_f64()
        .min(MAX_SCROLL_FRAME_DT);
    animation.last_at = now;
    let delta = animation.target_height_px - animation.visual_height_px;
    if dt > 0.0 && delta.abs() > 0.1 {
        let amount = 1.0 - (-ROW_RESIZE_LERP_SPEED * dt).exp();
        animation.visual_height_px += delta * amount;
    }
    if (animation.target_height_px - animation.visual_height_px).abs() <= 0.25 {
        animation.visual_height_px = animation.target_height_px;
    }
    let changed = state.row_resize_animation.is_some_and(|previous| {
        (previous.visual_height_px - animation.visual_height_px).abs() > f64::EPSILON
            || previous.show_separator != animation.show_separator
    });
    if !animation.show_separator
        && (animation.target_height_px - animation.visual_height_px).abs() <= f64::EPSILON
    {
        state.row_resize_animation = None;
    } else {
        state.row_resize_animation = Some(animation);
    }
    changed
}

fn row_resize_animation_frame(state: &AppState) -> Option<RowResizeAnimation> {
    state
        .row_resize_animation
        .map(|animation| RowResizeAnimation {
            row: animation.row,
            visual_row_height_px: animation.visual_height_px as f32,
            separator_y_px: animation.separator_y_px as f32,
            show_separator: animation.show_separator,
        })
}

fn row_resize_animation_active(state: &AppState) -> bool {
    state.row_resize_animation.is_some_and(|animation| {
        (animation.target_height_px - animation.visual_height_px).abs() > 0.25
    })
}

fn apply_canvas_pan_drag(
    state: &mut AppState,
    drag: &mut CanvasPanDrag,
    position: PhysicalPosition<f64>,
    now: Instant,
) -> bool {
    let delta_x = position.x - drag.last_position.x;
    let delta_y = position.y - drag.last_position.y;
    let dt = now
        .saturating_duration_since(drag.last_at)
        .as_secs_f64()
        .min(MAX_SCROLL_FRAME_DT);
    drag.last_position = position;
    drag.last_at = now;
    if dt > 0.0 {
        let sample = ScrollVelocity {
            x_px_s: -delta_x / dt,
            y_px_s: -delta_y / dt,
        };
        let velocity = smoothed_pan_velocity(
            ScrollVelocity {
                x_px_s: drag.velocity_x_px_s,
                y_px_s: drag.velocity_y_px_s,
            },
            sample,
        );
        drag.velocity_x_px_s = velocity.x_px_s;
        drag.velocity_y_px_s = velocity.y_px_s;
    }
    if delta_x.abs() < f64::EPSILON && delta_y.abs() < f64::EPSILON {
        return false;
    }
    state.ui.scroll_x_pixels(-delta_x);
    state.ui.scroll_y_pixels(-delta_y);
    true
}

fn smoothed_pan_velocity(previous: ScrollVelocity, sample: ScrollVelocity) -> ScrollVelocity {
    ScrollVelocity {
        x_px_s: previous.x_px_s * 0.35 + sample.x_px_s * 0.65,
        y_px_s: previous.y_px_s * 0.35 + sample.y_px_s * 0.65,
    }
}

fn start_pan_inertia(state: &mut AppState, drag: CanvasPanDrag) {
    let velocity = clamped_scroll_velocity_to_viewport(
        &state.ui,
        ScrollVelocity {
            x_px_s: drag.velocity_x_px_s,
            y_px_s: drag.velocity_y_px_s,
        },
    );
    state.scroll_velocity_x_px_s = velocity.x_px_s;
    state.scroll_velocity_y_px_s = velocity.y_px_s;
    state.last_frame_time = Instant::now();
}

fn clamped_scroll_velocity_to_viewport(ui: &UiState, velocity: ScrollVelocity) -> ScrollVelocity {
    let max_x = f64::from(ui.viewport.body_width()) * MAX_SCROLL_VELOCITY_VIEWPORTS;
    let max_y = f64::from(ui.viewport.body_height()) * MAX_SCROLL_VELOCITY_VIEWPORTS;
    ScrollVelocity {
        x_px_s: velocity.x_px_s.clamp(-max_x, max_x),
        y_px_s: velocity.y_px_s.clamp(-max_y, max_y),
    }
}

fn editor_caret_animation_frame(state: &AppState, now: Instant) -> Option<EditorCaretAnimation> {
    let animation = state.editor_caret_animation.as_ref()?;
    let selection = state.ui.grid_edit_state.selection()?;
    if selection.caret != animation.to_caret {
        return None;
    }
    let duration = Duration::from_millis(EDITOR_CARET_ANIMATION_MS);
    let progress = now
        .saturating_duration_since(animation.started_at)
        .as_secs_f32()
        / duration.as_secs_f32();
    (progress < 1.0).then(|| EditorCaretAnimation {
        from_buffer: animation.from_buffer.clone(),
        from_caret: animation.from_caret,
        to_caret: animation.to_caret,
        progress: progress.clamp(0.0, 1.0),
    })
}

fn editor_caret_animation_active(state: &AppState, now: Instant) -> bool {
    editor_caret_animation_frame(state, now).is_some()
}

fn apply_selection_hit(
    state: &mut AppState,
    hit: GridHit,
    action: SelectionAction,
    position: PhysicalPosition<f64>,
) -> Option<SelectionDrag> {
    match hit {
        GridHit::Cell(cell) => {
            apply_selection_change(state, |ui| ui.select_cell(cell, action));
            Some(SelectionDrag {
                target: SelectionTargetKind::Cell,
                last_position: position,
                auto_scroll_x_px_s: 0.0,
                auto_scroll_y_px_s: 0.0,
            })
        }
        GridHit::Row(row) => {
            apply_selection_change(state, |ui| ui.select_row(row, action));
            Some(SelectionDrag {
                target: SelectionTargetKind::Row,
                last_position: position,
                auto_scroll_x_px_s: 0.0,
                auto_scroll_y_px_s: 0.0,
            })
        }
        GridHit::Column(column) => {
            apply_selection_change(state, |ui| ui.select_column(column, action));
            Some(SelectionDrag {
                target: SelectionTargetKind::Column,
                last_position: position,
                auto_scroll_x_px_s: 0.0,
                auto_scroll_y_px_s: 0.0,
            })
        }
        GridHit::Corner => {
            apply_selection_change(state, UiState::select_all);
            None
        }
    }
}

fn apply_selection_drag(
    state: &mut AppState,
    drag: &mut SelectionDrag,
    position: PhysicalPosition<f64>,
) -> bool {
    drag.last_position = position;
    let scale_factor = state.renderer.scale_factor();
    let velocity = selection_autoscroll_velocity(&state.ui, drag.target, position, scale_factor);
    drag.auto_scroll_x_px_s = velocity.x_px_s;
    drag.auto_scroll_y_px_s = velocity.y_px_s;
    let action = SelectionAction {
        extend: true,
        additive: false,
    };
    let before = state.ui.selection.clone();
    match drag.target {
        SelectionTargetKind::Cell => {
            let Some(cell) = drag_cell_at_position(&state.ui, position, scale_factor) else {
                return false;
            };
            apply_selection_change(state, |ui| ui.select_cell(cell, action));
        }
        SelectionTargetKind::Row => {
            let Some(row) = drag_row_at_position(&state.ui, position, scale_factor) else {
                return false;
            };
            apply_selection_change(state, |ui| ui.select_row(row, action));
        }
        SelectionTargetKind::Column => {
            let Some(column) = drag_column_at_position(&state.ui, position, scale_factor) else {
                return false;
            };
            apply_selection_change(state, |ui| ui.select_column(column, action));
        }
    }
    state.ui.selection != before || scroll_velocity_active(velocity)
}

fn selection_autoscroll_velocity(
    ui: &UiState,
    target: SelectionTargetKind,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> ScrollVelocity {
    let (local_x, local_y) = grid_local_position(position, scale_factor);
    let metrics = &ui.viewport.metrics;
    let header_width = f64::from(metrics.header_width);
    let header_height = f64::from(metrics.header_height);
    let body_right = f64::from(
        ui.viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    let body_bottom = f64::from(
        ui.viewport
            .pixel_height
            .saturating_sub(metrics.scrollbar_thickness),
    );
    let max_x = f64::from(ui.viewport.body_width()) * SELECTION_AUTOSCROLL_MAX_VIEWPORTS;
    let max_y = f64::from(ui.viewport.body_height()) * SELECTION_AUTOSCROLL_MAX_VIEWPORTS;
    ScrollVelocity {
        x_px_s: if matches!(
            target,
            SelectionTargetKind::Cell | SelectionTargetKind::Column
        ) {
            edge_autoscroll_velocity(local_x, header_width, body_right, max_x)
        } else {
            0.0
        },
        y_px_s: if matches!(target, SelectionTargetKind::Cell | SelectionTargetKind::Row) {
            edge_autoscroll_velocity(local_y, header_height, body_bottom, max_y)
        } else {
            0.0
        },
    }
}

fn edge_autoscroll_velocity(position: f64, min: f64, max: f64, max_velocity: f64) -> f64 {
    if max <= min || max_velocity <= 0.0 {
        return 0.0;
    }
    let edge = SELECTION_AUTOSCROLL_EDGE_PX.min((max - min) / 3.0).max(1.0);
    if position < min + edge {
        let pressure = ((min + edge - position) / edge).clamp(0.0, 1.0);
        -max_velocity * pressure * pressure
    } else if position > max - edge {
        let pressure = ((position - (max - edge)) / edge).clamp(0.0, 1.0);
        max_velocity * pressure * pressure
    } else {
        0.0
    }
}

fn grid_hit(ui: &UiState, position: PhysicalPosition<f64>, scale_factor: f64) -> Option<GridHit> {
    let (local_x, local_y) = grid_local_position(position, scale_factor);
    if local_x < 0.0 || local_y < 0.0 {
        return None;
    }
    let metrics = &ui.viewport.metrics;
    let body_right = f64::from(
        ui.viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    let body_bottom = f64::from(
        ui.viewport
            .pixel_height
            .saturating_sub(metrics.scrollbar_thickness),
    );
    if local_x >= body_right || local_y >= body_bottom {
        return None;
    }

    let header_width = f64::from(metrics.header_width);
    let header_height = f64::from(metrics.header_height);
    if local_y < header_height {
        if local_x < header_width {
            Some(GridHit::Corner)
        } else {
            grid_column_at_position(ui, position, scale_factor).map(GridHit::Column)
        }
    } else if local_x < header_width {
        grid_row_at_position(ui, position, scale_factor).map(GridHit::Row)
    } else {
        grid_cell_at_position(ui, position, scale_factor).map(GridHit::Cell)
    }
}

fn position_is_over_grid_body(state: &AppState, position: PhysicalPosition<f64>) -> bool {
    let (local_x, local_y) = grid_local_position(position, state.renderer.scale_factor());
    let metrics = &state.ui.viewport.metrics;
    let header_width = f64::from(metrics.header_width);
    let header_height = f64::from(metrics.header_height);
    let body_right = f64::from(
        state
            .ui
            .viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    let body_bottom = f64::from(
        state
            .ui
            .viewport
            .pixel_height
            .saturating_sub(metrics.scrollbar_thickness),
    );
    local_x >= header_width
        && local_x < body_right
        && local_y >= header_height
        && local_y < body_bottom
}

fn resize_edge_hover(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<ChromeHoverTarget> {
    let (local_x, local_y) = grid_local_position(position, scale_factor);
    if local_x < 0.0 || local_y < 0.0 {
        return None;
    }
    let metrics = &ui.viewport.metrics;
    let header_width = f64::from(metrics.header_width);
    let header_height = f64::from(metrics.header_height);
    let body_right = f64::from(
        ui.viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    let body_bottom = f64::from(
        ui.viewport
            .pixel_height
            .saturating_sub(metrics.scrollbar_thickness),
    );
    if local_x >= body_right || local_y >= body_bottom {
        return None;
    }
    let window = ui.viewport.visible_window().ok()?;
    if local_y <= header_height && local_x >= header_width {
        for edge in 1..=window.column_count {
            let edge_x = header_width + visible_column_offset_x(&window, edge);
            if (local_x - edge_x).abs() <= GRID_RESIZE_HIT_SLOP * scale_factor.max(1.0) {
                let column = window.start_column.saturating_add(edge);
                return Some(ChromeHoverTarget::ColumnResize(column));
            }
        }
    }
    if local_x <= header_width && local_y >= header_height {
        for edge in 1..=window.row_count {
            let edge_y = header_height + visible_row_offset_y(&window, u64::from(edge));
            if (local_y - edge_y).abs() <= GRID_RESIZE_HIT_SLOP * scale_factor.max(1.0) {
                let row = u32::try_from(window.start_row.saturating_add(u64::from(edge)))
                    .ok()
                    .unwrap_or(u32::MAX)
                    .max(1);
                return Some(ChromeHoverTarget::RowResize(row));
            }
        }
    }
    None
}

fn grid_cell_at_position(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<CellRef> {
    let row = grid_row_at_position(ui, position, scale_factor)?;
    let column = grid_column_at_position(ui, position, scale_factor)?;
    Some(CellRef::new(row, column))
}

fn drag_cell_at_position(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<CellRef> {
    let row = drag_row_at_position(ui, position, scale_factor)?;
    let column = drag_column_at_position(ui, position, scale_factor)?;
    Some(CellRef::new(row, column))
}

fn grid_row_at_position(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<u32> {
    let (_, local_y) = grid_local_position(position, scale_factor);
    row_at_local_y(ui, local_y)
}

fn drag_row_at_position(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<u32> {
    row_at_local_y(ui, clamped_drag_local_y(ui, position, scale_factor))
}

fn grid_column_at_position(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<u32> {
    let (local_x, _) = grid_local_position(position, scale_factor);
    column_at_local_x(ui, local_x)
}

fn drag_column_at_position(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<u32> {
    column_at_local_x(ui, clamped_drag_local_x(ui, position, scale_factor))
}

fn row_at_local_y(ui: &UiState, local_y: f64) -> Option<u32> {
    let metrics = &ui.viewport.metrics;
    let header_height = f64::from(metrics.header_height);
    let body_bottom = f64::from(
        ui.viewport
            .pixel_height
            .saturating_sub(metrics.scrollbar_thickness),
    );
    if local_y < header_height || local_y >= body_bottom {
        return None;
    }
    let window = ui.viewport.visible_window().ok()?;
    let row_offset = visible_row_at_y(&window, local_y - header_height)?;
    u32::try_from(window.start_row.saturating_add(u64::from(row_offset)) + 1).ok()
}

fn column_at_local_x(ui: &UiState, local_x: f64) -> Option<u32> {
    let metrics = &ui.viewport.metrics;
    let header_width = f64::from(metrics.header_width);
    let body_right = f64::from(
        ui.viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    if local_x < header_width || local_x >= body_right {
        return None;
    }
    let window = ui.viewport.visible_window().ok()?;
    let column_offset = visible_column_at_x(&window, local_x - header_width)?;
    window
        .start_column
        .saturating_add(column_offset)
        .checked_add(1)
}

fn visible_row_at_y(window: &VisibleWindow, body_y: f64) -> Option<u32> {
    let mut y = -window.row_offset_px;
    for (index, height) in window.row_heights.iter().copied().enumerate() {
        let next_y = y + f64::from(height);
        if body_y >= y && body_y < next_y {
            return u32::try_from(index).ok();
        }
        y = next_y;
    }
    None
}

fn visible_column_at_x(window: &VisibleWindow, body_x: f64) -> Option<u32> {
    let mut x = -window.column_offset_px;
    for (index, width) in window.column_widths.iter().copied().enumerate() {
        let next_x = x + f64::from(width);
        if body_x >= x && body_x < next_x {
            return u32::try_from(index).ok();
        }
        x = next_x;
    }
    None
}

pub(super) fn visible_row_offset_y(window: &VisibleWindow, visible_row: u64) -> f64 {
    window
        .row_heights
        .iter()
        .take(visible_row as usize)
        .map(|&height| f64::from(height))
        .sum::<f64>()
        - window.row_offset_px
}

pub(super) fn visible_column_offset_x(window: &VisibleWindow, visible_column: u32) -> f64 {
    window
        .column_widths
        .iter()
        .take(visible_column as usize)
        .map(|&width| f64::from(width))
        .sum::<f64>()
        - window.column_offset_px
}

fn clamped_drag_local_x(ui: &UiState, position: PhysicalPosition<f64>, scale_factor: f64) -> f64 {
    let (local_x, _) = grid_local_position(position, scale_factor);
    let metrics = &ui.viewport.metrics;
    let header_width = f64::from(metrics.header_width);
    let body_right = f64::from(
        ui.viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    local_x.clamp(header_width, (body_right - 1.0).max(header_width))
}

fn clamped_drag_local_y(ui: &UiState, position: PhysicalPosition<f64>, scale_factor: f64) -> f64 {
    let (_, local_y) = grid_local_position(position, scale_factor);
    let metrics = &ui.viewport.metrics;
    let header_height = f64::from(metrics.header_height);
    let body_bottom = f64::from(
        ui.viewport
            .pixel_height
            .saturating_sub(metrics.scrollbar_thickness),
    );
    local_y.clamp(header_height, (body_bottom - 1.0).max(header_height))
}

fn should_start_exact_row_count(path: &Path) -> bool {
    matches!(
        cellium_data::sniff_file_kind(path),
        Ok(DataFileKind::Csv | DataFileKind::Parquet)
    )
}

fn take_matching_pending_row_count(
    pending: &mut Option<RowCountResult>,
    table: &ActiveTable,
) -> Option<u64> {
    if pending
        .as_ref()
        .is_some_and(|result| row_count_result_matches_table(result, table))
    {
        pending.take().map(|result| result.row_count)
    } else {
        None
    }
}

fn row_count_result_matches_table(result: &RowCountResult, table: &ActiveTable) -> bool {
    result.logical_table_name == table.table_name && result.source_path == table.source_path
}

fn update_snapshot_row_count(ui: &mut UiState, table: &ActiveTable) {
    if let Some(snapshot) = ui.snapshot.as_mut()
        && snapshot.table_name == table.table_name
    {
        snapshot.row_count = table.row_count;
    }
}

fn apply_wheel_scroll_impulse(state: &mut AppState, horizontal: f32, vertical: f32) {
    let velocity = wheel_velocity_after_impulse(WheelImpulse {
        velocity: ScrollVelocity {
            x_px_s: state.scroll_velocity_x_px_s,
            y_px_s: state.scroll_velocity_y_px_s,
        },
        horizontal_lines: horizontal,
        vertical_lines: vertical,
        row_height: state.ui.viewport.metrics.row_height,
        column_width: state.ui.viewport.metrics.column_width,
        body_width: state.ui.viewport.body_width(),
        body_height: state.ui.viewport.body_height(),
    });
    state.scroll_velocity_x_px_s = velocity.x_px_s;
    state.scroll_velocity_y_px_s = velocity.y_px_s;
    state.last_frame_time = Instant::now();
    state.needs_redraw = true;
}

fn wheel_velocity_after_impulse(input: WheelImpulse) -> ScrollVelocity {
    let row_impulse = f64::from(input.row_height) * WHEEL_SCROLL_IMPULSE;
    let column_impulse = f64::from(input.column_width) * WHEEL_SCROLL_IMPULSE;
    let next_x = input.velocity.x_px_s - f64::from(input.horizontal_lines) * column_impulse;
    let next_y = input.velocity.y_px_s - f64::from(input.vertical_lines) * row_impulse;
    let max_x = f64::from(input.body_width) * MAX_SCROLL_VELOCITY_VIEWPORTS;
    let max_y = f64::from(input.body_height) * MAX_SCROLL_VELOCITY_VIEWPORTS;
    ScrollVelocity {
        x_px_s: next_x.clamp(-max_x, max_x),
        y_px_s: next_y.clamp(-max_y, max_y),
    }
}

fn advance_scroll_inertia(state: &mut AppState, now: Instant) -> bool {
    let dt = now
        .saturating_duration_since(state.last_frame_time)
        .as_secs_f64()
        .min(MAX_SCROLL_FRAME_DT);
    state.last_frame_time = now;
    if !scroll_inertia_active(state) || dt <= 0.0 {
        return false;
    }

    let before_window = state.ui.viewport.visible_window().ok();
    let (row_count, column_count) = active_table_extents(state.active_table.as_ref());
    let mut velocity = ScrollVelocity {
        x_px_s: state.scroll_velocity_x_px_s,
        y_px_s: state.scroll_velocity_y_px_s,
    };
    let scrolled = advance_scroll_inertia_for_viewport(
        &mut state.ui,
        row_count,
        column_count,
        &mut velocity,
        dt,
    );
    state.scroll_velocity_x_px_s = velocity.x_px_s;
    state.scroll_velocity_y_px_s = velocity.y_px_s;

    let after_window = state.ui.viewport.visible_window().ok();
    if visible_query_window_changed(before_window.as_ref(), after_window.as_ref()) {
        state.needs_visible_window_query = true;
    }
    scrolled
}

fn advance_selection_autoscroll(state: &mut AppState, now: Instant) -> bool {
    let dt = now
        .saturating_duration_since(state.last_frame_time)
        .as_secs_f64()
        .min(MAX_SCROLL_FRAME_DT);
    let Some(mut drag) = state.selection_drag else {
        return false;
    };
    if !selection_drag_velocity_active(drag) || dt <= 0.0 {
        return false;
    }

    let before_window = state.ui.viewport.visible_window().ok();
    let (row_count, column_count) = active_table_extents(state.active_table.as_ref());
    state.ui.scroll_x_pixels(drag.auto_scroll_x_px_s * dt);
    state.ui.scroll_y_pixels(drag.auto_scroll_y_px_s * dt);
    state.ui.clamp_to_table(row_count, column_count);
    let scrolled = before_window.as_ref() != state.ui.viewport.visible_window().ok().as_ref();
    let last_position = drag.last_position;
    let changed_selection = apply_selection_drag(state, &mut drag, last_position);
    state.selection_drag = Some(drag);
    if visible_query_window_changed(
        before_window.as_ref(),
        state.ui.viewport.visible_window().ok().as_ref(),
    ) {
        state.needs_visible_window_query = true;
    }
    scrolled || changed_selection
}

fn advance_scroll_inertia_for_viewport(
    ui: &mut UiState,
    row_count: Option<u64>,
    column_count: usize,
    velocity: &mut ScrollVelocity,
    dt: f64,
) -> bool {
    if !scroll_velocity_active(*velocity) || dt <= 0.0 {
        return false;
    }

    let delta_x = velocity.x_px_s * dt;
    let delta_y = velocity.y_px_s * dt;
    let before_scroll_x = ui.viewport.scroll_x_px;
    let before_scroll_y = ui.viewport.scroll_y_px;
    ui.scroll_x_pixels(delta_x);
    ui.scroll_y_pixels(delta_y);
    let hit_min_x = delta_x < 0.0 && before_scroll_x <= 0.0 && ui.viewport.scroll_x_px <= 0.0;
    let hit_min_y = delta_y < 0.0 && before_scroll_y <= 0.0 && ui.viewport.scroll_y_px <= 0.0;
    let before_clamp_x = ui.viewport.scroll_x_px;
    let before_clamp_y = ui.viewport.scroll_y_px;
    ui.clamp_to_table(row_count, column_count);
    if hit_min_x || (ui.viewport.scroll_x_px - before_clamp_x).abs() > f64::EPSILON {
        velocity.x_px_s = 0.0;
    }
    if hit_min_y || (ui.viewport.scroll_y_px - before_clamp_y).abs() > f64::EPSILON {
        velocity.y_px_s = 0.0;
    }

    let decay = (-SCROLL_FRICTION * dt).exp();
    velocity.x_px_s = decayed_scroll_velocity(velocity.x_px_s, decay);
    velocity.y_px_s = decayed_scroll_velocity(velocity.y_px_s, decay);
    true
}

fn decayed_scroll_velocity(velocity_px_s: f64, decay: f64) -> f64 {
    let velocity = velocity_px_s * decay;
    if velocity.abs() < MIN_SCROLL_VELOCITY {
        0.0
    } else {
        velocity
    }
}

fn stop_scroll_inertia(state: &mut AppState) {
    state.scroll_velocity_x_px_s = 0.0;
    state.scroll_velocity_y_px_s = 0.0;
    state.last_frame_time = Instant::now();
}

fn scroll_inertia_active(state: &AppState) -> bool {
    scroll_velocity_active(ScrollVelocity {
        x_px_s: state.scroll_velocity_x_px_s,
        y_px_s: state.scroll_velocity_y_px_s,
    })
}

fn selection_autoscroll_active(state: &AppState) -> bool {
    state
        .selection_drag
        .is_some_and(selection_drag_velocity_active)
}

fn selection_drag_velocity_active(drag: SelectionDrag) -> bool {
    scroll_velocity_active(ScrollVelocity {
        x_px_s: drag.auto_scroll_x_px_s,
        y_px_s: drag.auto_scroll_y_px_s,
    })
}

fn scroll_velocity_active(velocity: ScrollVelocity) -> bool {
    velocity.x_px_s.abs() >= MIN_SCROLL_VELOCITY || velocity.y_px_s.abs() >= MIN_SCROLL_VELOCITY
}

fn visible_query_window_changed(
    before: Option<&VisibleWindow>,
    after: Option<&VisibleWindow>,
) -> bool {
    before.is_none_or(|before| {
        after.is_none_or(|after| {
            before.start_row != after.start_row
                || before.row_count != after.row_count
                || before.start_column != after.start_column
                || before.column_count != after.column_count
        })
    })
}

fn viewport_needs_query_after_scroll(ui: &UiState, before: Option<&VisibleWindow>) -> bool {
    visible_query_window_changed(before, ui.viewport.visible_window().ok().as_ref())
}

fn handle_zoom_shortcut(state: &mut AppState, key: &Key) -> bool {
    let zoom_in = matches!(
        key,
        Key::Character(character) if character == "=" || character == "+"
    );
    let zoom_out = matches!(key, Key::Character(character) if character == "-");
    let zoom_reset = matches!(key, Key::Character(character) if character == "0");

    let Some(position) = state.cursor_position else {
        return false;
    };
    let cursor_x = position.x;
    let cursor_y = position.y - f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT);
    let old_zoom = state.ui.table_zoom();
    let next_zoom = if zoom_in {
        Some(old_zoom * GRID_ZOOM_STEP)
    } else if zoom_out {
        Some(old_zoom / GRID_ZOOM_STEP)
    } else if zoom_reset {
        Some(1.0)
    } else {
        None
    };

    let Some(next_zoom) = next_zoom else {
        return false;
    };
    if !state.ui.set_table_zoom_anchored(
        next_zoom,
        state.renderer.scale_factor(),
        cursor_x,
        cursor_y,
    ) {
        return false;
    }
    true
}

fn handle_select_all_shortcut(state: &mut AppState, key: &Key) -> bool {
    let shortcut = state.modifiers.super_key() || state.modifiers.control_key();
    let is_select_all =
        matches!(key, Key::Character(character) if character.eq_ignore_ascii_case("a"));
    if shortcut && is_select_all {
        apply_selection_change(state, UiState::select_all);
        return true;
    }
    false
}

fn handle_dashboard_key(state: &mut AppState, key: &Key, text: Option<&str>) -> DashboardKeyAction {
    if state.dashboard.search_focused {
        return handle_dashboard_search_key(state, key, text);
    }

    if !dashboard_shortcut_modifier_active(state) && dashboard_search_focus_key(key, text) {
        state.dashboard.search_focused = true;
        return DashboardKeyAction::Redraw;
    }

    DashboardKeyAction::Ignored
}

fn handle_dashboard_search_key(
    state: &mut AppState,
    key: &Key,
    text: Option<&str>,
) -> DashboardKeyAction {
    match key {
        Key::Named(NamedKey::Escape) => {
            state.dashboard.search_query.clear();
            state.dashboard.search_focused = false;
            return DashboardKeyAction::Redraw;
        }
        Key::Named(NamedKey::Enter) => return DashboardKeyAction::OpenFirstVisibleWorkbook,
        Key::Named(NamedKey::Backspace) => {
            state.dashboard.search_query.pop();
            return DashboardKeyAction::Redraw;
        }
        _ => {}
    }

    if dashboard_shortcut_modifier_active(state) {
        return DashboardKeyAction::Ignored;
    }

    let appended = if let Some(text) = text {
        append_dashboard_search_text(state, text)
    } else if let Key::Character(character) = key {
        append_dashboard_search_text(state, character)
    } else {
        false
    };
    if appended {
        DashboardKeyAction::Redraw
    } else {
        DashboardKeyAction::Ignored
    }
}

fn append_dashboard_search_text(state: &mut AppState, text: &str) -> bool {
    let clean = text
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    if clean.is_empty() {
        return false;
    }
    state.dashboard.search_query.push_str(&clean);
    true
}

fn dashboard_search_focus_key(key: &Key, text: Option<&str>) -> bool {
    matches!(key, Key::Character(character) if character == "/") || text == Some("/")
}

fn dashboard_shortcut_modifier_active(state: &AppState) -> bool {
    state.modifiers.super_key() || state.modifiers.control_key()
}

fn wheel_zoom_delta(delta: &MouseScrollDelta) -> f64 {
    match delta {
        MouseScrollDelta::LineDelta(_, vertical) => f64::from(*vertical),
        MouseScrollDelta::PixelDelta(position) => position.y / 120.0,
    }
}

fn render_primitives(state: &AppState) -> Vec<DrawPrimitive> {
    if state.screen == AppScreen::Dashboard {
        return vec![DrawPrimitive::DashboardFrame(Box::new(dashboard_frame(
            state,
        )))];
    }
    let Ok(frame) = state.ui.grid_frame(
        selection_animation_frame(state, Instant::now()),
        column_resize_animation_frame(state),
        row_resize_animation_frame(state),
        editor_caret_animation_frame(state, Instant::now()),
    ) else {
        return Vec::new();
    };
    let mut primitives = vec![DrawPrimitive::GridFrame(Box::new(frame))];
    if let Some(tooltip) = column_header_tooltip(state) {
        primitives.push(tooltip);
    }
    if let Some(tooltip) = view_control_tooltip(state) {
        primitives.push(tooltip);
    }
    primitives
}

fn dashboard_frame(state: &AppState) -> DashboardFrame {
    DashboardFrame {
        active_section: state.dashboard.active_section,
        workbooks: state.dashboard.workbooks.clone(),
        search_query: state.dashboard.search_query.clone(),
        search_focused: state.dashboard.search_focused,
        hovered: state.dashboard.hovered.clone(),
        status: state.dashboard.status.clone(),
    }
}

fn column_header_tooltip(state: &AppState) -> Option<DrawPrimitive> {
    let column_index = state.hovered_column_header?;
    let table = state.active_table.as_ref()?;
    let text = table.columns.get(column_index as usize)?;
    if text.chars().count() <= COLUMN_TOOLTIP_MIN_CHARS {
        return None;
    }

    let position = state.cursor_position?;
    let scale_factor = state.renderer.scale_factor().max(1.0);
    let size = state.renderer.size();
    let width = column_tooltip_width(text);
    let logical_width = f64::from(size.width) / scale_factor;
    let logical_height = f64::from(size.height) / scale_factor;
    let x = (position.x / scale_factor + COLUMN_TOOLTIP_POINTER_OFFSET_X)
        .min((logical_width - f64::from(width) - COLUMN_TOOLTIP_MARGIN).max(COLUMN_TOOLTIP_MARGIN));
    let y = (position.y / scale_factor + COLUMN_TOOLTIP_POINTER_OFFSET_Y)
        .min((logical_height - 48.0).max(COLUMN_TOOLTIP_MARGIN));

    Some(DrawPrimitive::Tooltip {
        x: x as f32,
        y: y as f32,
        width,
        text: text.clone(),
    })
}

fn column_tooltip_width(text: &str) -> f32 {
    (text.chars().count() as f32 * 7.2 + 24.0).clamp(180.0, 520.0)
}

fn view_control_tooltip(state: &AppState) -> Option<DrawPrimitive> {
    let (x, width, text) = match state.ui.chrome.hovered.as_ref()? {
        ChromeHoverTarget::SortButton => (
            SORT_BUTTON_LEFT as f32,
            250.0,
            "Cycle sort for the selected column",
        ),
        ChromeHoverTarget::FilterButton => (
            FILTER_BUTTON_LEFT as f32,
            310.0,
            "Filter the selected column to the active cell value",
        ),
        ChromeHoverTarget::ClearViewButton => (
            CLEAR_VIEW_BUTTON_LEFT as f32,
            190.0,
            "Clear active sorts and filters",
        ),
        _ => return None,
    };
    Some(DrawPrimitive::Tooltip {
        x,
        y: 46.0,
        width,
        text: text.to_string(),
    })
}

fn refresh_hover_state(state: &mut AppState) {
    if state.screen == AppScreen::Dashboard {
        let hover_target = state
            .cursor_position
            .and_then(|position| dashboard_hit(state, position));
        let cursor = match hover_target {
            Some(DashboardHoverTarget::Search) => CursorIcon::Text,
            Some(_) => CursorIcon::Pointer,
            None => CursorIcon::Default,
        };
        state.renderer.window().set_cursor(cursor);
        if state.dashboard.hovered != hover_target {
            state.dashboard.hovered = hover_target;
            state.request_redraw();
        }
        return;
    }
    let column_header = if state.scrollbar_drag.is_some() {
        None
    } else {
        state.cursor_position.and_then(|position| {
            hovered_column_header(&state.ui, position, state.renderer.scale_factor())
        })
    };
    let hover_target = state
        .cursor_position
        .and_then(|position| hover_target(state, position));
    let cursor = cursor_icon_for_state(state, hover_target.as_ref());
    state.renderer.window().set_cursor(cursor);

    if state.hovered_column_header != column_header || state.ui.chrome.hovered != hover_target {
        state.hovered_column_header = column_header;
        state.ui.chrome.hovered = hover_target;
        state.request_redraw();
    }
}

fn dashboard_hit(
    state: &AppState,
    position: PhysicalPosition<f64>,
) -> Option<DashboardHoverTarget> {
    let scale_factor = state.renderer.scale_factor().max(1.0);
    let x = position.x / scale_factor;
    let y = position.y / scale_factor;
    let size = state.renderer.size();
    dashboard_hit_test(
        &dashboard_frame(state),
        (f64::from(size.width) / scale_factor) as f32,
        (f64::from(size.height) / scale_factor) as f32,
        x as f32,
        y as f32,
    )
}

fn hover_target(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
) -> Option<ChromeHoverTarget> {
    let scale_factor = state.renderer.scale_factor();
    let (row_count, column_count) = active_table_extents(state.active_table.as_ref());
    if let Some(drag) = state.scrollbar_drag {
        return Some(match drag.axis {
            ScrollAxis::Vertical => ChromeHoverTarget::VerticalScrollbar,
            ScrollAxis::Horizontal => ChromeHoverTarget::HorizontalScrollbar,
        });
    }
    if let Some(drag) = state.selection_drag {
        return Some(match drag.target {
            SelectionTargetKind::Cell => {
                ChromeHoverTarget::GridCell(state.ui.selection.active.clone())
            }
            SelectionTargetKind::Row => ChromeHoverTarget::RowHeader(state.ui.selection.active.row),
            SelectionTargetKind::Column => {
                ChromeHoverTarget::ColumnHeader(state.ui.selection.active.column)
            }
        });
    }
    if let Some(resize) = resize_edge_hover(&state.ui, position, scale_factor) {
        return Some(resize);
    }
    if let Some(hit) = scrollbar_hit(&state.ui, position, scale_factor, row_count, column_count) {
        return Some(match hit {
            ScrollbarHit::Thumb(drag) => match drag.axis {
                ScrollAxis::Vertical => ChromeHoverTarget::VerticalScrollbar,
                ScrollAxis::Horizontal => ChromeHoverTarget::HorizontalScrollbar,
            },
            ScrollbarHit::Track { axis, .. } => match axis {
                ScrollAxis::Vertical => ChromeHoverTarget::VerticalScrollbar,
                ScrollAxis::Horizontal => ChromeHoverTarget::HorizontalScrollbar,
            },
        });
    }
    if position_is_over_open_button(position, scale_factor) {
        return Some(ChromeHoverTarget::OpenButton);
    }
    if position_is_over_rect(
        position,
        scale_factor,
        SORT_BUTTON_LEFT,
        ACTION_BUTTON_TOP,
        SORT_BUTTON_RIGHT,
        ACTION_BUTTON_BOTTOM,
    ) {
        return Some(ChromeHoverTarget::SortButton);
    }
    if position_is_over_rect(
        position,
        scale_factor,
        FILTER_BUTTON_LEFT,
        ACTION_BUTTON_TOP,
        FILTER_BUTTON_RIGHT,
        ACTION_BUTTON_BOTTOM,
    ) {
        return Some(ChromeHoverTarget::FilterButton);
    }
    if position_is_over_rect(
        position,
        scale_factor,
        CLEAR_VIEW_BUTTON_LEFT,
        ACTION_BUTTON_TOP,
        CLEAR_VIEW_BUTTON_RIGHT,
        ACTION_BUTTON_BOTTOM,
    ) {
        return Some(ChromeHoverTarget::ClearViewButton);
    }
    if position_is_over_formula_bar(position, state) {
        return Some(ChromeHoverTarget::FormulaBar);
    }
    if position_is_over_sheet_tab(position, state) {
        return Some(ChromeHoverTarget::SheetTab);
    }
    grid_hit(&state.ui, position, scale_factor).map(|hit| match hit {
        GridHit::Cell(cell) => ChromeHoverTarget::GridCell(cell),
        GridHit::Row(row) => ChromeHoverTarget::RowHeader(row),
        GridHit::Column(column) => ChromeHoverTarget::ColumnHeader(column),
        GridHit::Corner => ChromeHoverTarget::SelectAllCorner,
    })
}

fn cursor_icon_for_state(state: &mut AppState, hover: Option<&ChromeHoverTarget>) -> CursorIcon {
    if state.canvas_pan_drag.is_some() {
        return CursorIcon::Grabbing;
    }
    if state.column_resize_drag.is_some() {
        return CursorIcon::ColResize;
    }
    if let Some(drag) = state.scrollbar_drag {
        return match drag.axis {
            ScrollAxis::Vertical => CursorIcon::RowResize,
            ScrollAxis::Horizontal => CursorIcon::ColResize,
        };
    }
    if state
        .cursor_position
        .is_some_and(|position| position_is_over_active_editor(state, position))
    {
        return CursorIcon::Text;
    }
    if state.selection_drag.is_some() {
        return CursorIcon::Cell;
    }
    cursor_icon_for_state_ref(None, hover)
}

fn cursor_icon_for_state_ref(
    active_drag_axis: Option<ScrollAxis>,
    hover: Option<&ChromeHoverTarget>,
) -> CursorIcon {
    if let Some(axis) = active_drag_axis {
        return match axis {
            ScrollAxis::Vertical => CursorIcon::RowResize,
            ScrollAxis::Horizontal => CursorIcon::ColResize,
        };
    }
    match hover {
        Some(
            ChromeHoverTarget::OpenButton
            | ChromeHoverTarget::SortButton
            | ChromeHoverTarget::FilterButton
            | ChromeHoverTarget::ClearViewButton,
        )
        | Some(ChromeHoverTarget::SheetTab)
        | Some(ChromeHoverTarget::VerticalScrollbar | ChromeHoverTarget::HorizontalScrollbar)
        | Some(ChromeHoverTarget::SelectAllCorner) => CursorIcon::Pointer,
        Some(ChromeHoverTarget::FormulaBar) => CursorIcon::Text,
        Some(ChromeHoverTarget::ColumnResize(_)) => CursorIcon::ColResize,
        Some(ChromeHoverTarget::RowResize(_)) => CursorIcon::RowResize,
        Some(ChromeHoverTarget::GridCell(_)) => CursorIcon::Cell,
        Some(ChromeHoverTarget::RowHeader(_) | ChromeHoverTarget::ColumnHeader(_)) => {
            CursorIcon::Pointer
        }
        None => CursorIcon::Default,
    }
}

fn hovered_column_header(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<u32> {
    let (local_x, local_y) = grid_local_position(position, scale_factor);
    let metrics = &ui.viewport.metrics;
    let header_height = f64::from(metrics.header_height);
    let header_width = f64::from(metrics.header_width);
    let body_right = f64::from(
        ui.viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    if local_y < 0.0 || local_y > header_height || local_x < header_width || local_x >= body_right {
        return None;
    }

    let window = ui.viewport.visible_window().ok()?;
    let column_offset = visible_column_at_x(&window, local_x - header_width)?;
    Some(window.start_column.saturating_add(column_offset))
}

fn scrollbar_hit(
    ui: &UiState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
    row_count: Option<u64>,
    column_count: usize,
) -> Option<ScrollbarHit> {
    let (local_x, local_y) = grid_local_position(position, scale_factor);
    if let Some(layout) = ui.viewport.vertical_scrollbar(row_count)
        && rect_contains(
            local_x,
            local_y,
            layout.track_x,
            layout.track_y,
            layout.track_width,
            layout.track_height,
        )
    {
        return Some(scrollbar_layout_hit(layout, local_y));
    }
    if let Some(layout) = ui.viewport.horizontal_scrollbar(column_count)
        && rect_contains(
            local_x,
            local_y,
            layout.track_x,
            layout.track_y,
            layout.track_width,
            layout.track_height,
        )
    {
        return Some(scrollbar_layout_hit(layout, local_x));
    }
    None
}

fn scrollbar_layout_hit(layout: ScrollbarLayout, local_position_px: f64) -> ScrollbarHit {
    let thumb_start = layout.thumb_axis_start();
    let thumb_end = thumb_start + layout.thumb_axis_length();
    if local_position_px >= thumb_start && local_position_px <= thumb_end {
        ScrollbarHit::Thumb(ScrollbarDrag {
            axis: layout.axis,
            pointer_offset_px: local_position_px - thumb_start,
        })
    } else {
        ScrollbarHit::Track {
            axis: layout.axis,
            local_position_px,
        }
    }
}

fn apply_scrollbar_drag(
    ui: &mut UiState,
    drag: ScrollbarDrag,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
    row_count: Option<u64>,
    column_count: usize,
) -> bool {
    let (local_x, local_y) = grid_local_position(position, scale_factor);
    let Some(layout) = scrollbar_layout_for_axis(&ui.viewport, drag.axis, row_count, column_count)
    else {
        return false;
    };
    let track_travel = layout.track_axis_length() - layout.thumb_axis_length();
    if track_travel <= 0.0 || layout.max_scroll_px <= 0.0 {
        return false;
    }
    let pointer_axis_px = match drag.axis {
        ScrollAxis::Vertical => local_y,
        ScrollAxis::Horizontal => local_x,
    };
    let thumb_start = (pointer_axis_px - drag.pointer_offset_px).clamp(0.0, track_travel);
    let scroll_px = layout.max_scroll_px * thumb_start / track_travel;
    match drag.axis {
        ScrollAxis::Vertical => {
            if (ui.viewport.scroll_y_px - scroll_px).abs() < f64::EPSILON {
                false
            } else {
                ui.viewport.scroll_y_px = scroll_px;
                true
            }
        }
        ScrollAxis::Horizontal => {
            if (ui.viewport.scroll_x_px - scroll_px).abs() < f64::EPSILON {
                false
            } else {
                ui.viewport.scroll_x_px = scroll_px;
                true
            }
        }
    }
}

fn page_scrollbar_track(
    ui: &mut UiState,
    axis: ScrollAxis,
    local_position_px: f64,
    row_count: Option<u64>,
    column_count: usize,
) {
    let Some(layout) = scrollbar_layout_for_axis(&ui.viewport, axis, row_count, column_count)
    else {
        return;
    };
    let delta = if local_position_px < layout.thumb_axis_start() {
        -page_scroll_delta(&ui.viewport, axis)
    } else {
        page_scroll_delta(&ui.viewport, axis)
    };
    match axis {
        ScrollAxis::Vertical => ui.scroll_y_pixels(delta as f64),
        ScrollAxis::Horizontal => ui.scroll_x_pixels(delta as f64),
    }
}

fn scrollbar_layout_for_axis(
    viewport: &cellium_ui::GridViewport,
    axis: ScrollAxis,
    row_count: Option<u64>,
    column_count: usize,
) -> Option<ScrollbarLayout> {
    match axis {
        ScrollAxis::Vertical => viewport.vertical_scrollbar(row_count),
        ScrollAxis::Horizontal => viewport.horizontal_scrollbar(column_count),
    }
}

fn page_scroll_delta(viewport: &cellium_ui::GridViewport, axis: ScrollAxis) -> i64 {
    match axis {
        ScrollAxis::Vertical => i64::from(viewport.body_height()),
        ScrollAxis::Horizontal => i64::from(viewport.body_width()),
    }
}

fn grid_local_position(position: PhysicalPosition<f64>, scale_factor: f64) -> (f64, f64) {
    (
        position.x,
        position.y - f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT) * scale_factor.max(1.0),
    )
}

fn rect_contains(
    x: f64,
    y: f64,
    rect_x: f64,
    rect_y: f64,
    rect_width: f64,
    rect_height: f64,
) -> bool {
    x >= rect_x && x <= rect_x + rect_width && y >= rect_y && y <= rect_y + rect_height
}

fn snapshot_covers_window(snapshot: &GridSnapshot, window: &VisibleWindow) -> bool {
    let snapshot_start = snapshot.start_row;
    let snapshot_end = snapshot_start.saturating_add(snapshot.rows.len() as u64);
    let window_end = window.start_row.saturating_add(u64::from(window.row_count));
    let snapshot_column_start = snapshot.start_column;
    let snapshot_column_end = snapshot_column_start.saturating_add(snapshot_row_width(snapshot));
    let window_column_end = window.start_column.saturating_add(window.column_count);
    snapshot_start <= window.start_row
        && snapshot_end >= window_end
        && snapshot_column_start <= window.start_column
        && snapshot_column_end >= window_column_end
}

fn snapshot_should_replace_current(
    snapshot: &GridSnapshot,
    current_window: Option<&VisibleWindow>,
    generation: u64,
    latest_generation: u64,
) -> bool {
    generation == latest_generation
        || current_window.is_some_and(|window| snapshot_covers_window(snapshot, window))
}

fn snapshot_has_prerender_margin(snapshot: &GridSnapshot, window: &VisibleWindow) -> bool {
    if !snapshot_covers_window(snapshot, window) {
        return false;
    }

    let snapshot_end = snapshot
        .start_row
        .saturating_add(snapshot.rows.len() as u64);
    let window_end = window.start_row.saturating_add(u64::from(window.row_count));
    let snapshot_column_end = snapshot
        .start_column
        .saturating_add(snapshot_row_width(snapshot));
    let window_column_end = window.start_column.saturating_add(window.column_count);

    let has_row_headroom = window.start_row == 0
        || window.start_row.saturating_sub(snapshot.start_row) >= ROW_PREFETCH_MARGIN;
    let has_row_tailroom = snapshot_end.saturating_sub(window_end) >= ROW_PREFETCH_MARGIN;
    let has_column_headroom = window.start_column == 0
        || window.start_column.saturating_sub(snapshot.start_column) >= COLUMN_PREFETCH_MARGIN;
    let has_column_tailroom =
        snapshot_column_end.saturating_sub(window_column_end) >= COLUMN_PREFETCH_MARGIN;

    has_row_headroom && has_row_tailroom && has_column_headroom && has_column_tailroom
}

fn snapshot_row_width(snapshot: &GridSnapshot) -> u32 {
    snapshot
        .rows
        .first()
        .map_or(0, Vec::len)
        .min(u32::MAX as usize) as u32
}

#[cfg(test)]
mod tests;
