use super::*;

#[test]
fn visible_window_includes_partially_visible_row() {
    let viewport = GridViewport {
        scroll_y_px: 10.0 * f64::from(GRID_ROW_HEIGHT),
        scroll_x_px: 2.0 * f64::from(GRID_COLUMN_WIDTH),
        pixel_width: 300,
        pixel_height: 100,
        metrics: GridMetrics::default(),
        column_widths: BTreeMap::new(),
        row_heights: BTreeMap::new(),
    };

    assert_eq!(
        viewport.visible_window().unwrap(),
        VisibleWindow {
            start_row: 10,
            row_count: 4,
            start_column: 2,
            column_count: 4,
            row_offset_px: 0.0,
            column_offset_px: 0.0,
            row_heights: vec![GRID_ROW_HEIGHT; 4],
            column_widths: vec![GRID_COLUMN_WIDTH; 4],
        }
    );
}

#[test]
fn visible_window_preserves_subpixel_offsets() {
    let viewport = GridViewport {
        scroll_y_px: 10.5 * f64::from(GRID_ROW_HEIGHT),
        scroll_x_px: 2.25 * f64::from(GRID_COLUMN_WIDTH),
        pixel_width: 300,
        pixel_height: 100,
        metrics: GridMetrics::default(),
        column_widths: BTreeMap::new(),
        row_heights: BTreeMap::new(),
    };

    assert_eq!(
        viewport.visible_window().unwrap(),
        VisibleWindow {
            start_row: 10,
            row_count: 5,
            start_column: 2,
            column_count: 4,
            row_offset_px: f64::from(GRID_ROW_HEIGHT) * 0.5,
            column_offset_px: f64::from(GRID_COLUMN_WIDTH) * 0.25,
            row_heights: vec![GRID_ROW_HEIGHT; 5],
            column_widths: vec![GRID_COLUMN_WIDTH; 4],
        }
    );
}

#[test]
fn enter_in_grid_begins_cell_edit() {
    let mut state = UiState::new(SheetId(1));

    let command = state.route_key(&Key::Named(NamedKey::Enter));

    assert_eq!(command, Some(UiCommand::BeginCellEdit));
}

#[test]
fn typing_while_selected_starts_overwrite_edit_and_replaces_content() {
    let mut state = UiState::new(SheetId(1));
    let cell = CellRef::new(2, 3);
    state.begin_overwrite_cell_edit(cell.clone(), "Acme", "N");

    let committed = state.commit_cell_edit();

    assert_eq!(committed, Some((cell.clone(), "N".to_string())));
    assert_eq!(state.edited_value(&cell), Some("N"));
}

#[test]
fn enter_or_f2_style_edit_loads_existing_content_at_end() {
    let mut state = UiState::new(SheetId(1));
    let cell = CellRef::new(2, 3);

    state.begin_in_place_cell_edit(cell, "Acme");

    assert_eq!(state.cell_edit_mode(), Some(EditMode::InPlace));
    assert_eq!(
        state.grid_edit_state.selection().map(|s| s.caret),
        Some("Acme".len())
    );
}

#[test]
fn cell_editor_backspace_respects_utf8_boundaries() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "éx");

    assert!(state.backspace_editor());

    assert_eq!(
        state
            .grid_edit_state
            .editing_buffer()
            .map(str::to_string)
            .as_deref(),
        Some("é")
    );
}

#[test]
fn cell_editor_backspace_respects_grapheme_boundaries() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "🇺🇸x");

    assert!(state.backspace_editor());
    assert!(state.backspace_editor());

    assert_eq!(state.grid_edit_state.editing_buffer(), Some(""));
}

#[test]
fn cell_editor_delete_respects_grapheme_boundaries() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "🇺🇸x");
    state.move_editor_cursor_to_start();

    assert!(state.delete_editor());

    assert_eq!(state.grid_edit_state.editing_buffer(), Some("x"));
}

#[test]
fn select_all_editor_text_then_backspace_clears_buffer() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");

    assert!(state.select_all_editor_text());
    assert!(state.backspace_editor());

    assert_eq!(state.grid_edit_state.editing_buffer(), Some(""));
}

#[test]
fn dashboard_visible_workbooks_returns_all_when_search_is_empty() {
    let workbooks = dashboard_workbook_samples();

    let visible = dashboard_visible_workbooks(&workbooks, DashboardSection::Home, "");

    assert_eq!(visible.len(), 2);
}

#[test]
fn dashboard_visible_workbooks_filters_by_name() {
    let workbooks = dashboard_workbook_samples();

    let visible = dashboard_visible_workbooks(&workbooks, DashboardSection::Home, "market");

    assert_eq!(visible.first().map(|workbook| workbook.id), Some(2));
}

