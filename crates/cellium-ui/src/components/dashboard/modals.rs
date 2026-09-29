use crate::DashboardHoverTarget;

use super::super::{DashboardTheme, UiBuilder, UiColor, UiRect, UiTextWeight};

#[derive(Debug, Clone, PartialEq)]
pub struct ModalSpec {
    pub rect: UiRect,
    pub title: String,
    pub subtitle: String,
}

pub fn modal_surface(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    spec: ModalSpec,
) {
    builder.panel(
        UiRect::new(0.0, 0.0, 10_000.0, 10_000.0),
        UiColor::rgba(0, 0, 0, 0.48),
        None,
    );
    builder.panel(spec.rect, theme.panel, Some(theme.border_strong));
    builder.text(
        spec.title,
        UiRect::new(
            spec.rect.x + 22.0,
            spec.rect.y + 20.0,
            spec.rect.width - 44.0,
            24.0,
        ),
        theme.text,
        18.0,
        UiTextWeight::Semibold,
    );
    builder.text(
        spec.subtitle,
        UiRect::new(
            spec.rect.x + 22.0,
            spec.rect.y + 50.0,
            spec.rect.width - 44.0,
            18.0,
        ),
        theme.text_muted,
        13.0,
        UiTextWeight::Regular,
    );
}
