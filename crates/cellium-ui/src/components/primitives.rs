use super::UiColor;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl UiRect {
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiTextWeight {
    Regular,
    Semibold,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiIconKind {
    CelliumMark,
    Plus,
    Home,
    Star,
    Workbooks,
    Square,
    Table,
    Database,
    MoreHorizontal,
    Search,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiPanel {
    pub rect: UiRect,
    pub fill: UiColor,
    pub border: Option<UiColor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiText {
    pub rect: UiRect,
    pub text: String,
    pub color: UiColor,
    pub font_size: f32,
    pub weight: UiTextWeight,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiIcon {
    pub rect: UiRect,
    pub kind: UiIconKind,
    pub color: UiColor,
    pub stroke_width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiNode {
    Panel(UiPanel),
    Text(UiText),
    Icon(UiIcon),
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiHitRegion<T> {
    pub rect: UiRect,
    pub target: T,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiTree<T> {
    pub nodes: Vec<UiNode>,
    pub hit_regions: Vec<UiHitRegion<T>>,
}

impl<T> Default for UiTree<T> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            hit_regions: Vec::new(),
        }
    }
}

impl<T: Clone> UiTree<T> {
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> Option<T> {
        self.hit_regions
            .iter()
            .rev()
            .find(|region| region.rect.contains(x, y))
            .map(|region| region.target.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiBuilder<T> {
    tree: UiTree<T>,
}

impl<T> Default for UiBuilder<T> {
    fn default() -> Self {
        Self {
            tree: UiTree::default(),
        }
    }
}

impl<T> UiBuilder<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn finish(self) -> UiTree<T> {
        self.tree
    }

    pub fn panel(&mut self, rect: UiRect, fill: UiColor, border: Option<UiColor>) {
        self.tree
            .nodes
            .push(UiNode::Panel(UiPanel { rect, fill, border }));
    }

    pub fn text(
        &mut self,
        text: impl Into<String>,
        rect: UiRect,
        color: UiColor,
        font_size: f32,
        weight: UiTextWeight,
    ) {
        self.tree.nodes.push(UiNode::Text(UiText {
            rect,
            text: text.into(),
            color,
            font_size,
            weight,
        }));
    }

    pub fn icon(&mut self, kind: UiIconKind, rect: UiRect, color: UiColor, stroke_width: f32) {
        self.tree.nodes.push(UiNode::Icon(UiIcon {
            rect,
            kind,
            color,
            stroke_width,
        }));
    }

    pub fn hit(&mut self, rect: UiRect, target: T) {
        self.tree.hit_regions.push(UiHitRegion { rect, target });
    }
}