#[test]
fn dashboard_visible_workbooks_filters_by_path() {
    let workbooks = dashboard_workbook_samples();

    let visible = dashboard_visible_workbooks(&workbooks, DashboardSection::Home, "imports");

    assert_eq!(visible.first().map(|workbook| workbook.id), Some(1));
}

#[test]
fn dashboard_first_visible_workbook_id_returns_first_filtered_result() {
    let workbooks = dashboard_workbook_samples();

    let first_id =
        dashboard_first_visible_workbook_id(&workbooks, DashboardSection::Home, "market");

    assert_eq!(first_id, Some(2));
}

fn dashboard_workbook_samples() -> Vec<DashboardWorkbook> {
    vec![
        DashboardWorkbook {
            id: 1,
            name: "Hardscapers import".to_string(),
            file_path: "/Users/seczoid/imports/hardscapers.cellium".to_string(),
            updated_label: "now".to_string(),
            starred: false,
        },
        DashboardWorkbook {
            id: 2,
            name: "Market model".to_string(),
            file_path: "/Users/seczoid/models/market.cellium".to_string(),
            updated_label: "today".to_string(),
            starred: true,
        },
    ]
}

#[test]
fn shift_left_extends_editor_text_selection() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");

    assert!(state.move_editor_cursor_left_with_selection(true));

    assert_eq!(
        state.grid_edit_state.selection(),
        Some(TextSelection {
            anchor: "hello".len(),
            caret: "hell".len()
        })
    );
}

#[test]
fn non_extending_left_collapses_selection_to_left_edge() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");
    state.set_editor_selection_to_grapheme_offsets(1, 4);

    assert!(
        state
            .apply_editor_intent(EditorIntent::Move {
                movement: EditorMove::Left,
                extend: false,
            })
            .changed
    );

    assert_eq!(
        state.grid_edit_state.selection(),
        Some(TextSelection {
            anchor: 1,
            caret: 1
        })
    );
}

#[test]
fn non_extending_right_collapses_selection_to_right_edge() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");
    state.set_editor_selection_to_grapheme_offsets(4, 1);

    assert!(
        state
            .apply_editor_intent(EditorIntent::Move {
                movement: EditorMove::Right,
                extend: false,
            })
            .changed
    );

    assert_eq!(
        state.grid_edit_state.selection(),
        Some(TextSelection {
            anchor: 4,
            caret: 4
        })
    );
}

#[test]
fn editor_intent_insert_replaces_selected_text() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");
    state.set_editor_selection_to_grapheme_offsets(1, 4);

    assert!(
        state
            .apply_editor_intent(EditorIntent::InsertText("a".to_string()))
            .changed
    );

    assert_eq!(state.grid_edit_state.editing_buffer(), Some("hao"));
}

#[test]
fn editor_intent_move_resets_caret_visibility() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");
    state.set_editor_caret_visible(false);

    state.apply_editor_intent(EditorIntent::Move {
        movement: EditorMove::Left,
        extend: false,
    });

    assert!(state.editor_caret_visible());
}

#[test]
fn set_caret_with_extend_keeps_existing_anchor() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");
    state.set_editor_caret_to_grapheme_offset(2);

    state.apply_editor_intent(EditorIntent::SetCaret {
        grapheme_offset: 5,
        extend: true,
    });

    assert_eq!(
        state.grid_edit_state.selection(),
        Some(TextSelection {
            anchor: 2,
            caret: 5
        })
    );
}

#[test]
fn paste_then_undo_and_redo_restores_editor_buffer() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hi");

    assert!(state.paste_editor_text("!"));
    assert!(state.undo_editor_edit());
    assert!(state.redo_editor_edit());

    assert_eq!(state.grid_edit_state.editing_buffer(), Some("hi!"));
}

#[test]
fn cut_editor_selection_removes_and_returns_selected_text() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello");
    state.set_editor_selection_to_character_offsets(1, 4);

    let cut = state.cut_editor_selection();

    assert_eq!(cut.as_deref(), Some("ell"));
    assert_eq!(state.grid_edit_state.editing_buffer(), Some("ho"));
}

#[test]
fn word_left_moves_to_previous_word_boundary() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello world");

    assert!(state.move_editor_cursor_word_left_with_selection(false));

    assert_eq!(
        state.grid_edit_state.selection(),
        Some(TextSelection {
            anchor: "hello ".len(),
            caret: "hello ".len()
        })
    );
}

#[test]
fn word_backspace_deletes_previous_word() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello world");

    assert!(state.backspace_editor_word());

    assert_eq!(state.grid_edit_state.editing_buffer(), Some("hello "));
}

