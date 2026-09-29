use std::collections::BTreeMap;

use cellium_core::CellRef;
use cellium_core::TableViewState;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const GRID_ROW_HEIGHT: u32 = 28;
pub const GRID_COLUMN_WIDTH: u32 = 162;
pub const GRID_HEADER_HEIGHT: u32 = 32;
pub const GRID_HEADER_WIDTH: u32 = 58;
pub const GRID_CELL_FONT_SIZE: f32 = 14.0;
pub const GRID_CELL_TEXT_HEIGHT: f32 = 20.0;
pub const GRID_SCROLLBAR_THICKNESS: u32 = 12;
pub const GRID_SCROLLBAR_MIN_THUMB: u32 = 28;
pub const GRID_MIN_ZOOM: f64 = 0.55;
pub const GRID_MAX_ZOOM: f64 = 2.25;
pub const GRID_ZOOM_STEP: f64 = 1.12;
pub const FORMULA_BAR_TOP: u32 = 53;
pub const FORMULA_BAR_HEIGHT: u32 = 44;
pub const SHEET_TAB_HEIGHT: u32 = 36;
pub const WORKSHEET_CHROME_HEIGHT: u32 = FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + SHEET_TAB_HEIGHT;
pub(super) const MIN_GRID_HEIGHT: u32 = 120;
pub(super) const EDIT_HISTORY_LIMIT: usize = 256;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChromeState {
    pub hovered: Option<ChromeHoverTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChromeHoverTarget {
    OpenButton,
    FormulaBar,
    SortButton,
    FilterButton,
    ClearViewButton,
    SheetTab,
    VerticalScrollbar,
    HorizontalScrollbar,
    ColumnResize(u32),
    RowResize(u32),
    GridCell(CellRef),
    RowHeader(u32),
    ColumnHeader(u32),
    SelectAllCorner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DashboardSection {
    Home,
    Starred,
    Workbooks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DashboardHoverTarget {
    NewWorkbook,
    CreateBlankWorkbook,
    FileUpload,
    Search,
    Sidebar(DashboardSection),
    WorkbookRow(u64),
    ViewAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardWorkbook {
    pub id: u64,
    pub name: String,
    pub file_path: String,
    pub updated_label: String,
    pub starred: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardFrame {
    pub active_section: DashboardSection,
    pub workbooks: Vec<DashboardWorkbook>,
    pub search_query: String,
    pub search_focused: bool,
    pub hovered: Option<DashboardHoverTarget>,
    pub status: String,
}

#[must_use]
pub fn dashboard_visible_workbooks<'a>(
    workbooks: &'a [DashboardWorkbook],
    section: DashboardSection,
    search_query: &str,
) -> Vec<&'a DashboardWorkbook> {
    let query = search_query.trim().to_lowercase();
    workbooks
        .iter()
        .filter(|workbook| {
            (section != DashboardSection::Starred || workbook.starred)
                && (query.is_empty() || dashboard_workbook_matches_search(workbook, &query))
        })
        .collect()
}

#[must_use]
pub fn dashboard_first_visible_workbook_id(
    workbooks: &[DashboardWorkbook],
    section: DashboardSection,
    search_query: &str,
) -> Option<u64> {
    dashboard_visible_workbooks(workbooks, section, search_query)
        .first()
        .map(|workbook| workbook.id)
}

#[must_use]
pub fn dashboard_workbook_matches_search(
    workbook: &DashboardWorkbook,
    normalized_query: &str,
) -> bool {
    workbook.name.to_lowercase().contains(normalized_query)
        || workbook.file_path.to_lowercase().contains(normalized_query)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UiError {
    #[error("grid has no visible rows or columns")]
    EmptyViewport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusTarget {
    Grid,
    FormulaBar,
    Dialog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiEditMode {
    Navigating,
    EditingFormulaBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditMode {
    Overwrite,
    InPlace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSelection {
    pub anchor: usize,
    pub caret: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMove {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorDelete {
    Backward,
    Forward,
    WordBackward,
    WordForward,
    ToLineStart,
    ToLineEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorIntent {
    InsertText(String),
    Move {
        movement: EditorMove,
        extend: bool,
    },
    Delete(EditorDelete),
    SelectAll,
    Copy,
    Cut,
    Paste(String),
    Undo,
    Redo,
    SetCaret {
        grapheme_offset: usize,
        extend: bool,
    },
    SetSelection {
        anchor_grapheme_offset: usize,
        caret_grapheme_offset: usize,
    },
    SelectWordAt {
        grapheme_offset: usize,
    },
    SelectWordRange {
        anchor_start_grapheme_offset: usize,
        anchor_end_grapheme_offset: usize,
        caret_grapheme_offset: usize,
    },
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EditorIntentResult {
    pub changed: bool,
    pub copied_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridEditState {
    Idle,
    Selected {
        cell: CellRef,
    },
    Editing {
        cell: CellRef,
        mode: EditMode,
        buffer: String,
        selection: TextSelection,
        original: String,
    },
}

impl GridEditState {
    #[must_use]
    pub fn selected_cell(&self) -> Option<&CellRef> {
        match self {
            Self::Selected { cell } | Self::Editing { cell, .. } => Some(cell),
            Self::Idle => None,
        }
    }

    #[must_use]
    pub fn editing_cell(&self) -> Option<&CellRef> {
        match self {
            Self::Editing { cell, .. } => Some(cell),
            Self::Idle | Self::Selected { .. } => None,
        }
    }

    #[must_use]
    pub fn edit_mode(&self) -> Option<EditMode> {
        match self {
            Self::Editing { mode, .. } => Some(*mode),
            Self::Idle | Self::Selected { .. } => None,
        }
    }

    #[must_use]
    pub fn editing_buffer(&self) -> Option<&str> {
        match self {
            Self::Editing { buffer, .. } => Some(buffer.as_str()),
            Self::Idle | Self::Selected { .. } => None,
        }
    }

    #[must_use]
    pub fn selection(&self) -> Option<TextSelection> {
        match self {
            Self::Editing { selection, .. } => Some(*selection),
            Self::Idle | Self::Selected { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct TextEditSnapshot {
    pub(super) buffer: String,
    pub(super) selection: TextSelection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub anchor: SelectionAnchor,
    pub active: CellRef,
    pub ranges: Vec<SelectionRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionAnchor {
    Cell(CellRef),
    Row(u32),
    Column(u32),
    Sheet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionRange {
    Cells { start: CellRef, end: CellRef },
    Rows { start: u32, end: u32 },
    Columns { start: u32, end: u32 },
    Sheet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelectionAnimation {
    pub from: SelectionRange,
    pub to: SelectionRange,
    pub progress: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnResizeAnimation {
    pub column: u32,
    pub visual_column_width_px: f32,
    pub separator_x_px: f32,
    pub show_separator: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RowResizeAnimation {
    pub row: u64,
    pub visual_row_height_px: f32,
    pub separator_y_px: f32,
    pub show_separator: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorCaretAnimation {
    pub from_buffer: String,
    pub from_caret: usize,
    pub to_caret: usize,
    pub progress: f32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SelectionAction {
    pub extend: bool,
    pub additive: bool,
}

impl Selection {
    #[must_use]
    pub fn single(cell: CellRef) -> Self {
        Self {
            anchor: SelectionAnchor::Cell(cell.clone()),
            active: cell.clone(),
            ranges: vec![SelectionRange::Cells {
                start: cell.clone(),
                end: cell,
            }],
        }
    }

    pub(super) fn replace(
        &mut self,
        anchor: SelectionAnchor,
        active: CellRef,
        range: SelectionRange,
    ) {
        self.anchor = anchor;
        self.active = active;
        self.ranges.clear();
        self.ranges.push(range);
    }

    pub(super) fn push(&mut self, anchor: SelectionAnchor, active: CellRef, range: SelectionRange) {
        self.anchor = anchor;
        self.active = active;
        self.ranges.push(range);
    }

    pub(super) fn update_current(&mut self, active: CellRef, range: SelectionRange) {
        self.active = active;
        if let Some(current) = self.ranges.last_mut() {
            *current = range;
        } else {
            self.ranges.push(range);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridMetrics {
    pub row_height: u32,
    pub column_width: u32,
    pub header_height: u32,
    pub header_width: u32,
    pub scrollbar_thickness: u32,
    pub scrollbar_min_thumb: u32,
    pub zoom: f64,
}

impl Default for GridMetrics {
    fn default() -> Self {
        Self {
            row_height: GRID_ROW_HEIGHT,
            column_width: GRID_COLUMN_WIDTH,
            header_height: GRID_HEADER_HEIGHT,
            header_width: GRID_HEADER_WIDTH,
            scrollbar_thickness: GRID_SCROLLBAR_THICKNESS,
            scrollbar_min_thumb: GRID_SCROLLBAR_MIN_THUMB,
            zoom: 1.0,
        }
    }
}

impl GridMetrics {
    #[must_use]
    pub fn for_scale_factor(scale_factor: f64, zoom: f64) -> Self {
        Self::for_scale_factor_with_base_column_width(scale_factor, zoom, GRID_COLUMN_WIDTH)
    }

    #[must_use]
    pub fn for_scale_factor_with_base_column_width(
        scale_factor: f64,
        zoom: f64,
        base_column_width: u32,
    ) -> Self {
        let zoom = normalized_zoom(zoom);
        let table_scale = scale_factor.max(1.0) * zoom;
        Self {
            row_height: scaled_dimension(GRID_ROW_HEIGHT, table_scale),
            column_width: scaled_dimension(base_column_width, table_scale),
            header_height: scaled_dimension(GRID_HEADER_HEIGHT, table_scale),
            header_width: scaled_dimension(GRID_HEADER_WIDTH, table_scale),
            scrollbar_thickness: scaled_dimension(GRID_SCROLLBAR_THICKNESS, scale_factor),
            scrollbar_min_thumb: scaled_dimension(GRID_SCROLLBAR_MIN_THUMB, scale_factor),
            zoom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScrollbarLayout {
    pub axis: ScrollAxis,
    pub track_x: f64,
    pub track_y: f64,
    pub track_width: f64,
    pub track_height: f64,
    pub thumb_x: f64,
    pub thumb_y: f64,
    pub thumb_width: f64,
    pub thumb_height: f64,
    pub max_scroll_px: f64,
}

impl ScrollbarLayout {
    #[must_use]
    pub fn thumb_axis_start(self) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => self.thumb_y,
            ScrollAxis::Horizontal => self.thumb_x,
        }
    }

    #[must_use]
    pub fn thumb_axis_length(self) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => self.thumb_height,
            ScrollAxis::Horizontal => self.thumb_width,
        }
    }

    #[must_use]
    pub fn track_axis_length(self) -> f64 {
        match self.axis {
            ScrollAxis::Vertical => self.track_height,
            ScrollAxis::Horizontal => self.track_width,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridViewport {
    pub scroll_x_px: f64,
    pub scroll_y_px: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub metrics: GridMetrics,
    pub column_widths: BTreeMap<u32, u32>,
    pub row_heights: BTreeMap<u64, u32>,
}

impl GridViewport {
    pub fn visible_window(&self) -> Result<VisibleWindow, UiError> {
        let body_width = self.body_width();
        let body_height = self.body_height();
        if body_width == 0 || body_height == 0 {
            return Err(UiError::EmptyViewport);
        }
        let AxisWindow {
            start_index: start_row,
            count: row_count,
            offset_px: row_offset_px,
            sizes: row_heights,
        } = axis_window_u64(
            self.scroll_y_px,
            body_height,
            self.metrics.row_height,
            &self.row_heights,
        );
        let AxisWindowU32 {
            start_index: start_column,
            count: column_count,
            offset_px: column_offset_px,
            sizes: column_widths,
        } = axis_window_u32(
            self.scroll_x_px,
            body_width,
            self.metrics.column_width,
            &self.column_widths,
        );
        Ok(VisibleWindow {
            start_row,
            row_count,
            start_column,
            column_count,
            row_offset_px,
            column_offset_px,
            row_heights,
            column_widths,
        })
    }

    #[must_use]
    pub fn body_width(&self) -> u32 {
        self.pixel_width
            .saturating_sub(self.metrics.header_width)
            .saturating_sub(self.metrics.scrollbar_thickness)
    }

    #[must_use]
    pub fn body_height(&self) -> u32 {
        self.pixel_height
            .saturating_sub(self.metrics.header_height)
            .saturating_sub(self.metrics.scrollbar_thickness)
    }

    #[must_use]
    pub fn vertical_scrollbar(&self, row_count: Option<u64>) -> Option<ScrollbarLayout> {
        let row_count = row_count?;
        let body_height = self.body_height();
        scrollbar_layout(ScrollbarInput {
            axis: ScrollAxis::Vertical,
            track_x: f64::from(
                self.pixel_width
                    .saturating_sub(self.metrics.scrollbar_thickness),
            ),
            track_y: f64::from(self.metrics.header_height),
            track_width: f64::from(self.metrics.scrollbar_thickness),
            track_height: f64::from(body_height),
            viewport_length: body_height,
            content_length: self.row_content_length(row_count),
            scroll_px: self.scroll_y_px,
            min_thumb: self.metrics.scrollbar_min_thumb,
        })
    }

    #[must_use]
    pub fn horizontal_scrollbar(&self, column_count: usize) -> Option<ScrollbarLayout> {
        let column_count = u64::try_from(column_count).ok()?;
        let body_width = self.body_width();
        scrollbar_layout(ScrollbarInput {
            axis: ScrollAxis::Horizontal,
            track_x: f64::from(self.metrics.header_width),
            track_y: f64::from(
                self.pixel_height
                    .saturating_sub(self.metrics.scrollbar_thickness),
            ),
            track_width: f64::from(body_width),
            track_height: f64::from(self.metrics.scrollbar_thickness),
            viewport_length: body_width,
            content_length: self.column_content_length(column_count),
            scroll_px: self.scroll_x_px,
            min_thumb: self.metrics.scrollbar_min_thumb,
        })
    }

    #[must_use]
    pub fn max_scroll_y_px(&self, row_count: Option<u64>) -> Option<f64> {
        let content_length = self.row_content_length(row_count?);
        Some((content_length - f64::from(self.body_height())).max(0.0))
    }

    #[must_use]
    pub fn max_scroll_x_px(&self, column_count: usize) -> f64 {
        let content_length = self.column_content_length(u64::try_from(column_count).unwrap_or(0));
        (content_length - f64::from(self.body_width())).max(0.0)
    }

    #[must_use]
    pub fn row_height_at(&self, row: u64) -> u32 {
        self.row_heights
            .get(&row)
            .copied()
            .unwrap_or(self.metrics.row_height)
    }

    #[must_use]
    pub fn column_width_at(&self, column: u32) -> u32 {
        self.column_widths
            .get(&column)
            .copied()
            .unwrap_or(self.metrics.column_width)
    }

    pub fn set_row_height_at(&mut self, row: u64, height: u32) {
        if height == self.metrics.row_height {
            self.row_heights.remove(&row);
        } else {
            self.row_heights.insert(row, height);
        }
    }

    pub fn set_column_width_at(&mut self, column: u32, width: u32) {
        if width == self.metrics.column_width {
            self.column_widths.remove(&column);
        } else {
            self.column_widths.insert(column, width);
        }
    }

    #[must_use]
    pub fn row_start_px(&self, row: u64) -> f64 {
        axis_start_px_u64(row, self.metrics.row_height, &self.row_heights)
    }

    #[must_use]
    pub fn column_start_px(&self, column: u32) -> f64 {
        axis_start_px_u32(column, self.metrics.column_width, &self.column_widths)
    }

    fn row_content_length(&self, row_count: u64) -> f64 {
        axis_content_length(row_count, self.metrics.row_height, &self.row_heights)
    }

    fn column_content_length(&self, column_count: u64) -> f64 {
        axis_content_length_u32(column_count, self.metrics.column_width, &self.column_widths)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct AxisWindow {
    start_index: u64,
    count: u32,
    offset_px: f64,
    sizes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
struct AxisWindowU32 {
    start_index: u32,
    count: u32,
    offset_px: f64,
    sizes: Vec<u32>,
}

fn axis_window_u64(
    scroll_px: f64,
    viewport_px: u32,
    default_size: u32,
    overrides: &impl AxisOverrides,
) -> AxisWindow {
    let default_size = default_size.max(1);
    let mut start_index = (scroll_px / f64::from(default_size)).floor().max(0.0) as u64;
    let mut start_px = axis_start_px_u64(start_index, default_size, overrides);
    while start_px > scroll_px && start_index > 0 {
        start_index = start_index.saturating_sub(1);
        start_px -= f64::from(size_at_u64(start_index, default_size, overrides));
    }
    loop {
        let size = f64::from(size_at_u64(start_index, default_size, overrides));
        if start_px + size > scroll_px || size <= 0.0 {
            break;
        }
        start_px += size;
        start_index = start_index.saturating_add(1);
    }

    let offset_px = (scroll_px - start_px).max(0.0);
    let mut sizes = Vec::new();
    let mut covered = -offset_px;
    let needed = f64::from(viewport_px);
    let mut index = start_index;
    while covered < needed + f64::from(default_size) * 2.0 {
        let size = size_at_u64(index, default_size, overrides);
        sizes.push(size);
        covered += f64::from(size);
        index = index.saturating_add(1);
        if sizes.len() >= u32::MAX as usize {
            break;
        }
    }
    let count = u32::try_from(sizes.len()).unwrap_or(u32::MAX);
    AxisWindow {
        start_index,
        count,
        offset_px,
        sizes,
    }
}

fn axis_window_u32(
    scroll_px: f64,
    viewport_px: u32,
    default_size: u32,
    overrides: &BTreeMap<u32, u32>,
) -> AxisWindowU32 {
    let column_overrides = ColumnOverrideAxis { overrides };
    let window = axis_window_u64(scroll_px, viewport_px, default_size, &column_overrides);
    AxisWindowU32 {
        start_index: u32::try_from(window.start_index).unwrap_or(u32::MAX),
        count: window.count,
        offset_px: window.offset_px,
        sizes: window.sizes,
    }
}

fn axis_start_px_u64(index: u64, default_size: u32, overrides: &impl AxisOverrides) -> f64 {
    let default = u64::from(default_size.max(1));
    let mut total = index.saturating_mul(default) as i128;
    total += overrides.size_delta_before(index, default_size);
    total.max(0) as f64
}

fn axis_start_px_u32(index: u32, default_size: u32, overrides: &BTreeMap<u32, u32>) -> f64 {
    let column_overrides = ColumnOverrideAxis { overrides };
    axis_start_px_u64(u64::from(index), default_size, &column_overrides)
}

fn axis_content_length(count: u64, default_size: u32, overrides: &impl AxisOverrides) -> f64 {
    let default = u64::from(default_size.max(1));
    let mut total = count.saturating_mul(default) as i128;
    total += overrides.size_delta_before(count, default_size);
    total.max(0) as f64
}

fn axis_content_length_u32(count: u64, default_size: u32, overrides: &BTreeMap<u32, u32>) -> f64 {
    let column_overrides = ColumnOverrideAxis { overrides };
    axis_content_length(count, default_size, &column_overrides)
}

fn size_at_u64(index: u64, default_size: u32, overrides: &impl AxisOverrides) -> u32 {
    overrides
        .size_override(index)
        .unwrap_or(default_size.max(1))
}

struct ColumnOverrideAxis<'a> {
    overrides: &'a BTreeMap<u32, u32>,
}

trait AxisOverrides {
    fn size_override(&self, index: u64) -> Option<u32>;
    fn size_delta_before(&self, index: u64, default_size: u32) -> i128;
}

impl AxisOverrides for BTreeMap<u64, u32> {
    fn size_override(&self, index: u64) -> Option<u32> {
        self.get(&index).copied()
    }

    fn size_delta_before(&self, index: u64, default_size: u32) -> i128 {
        self.range(..index)
            .map(|(_, &size)| i128::from(size) - i128::from(default_size))
            .sum()
    }
}

impl AxisOverrides for ColumnOverrideAxis<'_> {
    fn size_override(&self, index: u64) -> Option<u32> {
        let index = u32::try_from(index).ok()?;
        self.overrides.get(&index).copied()
    }

    fn size_delta_before(&self, index: u64, default_size: u32) -> i128 {
        let end = u32::try_from(index).unwrap_or(u32::MAX);
        self.overrides
            .range(..end)
            .map(|(_, &size)| i128::from(size) - i128::from(default_size))
            .sum()
    }
}

#[derive(Debug, Clone, Copy)]
struct ScrollbarInput {
    axis: ScrollAxis,
    track_x: f64,
    track_y: f64,
    track_width: f64,
    track_height: f64,
    viewport_length: u32,
    content_length: f64,
    scroll_px: f64,
    min_thumb: u32,
}

fn scrollbar_layout(input: ScrollbarInput) -> Option<ScrollbarLayout> {
    if input.viewport_length == 0 || input.content_length <= 0.0 {
        return None;
    }

    let track_length = match input.axis {
        ScrollAxis::Vertical => input.track_height,
        ScrollAxis::Horizontal => input.track_width,
    };
    if track_length <= 0.0 {
        return None;
    }

    let viewport_length = f64::from(input.viewport_length);
    let max_scroll_px = (input.content_length - viewport_length).max(0.0);
    let raw_thumb = if input.content_length <= viewport_length {
        track_length
    } else {
        track_length * viewport_length / input.content_length
    };
    let thumb_length = raw_thumb.max(f64::from(input.min_thumb)).min(track_length);
    let travel = (track_length - thumb_length).max(0.0);
    let thumb_offset = if max_scroll_px <= 0.0 || travel == 0.0 {
        0.0
    } else {
        travel * input.scroll_px.clamp(0.0, max_scroll_px) / max_scroll_px
    };

    Some(match input.axis {
        ScrollAxis::Vertical => ScrollbarLayout {
            axis: input.axis,
            track_x: input.track_x,
            track_y: input.track_y,
            track_width: input.track_width,
            track_height: input.track_height,
            thumb_x: input.track_x,
            thumb_y: input.track_y + thumb_offset,
            thumb_width: input.track_width,
            thumb_height: thumb_length,
            max_scroll_px,
        },
        ScrollAxis::Horizontal => ScrollbarLayout {
            axis: input.axis,
            track_x: input.track_x,
            track_y: input.track_y,
            track_width: input.track_width,
            track_height: input.track_height,
            thumb_x: input.track_x + thumb_offset,
            thumb_y: input.track_y,
            thumb_width: thumb_length,
            thumb_height: input.track_height,
            max_scroll_px,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisibleWindow {
    pub start_row: u64,
    pub row_count: u32,
    pub start_column: u32,
    pub column_count: u32,
    pub row_offset_px: f64,
    pub column_offset_px: f64,
    pub row_heights: Vec<u32>,
    pub column_widths: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridSnapshot {
    pub table_name: String,
    pub source_name: String,
    pub row_count: Option<u64>,
    pub start_row: u64,
    pub start_column: u32,
    pub header_row_count: u32,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub row_ids: Vec<Option<u64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridFrame {
    pub viewport: GridViewport,
    pub visible_window: VisibleWindow,
    pub selection: Selection,
    pub selection_animation: Option<SelectionAnimation>,
    pub column_resize_animation: Option<ColumnResizeAnimation>,
    pub row_resize_animation: Option<RowResizeAnimation>,
    pub snapshot: Option<GridSnapshot>,
    pub view: TableViewState,
    pub edited_cells: BTreeMap<CellRef, String>,
    pub edit_state: GridEditState,
    pub editor_caret_visible: bool,
    pub editor_caret_animation: Option<EditorCaretAnimation>,
    pub status: String,
    pub chrome: ChromeState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiCommand {
    MoveSelection { row_delta: i32, column_delta: i32 },
    BeginCellEdit,
    CommitEdit,
    CancelEdit,
}

pub(super) fn scaled_dimension(value: u32, scale_factor: f64) -> u32 {
    ((f64::from(value) * scale_factor.max(1.0)).round() as u32).max(1)
}

pub(super) fn normalized_zoom(zoom: f64) -> f64 {
    zoom.clamp(GRID_MIN_ZOOM, GRID_MAX_ZOOM)
}

pub(super) fn body_anchor_px(local_px: f64, header_px: u32, body_px: u32) -> f64 {
    (local_px - f64::from(header_px)).clamp(0.0, f64::from(body_px))
}
