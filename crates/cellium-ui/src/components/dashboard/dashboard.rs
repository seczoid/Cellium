use crate::{DashboardFrame, DashboardHoverTarget, DashboardSection};

use super::super::{
    DashboardLayout, DashboardTheme, UiBuilder, UiIconKind, UiTree, dashboard_layout,
};
use super::{
    ActionCardSpec, CardTone, ComponentState, PageHeaderSpec, SectionHeaderSpec, TableLayout,
    action_card, dashboard_top_bar, page_header, section_header, sidebar, workbook_table,
};

pub const DASHBOARD_RECENT_TABLE_Y: f32 = 394.0;
pub const DASHBOARD_SECTION_TABLE_Y: f32 = 204.0;

#[must_use]
pub fn dashboard_component_tree(
    frame: &DashboardFrame,
    width: f32,
    height: f32,
) -> UiTree<DashboardHoverTarget> {
    let width = width.max(1.0);
    let height = height.max(1.0);
    let theme = DashboardTheme::dark();
    let layout = dashboard_layout(width, height);
    let mut builder = UiBuilder::new();

    builder.panel(layout.root, theme.background, None);
    builder.panel(
        layout.top_bar,
        theme.background_lift,
        Some(theme.border_soft),
    );
    sidebar(&mut builder, theme, frame, layout.sidebar.height);
    dashboard_top_bar(&mut builder, theme, frame, layout.content.x, width);
    page_header(
        &mut builder,
        theme,
        PageHeaderSpec {
            x: layout.content.x,
            y: if frame.active_section == DashboardSection::Home {
                118.0
            } else {
                110.0
            },
            section: frame.active_section,
        },
    );

    match frame.active_section {
        DashboardSection::Home => home_dashboard(&mut builder, theme, frame, layout),
        DashboardSection::Starred | DashboardSection::Workbooks => workbook_table(
            &mut builder,
            theme,
            frame,
            TableLayout {
                x: layout.content.x,
                y: DASHBOARD_SECTION_TABLE_Y,
                width: layout.table_width(),
            },
        ),
    }

    builder.finish()
}

#[must_use]
pub fn dashboard_hit_test(
    frame: &DashboardFrame,
    width: f32,
    height: f32,
    x: f32,
    y: f32,
) -> Option<DashboardHoverTarget> {
    dashboard_component_tree(frame, width, height).hit_test(x, y)
}

#[must_use]
pub fn dashboard_section_title(section: DashboardSection) -> &'static str {
    match section {
        DashboardSection::Home => "Home",
        DashboardSection::Starred => "Starred",
        DashboardSection::Workbooks => "Workbooks",
    }
}

fn home_dashboard(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    layout: DashboardLayout,
) {
    section_header(
        builder,
        theme,
        frame,
        SectionHeaderSpec {
            x: layout.content.x,
            y: 190.0,
            width: layout.table_width(),
            title: "Start",
            action: None,
        },
    );
    create_cards(builder, theme, frame, layout);
    section_header(
        builder,
        theme,
        frame,
        SectionHeaderSpec {
            x: layout.content.x,
            y: 356.0,
            width: layout.table_width(),
            title: "Recent workbooks",
            action: Some(DashboardHoverTarget::ViewAll),
        },
    );
    workbook_table(
        builder,
        theme,
        frame,
        TableLayout {
            x: layout.content.x,
            y: DASHBOARD_RECENT_TABLE_Y,
            width: layout.table_width(),
        },
    );
}

