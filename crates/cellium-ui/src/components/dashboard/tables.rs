use crate::{DashboardFrame, DashboardHoverTarget, DashboardWorkbook, dashboard_visible_workbooks};

use super::super::{DashboardTheme, UiBuilder, UiIconKind, UiRect, UiTextWeight};

pub const DASHBOARD_TABLE_ROW_HEIGHT: f32 = 48.0;
pub const MAX_TABLE_ROWS: usize = 10;

const TABLE_HEADER_HEIGHT: f32 = 42.0;
const EMPTY_BODY_HEIGHT: f32 = 112.0;

#[derive(Debug, Clone, Copy)]
pub struct TableLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
}

#[derive(Debug, Clone, Copy)]
struct TableColumns {
    name_x: f32,
    name_width: f32,
    kind_x: f32,
    detail_x: f32,
    updated_x: f32,
    action_x: f32,
}

pub fn workbook_table(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    layout: TableLayout,
) {
    let visible_workbooks =
        dashboard_visible_workbooks(&frame.workbooks, frame.active_section, &frame.search_query);
    table_shell(
        builder,
        theme,
        layout,
        visible_workbooks.len(),
        ["Workbook", "Location", "", "Updated"],
    );

    if visible_workbooks.is_empty() {
        let (title, subtitle) = if frame.search_query.trim().is_empty() {
            ("No workbooks yet", "Open a CSV or create a blank workbook.")
        } else {
            ("No matching workbooks", "Try a different name or path.")
        };
        empty_panel(builder, theme, layout, UiIconKind::Square, title, subtitle);
        return;
    }

    for (index, workbook) in visible_workbooks
        .into_iter()
        .take(MAX_TABLE_ROWS)
        .enumerate()
    {
        workbook_row(
            builder,
            theme,
            frame,
            workbook,
            row_layout(layout, index),
            index,
        );
    }
}

fn workbook_row(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    workbook: &DashboardWorkbook,
    layout: TableLayout,
    index: usize,
) {
    let target = DashboardHoverTarget::WorkbookRow(workbook.id);
    let hovered = frame.hovered.as_ref() == Some(&target);
    let columns = TableColumns::new(layout);
    row_panel(builder, theme, layout, index, hovered);
    leading_icon(
        builder,
        theme,
        layout,
        UiIconKind::Square,
        theme.accent_soft,
        theme.accent,
    );
    if workbook.starred {
        builder.panel(
            UiRect::new(layout.x + 42.0, layout.y + 12.0, 5.0, 5.0),
            theme.amber,
            None,
        );
    }
    builder.text(
        truncate_text(&workbook.name, 48),
        UiRect::new(columns.name_x, layout.y + 8.0, columns.name_width, 18.0),
        theme.text,
        14.0,
        UiTextWeight::Semibold,
    );
    builder.text(
        truncate_text(&workbook.file_path, 58),
        UiRect::new(columns.name_x, layout.y + 28.0, columns.name_width, 16.0),
        theme.text_subtle,
        11.0,
        UiTextWeight::Regular,
    );
    builder.text(
        "Saved locally",
        UiRect::new(columns.kind_x, layout.y + 15.0, 120.0, 18.0),
        theme.text_muted,
        13.0,
        UiTextWeight::Regular,
    );
    builder.text(
        workbook.updated_label.as_str(),
        UiRect::new(columns.updated_x, layout.y + 15.0, 128.0, 18.0),
        theme.text_muted,
        13.0,
        UiTextWeight::Regular,
    );
    builder.icon(
        UiIconKind::MoreHorizontal,
        UiRect::new(columns.action_x, layout.y + 18.0, 20.0, 12.0),
        if hovered {
            theme.text_muted
        } else {
            theme.text_subtle
        },
        2.0,
    );
    builder.hit(
        UiRect::new(layout.x, layout.y, layout.width, DASHBOARD_TABLE_ROW_HEIGHT),
        target,
    );
}

