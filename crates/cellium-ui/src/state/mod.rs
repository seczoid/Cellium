//! UI state, input routing, and virtual grid math.

use std::collections::BTreeMap;

pub use cellium_core::{CellRef, SortDirection};
use cellium_core::{SheetId, TableViewState};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;
use winit::keyboard::{Key, NamedKey};

mod types;

pub use types::*;
use types::{
    EDIT_HISTORY_LIMIT, MIN_GRID_HEIGHT, TextEditSnapshot, body_anchor_px, normalized_zoom,
    scaled_dimension,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub active_sheet: SheetId,
    pub focus: FocusTarget,
    pub ui_edit_mode: UiEditMode,
    pub selection: Selection,
    pub viewport: GridViewport,
    pub snapshot: Option<GridSnapshot>,
    pub table_view: TableViewState,
    pub edited_cells: BTreeMap<CellRef, String>,
    pub grid_edit_state: GridEditState,
    base_column_width: u32,
    editor_caret_visible: bool,
    editor_undo_stack: Vec<TextEditSnapshot>,
    editor_redo_stack: Vec<TextEditSnapshot>,
    pub status: String,
    pub chrome: ChromeState,
}

impl UiState {
    #[must_use]
    pub fn new(active_sheet: SheetId) -> Self {
        Self {
            active_sheet,
            focus: FocusTarget::Grid,
            ui_edit_mode: UiEditMode::Navigating,
            selection: Selection::single(CellRef::new(1, 1)),
            viewport: GridViewport {
                scroll_x_px: 0.0,
                scroll_y_px: 0.0,
                pixel_width: 960,
                pixel_height: 640,
                metrics: GridMetrics::default(),
                column_widths: BTreeMap::new(),
                row_heights: BTreeMap::new(),
            },
            snapshot: None,
            table_view: TableViewState::default(),
            edited_cells: BTreeMap::new(),
            grid_edit_state: GridEditState::Selected {
                cell: CellRef::new(1, 1),
            },
            base_column_width: types::GRID_COLUMN_WIDTH,
            editor_caret_visible: true,
            editor_undo_stack: Vec::new(),
            editor_redo_stack: Vec::new(),
            status: "Click Open or drop a CSV, Parquet, or Arrow file".to_string(),
            chrome: ChromeState::default(),
        }
    }

    pub fn resize_viewport(&mut self, window_width: u32, window_height: u32, scale_factor: f64) {
        let zoom = self.viewport.metrics.zoom;
        let old_metrics = self.viewport.metrics.clone();
        self.viewport.metrics = GridMetrics::for_scale_factor_with_base_column_width(
            scale_factor,
            zoom,
            self.base_column_width,
        );
        scale_size_overrides_u32(
            &mut self.viewport.column_widths,
            old_metrics.column_width,
            self.viewport.metrics.column_width,
        );
        scale_size_overrides_u64(
            &mut self.viewport.row_heights,
            old_metrics.row_height,
            self.viewport.metrics.row_height,
        );
        self.viewport.pixel_width = window_width.max(1);
        self.viewport.pixel_height = window_height
            .saturating_sub(scaled_dimension(WORKSHEET_CHROME_HEIGHT, scale_factor))
            .max(scaled_dimension(MIN_GRID_HEIGHT, scale_factor));
    }

    pub fn set_table_zoom(&mut self, zoom: f64, scale_factor: f64) -> bool {
        let anchor_x = f64::from(self.viewport.metrics.header_width)
            + f64::from(self.viewport.body_width()) / 2.0;
        let anchor_y = f64::from(self.viewport.metrics.header_height)
            + f64::from(self.viewport.body_height()) / 2.0;
        self.set_table_zoom_anchored(zoom, scale_factor, anchor_x, anchor_y)
    }

    pub fn set_table_zoom_anchored(
        &mut self,
        zoom: f64,
        scale_factor: f64,
        anchor_x_px: f64,
        anchor_y_px: f64,
    ) -> bool {
        let zoom = normalized_zoom(zoom);
        let old_metrics = self.viewport.metrics.clone();
        if (old_metrics.zoom - zoom).abs() < f64::EPSILON {
            return false;
        }

        let old_anchor_x = body_anchor_px(
            anchor_x_px,
            old_metrics.header_width,
            self.viewport
                .pixel_width
                .saturating_sub(old_metrics.header_width)
                .saturating_sub(old_metrics.scrollbar_thickness),
        );
        let old_anchor_y = body_anchor_px(
            anchor_y_px,
            old_metrics.header_height,
            self.viewport
                .pixel_height
                .saturating_sub(old_metrics.header_height)
                .saturating_sub(old_metrics.scrollbar_thickness),
        );
        let content_x = self.viewport.scroll_x_px + old_anchor_x;
        let content_y = self.viewport.scroll_y_px + old_anchor_y;

        self.viewport.metrics = GridMetrics::for_scale_factor_with_base_column_width(
            scale_factor,
            zoom,
            self.base_column_width,
        );
        let column_scale =
            f64::from(self.viewport.metrics.column_width) / f64::from(old_metrics.column_width);
        let row_scale =
            f64::from(self.viewport.metrics.row_height) / f64::from(old_metrics.row_height);
        scale_size_overrides_u32(
            &mut self.viewport.column_widths,
            old_metrics.column_width,
            self.viewport.metrics.column_width,
        );
        scale_size_overrides_u64(
            &mut self.viewport.row_heights,
            old_metrics.row_height,
            self.viewport.metrics.row_height,
        );

        let new_anchor_x = body_anchor_px(
            anchor_x_px,
            self.viewport.metrics.header_width,
            self.viewport.body_width(),
        );
        let new_anchor_y = body_anchor_px(
            anchor_y_px,
            self.viewport.metrics.header_height,
            self.viewport.body_height(),
        );
        self.viewport.scroll_x_px = (content_x * column_scale - new_anchor_x).max(0.0).round();
        self.viewport.scroll_y_px = (content_y * row_scale - new_anchor_y).max(0.0).round();
        true
    }

