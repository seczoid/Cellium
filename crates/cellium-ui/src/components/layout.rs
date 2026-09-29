use taffy::TaffyError;
use taffy::prelude::{
    AvailableSpace, Dimension, FlexDirection, NodeId, Size, Style, TaffyTree, length,
};

use super::UiRect;

pub const DASHBOARD_SIDEBAR_WIDTH: f32 = 264.0;
pub const DASHBOARD_TOP_BAR_HEIGHT: f32 = 78.0;
pub const DASHBOARD_CONTENT_LEFT_PAD: f32 = 44.0;
pub const DASHBOARD_CONTENT_RIGHT_PAD: f32 = 56.0;
pub const DASHBOARD_MIN_TABLE_WIDTH: f32 = 420.0;
pub const DASHBOARD_CARD_HEIGHT: f32 = 104.0;
pub const DASHBOARD_CARD_GAP: f32 = 18.0;
pub const DASHBOARD_CARD_MIN_WIDTH: f32 = 260.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DashboardLayout {
    pub root: UiRect,
    pub sidebar: UiRect,
    pub top_bar: UiRect,
    pub body: UiRect,
    pub content: UiRect,
}

impl DashboardLayout {
    #[must_use]
    pub fn table_width(self) -> f32 {
        self.content.width.max(DASHBOARD_MIN_TABLE_WIDTH)
    }

    #[must_use]
    pub fn action_card_rects(self, y: f32, count: usize) -> Vec<UiRect> {
        let container = UiRect::new(self.content.x, y, self.table_width(), DASHBOARD_CARD_HEIGHT);
        flex_row_rects(
            container,
            count,
            DASHBOARD_CARD_GAP,
            DASHBOARD_CARD_MIN_WIDTH,
        )
        .unwrap_or_else(|_| fallback_row_rects(container, count, DASHBOARD_CARD_GAP))
    }

    #[must_use]
    pub fn fallback(width: f32, height: f32) -> Self {
        let width = width.max(1.0);
        let height = height.max(1.0);
        let sidebar = UiRect::new(0.0, 0.0, DASHBOARD_SIDEBAR_WIDTH.min(width), height);
        let main_x = sidebar.width;
        let main_width = (width - main_x).max(1.0);
        let top_bar = UiRect::new(
            main_x,
            0.0,
            main_width,
            DASHBOARD_TOP_BAR_HEIGHT.min(height),
        );
        let body = UiRect::new(
            main_x,
            top_bar.height,
            main_width,
            (height - top_bar.height).max(1.0),
        );
        let content = content_rect(body);

        Self {
            root: UiRect::new(0.0, 0.0, width, height),
            sidebar,
            top_bar,
            body,
            content,
        }
    }
}

#[must_use]
pub fn dashboard_layout(width: f32, height: f32) -> DashboardLayout {
    compute_dashboard_layout(width.max(1.0), height.max(1.0))
        .unwrap_or_else(|_| DashboardLayout::fallback(width, height))
}

fn compute_dashboard_layout(width: f32, height: f32) -> Result<DashboardLayout, TaffyError> {
    let mut taffy: TaffyTree<()> = TaffyTree::new();

    let sidebar = taffy.new_leaf(Style {
        size: Size {
            width: length(DASHBOARD_SIDEBAR_WIDTH),
            height: Dimension::percent(1.0),
        },
        flex_shrink: 0.0,
        ..Default::default()
    })?;
    let top_bar = taffy.new_leaf(Style {
        size: Size {
            width: Dimension::percent(1.0),
            height: length(DASHBOARD_TOP_BAR_HEIGHT),
        },
        flex_shrink: 0.0,
        ..Default::default()
    })?;
    let body = taffy.new_leaf(Style {
        flex_grow: 1.0,
        flex_shrink: 1.0,
        size: Size {
            width: Dimension::percent(1.0),
            height: Dimension::auto(),
        },
        ..Default::default()
    })?;
    let main = taffy.new_with_children(
        Style {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            min_size: Size {
                width: length(DASHBOARD_MIN_TABLE_WIDTH),
                height: length(1.0),
            },
            ..Default::default()
        },
        &[top_bar, body],
    )?;
    let root = taffy.new_with_children(
        Style {
            flex_direction: FlexDirection::Row,
            size: Size {
                width: length(width),
                height: length(height),
            },
            ..Default::default()
        },
        &[sidebar, main],
    )?;

    taffy.compute_layout(
        root,
        Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        },
    )?;

    let root_rect = node_rect(&taffy, root, UiRect::new(0.0, 0.0, 0.0, 0.0))?;
    let sidebar_rect = node_rect(&taffy, sidebar, root_rect)?;
    let main_rect = node_rect(&taffy, main, root_rect)?;
    let top_bar_rect = node_rect(&taffy, top_bar, main_rect)?;
    let body_rect = node_rect(&taffy, body, main_rect)?;

    Ok(DashboardLayout {
        root: root_rect,
        sidebar: sidebar_rect,
        top_bar: top_bar_rect,
        body: body_rect,
        content: content_rect(body_rect),
    })
}

