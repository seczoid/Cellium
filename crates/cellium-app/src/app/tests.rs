use super::*;
use std::time::Duration;

#[test]
fn snapshot_has_prerender_margin_when_window_is_inside_cached_band() {
    let snapshot = grid_snapshot(0, 0, 1_000, 40);
    let window = visible_window(300, 10, 10, 4);

    assert!(snapshot_has_prerender_margin(&snapshot, &window));
}

#[test]
fn snapshot_requests_prefetch_near_cached_row_edge() {
    let snapshot = grid_snapshot(0, 0, 1_000, 12);
    let window = visible_window(800, 10, 3, 4);

    assert!(!snapshot_has_prerender_margin(&snapshot, &window));
}

#[test]
fn visible_only_query_policy_accepts_exact_viewport_coverage() {
    let snapshot = grid_snapshot(800, 3, 10, 4);
    let window = visible_window(800, 10, 3, 4);

    assert!(snapshot_satisfies_policy(
        &snapshot,
        &window,
        QueryWindowPolicy::VisibleOnly
    ));
}

#[test]
fn prefetch_query_policy_rejects_snapshot_without_margin() {
    let snapshot = grid_snapshot(800, 3, 10, 4);
    let window = visible_window(800, 10, 3, 4);

    assert!(!snapshot_satisfies_policy(
        &snapshot,
        &window,
        QueryWindowPolicy::Prefetch
    ));
}

#[test]
fn snapshot_covers_projected_columns_by_row_width() {
    let snapshot = grid_snapshot(10, 4, 100, 8);
    let window = visible_window(20, 10, 6, 3);

    assert!(snapshot_covers_window(&snapshot, &window));
}

#[test]
fn snapshot_does_not_cover_columns_missing_from_cached_rows() {
    let snapshot = grid_snapshot(10, 4, 100, 8);
    let window = visible_window(20, 10, 30, 3);

    assert!(!snapshot_covers_window(&snapshot, &window));
}

#[test]
fn hovered_column_header_accounts_for_horizontal_scroll() {
    let mut ui = UiState::new(cellium_core::SheetId(1));
    ui.resize_viewport(800, 600, 1.0);
    ui.viewport.scroll_x_px = f64::from(ui.viewport.metrics.column_width);
    let position = PhysicalPosition {
        x: f64::from(ui.viewport.metrics.header_width + 10),
        y: f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + 8),
    };

    assert_eq!(hovered_column_header(&ui, position, 1.0), Some(1));
}

#[test]
fn grid_hit_detects_body_cell() {
    let mut ui = UiState::new(cellium_core::SheetId(1));
    ui.resize_viewport(800, 600, 1.0);
    let position = PhysicalPosition {
        x: f64::from(ui.viewport.metrics.header_width + 12),
        y: f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + ui.viewport.metrics.header_height + 8),
    };

    assert_eq!(
        grid_hit(&ui, position, 1.0),
        Some(GridHit::Cell(CellRef::new(1, 1)))
    );
}

#[test]
fn resize_edge_hover_detects_column_and_row_edges() {
    let mut ui = UiState::new(cellium_core::SheetId(1));
    ui.resize_viewport(800, 600, 1.0);

    assert_eq!(
        resize_edge_hover(
            &ui,
            PhysicalPosition {
                x: f64::from(ui.viewport.metrics.header_width + ui.viewport.metrics.column_width),
                y: f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + 8),
            },
            1.0,
        ),
        Some(ChromeHoverTarget::ColumnResize(1))
    );
    assert_eq!(
        resize_edge_hover(
            &ui,
            PhysicalPosition {
                x: 12.0,
                y: f64::from(
                    FORMULA_BAR_TOP
                        + FORMULA_BAR_HEIGHT
                        + ui.viewport.metrics.header_height
                        + ui.viewport.metrics.row_height
                ),
            },
            1.0,
        ),
        Some(ChromeHoverTarget::RowResize(1))
    );
}

#[test]
fn resize_edge_hover_uses_variable_column_and_row_sizes() {
    let mut ui = UiState::new(cellium_core::SheetId(1));
    ui.resize_viewport(800, 600, 1.0);
    ui.viewport.set_column_width_at(0, 240);
    ui.viewport.set_row_height_at(0, 52);

    assert_eq!(
        resize_edge_hover(
            &ui,
            PhysicalPosition {
                x: f64::from(ui.viewport.metrics.header_width) + 240.0,
                y: f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + 8),
            },
            1.0,
        ),
        Some(ChromeHoverTarget::ColumnResize(1))
    );
    assert_eq!(
        resize_edge_hover(
            &ui,
            PhysicalPosition {
                x: 12.0,
                y: f64::from(
                    FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + ui.viewport.metrics.header_height
                ) + 52.0,
            },
            1.0,
        ),
        Some(ChromeHoverTarget::RowResize(1))
    );
}