    pub fn set_column_width_px(&mut self, column: u32, target_width_px: f64) -> bool {
        if !target_width_px.is_finite() {
            return false;
        }
        let target_width_px = target_width_px.round().max(1.0);
        let old_column_width = f64::from(self.viewport.column_width_at(column)).max(1.0);
        if (old_column_width - target_width_px).abs() < f64::EPSILON {
            return false;
        }
        let column_start_px = self.viewport.column_start_px(column);
        let delta = target_width_px - old_column_width;
        self.viewport
            .set_column_width_at(column, target_width_px as u32);
        if column_start_px < self.viewport.scroll_x_px {
            self.viewport.scroll_x_px = (self.viewport.scroll_x_px + delta).max(0.0);
        }
        true
    }

    pub fn set_row_height_px(&mut self, row: u64, target_height_px: f64) -> bool {
        if !target_height_px.is_finite() {
            return false;
        }
        let target_height_px = target_height_px.round().max(1.0);
        let old_row_height = f64::from(self.viewport.row_height_at(row)).max(1.0);
        if (old_row_height - target_height_px).abs() < f64::EPSILON {
            return false;
        }
        let row_start_px = self.viewport.row_start_px(row);
        let delta = target_height_px - old_row_height;
        self.viewport
            .set_row_height_at(row, target_height_px as u32);
        if row_start_px < self.viewport.scroll_y_px {
            self.viewport.scroll_y_px = (self.viewport.scroll_y_px + delta).max(0.0);
        }
        true
    }

    #[must_use]
    pub fn table_zoom(&self) -> f64 {
        self.viewport.metrics.zoom
    }

    pub fn grid_frame(
        &self,
        selection_animation: Option<SelectionAnimation>,
        column_resize_animation: Option<ColumnResizeAnimation>,
        row_resize_animation: Option<RowResizeAnimation>,
        editor_caret_animation: Option<EditorCaretAnimation>,
    ) -> Result<GridFrame, UiError> {
        Ok(GridFrame {
            viewport: self.viewport.clone(),
            visible_window: self.viewport.visible_window()?,
            selection: self.selection.clone(),
            selection_animation,
            column_resize_animation,
            row_resize_animation,
            snapshot: self.snapshot.clone(),
            view: self.table_view.clone(),
            edited_cells: self.edited_cells.clone(),
            edit_state: self.grid_edit_state.clone(),
            editor_caret_visible: self.editor_caret_visible,
            editor_caret_animation,
            status: self.status.clone(),
            chrome: self.chrome.clone(),
        })
    }

    pub fn set_snapshot(&mut self, snapshot: GridSnapshot) {
        self.snapshot = Some(snapshot);
    }

    pub fn clear_snapshot(&mut self) {
        self.snapshot = None;
    }

    pub fn set_status(&mut self, status: impl Into<String>) {
        self.status = status.into();
    }

    #[must_use]
    pub fn is_editing_cell(&self) -> bool {
        matches!(self.grid_edit_state, GridEditState::Editing { .. })
    }

    #[must_use]
    pub fn editing_cell(&self) -> Option<&CellRef> {
        self.grid_edit_state.editing_cell()
    }

    #[must_use]
    pub fn cell_edit_mode(&self) -> Option<EditMode> {
        self.grid_edit_state.edit_mode()
    }

    #[must_use]
    pub fn selected_cell(&self) -> Option<&CellRef> {
        self.grid_edit_state.selected_cell()
    }

    #[must_use]
    pub fn edited_value(&self, cell: &CellRef) -> Option<&str> {
        self.edited_cells.get(cell).map(String::as_str)
    }

    #[must_use]
    pub fn editor_caret_visible(&self) -> bool {
        self.editor_caret_visible
    }

    pub fn set_editor_caret_visible(&mut self, visible: bool) -> bool {
        if self.editor_caret_visible == visible {
            return false;
        }
        self.editor_caret_visible = visible;
        true
    }