#[test]
fn double_click_style_word_selection_selects_word_under_offset() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "hello world");

    assert!(state.select_editor_word_at_character_offset(7));

    assert_eq!(state.selected_editor_text().as_deref(), Some("world"));
}

#[test]
fn word_drag_selection_expands_by_whole_words() {
    let mut state = UiState::new(SheetId(1));
    state.begin_in_place_cell_edit(CellRef::new(1, 1), "alpha beta gamma");
    let (anchor_start, anchor_end) = state.editor_word_grapheme_range_at_offset(7).unwrap();

    assert!(
        state
            .apply_editor_intent(EditorIntent::SelectWordRange {
                anchor_start_grapheme_offset: anchor_start,
                anchor_end_grapheme_offset: anchor_end,
                caret_grapheme_offset: 13,
            })
            .changed
    );

    assert_eq!(state.selected_editor_text().as_deref(), Some("beta gamma"));
}

#[test]
fn cell_editor_cancel_discards_pending_text() {
    let mut state = UiState::new(SheetId(1));
    let cell = CellRef::new(1, 1);
    state.begin_in_place_cell_edit(cell.clone(), "old");
    assert!(state.insert_editor_text("new"));

    assert!(state.cancel_cell_edit());

    assert_eq!(state.edited_value(&cell), None);
}

#[test]
fn backspace_from_selected_enters_empty_overwrite_edit() {
    let mut state = UiState::new(SheetId(1));
    let cell = CellRef::new(3, 4);

    state.begin_overwrite_cell_edit(cell, "abc", "");

    assert_eq!(state.cell_edit_mode(), Some(EditMode::Overwrite));
    assert_eq!(state.grid_edit_state.editing_buffer(), Some(""));
}

#[test]
fn delete_while_selected_clears_active_cell_without_editing() {
    let mut state = UiState::new(SheetId(1));
    let cell = CellRef::new(3, 4);
    state.select_cell(cell.clone(), SelectionAction::default());

    let cleared = state.clear_selected_cell();

    assert_eq!(cleared, Some((cell.clone(), String::new())));
    assert_eq!(state.edited_value(&cell), Some(""));
}

#[test]
fn moving_selection_scrolls_viewport_when_active_cell_leaves_window() {
    let mut state = UiState::new(SheetId(1));
    state.resize_viewport(300, 260, 1.0);

    state.move_selection(12, 0);

    assert!(state.viewport.scroll_y_px > 0.0);
}

#[test]
fn shift_select_cell_extends_from_anchor() {
    let mut state = UiState::new(SheetId(1));
    state.select_cell(CellRef::new(2, 2), SelectionAction::default());

    state.select_cell(
        CellRef::new(4, 5),
        SelectionAction {
            extend: true,
            additive: false,
        },
    );

    assert_eq!(
        state.selection.ranges,
        vec![SelectionRange::Cells {
            start: CellRef::new(2, 2),
            end: CellRef::new(4, 5)
        }]
    );
}

#[test]
fn additive_cell_selection_keeps_existing_range() {
    let mut state = UiState::new(SheetId(1));

    state.select_cell(
        CellRef::new(5, 3),
        SelectionAction {
            extend: false,
            additive: true,
        },
    );

    assert_eq!(state.selection.ranges.len(), 2);
}

#[test]
fn row_and_column_header_selection_record_axis_ranges() {
    let mut state = UiState::new(SheetId(1));
    state.select_row(7, SelectionAction::default());
    assert_eq!(
        state.selection.ranges,
        vec![SelectionRange::Rows { start: 7, end: 7 }]
    );

    state.select_column(4, SelectionAction::default());
    assert_eq!(
        state.selection.ranges,
        vec![SelectionRange::Columns { start: 4, end: 4 }]
    );
}

#[test]
fn select_all_marks_sheet_range() {
    let mut state = UiState::new(SheetId(1));

    state.select_all();

    assert_eq!(state.selection.ranges, vec![SelectionRange::Sheet]);
}

#[test]
fn set_column_width_px_changes_only_target_column() {
    let mut state = UiState::new(SheetId(1));

    assert!(state.set_column_width_px(2, 240.0));

    assert_eq!(state.viewport.column_width_at(1), GRID_COLUMN_WIDTH);
    assert_eq!(state.viewport.column_width_at(2), 240);
    assert_eq!(state.viewport.column_width_at(3), GRID_COLUMN_WIDTH);
}

#[test]
fn set_row_height_px_changes_only_target_row() {
    let mut state = UiState::new(SheetId(1));

    assert!(state.set_row_height_px(3, 52.0));

    assert_eq!(state.viewport.row_height_at(2), GRID_ROW_HEIGHT);
    assert_eq!(state.viewport.row_height_at(3), 52);
    assert_eq!(state.viewport.row_height_at(4), GRID_ROW_HEIGHT);
}