fn flex_row_rects(
    container: UiRect,
    count: usize,
    gap: f32,
    min_child_width: f32,
) -> Result<Vec<UiRect>, TaffyError> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let mut taffy: TaffyTree<()> = TaffyTree::new();
    let child_style = Style {
        flex_basis: length(0.0),
        flex_grow: 1.0,
        flex_shrink: 1.0,
        min_size: Size {
            width: length(min_child_width),
            height: length(container.height),
        },
        size: Size {
            width: Dimension::auto(),
            height: length(container.height),
        },
        ..Default::default()
    };
    let children = (0..count)
        .map(|_| taffy.new_leaf(child_style.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let root = taffy.new_with_children(
        Style {
            flex_direction: FlexDirection::Row,
            gap: Size {
                width: length(gap),
                height: length(0.0),
            },
            size: Size {
                width: length(container.width),
                height: length(container.height),
            },
            ..Default::default()
        },
        &children,
    )?;
    taffy.compute_layout(
        root,
        Size {
            width: AvailableSpace::Definite(container.width),
            height: AvailableSpace::Definite(container.height),
        },
    )?;

    children
        .into_iter()
        .map(|child| node_rect(&taffy, child, container))
        .collect()
}

fn fallback_row_rects(container: UiRect, count: usize, gap: f32) -> Vec<UiRect> {
    if count == 0 {
        return Vec::new();
    }
    let total_gap = gap * count.saturating_sub(1) as f32;
    let width = ((container.width - total_gap) / count as f32).max(1.0);
    (0..count)
        .map(|index| UiRect {
            x: container.x + index as f32 * (width + gap),
            y: container.y,
            width,
            height: container.height,
        })
        .collect()
}

fn content_rect(body: UiRect) -> UiRect {
    UiRect::new(
        body.x + DASHBOARD_CONTENT_LEFT_PAD,
        body.y,
        (body.width - DASHBOARD_CONTENT_LEFT_PAD - DASHBOARD_CONTENT_RIGHT_PAD)
            .max(DASHBOARD_MIN_TABLE_WIDTH),
        body.height,
    )
}

fn node_rect(taffy: &TaffyTree<()>, node: NodeId, parent: UiRect) -> Result<UiRect, TaffyError> {
    let layout = taffy.layout(node)?;
    Ok(UiRect::new(
        parent.x + layout.location.x,
        parent.y + layout.location.y,
        layout.size.width,
        layout.size.height,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_layout_positions_content_after_sidebar_padding() {
        let layout = dashboard_layout(1_400.0, 820.0);

        assert_eq!(layout.sidebar.width, DASHBOARD_SIDEBAR_WIDTH);
        assert_eq!(
            layout.content.x,
            DASHBOARD_SIDEBAR_WIDTH + DASHBOARD_CONTENT_LEFT_PAD
        );
    }

    #[test]
    fn dashboard_layout_sizes_content_from_remaining_width() {
        let layout = dashboard_layout(1_400.0, 820.0);

        assert_eq!(
            layout.table_width(),
            1_400.0
                - DASHBOARD_SIDEBAR_WIDTH
                - DASHBOARD_CONTENT_LEFT_PAD
                - DASHBOARD_CONTENT_RIGHT_PAD
        );
    }

    #[test]
    fn action_card_rects_fill_available_row_with_gap() {
        let layout = dashboard_layout(1_400.0, 820.0);

        let cards = layout.action_card_rects(294.0, 3);

        assert_eq!(cards.len(), 3);
        assert_eq!(cards[1].x, cards[0].x + cards[0].width + DASHBOARD_CARD_GAP);
    }
}