    pub fn apply_editor_intent(&mut self, intent: EditorIntent) -> EditorIntentResult {
        let mut result = EditorIntentResult::default();
        match intent {
            EditorIntent::InsertText(text) | EditorIntent::Paste(text) => {
                result.changed = self.insert_editor_text(&text);
            }
            EditorIntent::Move { movement, extend } => {
                result.changed = self.move_editor(movement, extend);
            }
            EditorIntent::Delete(delete) => {
                result.changed = self.delete_editor_by_intent(delete);
            }
            EditorIntent::SelectAll => {
                result.changed = self.select_all_editor_text();
            }
            EditorIntent::Copy => {
                result.copied_text = self.selected_editor_text();
            }
            EditorIntent::Cut => {
                result.copied_text = self.cut_editor_selection();
                result.changed = result.copied_text.is_some();
            }
            EditorIntent::Undo => {
                result.changed = self.undo_editor_edit();
            }
            EditorIntent::Redo => {
                result.changed = self.redo_editor_edit();
            }
            EditorIntent::SetCaret {
                grapheme_offset,
                extend,
            } => {
                result.changed = if extend {
                    self.extend_editor_selection_to_grapheme_offset(grapheme_offset)
                } else {
                    self.set_editor_caret_to_grapheme_offset(grapheme_offset)
                };
            }
            EditorIntent::SetSelection {
                anchor_grapheme_offset,
                caret_grapheme_offset,
            } => {
                result.changed = self.set_editor_selection_to_grapheme_offsets(
                    anchor_grapheme_offset,
                    caret_grapheme_offset,
                );
            }
            EditorIntent::SelectWordAt { grapheme_offset } => {
                result.changed = self.select_editor_word_at_grapheme_offset(grapheme_offset);
            }
            EditorIntent::SelectWordRange {
                anchor_start_grapheme_offset,
                anchor_end_grapheme_offset,
                caret_grapheme_offset,
            } => {
                result.changed = self.select_editor_word_range_to_grapheme_offset(
                    anchor_start_grapheme_offset,
                    anchor_end_grapheme_offset,
                    caret_grapheme_offset,
                );
            }
        }
        if result.changed {
            self.editor_caret_visible = true;
        }
        result
    }

