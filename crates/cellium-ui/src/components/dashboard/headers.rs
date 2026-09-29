use crate::{DashboardFrame, DashboardHoverTarget, DashboardSection};

use super::super::{DashboardTheme, UiBuilder, UiRect, UiTextWeight};
use super::{
    BadgeSpec, BadgeTone, ComponentState, TextInputSpec, badge, dashboard_section_title, text_input,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageHeaderSpec {
    pub x: f32,
    pub y: f32,
    pub section: DashboardSection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionHeaderSpec {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub title: &'static str,
    pub action: Option<DashboardHoverTarget>,
}

pub fn dashboard_top_bar(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    x: f32,
    width: f32,
) {
    text_input(
        builder,
        theme,
        TextInputSpec {
            rect: UiRect::new(x, 20.0, 470.0, 40.0),
            value: frame.search_query.clone(),
            placeholder: "Search workbooks".to_string(),
            state: search_state(frame),
            target: Some(DashboardHoverTarget::Search),
        },
    );

    let right = (width - 54.0).max(x + 500.0);
    badge(
        builder,
        theme,
        BadgeSpec {
            rect: UiRect::new(right - 294.0, 27.0, 116.0, 26.0),
            label: "Saved locally".to_string(),
            tone: BadgeTone::Success,
        },
    );
    badge(
        builder,
        theme,
        BadgeSpec {
            rect: UiRect::new(right - 162.0, 27.0, 108.0, 26.0),
            label: format!("{} workbooks", frame.workbooks.len()),
            tone: BadgeTone::Blue,
        },
    );

    let status = if frame.status == "Ready" {
        "Files stay on this Mac"
    } else {
        frame.status.as_str()
    };
    builder.text(
        status,
        UiRect::new(right - 520.0, 32.0, 210.0, 18.0),
        theme.text_muted,
        12.0,
        UiTextWeight::Regular,
    );
}

fn search_state(frame: &DashboardFrame) -> ComponentState {
    if frame.search_focused {
        ComponentState::Active
    } else if frame.hovered.as_ref() == Some(&DashboardHoverTarget::Search) {
        ComponentState::Hovered
    } else {
        ComponentState::Default
    }
}

pub fn page_header(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    spec: PageHeaderSpec,
) {
    builder.text(
        "Saved locally",
        UiRect::new(spec.x, spec.y - 22.0, 120.0, 16.0),
        theme.accent,
        11.0,
        UiTextWeight::Semibold,
    );
    builder.text(
        dashboard_section_title(spec.section),
        UiRect::new(spec.x, spec.y, 420.0, 40.0),
        theme.text,
        30.0,
        UiTextWeight::Semibold,
    );
    builder.text(
        "Open recent work, import a file, or start a blank workbook.",
        UiRect::new(spec.x, spec.y + 38.0, 520.0, 18.0),
        theme.text_muted,
        13.0,
        UiTextWeight::Regular,
    );
}

pub fn section_header(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    spec: SectionHeaderSpec,
) {
    builder.text(
        spec.title,
        UiRect::new(spec.x, spec.y, 260.0, 24.0),
        theme.text_muted,
        18.0,
        UiTextWeight::Semibold,
    );

    if let Some(target) = spec.action {
        let rect = UiRect::new(spec.x + spec.width - 72.0, spec.y + 2.0, 72.0, 22.0);
        let hovered = frame.hovered.as_ref() == Some(&target);
        builder.text(
            "View all",
            rect,
            if hovered {
                theme.accent
            } else {
                theme.text_muted
            },
            13.0,
            UiTextWeight::Semibold,
        );
        builder.hit(rect, target);
    }
}