#[test]
fn grid_hit_uses_variable_column_and_row_sizes() {
    let mut ui = UiState::new(cellium_core::SheetId(1));
    ui.resize_viewport(800, 600, 1.0);
    ui.viewport.set_column_width_at(0, 240);
    ui.viewport.set_row_height_at(0, 52);
    let body_y =
        f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + ui.viewport.metrics.header_height + 8);

    assert_eq!(
        grid_hit(
            &ui,
            PhysicalPosition {
                x: f64::from(ui.viewport.metrics.header_width) + 220.0,
                y: body_y,
            },
            1.0,
        ),
        Some(GridHit::Cell(CellRef::new(1, 1)))
    );
    assert_eq!(
        grid_hit(
            &ui,
            PhysicalPosition {
                x: f64::from(ui.viewport.metrics.header_width) + 260.0,
                y: body_y + 52.0,
            },
            1.0,
        ),
        Some(GridHit::Cell(CellRef::new(2, 2)))
    );
}

#[test]
fn cursor_icon_tracks_grid_and_chrome_regions() {
    assert_eq!(
        cursor_icon_for_state_ref(None, Some(&ChromeHoverTarget::GridCell(CellRef::new(1, 1)))),
        CursorIcon::Cell
    );
    assert_eq!(
        cursor_icon_for_state_ref(None, Some(&ChromeHoverTarget::FormulaBar)),
        CursorIcon::Text
    );
    assert_eq!(
        cursor_icon_for_state_ref(None, Some(&ChromeHoverTarget::ColumnResize(2))),
        CursorIcon::ColResize
    );
}

#[test]
fn next_cell_click_count_tracks_triple_clicks_on_same_cell() {
    let cell = CellRef::new(4, 2);
    let now = Instant::now();
    let first = CellClick {
        cell: cell.clone(),
        at: now,
        count: 1,
    };
    let second = CellClick {
        cell: cell.clone(),
        at: now,
        count: next_cell_click_count(Some(&first), &cell, now),
    };

    assert_eq!(next_cell_click_count(Some(&second), &cell, now), 3);
}

#[test]
fn next_cell_click_count_resets_after_timeout() {
    let cell = CellRef::new(4, 2);
    let first_at = Instant::now();
    let last = CellClick {
        cell: cell.clone(),
        at: first_at,
        count: 2,
    };
    let after_timeout = first_at + Duration::from_millis(CELL_DOUBLE_CLICK_MAX_MS as u64 + 1);

    assert_eq!(next_cell_click_count(Some(&last), &cell, after_timeout), 1);
}

#[test]
fn duplicate_text_commit_is_suppressed_across_keyboard_and_ime() {
    let now = Instant::now();
    let last = TextCommit {
        text: " ".to_string(),
        source: TextCommitSource::Keyboard,
        at: now,
    };

    assert!(is_duplicate_text_commit(
        Some(&last),
        " ",
        TextCommitSource::Ime,
        now + Duration::from_millis(TEXT_COMMIT_DUPLICATE_WINDOW_MS)
    ));
}

#[test]
fn duplicate_text_commit_allows_keyboard_repeats() {
    let now = Instant::now();
    let last = TextCommit {
        text: " ".to_string(),
        source: TextCommitSource::Keyboard,
        at: now,
    };

    assert!(!is_duplicate_text_commit(
        Some(&last),
        " ",
        TextCommitSource::Keyboard,
        now + Duration::from_millis(1)
    ));
}

#[test]
fn grid_hit_detects_row_column_and_corner_headers() {
    let mut ui = UiState::new(cellium_core::SheetId(1));
    ui.resize_viewport(800, 600, 1.0);

    assert_eq!(
        grid_hit(
            &ui,
            PhysicalPosition {
                x: f64::from(ui.viewport.metrics.header_width + 12),
                y: f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + 8),
            },
            1.0,
        ),
        Some(GridHit::Column(1))
    );
    assert_eq!(
        grid_hit(
            &ui,
            PhysicalPosition {
                x: 12.0,
                y: f64::from(
                    FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + ui.viewport.metrics.header_height + 8,
                ),
            },
            1.0,
        ),
        Some(GridHit::Row(1))
    );
    assert_eq!(
        grid_hit(
            &ui,
            PhysicalPosition {
                x: 12.0,
                y: f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT + 8),
            },
            1.0,
        ),
        Some(GridHit::Corner)
    );
}

#[test]
fn visible_query_window_changed_ignores_subpixel_only_motion() {
    let before = visible_window(10, 20, 3, 4);
    let mut after = before.clone();
    after.row_offset_px = 12.5;
    after.column_offset_px = 7.25;

    assert!(!visible_query_window_changed(Some(&before), Some(&after)));
}