    #[must_use]
    pub fn selected_editor_text(&self) -> Option<String> {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return None;
        };
        let (start, end) = selection_bounds(*selection, buffer.len());
        if start == end || !buffer.is_char_boundary(start) || !buffer.is_char_boundary(end) {
            return None;
        }
        Some(buffer[start..end].to_string())
    }

    pub fn begin_cell_edit(&mut self, cell: CellRef, initial_text: impl Into<String>) {
        self.begin_in_place_cell_edit(cell, initial_text);
    }

    pub fn begin_in_place_cell_edit(&mut self, cell: CellRef, original_text: impl Into<String>) {
        let original = original_text.into();
        self.selection.replace(
            SelectionAnchor::Cell(cell.clone()),
            cell.clone(),
            SelectionRange::Cells {
                start: cell.clone(),
                end: cell.clone(),
            },
        );
        self.focus = FocusTarget::Grid;
        self.ui_edit_mode = UiEditMode::Navigating;
        self.editor_caret_visible = true;
        self.clear_editor_history();
        self.grid_edit_state = GridEditState::Editing {
            cell,
            mode: EditMode::InPlace,
            buffer: original.clone(),
            selection: TextSelection {
                anchor: original.len(),
                caret: original.len(),
            },
            original,
        };
    }

    pub fn begin_cell_edit_replacing(&mut self, cell: CellRef, text: impl Into<String>) {
        let original = self.edited_cells.get(&cell).cloned().unwrap_or_default();
        self.begin_overwrite_cell_edit(cell, original, text);
    }

    pub fn begin_overwrite_cell_edit(
        &mut self,
        cell: CellRef,
        original_text: impl Into<String>,
        typed_text: impl Into<String>,
    ) {
        let original = original_text.into();
        let buffer = sanitize_editor_insert(&typed_text.into());
        self.selection.replace(
            SelectionAnchor::Cell(cell.clone()),
            cell.clone(),
            SelectionRange::Cells {
                start: cell.clone(),
                end: cell.clone(),
            },
        );
        self.focus = FocusTarget::Grid;
        self.ui_edit_mode = UiEditMode::Navigating;
        self.editor_caret_visible = true;
        self.clear_editor_history();
        self.grid_edit_state = GridEditState::Editing {
            cell,
            mode: EditMode::Overwrite,
            selection: TextSelection {
                anchor: buffer.len(),
                caret: buffer.len(),
            },
            buffer,
            original,
        };
    }

    pub fn insert_editor_text(&mut self, text: &str) -> bool {
        let Some(snapshot) = self.current_text_edit_snapshot() else {
            return false;
        };
        let text = sanitize_editor_insert(text);
        if text.is_empty() {
            return false;
        }
        self.push_editor_undo_snapshot(snapshot);
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        delete_editor_selection(buffer, selection);
        buffer.insert_str(selection.caret, &text);
        selection.caret = selection.caret.saturating_add(text.len());
        selection.anchor = selection.caret;
        true
    }

    pub fn backspace_editor(&mut self) -> bool {
        let Some(snapshot) = self.current_text_edit_snapshot() else {
            return false;
        };
        if !self.editor_can_backspace() {
            return false;
        }
        self.push_editor_undo_snapshot(snapshot);
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if delete_editor_selection(buffer, selection) {
            return true;
        }
        let Some(previous_cursor) = previous_char_boundary(buffer, selection.caret) else {
            return false;
        };
        buffer.drain(previous_cursor..selection.caret);
        selection.caret = previous_cursor;
        selection.anchor = previous_cursor;
        true
    }

    pub fn delete_editor(&mut self) -> bool {
        let Some(snapshot) = self.current_text_edit_snapshot() else {
            return false;
        };
        if !self.editor_can_delete() {
            return false;
        }
        self.push_editor_undo_snapshot(snapshot);
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if delete_editor_selection(buffer, selection) {
            return true;
        }
        let Some(next_cursor) = next_char_boundary(buffer, selection.caret) else {
            return false;
        };
        buffer.drain(selection.caret..next_cursor);
        true
    }

    pub fn backspace_editor_word(&mut self) -> bool {
        let Some(snapshot) = self.current_text_edit_snapshot() else {
            return false;
        };
        let Some((start, end)) = self.editor_previous_word_delete_range() else {
            return false;
        };
        self.push_editor_undo_snapshot(snapshot);
        self.delete_editor_range(start, end)
    }

    pub fn delete_editor_word(&mut self) -> bool {
        let Some(snapshot) = self.current_text_edit_snapshot() else {
            return false;
        };
        let Some((start, end)) = self.editor_next_word_delete_range() else {
            return false;
        };
        self.push_editor_undo_snapshot(snapshot);
        self.delete_editor_range(start, end)
    }

    pub fn backspace_editor_to_start(&mut self) -> bool {
        let Some(snapshot) = self.current_text_edit_snapshot() else {
            return false;
        };
        let Some((start, end)) = self.editor_to_start_delete_range() else {
            return false;
        };
        self.push_editor_undo_snapshot(snapshot);
        self.delete_editor_range(start, end)
    }

    pub fn delete_editor_to_end(&mut self) -> bool {
        let Some(snapshot) = self.current_text_edit_snapshot() else {
            return false;
        };
        let Some((start, end)) = self.editor_to_end_delete_range() else {
            return false;
        };
        self.push_editor_undo_snapshot(snapshot);
        self.delete_editor_range(start, end)
    }

    pub fn cut_editor_selection(&mut self) -> Option<String> {
        let selected = self.selected_editor_text()?;
        let snapshot = self.current_text_edit_snapshot()?;
        self.push_editor_undo_snapshot(snapshot);
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return None;
        };
        if delete_editor_selection(buffer, selection) {
            Some(selected)
        } else {
            None
        }
    }

    pub fn paste_editor_text(&mut self, text: &str) -> bool {
        self.insert_editor_text(text)
    }

    fn move_editor(&mut self, movement: EditorMove, extend: bool) -> bool {
        match movement {
            EditorMove::Left => self.move_editor_cursor_left_with_selection(extend),
            EditorMove::Right => self.move_editor_cursor_right_with_selection(extend),
            EditorMove::WordLeft => self.move_editor_cursor_word_left_with_selection(extend),
            EditorMove::WordRight => self.move_editor_cursor_word_right_with_selection(extend),
            EditorMove::LineStart => self.move_editor_cursor_to_start_with_selection(extend),
            EditorMove::LineEnd => self.move_editor_cursor_to_end_with_selection(extend),
        }
    }

    fn delete_editor_by_intent(&mut self, delete: EditorDelete) -> bool {
        match delete {
            EditorDelete::Backward => self.backspace_editor(),
            EditorDelete::Forward => self.delete_editor(),
            EditorDelete::WordBackward => self.backspace_editor_word(),
            EditorDelete::WordForward => self.delete_editor_word(),
            EditorDelete::ToLineStart => self.backspace_editor_to_start(),
            EditorDelete::ToLineEnd => self.delete_editor_to_end(),
        }
    }

    pub fn undo_editor_edit(&mut self) -> bool {
        let Some(previous) = self.editor_undo_stack.pop() else {
            return false;
        };
        let Some(current) = self.current_text_edit_snapshot() else {
            return false;
        };
        push_limited_snapshot(&mut self.editor_redo_stack, current);
        self.restore_text_edit_snapshot(previous)
    }

    pub fn redo_editor_edit(&mut self) -> bool {
        let Some(next) = self.editor_redo_stack.pop() else {
            return false;
        };
        let Some(current) = self.current_text_edit_snapshot() else {
            return false;
        };
        push_limited_snapshot(&mut self.editor_undo_stack, current);
        self.restore_text_edit_snapshot(next)
    }

    pub fn move_editor_cursor_left(&mut self) -> bool {
        self.move_editor_cursor_left_with_selection(false)
    }

    pub fn move_editor_cursor_left_with_selection(&mut self, extend: bool) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if !extend && selection.anchor != selection.caret {
            let start = selection.anchor.min(selection.caret);
            selection.anchor = start;
            selection.caret = start;
            return true;
        }
        let Some(previous_cursor) = previous_char_boundary(buffer, selection.caret) else {
            return false;
        };
        selection.caret = previous_cursor;
        if !extend {
            selection.anchor = previous_cursor;
        }
        true
    }

    pub fn move_editor_cursor_right(&mut self) -> bool {
        self.move_editor_cursor_right_with_selection(false)
    }

    pub fn move_editor_cursor_right_with_selection(&mut self, extend: bool) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if !extend && selection.anchor != selection.caret {
            let end = selection.anchor.max(selection.caret);
            selection.anchor = end;
            selection.caret = end;
            return true;
        }
        let Some(next_cursor) = next_char_boundary(buffer, selection.caret) else {
            return false;
        };
        selection.caret = next_cursor;
        if !extend {
            selection.anchor = next_cursor;
        }
        true
    }

    pub fn move_editor_cursor_word_left_with_selection(&mut self, extend: bool) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if !extend && selection.anchor != selection.caret {
            let start = selection.anchor.min(selection.caret);
            selection.anchor = start;
            selection.caret = start;
            return true;
        }
        let previous = previous_word_boundary(buffer, selection.caret);
        if previous == selection.caret {
            return false;
        }
        selection.caret = previous;
        if !extend {
            selection.anchor = previous;
        }
        true
    }

    pub fn move_editor_cursor_word_right_with_selection(&mut self, extend: bool) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if !extend && selection.anchor != selection.caret {
            let end = selection.anchor.max(selection.caret);
            selection.anchor = end;
            selection.caret = end;
            return true;
        }
        let next = next_word_boundary(buffer, selection.caret);
        if next == selection.caret {
            return false;
        }
        selection.caret = next;
        if !extend {
            selection.anchor = next;
        }
        true
    }

    pub fn move_editor_cursor_to_start(&mut self) -> bool {
        self.move_editor_cursor_to_start_with_selection(false)
    }

    pub fn move_editor_cursor_to_start_with_selection(&mut self, extend: bool) -> bool {
        let GridEditState::Editing { selection, .. } = &mut self.grid_edit_state else {
            return false;
        };
        if selection.caret == 0 {
            return false;
        }
        selection.caret = 0;
        if !extend {
            selection.anchor = 0;
        }
        true
    }

    pub fn move_editor_cursor_to_end(&mut self) -> bool {
        self.move_editor_cursor_to_end_with_selection(false)
    }

    pub fn move_editor_cursor_to_end_with_selection(&mut self, extend: bool) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if selection.caret == buffer.len() {
            return false;
        }
        selection.caret = buffer.len();
        if !extend {
            selection.anchor = buffer.len();
        }
        true
    }

    pub fn select_all_editor_text(&mut self) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        let changed = selection.anchor != 0 || selection.caret != buffer.len();
        selection.anchor = 0;
        selection.caret = buffer.len();
        changed
    }

    #[must_use]
    pub fn editor_selection_grapheme_offsets(&self) -> Option<(usize, usize)> {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return None;
        };
        Some((
            grapheme_offset_for_byte_index(buffer, selection.anchor),
            grapheme_offset_for_byte_index(buffer, selection.caret),
        ))
    }

    #[must_use]
    pub fn editor_grapheme_offset_for_byte_index(&self, byte_index: usize) -> Option<usize> {
        let GridEditState::Editing { buffer, .. } = &self.grid_edit_state else {
            return None;
        };
        Some(grapheme_offset_for_byte_index(buffer, byte_index))
    }

    #[must_use]
    pub fn editor_word_grapheme_range_at_offset(
        &self,
        grapheme_offset: usize,
    ) -> Option<(usize, usize)> {
        let GridEditState::Editing { buffer, .. } = &self.grid_edit_state else {
            return None;
        };
        let byte_index = byte_index_for_grapheme_offset(buffer, grapheme_offset);
        let (start, end) = word_bounds_containing(buffer, byte_index)?;
        Some((
            grapheme_offset_for_byte_index(buffer, start),
            grapheme_offset_for_byte_index(buffer, end),
        ))
    }

    pub fn set_editor_caret_to_grapheme_offset(&mut self, grapheme_offset: usize) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        let next_caret = byte_index_for_grapheme_offset(buffer, grapheme_offset);
        if selection.caret == next_caret && selection.anchor == next_caret {
            return false;
        }
        selection.caret = next_caret;
        selection.anchor = next_caret;
        true
    }

    pub fn extend_editor_selection_to_grapheme_offset(&mut self, grapheme_offset: usize) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        let caret = byte_index_for_grapheme_offset(buffer, grapheme_offset);
        if selection.caret == caret {
            return false;
        }
        selection.caret = caret;
        true
    }

    pub fn set_editor_caret_to_character_offset(&mut self, character_offset: usize) -> bool {
        self.set_editor_caret_to_grapheme_offset(character_offset)
    }

    pub fn set_editor_selection_to_grapheme_offsets(
        &mut self,
        anchor_offset: usize,
        caret_offset: usize,
    ) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        let anchor = byte_index_for_grapheme_offset(buffer, anchor_offset);
        let caret = byte_index_for_grapheme_offset(buffer, caret_offset);
        if selection.anchor == anchor && selection.caret == caret {
            return false;
        }
        selection.anchor = anchor;
        selection.caret = caret;
        true
    }

    pub fn set_editor_selection_to_character_offsets(
        &mut self,
        anchor_offset: usize,
        caret_offset: usize,
    ) -> bool {
        self.set_editor_selection_to_grapheme_offsets(anchor_offset, caret_offset)
    }

    pub fn select_editor_word_at_grapheme_offset(&mut self, grapheme_offset: usize) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        let byte_index = byte_index_for_grapheme_offset(buffer, grapheme_offset);
        let Some((start, end)) = word_bounds_containing(buffer, byte_index) else {
            selection.anchor = byte_index;
            selection.caret = byte_index;
            return false;
        };
        selection.anchor = start;
        selection.caret = end;
        true
    }

    pub fn select_editor_word_at_character_offset(&mut self, character_offset: usize) -> bool {
        self.select_editor_word_at_grapheme_offset(character_offset)
    }

    pub fn select_editor_word_range_to_grapheme_offset(
        &mut self,
        anchor_start_offset: usize,
        anchor_end_offset: usize,
        caret_offset: usize,
    ) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        let anchor_start = byte_index_for_grapheme_offset(buffer, anchor_start_offset);
        let anchor_end = byte_index_for_grapheme_offset(buffer, anchor_end_offset);
        let caret = byte_index_for_grapheme_offset(buffer, caret_offset);
        let (target_start, target_end) =
            word_bounds_containing(buffer, caret).unwrap_or((caret, caret));
        let (anchor, caret) = if caret < anchor_start {
            (anchor_end, target_start)
        } else {
            (anchor_start, target_end)
        };
        if selection.anchor == anchor && selection.caret == caret {
            return false;
        }
        selection.anchor = anchor;
        selection.caret = caret;
        true
    }

    pub fn commit_cell_edit(&mut self) -> Option<(CellRef, String)> {
        let state = std::mem::replace(&mut self.grid_edit_state, GridEditState::Idle);
        match state {
            GridEditState::Editing { cell, buffer, .. } => {
                self.ui_edit_mode = UiEditMode::Navigating;
                self.focus = FocusTarget::Grid;
                self.edited_cells.insert(cell.clone(), buffer.clone());
                self.grid_edit_state = GridEditState::Selected { cell: cell.clone() };
                self.editor_caret_visible = false;
                self.clear_editor_history();
                Some((cell, buffer))
            }
            other => {
                self.grid_edit_state = other;
                None
            }
        }
    }

    fn clear_editor_history(&mut self) {
        self.editor_undo_stack.clear();
        self.editor_redo_stack.clear();
    }

    fn current_text_edit_snapshot(&self) -> Option<TextEditSnapshot> {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return None;
        };
        Some(TextEditSnapshot {
            buffer: buffer.clone(),
            selection: *selection,
        })
    }

    fn push_editor_undo_snapshot(&mut self, snapshot: TextEditSnapshot) {
        if self.editor_undo_stack.last() == Some(&snapshot) {
            return;
        }
        push_limited_snapshot(&mut self.editor_undo_stack, snapshot);
        self.editor_redo_stack.clear();
    }

    fn restore_text_edit_snapshot(&mut self, snapshot: TextEditSnapshot) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        *buffer = snapshot.buffer;
        *selection = snapshot.selection;
        true
    }

    fn editor_can_backspace(&self) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return false;
        };
        let (start, end) = selection_bounds(*selection, buffer.len());
        start != end || previous_char_boundary(buffer, selection.caret).is_some()
    }

    fn editor_can_delete(&self) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return false;
        };
        let (start, end) = selection_bounds(*selection, buffer.len());
        start != end || next_char_boundary(buffer, selection.caret).is_some()
    }

    fn editor_previous_word_delete_range(&self) -> Option<(usize, usize)> {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return None;
        };
        let (start, end) = selection_bounds(*selection, buffer.len());
        if start != end {
            return Some((start, end));
        }
        let previous = previous_word_boundary(buffer, selection.caret);
        (previous < selection.caret).then_some((previous, selection.caret))
    }

    fn editor_next_word_delete_range(&self) -> Option<(usize, usize)> {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return None;
        };
        let (start, end) = selection_bounds(*selection, buffer.len());
        if start != end {
            return Some((start, end));
        }
        let next = next_word_boundary(buffer, selection.caret);
        (next > selection.caret).then_some((selection.caret, next))
    }

    fn editor_to_start_delete_range(&self) -> Option<(usize, usize)> {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return None;
        };
        let (start, end) = selection_bounds(*selection, buffer.len());
        if start != end {
            return Some((start, end));
        }
        (selection.caret > 0).then_some((0, selection.caret))
    }

    fn editor_to_end_delete_range(&self) -> Option<(usize, usize)> {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &self.grid_edit_state
        else {
            return None;
        };
        let (start, end) = selection_bounds(*selection, buffer.len());
        if start != end {
            return Some((start, end));
        }
        (selection.caret < buffer.len()).then_some((selection.caret, buffer.len()))
    }

    fn delete_editor_range(&mut self, start: usize, end: usize) -> bool {
        let GridEditState::Editing {
            buffer, selection, ..
        } = &mut self.grid_edit_state
        else {
            return false;
        };
        if start >= end || !buffer.is_char_boundary(start) || !buffer.is_char_boundary(end) {
            return false;
        }
        buffer.drain(start..end);
        selection.anchor = start;
        selection.caret = start;
        true
    }

    pub fn cancel_cell_edit(&mut self) -> bool {
        let state = std::mem::replace(&mut self.grid_edit_state, GridEditState::Idle);
        match state {
            GridEditState::Editing { cell, .. } => {
                self.ui_edit_mode = UiEditMode::Navigating;
                self.focus = FocusTarget::Grid;
                self.grid_edit_state = GridEditState::Selected { cell };
                self.editor_caret_visible = false;
                self.clear_editor_history();
                true
            }
            other => {
                self.grid_edit_state = other;
                false
            }
        }
    }

    pub fn clear_selected_cell(&mut self) -> Option<(CellRef, String)> {
        if self.is_editing_cell() {
            return None;
        }
        let cell = self.selection.active.clone();
        self.edited_cells.insert(cell.clone(), String::new());
        self.grid_edit_state = GridEditState::Selected { cell: cell.clone() };
        Some((cell, String::new()))
    }

    pub fn route_key(&mut self, key: &Key) -> Option<UiCommand> {
        match (&self.focus, key) {
            (FocusTarget::Grid, Key::Named(NamedKey::Enter)) if !self.is_editing_cell() => {
                Some(UiCommand::BeginCellEdit)
            }
            (FocusTarget::FormulaBar, Key::Named(NamedKey::Enter)) => {
                self.focus = FocusTarget::Grid;
                self.ui_edit_mode = UiEditMode::Navigating;
                Some(UiCommand::CommitEdit)
            }
            (FocusTarget::Grid, Key::Named(NamedKey::ArrowDown)) if !self.is_editing_cell() => {
                Some(UiCommand::MoveSelection {
                    row_delta: 1,
                    column_delta: 0,
                })
            }
            (FocusTarget::Grid, Key::Named(NamedKey::ArrowUp)) if !self.is_editing_cell() => {
                Some(UiCommand::MoveSelection {
                    row_delta: -1,
                    column_delta: 0,
                })
            }
            (FocusTarget::Grid, Key::Named(NamedKey::ArrowRight)) if !self.is_editing_cell() => {
                Some(UiCommand::MoveSelection {
                    row_delta: 0,
                    column_delta: 1,
                })
            }
            (FocusTarget::Grid, Key::Named(NamedKey::ArrowLeft)) if !self.is_editing_cell() => {
                Some(UiCommand::MoveSelection {
                    row_delta: 0,
                    column_delta: -1,
                })
            }
            _ => None,
        }
    }

    pub fn move_selection(&mut self, row_delta: i32, column_delta: i32) {
        self.move_selection_with(row_delta, column_delta, false);
    }

    pub fn move_selection_with(&mut self, row_delta: i32, column_delta: i32, extend: bool) {
        let row = self
            .selection
            .active
            .row
            .saturating_add_signed(row_delta)
            .max(1);
        let column = self
            .selection
            .active
            .column
            .saturating_add_signed(column_delta)
            .max(1);
        self.select_cell(
            CellRef::new(row, column),
            SelectionAction {
                extend,
                additive: false,
            },
        );
        self.keep_selection_visible();
    }

    pub fn select_cell(&mut self, cell: CellRef, action: SelectionAction) {
        let range = match (action.extend, &self.selection.anchor) {
            (true, SelectionAnchor::Cell(anchor)) => SelectionRange::Cells {
                start: anchor.clone(),
                end: cell.clone(),
            },
            (true, SelectionAnchor::Row(row)) => SelectionRange::Rows {
                start: *row,
                end: cell.row,
            },
            (true, SelectionAnchor::Column(column)) => SelectionRange::Columns {
                start: *column,
                end: cell.column,
            },
            (true, SelectionAnchor::Sheet) => SelectionRange::Sheet,
            (false, _) => SelectionRange::Cells {
                start: cell.clone(),
                end: cell.clone(),
            },
        };
        if action.extend {
            self.selection.update_current(cell, range);
        } else if action.additive {
            self.selection
                .push(SelectionAnchor::Cell(cell.clone()), cell, range);
        } else {
            self.selection
                .replace(SelectionAnchor::Cell(cell.clone()), cell, range);
        }
        if !self.is_editing_cell() {
            self.grid_edit_state = GridEditState::Selected {
                cell: self.selection.active.clone(),
            };
        }
    }

    pub fn select_row(&mut self, row: u32, action: SelectionAction) {
        let range = if action.extend {
            let start = match &self.selection.anchor {
                SelectionAnchor::Row(anchor) => *anchor,
                SelectionAnchor::Cell(anchor) => anchor.row,
                SelectionAnchor::Column(_) | SelectionAnchor::Sheet => row,
            };
            SelectionRange::Rows { start, end: row }
        } else {
            SelectionRange::Rows {
                start: row,
                end: row,
            }
        };
        let active = CellRef::new(row, self.selection.active.column.max(1));
        if action.extend {
            self.selection.update_current(active, range);
        } else if action.additive {
            self.selection
                .push(SelectionAnchor::Row(row), active, range);
        } else {
            self.selection
                .replace(SelectionAnchor::Row(row), active, range);
        }
        if !self.is_editing_cell() {
            self.grid_edit_state = GridEditState::Selected {
                cell: self.selection.active.clone(),
            };
        }
    }

    pub fn select_column(&mut self, column: u32, action: SelectionAction) {
        let range = if action.extend {
            let start = match &self.selection.anchor {
                SelectionAnchor::Column(anchor) => *anchor,
                SelectionAnchor::Cell(anchor) => anchor.column,
                SelectionAnchor::Row(_) | SelectionAnchor::Sheet => column,
            };
            SelectionRange::Columns { start, end: column }
        } else {
            SelectionRange::Columns {
                start: column,
                end: column,
            }
        };
        let active = CellRef::new(self.selection.active.row.max(1), column);
        if action.extend {
            self.selection.update_current(active, range);
        } else if action.additive {
            self.selection
                .push(SelectionAnchor::Column(column), active, range);
        } else {
            self.selection
                .replace(SelectionAnchor::Column(column), active, range);
        }
        if !self.is_editing_cell() {
            self.grid_edit_state = GridEditState::Selected {
                cell: self.selection.active.clone(),
            };
        }
    }

    pub fn select_all(&mut self) {
        self.selection.replace(
            SelectionAnchor::Sheet,
            CellRef::new(1, 1),
            SelectionRange::Sheet,
        );
        if !self.is_editing_cell() {
            self.grid_edit_state = GridEditState::Selected {
                cell: self.selection.active.clone(),
            };
        }
    }

    pub fn scroll_rows(&mut self, row_delta: i64) {
        self.scroll_y_pixels(row_delta as f64 * f64::from(self.viewport.metrics.row_height));
    }

    pub fn scroll_columns(&mut self, column_delta: i32) {
        self.scroll_x_pixels(column_delta as f64 * f64::from(self.viewport.metrics.column_width));
    }

    pub fn scroll_y_pixels(&mut self, pixel_delta: f64) {
        self.viewport.scroll_y_px = (self.viewport.scroll_y_px + pixel_delta).max(0.0);
    }

    pub fn scroll_x_pixels(&mut self, pixel_delta: f64) {
        self.viewport.scroll_x_px = (self.viewport.scroll_x_px + pixel_delta).max(0.0);
    }

    pub fn clamp_to_table(&mut self, row_count: Option<u64>, column_count: usize) {
        if let Some(max_y) = self.viewport.max_scroll_y_px(row_count) {
            self.viewport.scroll_y_px = self.viewport.scroll_y_px.min(max_y);
        }
        let max_x = self.viewport.max_scroll_x_px(column_count);
        self.viewport.scroll_x_px = self.viewport.scroll_x_px.min(max_x);
    }

    fn keep_selection_visible(&mut self) {
        if self.viewport.visible_window().is_err() {
            return;
        };
        let active_row = u64::from(self.selection.active.row.saturating_sub(1));
        let active_row_start = self.viewport.row_start_px(active_row);
        let active_row_end = active_row_start + f64::from(self.viewport.row_height_at(active_row));
        if active_row_start < self.viewport.scroll_y_px {
            self.viewport.scroll_y_px = active_row_start;
        } else {
            let viewport_bottom =
                self.viewport.scroll_y_px + f64::from(self.viewport.body_height());
            if active_row_end > viewport_bottom {
                self.viewport.scroll_y_px =
                    (active_row_end - f64::from(self.viewport.body_height())).max(0.0);
            }
        }

        let active_column = self.selection.active.column.saturating_sub(1);
        let active_column_start = self.viewport.column_start_px(active_column);
        let active_column_end =
            active_column_start + f64::from(self.viewport.column_width_at(active_column));
        if active_column_start < self.viewport.scroll_x_px {
            self.viewport.scroll_x_px = active_column_start;
        } else {
            let viewport_right = self.viewport.scroll_x_px + f64::from(self.viewport.body_width());
            if active_column_end > viewport_right {
                self.viewport.scroll_x_px =
                    (active_column_end - f64::from(self.viewport.body_width())).max(0.0);
            }
        }
    }
}

