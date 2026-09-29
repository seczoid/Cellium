use crate::{DashboardFrame, DashboardHoverTarget, DashboardSection};

use super::super::{
    DASHBOARD_SIDEBAR_WIDTH, DashboardTheme, UiBuilder, UiIconKind, UiRect, UiTextWeight,
};
use super::{ButtonSpec, ButtonVariant, ComponentState, button, dashboard_section_title};

#[must_use]
pub fn dashboard_nav_items() -> [(DashboardSection, f32); 3] {
    [
        (DashboardSection::Home, 150.0),
        (DashboardSection::Workbooks, 192.0),
        (DashboardSection::Starred, 234.0),
    ]
}

pub fn sidebar(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    height: f32,
) {
    builder.panel(
        UiRect::new(0.0, 0.0, DASHBOARD_SIDEBAR_WIDTH, height),
        theme.sidebar,
        None,
    );
    builder.panel(
        UiRect::new(12.0, 12.0, DASHBOARD_SIDEBAR_WIDTH - 24.0, height - 24.0),
        theme.sidebar_lift,
        Some(theme.border_soft),
    );
    builder.panel(
        UiRect::new(DASHBOARD_SIDEBAR_WIDTH, 0.0, 1.0, height),
        theme.sidebar_border,
        None,
    );
    builder.panel(UiRect::new(17.0, 18.0, 30.0, 30.0), theme.glow, None);
    builder.icon(
        UiIconKind::CelliumMark,
        UiRect::new(22.0, 23.0, 20.0, 20.0),
        theme.accent,
        2.0,
    );
    builder.text(
        "Cellium",
        UiRect::new(54.0, 18.0, 168.0, 28.0),
        theme.text,
        22.0,
        UiTextWeight::Semibold,
    );

    let new_hovered = matches!(
        frame.hovered.as_ref(),
        Some(DashboardHoverTarget::NewWorkbook | DashboardHoverTarget::CreateBlankWorkbook)
    );
    button(
        builder,
        theme,
        ButtonSpec {
            rect: UiRect::new(18.0, 74.0, DASHBOARD_SIDEBAR_WIDTH - 36.0, 44.0),
            label: "New blank workbook".to_string(),
            icon: Some(UiIconKind::Plus),
            variant: ButtonVariant::Primary,
            state: if new_hovered {
                ComponentState::Hovered
            } else {
                ComponentState::Default
            },
            target: Some(DashboardHoverTarget::NewWorkbook),
        },
    );

    section_label(builder, theme, "Workspace", 128.0);
    for (section, y) in dashboard_nav_items() {
        nav_item(builder, theme, frame, section, y);
    }
    footer(builder, theme, height);
}

fn nav_item(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    section: DashboardSection,
    y: f32,
) {
    let active = frame.active_section == section;
    let hovered = matches!(
        frame.hovered.as_ref(),
        Some(DashboardHoverTarget::Sidebar(target)) if *target == section
    );
    let rect = UiRect::new(14.0, y, DASHBOARD_SIDEBAR_WIDTH - 28.0, 36.0);
    if active || hovered {
        builder.panel(
            rect,
            if active {
                theme.panel_active
            } else {
                theme.panel_hover
            },
            Some(if active {
                theme.border_strong
            } else {
                theme.border
            }),
        );
        if active {
            builder.panel(
                UiRect::new(rect.x + 14.0, rect.y + 15.0, 6.0, 6.0),
                theme.accent,
                None,
            );
        }
    }
    builder.icon(
        nav_icon(section),
        UiRect::new(32.0, y + 11.0, 14.0, 14.0),
        if active {
            theme.accent
        } else {
            theme.text_subtle
        },
        2.0,
    );
    builder.text(
        dashboard_section_title(section),
        UiRect::new(62.0, y + 8.0, 164.0, 20.0),
        if active { theme.text } else { theme.text_muted },
        14.0,
        if active {
            UiTextWeight::Semibold
        } else {
            UiTextWeight::Regular
        },
    );
    builder.hit(rect, DashboardHoverTarget::Sidebar(section));
}

const fn nav_icon(section: DashboardSection) -> UiIconKind {
    match section {
        DashboardSection::Home => UiIconKind::Home,
        DashboardSection::Starred => UiIconKind::Star,
        DashboardSection::Workbooks => UiIconKind::Workbooks,
    }
}

fn section_label(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    label: &'static str,
    y: f32,
) {
    builder.text(
        label,
        UiRect::new(30.0, y, 180.0, 14.0),
        theme.text_subtle,
        10.5,
        UiTextWeight::Semibold,
    );
}

fn footer(builder: &mut UiBuilder<DashboardHoverTarget>, theme: DashboardTheme, height: f32) {
    let y = (height - 126.0).max(410.0);
    builder.panel(
        UiRect::new(20.0, y + 3.0, DASHBOARD_SIDEBAR_WIDTH - 40.0, 68.0),
        theme.shadow,
        None,
    );
    builder.panel(
        UiRect::new(20.0, y, DASHBOARD_SIDEBAR_WIDTH - 40.0, 68.0),
        theme.panel,
        Some(theme.border),
    );
    builder.panel(UiRect::new(34.0, y + 18.0, 8.0, 8.0), theme.success, None);
    builder.text(
        "Files stay on this Mac",
        UiRect::new(52.0, y + 14.0, 170.0, 18.0),
        theme.text,
        13.0,
        UiTextWeight::Semibold,
    );
    builder.text(
        "Saved as local workbooks",
        UiRect::new(52.0, y + 38.0, 170.0, 16.0),
        theme.text_muted,
        12.0,
        UiTextWeight::Regular,
    );
}