fn create_cards(
    builder: &mut UiBuilder<DashboardHoverTarget>,
    theme: DashboardTheme,
    frame: &DashboardFrame,
    layout: DashboardLayout,
) {
    let cards = [
        (
            DashboardHoverTarget::FileUpload,
            UiIconKind::Table,
            "Open or import file",
            "CSV, Parquet, Arrow, or workbook",
            "Opens in a new window",
            CardTone::Accent,
        ),
        (
            DashboardHoverTarget::CreateBlankWorkbook,
            UiIconKind::Square,
            "New blank workbook",
            "Start with an empty sheet",
            "Saved locally",
            CardTone::Blue,
        ),
    ];
    let card_rects = layout.action_card_rects(226.0, cards.len());
    for ((target, icon, title, subtitle, meta, tone), rect) in cards.into_iter().zip(card_rects) {
        action_card(
            builder,
            theme,
            ActionCardSpec {
                rect,
                icon,
                title: title.to_string(),
                subtitle: subtitle.to_string(),
                meta: meta.to_string(),
                tone,
                state: if frame.hovered.as_ref() == Some(&target) {
                    ComponentState::Hovered
                } else {
                    ComponentState::Default
                },
                target,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::{DashboardWorkbook, UiNode};

    use super::super::dashboard_nav_items;
    use super::*;

    fn frame() -> DashboardFrame {
        DashboardFrame {
            active_section: DashboardSection::Home,
            workbooks: vec![DashboardWorkbook {
                id: 7,
                name: "hardscapers.csv".to_string(),
                file_path: "/tmp/hardscapers.cellium".to_string(),
                updated_label: "now".to_string(),
                starred: false,
            }],
            search_query: String::new(),
            search_focused: false,
            hovered: None,
            status: "Ready".to_string(),
        }
    }

    fn text_nodes(frame: &DashboardFrame) -> Vec<String> {
        dashboard_component_tree(frame, 1400.0, 820.0)
            .nodes
            .into_iter()
            .filter_map(|node| match node {
                UiNode::Text(text) => Some(text.text),
                UiNode::Panel(_) | UiNode::Icon(_) => None,
            })
            .collect()
    }

    #[test]
    fn dashboard_hit_test_returns_file_upload_from_card_region() {
        assert_eq!(
            dashboard_hit_test(&frame(), 1400.0, 820.0, 680.0, 310.0),
            Some(DashboardHoverTarget::FileUpload)
        );
    }

    #[test]
    fn dashboard_hit_test_returns_search_from_top_bar_input() {
        assert_eq!(
            dashboard_hit_test(&frame(), 1400.0, 820.0, 340.0, 40.0),
            Some(DashboardHoverTarget::Search)
        );
    }

    #[test]
    fn dashboard_component_tree_starts_with_dark_background_panel() {
        let tree = dashboard_component_tree(&frame(), 1400.0, 820.0);
        let Some(UiNode::Panel(panel)) = tree.nodes.first() else {
            panic!("first node should be a dashboard background panel");
        };

        assert_eq!(panel.fill, DashboardTheme::dark().background);
    }

    #[test]
    fn dashboard_nav_items_do_not_expose_unsupported_sections() {
        let labels = dashboard_nav_items()
            .into_iter()
            .map(|(section, _)| dashboard_section_title(section))
            .collect::<Vec<_>>();

        assert_eq!(labels, vec!["Home", "Workbooks", "Starred"]);
    }

    #[test]
    fn dashboard_component_tree_has_no_unsupported_dashboard_copy() {
        let copy = text_nodes(&frame()).join("\n");

        assert!(!copy.contains("Data sources"));
        assert!(!copy.contains("Connections"));
        assert!(!copy.contains("Database query"));
    }

    #[test]
    fn dashboard_component_tree_shows_unmatched_search_empty_state() {
        let mut frame = frame();
        frame.search_query = "missing".to_string();

        let copy = text_nodes(&frame).join("\n");

        assert!(copy.contains("No matching workbooks"));
    }

    #[test]
    fn dashboard_hit_test_returns_workbook_row_from_recent_table() {
        assert_eq!(
            dashboard_hit_test(
                &frame(),
                1400.0,
                820.0,
                340.0,
                DASHBOARD_RECENT_TABLE_Y + 54.0
            ),
            Some(DashboardHoverTarget::WorkbookRow(7))
        );
    }
}
