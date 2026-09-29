use crate::DashboardHoverTarget;

use super::super::{DashboardTheme, UiBuilder, UiColor, UiRect, UiTextWeight};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Neutral,
    Accent,
    Blue,
    Violet,
    Success,
    Amber,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BadgeSpec {
    pub rect: UiRect,
    pub label: String,
    pub tone: BadgeTone,
}

pub fn badge(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    spec: BadgeSpec,
) {
    let (fill, border, text) = badge_colors(theme, spec.tone);
    builder.panel(spec.rect, fill, Some(border));
    builder.text(
        spec.label,
        UiRect::new(
            spec.rect.x + 10.0,
            spec.rect.y + 5.0,
            spec.rect.width - 20.0,
            spec.rect.height - 8.0,
        ),
        text,
        11.0,
        UiTextWeight::Semibold,
    );
}

fn badge_colors(theme: DashboardTheme, tone: BadgeTone) -> (UiColor, UiColor, UiColor) {
    match tone {
        BadgeTone::Neutral => (theme.panel_subtle, theme.border, theme.text_muted),
        BadgeTone::Accent => (theme.accent_soft, theme.accent, theme.accent),
        BadgeTone::Blue => (theme.blue_soft, theme.blue, theme.blue),
        BadgeTone::Violet => (theme.violet_soft, theme.violet, theme.violet),
        BadgeTone::Success => (theme.success_soft, theme.success, theme.success),
        BadgeTone::Amber => (theme.amber_soft, theme.amber, theme.amber),
    }
}
