use crate::DashboardHoverTarget;

use super::super::{DashboardTheme, UiBuilder, UiIconKind, UiRect, UiTextWeight};
use super::ComponentState;

#[derive(Debug, Clone, PartialEq)]
pub struct TextInputSpec {
    pub rect: UiRect,
    pub value: String,
    pub placeholder: String,
    pub state: ComponentState,
    pub target: Option<DashboardHoverTarget>,
}

pub fn text_input(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    spec: TextInputSpec,
) {
    let active = matches!(spec.state, ComponentState::Active | ComponentState::Hovered);
    builder.panel(
        UiRect::new(
            spec.rect.x + 2.0,
            spec.rect.y + 3.0,
            spec.rect.width,
            spec.rect.height,
        ),
        theme.shadow,
        None,
    );
    builder.panel(
        spec.rect,
        if active {
            theme.panel_raised
        } else {
            theme.panel_subtle
        },
        Some(if active {
            theme.border_strong
        } else {
            theme.border
        }),
    );
    builder.panel(
        UiRect::new(
            spec.rect.x + 1.0,
            spec.rect.y + 1.0,
            spec.rect.width - 2.0,
            1.0,
        ),
        theme.border_soft,
        None,
    );
    if active {
        builder.panel(
            UiRect::new(
                spec.rect.x + 10.0,
                spec.rect.y + spec.rect.height - 2.0,
                spec.rect.width - 20.0,
                1.0,
            ),
            theme.accent,
            None,
        );
    }
    builder.icon(
        UiIconKind::Search,
        UiRect::new(spec.rect.x + 14.0, spec.rect.y + 11.0, 16.0, 16.0),
        if active {
            theme.accent
        } else {
            theme.text_subtle
        },
        2.0,
    );
    let value_empty = spec.value.is_empty();
    let (text, color) = if value_empty {
        (spec.placeholder, theme.text_subtle)
    } else {
        (spec.value, theme.text)
    };
    builder.text(
        text,
        UiRect::new(
            spec.rect.x + 42.0,
            spec.rect.y + 10.0,
            spec.rect.width - 56.0,
            20.0,
        ),
        color,
        13.0,
        UiTextWeight::Regular,
    );
    if value_empty {
        let chip_rect = UiRect::new(
            spec.rect.x + spec.rect.width - 36.0,
            spec.rect.y + 10.0,
            20.0,
            20.0,
        );
        builder.panel(chip_rect, theme.panel, Some(theme.border_soft));
        builder.text(
            "/",
            UiRect::new(chip_rect.x + 7.0, chip_rect.y + 2.0, 8.0, 16.0),
            theme.text_muted,
            12.0,
            UiTextWeight::Semibold,
        );
    }
    if let Some(target) = spec.target {
        builder.hit(spec.rect, target);
    }
}
