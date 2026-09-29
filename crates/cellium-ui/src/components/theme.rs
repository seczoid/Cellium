#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: f32,
}

impl UiColor {
    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 1.0,
        }
    }

    #[must_use]
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DashboardTheme {
    pub background: UiColor,
    pub background_lift: UiColor,
    pub sidebar: UiColor,
    pub sidebar_lift: UiColor,
    pub sidebar_border: UiColor,
    pub panel: UiColor,
    pub panel_hover: UiColor,
    pub panel_active: UiColor,
    pub panel_subtle: UiColor,
    pub panel_raised: UiColor,
    pub panel_overlay: UiColor,
    pub shadow: UiColor,
    pub glow: UiColor,
    pub border: UiColor,
    pub border_soft: UiColor,
    pub border_strong: UiColor,
    pub row: UiColor,
    pub row_alt: UiColor,
    pub row_hover: UiColor,
    pub text: UiColor,
    pub text_muted: UiColor,
    pub text_subtle: UiColor,
    pub text_inverse: UiColor,
    pub accent: UiColor,
    pub accent_hover: UiColor,
    pub accent_soft: UiColor,
    pub blue: UiColor,
    pub blue_soft: UiColor,
    pub violet: UiColor,
    pub violet_soft: UiColor,
    pub success: UiColor,
    pub success_soft: UiColor,
    pub amber: UiColor,
    pub amber_soft: UiColor,
    pub warning_fill: UiColor,
    pub warning_border: UiColor,
}

impl DashboardTheme {
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            background: UiColor::rgb(8, 10, 13),
            background_lift: UiColor::rgb(11, 14, 18),
            sidebar: UiColor::rgb(12, 14, 18),
            sidebar_lift: UiColor::rgb(16, 19, 24),
            sidebar_border: UiColor::rgb(31, 37, 45),
            panel: UiColor::rgb(18, 22, 27),
            panel_hover: UiColor::rgb(25, 31, 38),
            panel_active: UiColor::rgb(30, 38, 47),
            panel_subtle: UiColor::rgb(14, 17, 22),
            panel_raised: UiColor::rgb(22, 27, 33),
            panel_overlay: UiColor::rgb(28, 34, 42),
            shadow: UiColor::rgba(0, 0, 0, 0.30),
            glow: UiColor::rgba(49, 209, 184, 0.12),
            border: UiColor::rgb(41, 49, 59),
            border_soft: UiColor::rgba(255, 255, 255, 0.055),
            border_strong: UiColor::rgb(66, 78, 92),
            row: UiColor::rgb(15, 18, 23),
            row_alt: UiColor::rgb(18, 22, 27),
            row_hover: UiColor::rgb(25, 31, 38),
            text: UiColor::rgb(234, 239, 244),
            text_muted: UiColor::rgb(153, 166, 179),
            text_subtle: UiColor::rgb(103, 116, 130),
            text_inverse: UiColor::rgb(5, 8, 10),
            accent: UiColor::rgb(49, 209, 184),
            accent_hover: UiColor::rgb(64, 225, 199),
            accent_soft: UiColor::rgba(49, 209, 184, 0.16),
            blue: UiColor::rgb(99, 179, 237),
            blue_soft: UiColor::rgba(99, 179, 237, 0.15),
            violet: UiColor::rgb(178, 139, 255),
            violet_soft: UiColor::rgba(178, 139, 255, 0.14),
            success: UiColor::rgb(108, 214, 136),
            success_soft: UiColor::rgba(108, 214, 136, 0.14),
            amber: UiColor::rgb(238, 190, 101),
            amber_soft: UiColor::rgba(238, 190, 101, 0.13),
            warning_fill: UiColor::rgb(37, 32, 22),
            warning_border: UiColor::rgb(91, 72, 34),
        }
    }
}
