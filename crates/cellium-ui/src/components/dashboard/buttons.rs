use crate::DashboardHoverTarget;

use super::super::{DashboardTheme, UiBuilder, UiColor, UiIconKind, UiRect, UiTextWeight};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentState {
    Default,
    Hovered,
    Active,
    Disabled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonSpec {
    pub rect: UiRect,
    pub label: String,
    pub icon: Option<UiIconKind>,
    pub variant: ButtonVariant,
    pub state: ComponentState,
    pub target: Option<DashboardHoverTarget>,
}

pub fn button(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    spec: ButtonSpec,
) {
    let (fill, border, text_color) = button_colors(theme, spec.variant, spec.state);
    if !matches!(spec.variant, ButtonVariant::Ghost) {
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
    }
    builder.panel(spec.rect, fill, Some(border));
    builder.panel(
        UiRect::new(
            spec.rect.x + 1.0,
            spec.rect.y + 1.0,
            spec.rect.width - 2.0,
            1.0,
        ),
        match spec.variant {
            ButtonVariant::Primary => UiColor::rgba(255, 255, 255, 0.28),
            ButtonVariant::Secondary => theme.border_soft,
            ButtonVariant::Ghost => UiColor::rgba(255, 255, 255, 0.0),
        },
        None,
    );
    if let Some(icon) = spec.icon {
        builder.icon(
            icon,
            UiRect::new(spec.rect.x + 16.0, spec.rect.y + 15.0, 14.0, 14.0),
            text_color,
            2.0,
        );
    }
    let label_x = if spec.icon.is_some() { 43.0 } else { 16.0 };
    builder.text(
        spec.label,
        UiRect::new(
            spec.rect.x + label_x,
            spec.rect.y + 10.0,
            spec.rect.width - label_x - 16.0,
            22.0,
        ),
        text_color,
        14.0,
        UiTextWeight::Semibold,
    );
    if let Some(target) = spec.target {
        builder.hit(spec.rect, target);
    }
}

fn button_colors(
    theme: DashboardTheme,
    variant: ButtonVariant,
    state: ComponentState,
) -> (UiColor, UiColor, UiColor) {
    match (variant, state) {
        (_, ComponentState::Disabled) => (theme.panel_subtle, theme.border, theme.text_subtle),
        (ButtonVariant::Primary, ComponentState::Hovered) => {
            (theme.accent_hover, theme.accent_hover, theme.text_inverse)
        }
        (ButtonVariant::Primary, _) => (theme.accent, theme.accent, theme.text_inverse),
        (ButtonVariant::Secondary, ComponentState::Hovered | ComponentState::Active) => {
            (theme.panel_raised, theme.accent, theme.text)
        }
        (ButtonVariant::Secondary, _) => (theme.panel_raised, theme.border, theme.text),
        (ButtonVariant::Ghost, ComponentState::Hovered | ComponentState::Active) => {
            (theme.panel_active, theme.border_strong, theme.text)
        }
        (ButtonVariant::Ghost, _) => (theme.sidebar, theme.sidebar, theme.text_muted),
    }
}