fn sanitize_editor_insert(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .collect()
}

fn scale_size_overrides_u32(
    overrides: &mut BTreeMap<u32, u32>,
    old_default: u32,
    new_default: u32,
) {
    scale_size_overrides(overrides.values_mut(), old_default, new_default);
    overrides.retain(|_, size| *size != new_default);
}

fn scale_size_overrides_u64(
    overrides: &mut BTreeMap<u64, u32>,
    old_default: u32,
    new_default: u32,
) {
    scale_size_overrides(overrides.values_mut(), old_default, new_default);
    overrides.retain(|_, size| *size != new_default);
}

fn scale_size_overrides<'a>(
    sizes: impl Iterator<Item = &'a mut u32>,
    old_default: u32,
    new_default: u32,
) {
    if old_default == 0 || old_default == new_default {
        return;
    }
    let scale = f64::from(new_default) / f64::from(old_default);
    for size in sizes {
        *size = ((f64::from(*size) * scale).round() as u32).max(1);
    }
}

fn delete_editor_selection(buffer: &mut String, selection: &mut TextSelection) -> bool {
    let start = selection.anchor.min(selection.caret).min(buffer.len());
    let end = selection.anchor.max(selection.caret).min(buffer.len());
    if start >= end || !buffer.is_char_boundary(start) || !buffer.is_char_boundary(end) {
        return false;
    }
    buffer.drain(start..end);
    selection.caret = start;
    selection.anchor = start;
    true
}

