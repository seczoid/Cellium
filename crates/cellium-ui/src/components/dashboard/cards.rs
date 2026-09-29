use crate::DashboardHoverTarget;

use super::super::{DashboardTheme, UiBuilder, UiIconKind, UiRect, UiTextWeight};
use super::ComponentState;

#[derive(Debug, Clone, PartialEq)]
pub struct ActionCardSpec {
    pub rect: UiRect,
    pub icon: UiIconKind,
    pub title: String,
    pub subtitle: String,
    pub meta: String,
    pub tone: CardTone,
    pub state: ComponentState,
    pub target: DashboardHoverTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardTone {
    Accent,
    Blue,
    Violet,
}

pub fn action_card(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    spec: ActionCardSpec,
) {
    let hovered = spec.state == ComponentState::Hovered;
    let tone = card_tone(theme, spec.tone);
    builder.panel(
        UiRect::new(
            spec.rect.x + 3.0,
            spec.rect.y + 4.0,
            spec.rect.width,
            spec.rect.height,
        ),
        theme.shadow,
        None,
    );
    if hovered {
        builder.panel(
            UiRect::new(
                spec.rect.x - 2.0,
                spec.rect.y - 2.0,
                spec.rect.width + 4.0,
                spec.rect.height + 4.0,
            ),
            tone.1,
            None,
        );
    }
    builder.panel(
        spec.rect,
        if hovered {
            theme.panel_raised
        } else {
            theme.panel_subtle
        },
        Some(if hovered { tone.0 } else { theme.border }),
    );
    builder.panel(
        UiRect::new(
            spec.rect.x + 1.0,
            spec.rect.y + 1.0,
            spec.rect.width - 2.0,
            1.0,
        ),
        if hovered { tone.0 } else { theme.border_soft },
        None,
    );
    let icon_tile = UiRect::new(spec.rect.x + 22.0, spec.rect.y + 22.0, 42.0, 42.0);
    builder.panel(icon_tile, tone.1, Some(tone.0));
    builder.icon(
        spec.icon,
        UiRect::new(icon_tile.x + 11.0, icon_tile.y + 11.0, 20.0, 20.0),
        tone.0,
        2.0,
    );
    builder.text(
        spec.title,
        UiRect::new(spec.rect.x + 80.0, spec.rect.y + 20.0, 230.0, 20.0),
        theme.text,
        15.5,
        UiTextWeight::Semibold,
    );
    builder.text(
        spec.subtitle,
        UiRect::new(spec.rect.x + 80.0, spec.rect.y + 45.0, 250.0, 18.0),
        theme.text_muted,
        13.0,
        UiTextWeight::Regular,
    );
    builder.text(
        spec.meta,
        UiRect::new(spec.rect.x + 80.0, spec.rect.y + 70.0, 250.0, 16.0),
        theme.text_subtle,
        11.0,
        UiTextWeight::Semibold,
    );
    builder.hit(spec.rect, spec.target);
}

fn card_tone(
    theme: DashboardTheme,
    tone: CardTone,
) -> (super::super::UiColor, super::super::UiColor) {
    match tone {
        CardTone::Accent => (theme.accent, theme.accent_soft),
        CardTone::Blue => (theme.blue, theme.blue_soft),
        CardTone::Violet => (theme.violet, theme.violet_soft),
    }
}
