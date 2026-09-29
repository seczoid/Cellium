use std::time::{Duration, Instant};

use cellium_core::CellRef;
use cellium_ui::{
    EditMode, EditorDelete, EditorIntent, EditorMove, GRID_CELL_FONT_SIZE, GRID_CELL_TEXT_HEIGHT,
    GridEditState, TextSelection,
};
use tracing::warn;
use winit::{
    dpi::PhysicalPosition,
    keyboard::{Key, ModifiersState, NamedKey},
};

use crate::{constants::*, ids::unix_seconds};

use super::{
    AppState, CellClick, CellEditWrite, EditorCaretAnimationState, EditorTextDrag, TextCommit,
    TextCommitSource, active_table_extents, grid_local_position, move_selection_with_animation,
    rect_contains, visible_column_offset_x, visible_query_window_changed, visible_row_offset_y,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorInputResult {
    Ignored,
    Redraw,
    Query,
}

pub(super) fn begin_cell_edit_with_current_value(state: &mut AppState, cell: CellRef) {
    if !ensure_cell_editable(state, &cell) {
        return;
    }
    let text = displayed_cell_text(state, &cell);
    state.ui.begin_in_place_cell_edit(cell, text);
    state.renderer.window().set_ime_allowed(true);
    state.last_text_commit = None;
    state.editor_caret_animation = None;
    reset_editor_caret_blink(state);
    state.last_cell_click = None;
}

pub(super) fn begin_cell_edit_from_printable_key(
    state: &mut AppState,
    key: &Key,
    text: Option<&str>,
) -> bool {
    if state.modifiers.super_key() || state.modifiers.control_key() {
        return false;
    }
    let Some(text) = printable_text_from_key(key, text) else {
        return false;
    };
    let cell = state.ui.selection.active.clone();
    if !ensure_cell_editable(state, &cell) {
        return true;
    }
    let original = displayed_cell_text(state, &cell);
    state.ui.begin_overwrite_cell_edit(cell, original, text);
    state.renderer.window().set_ime_allowed(true);
    state.last_text_commit = None;
    state.editor_caret_animation = None;
    reset_editor_caret_blink(state);
    state.last_cell_click = None;
    true
}

pub(super) fn apply_editor_intent(
    state: &mut AppState,
    intent: EditorIntent,
) -> cellium_ui::EditorIntentResult {
    let before = editor_caret_snapshot(&state.ui.grid_edit_state);
    let result = state.ui.apply_editor_intent(intent);
    if result.changed {
        update_editor_caret_animation(state, before);
        reset_editor_caret_blink(state);
    }
    result
}

fn update_editor_caret_animation(state: &mut AppState, before: Option<(String, TextSelection)>) {
    let Some((from_buffer, from_selection)) = before else {
        state.editor_caret_animation = None;
        return;
    };
    let Some(after_selection) = state.ui.grid_edit_state.selection() else {
        state.editor_caret_animation = None;
        return;
    };
    if after_selection.anchor != after_selection.caret {
        state.editor_caret_animation = None;
        return;
    }
    let Some(after_buffer) = state.ui.grid_edit_state.editing_buffer() else {
        state.editor_caret_animation = None;
        return;
    };
    if from_selection.caret == after_selection.caret && from_buffer == after_buffer {
        return;
    }
    state.editor_caret_animation = Some(EditorCaretAnimationState {
        from_caret: from_selection.caret,
        from_buffer,
        to_caret: after_selection.caret,
        started_at: Instant::now(),
    });
}

fn editor_caret_snapshot(edit_state: &GridEditState) -> Option<(String, TextSelection)> {
    let GridEditState::Editing {
        buffer, selection, ..
    } = edit_state
    else {
        return None;
    };
    Some((buffer.clone(), *selection))
}

pub(super) fn commit_editor_text(
    state: &mut AppState,
    text: String,
    source: TextCommitSource,
) -> bool {
    let now = Instant::now();
    if is_duplicate_text_commit(state.last_text_commit.as_ref(), &text, source, now) {
        return false;
    }
    let changed = apply_editor_intent(state, EditorIntent::InsertText(text.clone())).changed;
    if changed {
        state.last_text_commit = Some(TextCommit {
            text,
            source,
            at: now,
        });
    }
    changed
}

pub(super) fn is_duplicate_text_commit(
    last_commit: Option<&TextCommit>,
    text: &str,
    source: TextCommitSource,
    now: Instant,
) -> bool {
    let Some(last_commit) = last_commit else {
        return false;
    };
    last_commit.source != source
        && last_commit.text == text
        && now
            .checked_duration_since(last_commit.at)
            .is_some_and(|elapsed| {
                elapsed <= Duration::from_millis(TEXT_COMMIT_DUPLICATE_WINDOW_MS)
            })
}

pub(super) fn reset_editor_caret_blink(state: &mut AppState) {
    state.editor_caret_blink_started = Instant::now();
    state.ui.set_editor_caret_visible(true);
}

pub(super) fn update_editor_caret_blink(state: &mut AppState, now: Instant) -> bool {
    if !state.ui.is_editing_cell() {
        return state.ui.set_editor_caret_visible(false);
    }
    let elapsed = now.saturating_duration_since(state.editor_caret_blink_started);
    let phase = elapsed.as_millis() / u128::from(EDITOR_CARET_BLINK_MS);
    state.ui.set_editor_caret_visible(phase.is_multiple_of(2))
}

pub(super) fn next_editor_caret_blink_deadline(state: &AppState, now: Instant) -> Instant {
    let elapsed = now.saturating_duration_since(state.editor_caret_blink_started);
    let phase = elapsed.as_millis() / u128::from(EDITOR_CARET_BLINK_MS);
    let next_phase_ms = (phase.saturating_add(1) as u64).saturating_mul(EDITOR_CARET_BLINK_MS);
    state.editor_caret_blink_started + Duration::from_millis(next_phase_ms)
}

pub(super) fn handle_cell_editor_key(
    state: &mut AppState,
    key: &Key,
    text: Option<&str>,
) -> EditorInputResult {
    if !state.ui.is_editing_cell() {
        return handle_selected_cell_key(state, key);
    }

    if handle_editor_shortcut(state, key) {
        return EditorInputResult::Redraw;
    }

    match key {
        Key::Named(NamedKey::Escape) => {
            cancel_active_cell_edit(state);
            EditorInputResult::Redraw
        }
        Key::Named(NamedKey::Enter) => {
            commit_active_cell_edit(state);
            let row_delta = if state.modifiers.shift_key() { -1 } else { 1 };
            move_selection_after_edit(state, row_delta, 0)
        }
        Key::Named(NamedKey::Tab) => {
            commit_active_cell_edit(state);
            let column_delta = if state.modifiers.shift_key() { -1 } else { 1 };
            move_selection_after_edit(state, 0, column_delta)
        }
        Key::Named(NamedKey::Space)
            if !state.modifiers.super_key() && !state.modifiers.control_key() =>
        {
            commit_editor_text(state, " ".to_string(), TextCommitSource::Keyboard);
            EditorInputResult::Redraw
        }
        Key::Named(NamedKey::Backspace) => {
            let delete = if state.modifiers.super_key() {
                EditorDelete::ToLineStart
            } else if editor_word_modifier(state.modifiers) {
                EditorDelete::WordBackward
            } else {
                EditorDelete::Backward
            };
            apply_editor_intent(state, EditorIntent::Delete(delete));
            EditorInputResult::Redraw
        }
        Key::Named(NamedKey::Delete) => {
            let delete = if state.modifiers.super_key() {
                EditorDelete::ToLineEnd
            } else if editor_word_modifier(state.modifiers) {
                EditorDelete::WordForward
            } else {
                EditorDelete::Forward
            };
            apply_editor_intent(state, EditorIntent::Delete(delete));
            EditorInputResult::Redraw
        }
        Key::Named(
            NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::ArrowUp | NamedKey::ArrowDown,
        ) => match state.ui.cell_edit_mode() {
            Some(EditMode::Overwrite) => {
                let (row_delta, column_delta) = arrow_key_delta(key);
                commit_active_cell_edit(state);
                move_selection_after_edit(state, row_delta, column_delta)
            }
            Some(EditMode::InPlace) => {
                match key {
                    Key::Named(NamedKey::ArrowLeft) => {
                        let movement = if state.modifiers.super_key() {
                            EditorMove::LineStart
                        } else if editor_word_modifier(state.modifiers) {
                            EditorMove::WordLeft
                        } else {
                            EditorMove::Left
                        };
                        apply_editor_intent(
                            state,
                            EditorIntent::Move {
                                movement,
                                extend: state.modifiers.shift_key(),
                            },
                        );
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        let movement = if state.modifiers.super_key() {
                            EditorMove::LineEnd
                        } else if editor_word_modifier(state.modifiers) {
                            EditorMove::WordRight
                        } else {
                            EditorMove::Right
                        };
                        apply_editor_intent(
                            state,
                            EditorIntent::Move {
                                movement,
                                extend: state.modifiers.shift_key(),
                            },
                        );
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        apply_editor_intent(
                            state,
                            EditorIntent::Move {
                                movement: EditorMove::LineStart,
                                extend: state.modifiers.shift_key(),
                            },
                        );
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        apply_editor_intent(
                            state,
                            EditorIntent::Move {
                                movement: EditorMove::LineEnd,
                                extend: state.modifiers.shift_key(),
                            },
                        );
                    }
                    _ => {}
                }
                EditorInputResult::Redraw
            }
            None => EditorInputResult::Ignored,
        },
        Key::Named(NamedKey::Home) => {
            apply_editor_intent(
                state,
                EditorIntent::Move {
                    movement: EditorMove::LineStart,
                    extend: state.modifiers.shift_key(),
                },
            );
            EditorInputResult::Redraw
        }
        Key::Named(NamedKey::End) => {
            apply_editor_intent(
                state,
                EditorIntent::Move {
                    movement: EditorMove::LineEnd,
                    extend: state.modifiers.shift_key(),
                },
            );
            EditorInputResult::Redraw
        }
        _ => {
            if state.modifiers.super_key() || state.modifiers.control_key() {
                return EditorInputResult::Redraw;
            }
            if let Some(text) = printable_text_from_key(key, text) {
                commit_editor_text(state, text, TextCommitSource::Keyboard);
            }
            EditorInputResult::Redraw
        }
    }
}

pub(super) fn handle_editor_shortcut(state: &mut AppState, key: &Key) -> bool {
    let shortcut = state.modifiers.super_key() || state.modifiers.control_key();
    if !shortcut {
        return false;
    }
    match key {
        Key::Character(character) if character.eq_ignore_ascii_case("a") => {
            apply_editor_intent(state, EditorIntent::SelectAll);
            true
        }
        Key::Character(character) if character.eq_ignore_ascii_case("z") => {
            if state.modifiers.shift_key() {
                apply_editor_intent(state, EditorIntent::Redo);
            } else {
                apply_editor_intent(state, EditorIntent::Undo);
            }
            true
        }
        Key::Character(character) if character.eq_ignore_ascii_case("y") => {
            apply_editor_intent(state, EditorIntent::Redo);
            true
        }
        Key::Character(character) if character.eq_ignore_ascii_case("c") => {
            copy_editor_selection_to_clipboard(state);
            true
        }
        Key::Character(character) if character.eq_ignore_ascii_case("x") => {
            cut_editor_selection_to_clipboard(state);
            true
        }
        Key::Character(character) if character.eq_ignore_ascii_case("v") => {
            paste_clipboard_into_editor(state);
            true
        }
        _ => false,
    }
}

pub(super) fn editor_word_modifier(modifiers: ModifiersState) -> bool {
    modifiers.alt_key() || modifiers.control_key()
}

pub(super) fn copy_editor_selection_to_clipboard(state: &mut AppState) -> bool {
    let result = state.ui.apply_editor_intent(EditorIntent::Copy);
    let Some(text) = result.copied_text else {
        return false;
    };
    set_clipboard_text(state, text)
}

pub(super) fn cut_editor_selection_to_clipboard(state: &mut AppState) -> bool {
    let Some(text) = state.ui.selected_editor_text() else {
        return false;
    };
    if !set_clipboard_text(state, text) {
        return false;
    }
    apply_editor_intent(state, EditorIntent::Cut).changed
}

pub(super) fn paste_clipboard_into_editor(state: &mut AppState) -> bool {
    let Some(text) = get_clipboard_text(state) else {
        return false;
    };
    apply_editor_intent(state, EditorIntent::Paste(text)).changed
}

pub(super) fn set_clipboard_text(state: &mut AppState, text: String) -> bool {
    let Some(clipboard) = state.clipboard.as_mut() else {
        return false;
    };
    if let Err(error) = clipboard.set_text(text) {
        warn!(%error, "failed to write clipboard text");
        return false;
    }
    true
}

pub(super) fn get_clipboard_text(state: &mut AppState) -> Option<String> {
    let clipboard = state.clipboard.as_mut()?;
    match clipboard.get_text() {
        Ok(text) => Some(text),
        Err(error) => {
            warn!(%error, "failed to read clipboard text");
            None
        }
    }
}

pub(super) fn handle_selected_cell_key(state: &mut AppState, key: &Key) -> EditorInputResult {
    match key {
        Key::Named(NamedKey::F2) => {
            begin_cell_edit_with_current_value(state, state.ui.selection.active.clone());
            EditorInputResult::Redraw
        }
        Key::Named(NamedKey::Backspace) => {
            let cell = state.ui.selection.active.clone();
            if !ensure_cell_editable(state, &cell) {
                return EditorInputResult::Redraw;
            }
            let original = displayed_cell_text(state, &cell);
            state.ui.begin_overwrite_cell_edit(cell, original, "");
            state.renderer.window().set_ime_allowed(true);
            reset_editor_caret_blink(state);
            state.last_cell_click = None;
            EditorInputResult::Redraw
        }
        Key::Named(NamedKey::Delete) => {
            let cell = state.ui.selection.active.clone();
            if !ensure_cell_editable(state, &cell) {
                return EditorInputResult::Redraw;
            }
            let original = displayed_cell_text(state, &cell);
            state.ui.begin_overwrite_cell_edit(cell, original, "");
            commit_active_cell_edit(state);
            EditorInputResult::Redraw
        }
        _ => EditorInputResult::Ignored,
    }
}

fn ensure_cell_editable(state: &mut AppState, cell: &CellRef) -> bool {
    let Some(table) = state.active_table.as_ref() else {
        return true;
    };
    if cell.column as usize > table.columns.len() {
        return true;
    }
    if cell.row <= super::display_header_row_count(table) {
        state
            .ui
            .set_status("Column names are managed from the column header");
        return false;
    }
    if !table.is_materialized {
        state
            .ui
            .set_status("Finishing import before this table can be edited...");
        return false;
    }
    true
}

pub(super) fn arrow_key_delta(key: &Key) -> (i32, i32) {
    match key {
        Key::Named(NamedKey::ArrowUp) => (-1, 0),
        Key::Named(NamedKey::ArrowDown) => (1, 0),
        Key::Named(NamedKey::ArrowLeft) => (0, -1),
        Key::Named(NamedKey::ArrowRight) => (0, 1),
        _ => (0, 0),
    }
}

pub(super) fn next_cell_click_count(
    last_click: Option<&CellClick>,
    cell: &CellRef,
    now: Instant,
) -> u8 {
    let Some(last) = last_click else {
        return 1;
    };
    if last.cell != *cell {
        return 1;
    }
    let is_repeated_click = now
        .checked_duration_since(last.at)
        .is_some_and(|elapsed| elapsed.as_millis() <= CELL_DOUBLE_CLICK_MAX_MS);
    if is_repeated_click {
        last.count.saturating_add(1).min(3)
    } else {
        1
    }
}

pub(super) fn handle_active_editor_click(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> bool {
    let Some(cell) = state.ui.editing_cell().cloned() else {
        return false;
    };
    let now = Instant::now();
    let click_count = next_cell_click_count(state.last_cell_click.as_ref(), &cell, now);
    state.last_cell_click = Some(CellClick {
        cell,
        at: now,
        count: click_count,
    });
    if click_count >= 3 {
        state.editor_text_drag = None;
        return apply_editor_intent(state, EditorIntent::SelectAll).changed;
    }
    if click_count == 2 {
        return begin_editor_word_drag(state, position, scale_factor);
    }
    begin_editor_text_drag(state, position, scale_factor)
}

pub(super) fn begin_editor_text_drag(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> bool {
    let Some(grapheme_offset) =
        editor_character_offset_from_position(state, position, scale_factor)
    else {
        return false;
    };
    let anchor_offset = if state.modifiers.shift_key() {
        state
            .ui
            .editor_selection_grapheme_offsets()
            .map_or(grapheme_offset, |(anchor, _)| anchor)
    } else {
        grapheme_offset
    };
    state.editor_text_drag = Some(EditorTextDrag::Character { anchor_offset });
    apply_editor_intent(
        state,
        EditorIntent::SetCaret {
            grapheme_offset,
            extend: state.modifiers.shift_key(),
        },
    )
    .changed
}

pub(super) fn extend_editor_text_selection_from_position(
    state: &mut AppState,
    drag: EditorTextDrag,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> bool {
    let Some(caret_offset) = editor_character_offset_from_position(state, position, scale_factor)
    else {
        return false;
    };
    let intent = match drag {
        EditorTextDrag::Character { anchor_offset } => EditorIntent::SetSelection {
            anchor_grapheme_offset: anchor_offset,
            caret_grapheme_offset: caret_offset,
        },
        EditorTextDrag::Word {
            anchor_start_offset,
            anchor_end_offset,
        } => EditorIntent::SelectWordRange {
            anchor_start_grapheme_offset: anchor_start_offset,
            anchor_end_grapheme_offset: anchor_end_offset,
            caret_grapheme_offset: caret_offset,
        },
    };
    apply_editor_intent(state, intent).changed
}

pub(super) fn begin_editor_word_drag(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> bool {
    let Some(grapheme_offset) =
        editor_character_offset_from_position(state, position, scale_factor)
    else {
        return false;
    };
    let changed =
        apply_editor_intent(state, EditorIntent::SelectWordAt { grapheme_offset }).changed;
    if let Some((anchor_start_offset, anchor_end_offset)) = state
        .ui
        .editor_word_grapheme_range_at_offset(grapheme_offset)
    {
        state.editor_text_drag = Some(EditorTextDrag::Word {
            anchor_start_offset,
            anchor_end_offset,
        });
    }
    changed
}

pub(super) fn editor_character_offset_from_position(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
    scale_factor: f64,
) -> Option<usize> {
    let (x, y, width, height) = active_editor_rect(state)?;
    let buffer = state.ui.grid_edit_state.editing_buffer()?.to_string();
    let (local_x, _) = grid_local_position(position, scale_factor);
    let (_, local_y) = grid_local_position(position, scale_factor);
    let table_scale = scale_factor.max(1.0) * state.ui.table_zoom();
    let text_left = x + CELL_EDITOR_TEXT_PAD_PX * table_scale;
    let text_height = f64::from(GRID_CELL_TEXT_HEIGHT) * table_scale;
    let font_size = f64::from(GRID_CELL_FONT_SIZE) * table_scale;
    let text_top = y + ((height - text_height) * 0.5).max(3.0 * table_scale);
    let text_width = (width - 18.0 * table_scale).max(12.0 * table_scale);
    let byte_index = state
        .renderer
        .hit_test_editor_text(
            &buffer,
            text_width as f32,
            text_height as f32,
            font_size as f32,
            (local_x - text_left) as f32,
            (local_y - text_top) as f32,
        )
        .unwrap_or_else(|| {
            let char_width = (CELL_EDITOR_APPROX_CHAR_WIDTH_PX * table_scale).max(1.0);
            let char_offset = ((local_x - text_left + char_width * 0.5) / char_width)
                .floor()
                .max(0.0) as usize;
            byte_index_for_grapheme_offset_fallback(&buffer, char_offset)
        });
    state.ui.editor_grapheme_offset_for_byte_index(byte_index)
}

pub(super) fn byte_index_for_grapheme_offset_fallback(text: &str, grapheme_offset: usize) -> usize {
    text.char_indices()
        .nth(grapheme_offset)
        .map_or(text.len(), |(index, _)| index)
}

pub(super) fn position_is_over_active_editor(
    state: &mut AppState,
    position: PhysicalPosition<f64>,
) -> bool {
    let Some((x, y, width, height)) = active_editor_rect(state) else {
        return false;
    };
    let (local_x, local_y) = grid_local_position(position, state.renderer.scale_factor());
    rect_contains(local_x, local_y, x, y, width, height)
}

pub(super) fn active_editor_rect(state: &mut AppState) -> Option<(f64, f64, f64, f64)> {
    let GridEditState::Editing { cell, buffer, .. } = &state.ui.grid_edit_state else {
        return None;
    };
    let window = state.ui.viewport.visible_window().ok()?;
    let row_index = u64::from(cell.row.saturating_sub(1));
    let column_index = cell.column.saturating_sub(1);
    if row_index < window.start_row || column_index < window.start_column {
        return None;
    }
    let visible_row = row_index.saturating_sub(window.start_row);
    let visible_column = column_index.saturating_sub(window.start_column);
    if visible_row >= u64::from(window.row_count) || visible_column >= window.column_count {
        return None;
    }

    let metrics = &state.ui.viewport.metrics;
    let cell_width = window
        .column_widths
        .get(visible_column as usize)
        .copied()
        .unwrap_or(metrics.column_width);
    let cell_height = window
        .row_heights
        .get(visible_row as usize)
        .copied()
        .unwrap_or(metrics.row_height);
    let x = f64::from(metrics.header_width) + visible_column_offset_x(&window, visible_column);
    let y = f64::from(metrics.header_height) + visible_row_offset_y(&window, visible_row);
    let table_scale = state.renderer.scale_factor().max(1.0) * state.ui.table_zoom();
    let fallback_text_width =
        buffer.chars().count() as f64 * CELL_EDITOR_APPROX_CHAR_WIDTH_PX * table_scale
            + 24.0 * table_scale;
    let body_right = f64::from(
        state
            .ui
            .viewport
            .pixel_width
            .saturating_sub(metrics.scrollbar_thickness),
    );
    let available_width = (body_right - x).max(f64::from(cell_width));
    let measured_text_width = state
        .renderer
        .measure_editor_text_width(
            buffer,
            (available_width - 18.0 * table_scale) as f32,
            (f64::from(GRID_CELL_TEXT_HEIGHT) * table_scale) as f32,
            (f64::from(GRID_CELL_FONT_SIZE) * table_scale) as f32,
        )
        .map_or(fallback_text_width, |width| {
            f64::from(width) + 24.0 * table_scale
        });
    let width = f64::from(cell_width)
        .max(measured_text_width)
        .min(available_width);
    Some((x, y, width, f64::from(cell_height)))
}

pub(super) fn commit_active_cell_edit(state: &mut AppState) -> bool {
    let table_target = state
        .ui
        .editing_cell()
        .and_then(|cell| materialized_table_cell_target(state, cell));
    if let Some((cell, value)) = state.ui.commit_cell_edit() {
        if let Some((row_id, column)) = table_target {
            state.ui.edited_cells.remove(&cell);
            update_snapshot_cell_value(state, &cell, &value);
            queue_table_cell_write(state, row_id, column, value);
        } else {
            queue_sparse_cell_write(state, cell, value);
        }
        state.renderer.window().set_ime_allowed(false);
        state.last_text_commit = None;
        state.last_cell_click = None;
        return true;
    }
    state.last_cell_click = None;
    false
}

fn queue_sparse_cell_write(state: &mut AppState, cell: CellRef, value: String) {
    state.pending_cell_writes.push(CellEditWrite::Sparse {
        sheet_id: state.ui.active_sheet,
        cell,
        value,
        updated_at: unix_seconds(),
    });
}

fn queue_table_cell_write(state: &mut AppState, row_id: u64, column: String, value: String) {
    let Some(table) = state.active_table.as_ref() else {
        return;
    };
    state.pending_cell_writes.push(CellEditWrite::Table {
        table_name: table.table_name.clone(),
        available_columns: table.columns.clone(),
        row_id,
        column,
        value,
    });
}

fn materialized_table_cell_target(state: &AppState, cell: &CellRef) -> Option<(u64, String)> {
    let table = state.active_table.as_ref()?;
    if !table.is_materialized {
        return None;
    }
    let snapshot = state.ui.snapshot.as_ref()?;
    stable_table_cell_target(snapshot, &table.columns, cell)
}

pub(super) fn stable_table_cell_target(
    snapshot: &cellium_ui::GridSnapshot,
    columns: &[String],
    cell: &CellRef,
) -> Option<(u64, String)> {
    let display_row = u64::from(cell.row.saturating_sub(1));
    let display_column = cell.column.saturating_sub(1);
    if display_row < snapshot.start_row || display_column < snapshot.start_column {
        return None;
    }
    let row_index = display_row.saturating_sub(snapshot.start_row) as usize;
    let column_index = display_column.saturating_sub(snapshot.start_column) as usize;
    let row_id = snapshot.row_ids.get(row_index).copied().flatten()?;
    snapshot.rows.get(row_index)?.get(column_index)?;
    let column = columns.get(display_column as usize)?.clone();
    Some((row_id, column))
}

fn update_snapshot_cell_value(state: &mut AppState, cell: &CellRef, value: &str) {
    let Some(snapshot) = state.ui.snapshot.as_mut() else {
        return;
    };
    let display_row = u64::from(cell.row.saturating_sub(1));
    let display_column = cell.column.saturating_sub(1);
    if display_row < snapshot.start_row || display_column < snapshot.start_column {
        return;
    }
    let row_index = display_row.saturating_sub(snapshot.start_row) as usize;
    let column_index = display_column.saturating_sub(snapshot.start_column) as usize;
    if let Some(row) = snapshot.rows.get_mut(row_index)
        && let Some(cell_value) = row.get_mut(column_index)
    {
        *cell_value = value.to_string();
    }
}

pub(super) fn cancel_active_cell_edit(state: &mut AppState) -> bool {
    let cancelled = state.ui.cancel_cell_edit();
    if cancelled {
        state.renderer.window().set_ime_allowed(false);
        state.last_text_commit = None;
    }
    state.last_cell_click = None;
    cancelled
}

pub(super) fn move_selection_after_edit(
    state: &mut AppState,
    row_delta: i32,
    column_delta: i32,
) -> EditorInputResult {
    let before = state.ui.viewport.visible_window().ok();
    move_selection_with_animation(state, row_delta, column_delta, false);
    let (row_count, column_count) = active_table_extents(state.active_table.as_ref());
    state.ui.clamp_to_table(row_count, column_count);
    if visible_query_window_changed(
        before.as_ref(),
        state.ui.viewport.visible_window().ok().as_ref(),
    ) {
        EditorInputResult::Query
    } else {
        EditorInputResult::Redraw
    }
}

pub(super) fn printable_text_from_key(key: &Key, text: Option<&str>) -> Option<String> {
    let text = text.or_else(|| key.to_text())?;
    if text.is_empty()
        || matches!(text, "\r" | "\n" | "\t")
        || !text.chars().all(|character| !character.is_control())
    {
        return None;
    }
    Some(text.to_string())
}

pub(super) fn displayed_cell_text(state: &AppState, cell: &CellRef) -> String {
    if let Some(value) = state.ui.edited_value(cell) {
        return sanitize_single_line_text(value);
    }

    let Some(snapshot) = state.ui.snapshot.as_ref() else {
        return String::new();
    };
    let display_row = u64::from(cell.row.saturating_sub(1));
    let display_column = cell.column.saturating_sub(1);
    if display_row < snapshot.start_row || display_column < snapshot.start_column {
        return String::new();
    }
    let row_index = display_row.saturating_sub(snapshot.start_row) as usize;
    let column_index = display_column.saturating_sub(snapshot.start_column) as usize;
    snapshot
        .rows
        .get(row_index)
        .and_then(|row| row.get(column_index))
        .cloned()
        .map(|text| sanitize_single_line_text(&text))
        .unwrap_or_default()
}

pub(super) fn sanitize_single_line_text(value: &str) -> String {
    if value.chars().all(|character| !character.is_control()) {
        value.to_string()
    } else {
        value
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect()
    }
}