fn table_shell(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    layout: TableLayout,
    row_count: usize,
    labels: [&'static str; 4],
) {
    let visible_rows = row_count.min(MAX_TABLE_ROWS);
    let body_height = if visible_rows == 0 {
        EMPTY_BODY_HEIGHT
    } else {
        visible_rows as f32 * DASHBOARD_TABLE_ROW_HEIGHT
    };
    let height = TABLE_HEADER_HEIGHT + body_height;
    builder.panel(
        UiRect::new(layout.x + 3.0, layout.y + 4.0, layout.width, height),
        theme.shadow,
        None,
    );
    builder.panel(
        UiRect::new(layout.x, layout.y, layout.width, height),
        theme.panel_subtle,
        Some(theme.border_soft),
    );
    builder.panel(
        UiRect::new(layout.x, layout.y, layout.width, TABLE_HEADER_HEIGHT),
        theme.panel,
        Some(theme.border),
    );

    let columns = TableColumns::new(TableLayout {
        y: layout.y,
        ..layout
    });
    header_label(builder, theme, labels[0], columns.name_x, layout);
    header_label(builder, theme, labels[1], columns.kind_x, layout);
    header_label(builder, theme, labels[2], columns.detail_x, layout);
    header_label(builder, theme, labels[3], columns.updated_x, layout);
}

fn header_label(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    text: &'static str,
    x: f32,
    layout: TableLayout,
) {
    if text.is_empty() {
        return;
    }
    builder.text(
        text,
        UiRect::new(x, layout.y + 14.0, 140.0, 16.0),
        theme.text_muted,
        12.0,
        UiTextWeight::Semibold,
    );
}

fn row_panel(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    layout: TableLayout,
    index: usize,
    hovered: bool,
) {
    let fill = if hovered {
        theme.row_hover
    } else if index.is_multiple_of(2) {
        theme.row
    } else {
        theme.row_alt
    };
    builder.panel(
        UiRect::new(layout.x, layout.y, layout.width, DASHBOARD_TABLE_ROW_HEIGHT),
        fill,
        None,
    );
    builder.panel(
        UiRect::new(
            layout.x + 16.0,
            layout.y + DASHBOARD_TABLE_ROW_HEIGHT - 1.0,
            layout.width - 32.0,
            1.0,
        ),
        theme.border_soft,
        None,
    );
}

fn leading_icon(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    layout: TableLayout,
    icon: UiIconKind,
    fill: super::super::UiColor,
    color: super::super::UiColor,
) {
    let tile = UiRect::new(layout.x + 18.0, layout.y + 12.0, 24.0, 24.0);
    builder.panel(tile, fill, Some(theme.border_soft));
    builder.icon(
        icon,
        UiRect::new(tile.x + 6.0, tile.y + 6.0, 12.0, 12.0),
        color,
        1.6,
    );
}

fn empty_panel(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    layout: TableLayout,
    icon: UiIconKind,
    title: &'static str,
    subtitle: &'static str,
) {
    let y = layout.y + TABLE_HEADER_HEIGHT;
    let icon_rect = UiRect::new(layout.x + 24.0, y + 30.0, 40.0, 40.0);
    builder.panel(icon_rect, theme.panel_raised, Some(theme.border));
    builder.icon(
        icon,
        UiRect::new(icon_rect.x + 11.0, icon_rect.y + 11.0, 18.0, 18.0),
        theme.accent,
        2.0,
    );
    builder.text(
        title,
        UiRect::new(layout.x + 82.0, y + 30.0, 420.0, 20.0),
        theme.text,
        15.0,
        UiTextWeight::Semibold,
    );
    builder.text(
        subtitle,
        UiRect::new(layout.x + 82.0, y + 56.0, 620.0, 18.0),
        theme.text_muted,
        13.0,
        UiTextWeight::Regular,
    );
}

fn row_layout(layout: TableLayout, index: usize) -> TableLayout {
    TableLayout {
        y: layout.y + TABLE_HEADER_HEIGHT + index as f32 * DASHBOARD_TABLE_ROW_HEIGHT,
        ..layout
    }
}

impl TableColumns {
    fn new(layout: TableLayout) -> Self {
        let action_x = layout.x + layout.width - 52.0;
        let updated_x = (action_x - 170.0).max(layout.x + 250.0);
        let detail_x = (updated_x - 210.0).max(layout.x + 170.0);
        let kind_x = (detail_x - 130.0).max(layout.x + 130.0);
        let name_x = layout.x + 56.0;
        let name_width = (kind_x - name_x - 18.0).max(90.0);

        Self {
            name_x,
            name_width,
            kind_x,
            detail_x,
            updated_x,
            action_x,
        }
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}