#[test]
fn visible_query_window_changed_detects_row_or_column_boundary_crossing() {
    let before = visible_window(10, 20, 3, 4);
    let after = visible_window(11, 20, 3, 4);

    assert!(visible_query_window_changed(Some(&before), Some(&after)));
}

#[test]
fn pixel_scroll_schedules_query_when_visible_window_changes() {
    let mut ui = test_ui_state();
    let before = ui.viewport.visible_window().ok();

    ui.scroll_y_pixels(f64::from(ui.viewport.metrics.row_height));

    assert!(viewport_needs_query_after_scroll(&ui, before.as_ref()));
}

#[test]
fn stale_query_result_can_replace_current_snapshot_when_it_covers_viewport() {
    let snapshot = grid_snapshot(10, 4, 100, 8);
    let window = visible_window(20, 10, 6, 3);

    assert!(snapshot_should_replace_current(
        &snapshot,
        Some(&window),
        1,
        2
    ));
}

#[test]
fn stale_query_result_is_ignored_when_it_does_not_cover_viewport() {
    let snapshot = grid_snapshot(10, 4, 100, 8);
    let window = visible_window(20, 10, 30, 3);

    assert!(!snapshot_should_replace_current(
        &snapshot,
        Some(&window),
        1,
        2
    ));
}

#[test]
fn display_rows_prepends_csv_header_row_at_top_only() {
    let columns = vec!["name".to_string(), "phone".to_string()];
    let data_rows = vec![vec!["Acme".to_string(), "555".to_string()]];

    let rows = display_rows(&columns, 0, 2, 1, 0, data_rows.clone());

    assert_eq!(
        rows,
        vec![
            vec!["name".to_string(), "phone".to_string()],
            vec!["Acme".to_string(), "555".to_string()]
        ]
    );
    assert_eq!(display_rows(&columns, 0, 2, 1, 4, data_rows), rows[1..]);
}

#[test]
fn selection_autoscroll_velocity_accelerates_near_edges() {
    let mut ui = test_ui_state();
    ui.resize_viewport(800, 600, 1.0);
    let position = PhysicalPosition {
        x: f64::from(ui.viewport.pixel_width - ui.viewport.metrics.scrollbar_thickness) - 2.0,
        y: f64::from(FORMULA_BAR_TOP + FORMULA_BAR_HEIGHT)
            + f64::from(ui.viewport.pixel_height - ui.viewport.metrics.scrollbar_thickness)
            - 2.0,
    };

    let velocity = selection_autoscroll_velocity(&ui, SelectionTargetKind::Cell, position, 1.0);

    assert!(velocity.x_px_s > 0.0 && velocity.y_px_s > 0.0);
}

#[test]
fn wheel_scroll_impulse_adds_velocity_without_moving_immediately() {
    let velocity = wheel_velocity_after_impulse(WheelImpulse {
        velocity: ScrollVelocity {
            x_px_s: 0.0,
            y_px_s: 0.0,
        },
        horizontal_lines: 0.0,
        vertical_lines: -1.0,
        row_height: 26,
        column_width: 150,
        body_width: 900,
        body_height: 600,
    });

    assert!(velocity.y_px_s > 0.0);
}

#[test]
fn smoothed_pan_velocity_tracks_recent_drag_sample() {
    let velocity = smoothed_pan_velocity(
        ScrollVelocity {
            x_px_s: 0.0,
            y_px_s: 0.0,
        },
        ScrollVelocity {
            x_px_s: 1_000.0,
            y_px_s: -500.0,
        },
    );

    assert_eq!(
        velocity,
        ScrollVelocity {
            x_px_s: 650.0,
            y_px_s: -325.0,
        }
    );
}

#[test]
fn clamped_scroll_velocity_caps_pan_throw_to_viewport_size() {
    let ui = test_ui_state();
    let velocity = clamped_scroll_velocity_to_viewport(
        &ui,
        ScrollVelocity {
            x_px_s: 100_000.0,
            y_px_s: -100_000.0,
        },
    );

    assert!(velocity.x_px_s < 100_000.0 && velocity.y_px_s > -100_000.0);
}

#[test]
fn advance_scroll_inertia_moves_with_subpixel_precision() {
    let mut ui = test_ui_state();
    let mut velocity = ScrollVelocity {
        x_px_s: 0.0,
        y_px_s: 120.0,
    };

    assert!(advance_scroll_inertia_for_viewport(
        &mut ui,
        Some(1_000),
        20,
        &mut velocity,
        1.0 / 120.0
    ));
    assert!(ui.viewport.scroll_y_px > 0.0 && ui.viewport.scroll_y_px < 2.0);
}

