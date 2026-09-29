use cellium_ui::{DashboardFrame, GridFrame};
use thiserror::Error;
use wgpu::Color;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("surface creation failed: {0}")]
    SurfaceCreation(String),
    #[error("adapter request failed: {0}")]
    AdapterRequest(String),
    #[error("device request failed: {0}")]
    DeviceRequest(String),
    #[error("surface has no supported formats")]
    NoSurfaceFormats,
    #[error("surface frame timed out")]
    Timeout,
    #[error("surface is occluded")]
    Occluded,
    #[error("surface is outdated")]
    Outdated,
    #[error("surface was lost")]
    Lost,
    #[error("surface validation failed")]
    Validation,
    #[error("surface frame was suboptimal")]
    Suboptimal,
    #[error("text prepare failed: {0}")]
    TextPrepare(#[from] glyphon::PrepareError),
    #[error("text render failed: {0}")]
    TextRender(#[from] glyphon::RenderError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl From<Rgba> for Color {
    fn from(value: Rgba) -> Self {
        Self {
            r: value.red,
            g: value.green,
            b: value.blue,
            a: value.alpha,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DrawPrimitive {
    Rect {
        rect: Rect,
        color: Rgba,
    },
    Text {
        x: f32,
        y: f32,
        text: String,
    },
    Tooltip {
        x: f32,
        y: f32,
        width: f32,
        text: String,
    },
    DashboardFrame(Box<DashboardFrame>),
    GridFrame(Box<GridFrame>),
}