fn selection_bounds(selection: TextSelection, buffer_len: usize) -> (usize, usize) {
    (
        selection.anchor.min(selection.caret).min(buffer_len),
        selection.anchor.max(selection.caret).min(buffer_len),
    )
}

fn push_limited_snapshot(stack: &mut Vec<TextEditSnapshot>, snapshot: TextEditSnapshot) {
    if stack.len() == EDIT_HISTORY_LIMIT {
        stack.remove(0);
    }
    stack.push(snapshot);
}

fn previous_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 || cursor > text.len() {
        return None;
    }
    let mut previous = None;
    for (index, _) in text.grapheme_indices(true) {
        if index >= cursor {
            return previous;
        }
        previous = Some(index);
    }
    previous
}

fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }
    text.grapheme_indices(true).find_map(|(index, grapheme)| {
        let end = index + grapheme.len();
        (end > cursor).then_some(end)
    })
}

fn previous_word_boundary(text: &str, cursor: usize) -> usize {
    if cursor == 0 || cursor > text.len() {
        return 0;
    }
    let mut previous_word_start = 0;
    for (start, word) in text.unicode_word_indices() {
        let end = start + word.len();
        if end < cursor {
            previous_word_start = start;
            continue;
        }
        if start < cursor {
            return start;
        }
        break;
    }
    previous_word_start
}

fn next_word_boundary(text: &str, cursor: usize) -> usize {
    if cursor >= text.len() {
        return text.len();
    }
    for (start, word) in text.unicode_word_indices() {
        let end = start + word.len();
        if end <= cursor {
            continue;
        }
        return end;
    }
    text.len()
}

fn word_bounds_containing(text: &str, cursor: usize) -> Option<(usize, usize)> {
    let cursor = cursor.min(text.len());
    for (start, word) in text.unicode_word_indices() {
        let end = start + word.len();
        if (start..=end).contains(&cursor) {
            return Some((start, end));
        }
    }
    None
}

fn byte_index_for_grapheme_offset(text: &str, grapheme_offset: usize) -> usize {
    text.grapheme_indices(true)
        .nth(grapheme_offset)
        .map_or(text.len(), |(index, _)| index)
}

fn grapheme_offset_for_byte_index(text: &str, byte_index: usize) -> usize {
    let byte_index = byte_index.min(text.len());
    text.grapheme_indices(true)
        .take_while(|(index, _)| *index < byte_index)
        .count()
}

#[cfg(test)]
mod tests;
