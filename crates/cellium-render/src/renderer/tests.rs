use super::*;

#[test]
fn intersect_rect_clips_partially_visible_row_to_body() {
    let row = Rect {
        x: 54.0,
        y: 112.0,
        width: 150.0,
        height: 18.0,
    };
    let body = Rect {
        x: 54.0,
        y: 127.0,
        width: 1_000.0,
        height: 500.0,
    };

    assert_eq!(
        intersect_rect(row, body),
        Some(Rect {
            x: 54.0,
            y: 127.0,
            width: 150.0,
            height: 3.0,
        })
    );
}

#[test]
fn intersect_rect_drops_labels_outside_the_clip_band() {
    let row = Rect {
        x: 54.0,
        y: 90.0,
        width: 150.0,
        height: 18.0,
    };
    let body = Rect {
        x: 54.0,
        y: 127.0,
        width: 1_000.0,
        height: 500.0,
    };

    assert_eq!(intersect_rect(row, body), None);
}

#[test]
fn truncate_cell_uses_single_ellipsis_inside_limit() {
    assert_eq!(
        truncate_cell("phone.phones_enricher.carrier", 18).as_ref(),
        "phone.phones_enri…"
    );
}

#[test]
fn truncate_cell_leaves_short_values_borrowed() {
    assert!(matches!(truncate_cell("short", 18), Cow::Borrowed("short")));
}

#[test]
fn truncate_cell_to_width_reveals_more_text_when_cell_gets_wider() {
    let value = "phone.phones_enricher.carrier";

    let narrow = truncate_cell_to_width(value, 72.0, 14.0);
    let wide = truncate_cell_to_width(value, 240.0, 14.0);

    assert!(wide.chars().count() > narrow.chars().count());
}

#[test]
fn truncate_cell_to_width_leaves_full_value_when_it_fits() {
    assert_eq!(
        truncate_cell_to_width("Greenscapes Cky", 240.0, 14.0).as_ref(),
        "Greenscapes Cky"
    );
}