#[test]
fn resize_viewport_scales_grid_metrics_for_high_dpi() {
    let mut state = UiState::new(SheetId(1));

    state.resize_viewport(2_400, 1_520, 2.0);

    assert_eq!(state.viewport.metrics.row_height, GRID_ROW_HEIGHT * 2);
}

#[test]
fn table_zoom_scales_grid_metrics_but_not_scrollbars() {
    let mut state = UiState::new(SheetId(1));
    state.resize_viewport(1_000, 800, 1.0);

    assert!(state.set_table_zoom(1.48, 1.0));

    assert_eq!(state.viewport.metrics.zoom, 1.48);
    assert_eq!(
        state.viewport.metrics.row_height,
        ((f64::from(GRID_ROW_HEIGHT) * 1.48).round() as u32)
    );
    assert_eq!(
        state.viewport.metrics.column_width,
        ((f64::from(GRID_COLUMN_WIDTH) * 1.48).round() as u32)
    );
    assert_eq!(
        state.viewport.metrics.scrollbar_thickness,
        GRID_SCROLLBAR_THICKNESS
    );
}

#[test]
fn anchored_table_zoom_preserves_cursor_row_and_column() {
    let mut state = UiState::new(SheetId(1));
    state.resize_viewport(1_200, 800, 1.0);
    state.viewport.scroll_x_px = f64::from(GRID_COLUMN_WIDTH) * 3.0 + 15.0;
    state.viewport.scroll_y_px = f64::from(GRID_ROW_HEIGHT) * 40.0 + 7.0;
    let anchor_x = f64::from(state.viewport.metrics.header_width) + 240.0;
    let anchor_y = f64::from(state.viewport.metrics.header_height) + 160.0;
    let old_column = cursor_column_position(&state.viewport, anchor_x);
    let old_row = cursor_row_position(&state.viewport, anchor_y);

    assert!(state.set_table_zoom_anchored(1.75, 1.0, anchor_x, anchor_y));

    assert!((cursor_column_position(&state.viewport, anchor_x) - old_column).abs() < 0.05);
    assert!((cursor_row_position(&state.viewport, anchor_y) - old_row).abs() < 0.05);
}

#[test]
fn vertical_scrollbar_clamps_thumb_for_huge_row_counts() {
    let viewport = GridViewport {
        scroll_y_px: 0.0,
        scroll_x_px: 0.0,
        pixel_width: 1_000,
        pixel_height: 800,
        metrics: GridMetrics::default(),
        column_widths: BTreeMap::new(),
        row_heights: BTreeMap::new(),
    };

    let layout = viewport.vertical_scrollbar(Some(1_000_000_000)).unwrap();

    assert_eq!(layout.thumb_height, f64::from(GRID_SCROLLBAR_MIN_THUMB));
}

#[test]
fn horizontal_scrollbar_maps_scroll_to_thumb_position() {
    let viewport = GridViewport {
        scroll_y_px: 0.0,
        scroll_x_px: 300.0,
        pixel_width: 516,
        pixel_height: 300,
        metrics: GridMetrics::default(),
        column_widths: BTreeMap::new(),
        row_heights: BTreeMap::new(),
    };

    let layout = viewport.horizontal_scrollbar(10).unwrap();

    assert_eq!(
        layout.max_scroll_px,
        f64::from(10 * GRID_COLUMN_WIDTH - 516 + GRID_HEADER_WIDTH + GRID_SCROLLBAR_THICKNESS)
    );
}

#[test]
fn clamp_to_table_uses_scrollbar_body_size() {
    let mut state = UiState::new(SheetId(1));
    state.resize_viewport(516, 300, 1.0);
    state.viewport.scroll_x_px = 10_000.0;

    state.clamp_to_table(Some(20), 10);

    assert_eq!(
        state.viewport.scroll_x_px,
        f64::from(10 * GRID_COLUMN_WIDTH - 516 + GRID_HEADER_WIDTH + GRID_SCROLLBAR_THICKNESS)
    );
}

fn cursor_column_position(viewport: &GridViewport, anchor_x: f64) -> f64 {
    (viewport.scroll_x_px
        + body_anchor_px(
            anchor_x,
            viewport.metrics.header_width,
            viewport.body_width(),
        ))
        / f64::from(viewport.metrics.column_width)
}

fn cursor_row_position(viewport: &GridViewport, anchor_y: f64) -> f64 {
    (viewport.scroll_y_px
        + body_anchor_px(
            anchor_y,
            viewport.metrics.header_height,
            viewport.body_height(),
        ))
        / f64::from(viewport.metrics.row_height)
}