#[test]
fn edge_clamp_stops_inertia_at_top() {
    let mut ui = test_ui_state();
    let mut velocity = ScrollVelocity {
        x_px_s: 0.0,
        y_px_s: -800.0,
    };

    assert!(advance_scroll_inertia_for_viewport(
        &mut ui,
        Some(1_000),
        20,
        &mut velocity,
        1.0 / 60.0
    ));
    assert_eq!(ui.viewport.scroll_y_px, 0.0);
    assert_eq!(velocity.y_px_s, 0.0);
}

#[test]
fn pending_row_count_applies_only_to_matching_table_and_path() {
    let table = active_table("import_orders", "/tmp/orders.csv");
    let mut pending = Some(RowCountResult {
        logical_table_name: "import_orders".to_string(),
        source_path: PathBuf::from("/tmp/orders.csv"),
        row_count: 42,
    });

    assert_eq!(
        take_matching_pending_row_count(&mut pending, &table),
        Some(42)
    );
}

#[test]
fn pending_row_count_ignores_stale_import() {
    let table = active_table("import_orders", "/tmp/current.csv");
    let mut pending = Some(RowCountResult {
        logical_table_name: "import_orders".to_string(),
        source_path: PathBuf::from("/tmp/old.csv"),
        row_count: 42,
    });

    assert_eq!(take_matching_pending_row_count(&mut pending, &table), None);
}

#[test]
fn cycle_sort_view_moves_ascending_to_descending_to_clear() {
    let initial = cellium_core::TableViewState::default();

    let ascending = cycle_sort_view(&initial, "amount", false);
    let descending = cycle_sort_view(&ascending, "amount", false);
    let cleared = cycle_sort_view(&descending, "amount", false);

    assert_eq!(
        (
            ascending.sorts[0].direction,
            descending.sorts[0].direction,
            cleared.sorts.len(),
        ),
        (
            cellium_core::SortDirection::Ascending,
            cellium_core::SortDirection::Descending,
            0,
        )
    );
}

#[test]
fn filter_to_value_view_toggles_selected_value_filter() {
    let initial = cellium_core::TableViewState::default();

    let filtered = filter_to_value_view(&initial, "status", "active");
    let cleared = filter_to_value_view(&filtered, "status", "active");

    assert_eq!((filtered.filters.len(), cleared.filters.len()), (1, 0));
}

#[test]
fn stable_table_cell_target_uses_snapshot_row_identity() {
    let mut snapshot = grid_snapshot(0, 0, 2, 1);
    snapshot.row_ids = vec![None, Some(42)];
    snapshot.rows[1][0] = "Acme".to_string();

    let target = stable_table_cell_target(&snapshot, &["company".to_string()], &CellRef::new(2, 1));

    assert_eq!(target, Some((42, "company".to_string())));
}

#[test]
fn stable_table_cell_target_rejects_header_row_without_identity() {
    let mut snapshot = grid_snapshot(0, 0, 1, 1);
    snapshot.row_ids = vec![None];

    let target = stable_table_cell_target(&snapshot, &["company".to_string()], &CellRef::new(1, 1));

    assert_eq!(target, None);
}

fn grid_snapshot(start_row: u64, start_column: u32, rows: usize, columns: usize) -> GridSnapshot {
    GridSnapshot {
        table_name: "table".to_string(),
        source_name: "source.csv".to_string(),
        row_count: None,
        start_row,
        start_column,
        header_row_count: 0,
        columns: (0..64).map(|index| format!("column_{index}")).collect(),
        rows: vec![vec![String::new(); columns]; rows],
        row_ids: vec![None; rows],
    }
}

fn visible_window(
    start_row: u64,
    row_count: u32,
    start_column: u32,
    column_count: u32,
) -> VisibleWindow {
    VisibleWindow {
        start_row,
        row_count,
        start_column,
        column_count,
        row_offset_px: 0.0,
        column_offset_px: 0.0,
        row_heights: vec![cellium_ui::GRID_ROW_HEIGHT; row_count as usize],
        column_widths: vec![cellium_ui::GRID_COLUMN_WIDTH; column_count as usize],
    }
}

fn test_ui_state() -> UiState {
    let mut ui = UiState::new(cellium_core::SheetId(1));
    ui.resize_viewport(1_000, 700, 1.0);
    ui
}

fn active_table(table_name: &str, source_path: &str) -> ActiveTable {
    ActiveTable {
        table_name: table_name.to_string(),
        source_name: "orders.csv".to_string(),
        source_path: PathBuf::from(source_path),
        source_kind: DataFileKind::Csv,
        source_row_count: None,
        row_count: None,
        columns: vec!["id".to_string()],
        view: cellium_core::TableViewState::default(),
        is_materialized: false,
        workbook_path: None,
        sheet_id: cellium_core::SheetId(1),
        table_id: cellium_core::TableId(1),
    }
}