#[test]
fn text_buffer_cursor_x_tracks_shaped_text_width() {
    let mut font_system = FontSystem::new();
    let mut buffer = TextBuffer::new(&mut font_system, Metrics::new(36.0, 44.0));
    buffer.set_size(&mut font_system, Some(800.0), Some(44.0));
    buffer.set_wrap(&mut font_system, Wrap::None);
    buffer.set_text(
        &mut font_system,
        "Greenscapes Cky",
        &default_text_attrs(primary_text(), Weight::NORMAL),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(&mut font_system, false);

    let end_x =
        text_buffer_cursor_x(&buffer, "Greenscapes Cky".len(), "Greenscapes Cky".len()).unwrap();

    assert!(end_x > 200.0);
}

#[test]
fn selection_fill_color_skips_single_cell_selection() {
    let cell = cellium_ui::CellRef::new(9, 4);

    assert_eq!(
        selection_fill_color(&SelectionRange::Cells {
            start: cell.clone(),
            end: cell,
        }),
        None
    );
}

#[test]
fn selection_fill_color_uses_subtle_alpha_for_ranges() {
    let start = cellium_ui::CellRef::new(9, 4);
    let end = cellium_ui::CellRef::new(12, 6);

    assert_eq!(
        selection_fill_color(&SelectionRange::Cells { start, end }),
        Some(rgba_f32(78, 216, 207, 0.055))
    );
}

#[test]
fn row_header_selection_rect_tracks_visible_row_span() {
    let window = visible_window(10, 8, 0, 4);
    let spec = test_grid_geometry();

    assert_eq!(
        row_header_selection_rect(12, 15, &window, &spec),
        Some(Rect {
            x: 0.0,
            y: spec.grid_top + spec.header_height + 2.0 * 26.0,
            width: spec.header_width,
            height: 3.0 * 26.0,
        })
    );
}

#[test]
fn column_header_selection_rect_tracks_visible_column_span() {
    let window = visible_window(0, 8, 2, 5);
    let spec = test_grid_geometry();

    assert_eq!(
        column_header_selection_rect(4, 6, &window, &spec),
        Some(Rect {
            x: spec.header_width + 2.0 * 150.0,
            y: spec.grid_top,
            width: 2.0 * 150.0,
            height: spec.header_height,
        })
    );
}

#[test]
fn selection_range_rect_uses_variable_column_widths() {
    let mut window = visible_window(0, 8, 0, 4);
    window.column_widths = vec![150, 220, 90, 150];
    let spec = test_grid_geometry();
    let range = SelectionRange::Cells {
        start: cellium_ui::CellRef::new(1, 2),
        end: cellium_ui::CellRef::new(1, 3),
    };

    assert_eq!(
        selection_range_rect(&range, &window, &spec).map(|rect| rect.width),
        Some(310.0)
    );
}

#[test]
fn selection_range_rect_uses_variable_row_heights() {
    let mut window = visible_window(0, 4, 0, 4);
    window.row_heights = vec![26, 44, 18, 26];
    let spec = test_grid_geometry();
    let range = SelectionRange::Cells {
        start: cellium_ui::CellRef::new(2, 1),
        end: cellium_ui::CellRef::new(3, 1),
    };

    assert_eq!(
        selection_range_rect(&range, &window, &spec).map(|rect| rect.height),
        Some(62.0)
    );
}

#[test]
fn ease_selection_progress_moves_fast_then_settles() {
    let eased = ease_selection_progress(0.5);

    assert!(eased > 0.5 && eased < 1.0);
}

#[test]
fn ease_caret_progress_slides_ahead_without_teleporting() {
    let eased = ease_caret_progress(0.5);

    assert!(eased > 0.5 && eased < 1.0);
}

#[test]
fn animated_selection_rect_interpolates_between_ranges() {
    let window = visible_window(0, 8, 0, 5);
    let spec = test_grid_geometry();
    let animation = cellium_ui::SelectionAnimation {
        from: SelectionRange::Cells {
            start: cellium_ui::CellRef::new(1, 1),
            end: cellium_ui::CellRef::new(1, 1),
        },
        to: SelectionRange::Cells {
            start: cellium_ui::CellRef::new(1, 1),
            end: cellium_ui::CellRef::new(3, 3),
        },
        progress: 0.0,
    };

    assert_eq!(
        animated_selection_rect(&animation, &window, &spec),
        selection_range_rect(&animation.from, &window, &spec)
    );
}

#[test]
fn visual_column_width_overrides_only_matching_visible_column() {
    let mut frame = test_grid_frame();
    frame.column_resize_animation = Some(cellium_ui::ColumnResizeAnimation {
        column: 1,
        visual_column_width_px: 212.0,
        separator_x_px: 300.0,
        show_separator: true,
    });

    assert_eq!(
        visible_column_width(&frame.visible_window, 0, Some(&frame)),
        150.0
    );
    assert_eq!(
        visible_column_width(&frame.visible_window, 1, Some(&frame)),
        212.0
    );
}

#[test]
fn visual_row_height_overrides_only_matching_visible_row() {
    let mut frame = test_grid_frame();
    frame.row_resize_animation = Some(cellium_ui::RowResizeAnimation {
        row: 1,
        visual_row_height_px: 48.0,
        separator_y_px: 90.0,
        show_separator: true,
    });

    assert_eq!(
        visible_row_height(&frame.visible_window, 0, Some(&frame)),
        26.0
    );
    assert_eq!(
        visible_row_height(&frame.visible_window, 1, Some(&frame)),
        48.0
    );
}

fn test_grid_geometry() -> GridGeometry {
    GridGeometry {
        scale_factor: 1.0,
        table_scale_factor: 1.0,
        formula_top: 53.0,
        formula_height: 44.0,
        tab_height: 36.0,
        grid_left: 0.0,
        grid_top: 97.0,
        grid_width: 1_000.0,
        grid_height: 600.0,
        header_width: 54.0,
        header_height: 30.0,
        scrollbar_thickness: 12.0,
    }
}

fn test_grid_frame() -> GridFrame {
    let mut viewport = cellium_ui::GridViewport {
        scroll_x_px: 0.0,
        scroll_y_px: 0.0,
        pixel_width: 1_000,
        pixel_height: 600,
        metrics: cellium_ui::GridMetrics::default(),
        column_widths: Default::default(),
        row_heights: Default::default(),
    };
    viewport.metrics.column_width = 150;
    viewport.metrics.row_height = 26;
    GridFrame {
        visible_window: viewport.visible_window().unwrap(),
        viewport,
        selection: cellium_ui::Selection::single(cellium_ui::CellRef::new(1, 1)),
        selection_animation: None,
        column_resize_animation: None,
        row_resize_animation: None,
        snapshot: None,
        view: Default::default(),
        edited_cells: Default::default(),
        edit_state: GridEditState::Selected {
            cell: cellium_ui::CellRef::new(1, 1),
        },
        editor_caret_visible: false,
        editor_caret_animation: None,
        status: String::new(),
        chrome: cellium_ui::ChromeState::default(),
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
        row_heights: vec![26; row_count as usize],
        column_widths: vec![150; column_count as usize],
    }
}
