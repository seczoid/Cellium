//! WGPU rendering and COSMIC Text rendering for Cellium.

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use cellium_ui::{
    CellRef, ChromeHoverTarget, DashboardFrame, FORMULA_BAR_HEIGHT, FORMULA_BAR_TOP,
    GRID_CELL_FONT_SIZE, GRID_CELL_TEXT_HEIGHT, GridEditState, GridFrame, GridSnapshot,
    SHEET_TAB_HEIGHT, SelectionAnchor, SelectionRange, TextSelection, UiColor, UiIconKind, UiNode,
    UiTextWeight, VisibleWindow, dashboard_component_tree,
};
use glyphon::{
    Attrs, Buffer as TextBuffer, Cache as TextCache, Color as TextColor, Cursor, Family,
    FontSystem, Metrics, PrepareError, Resolution, Shaping, SwashCache, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport, Weight, Wrap,
};
use wgpu::{
    Backends, BlendState, Buffer as GpuBuffer, BufferDescriptor, BufferUsages, ColorTargetState,
    ColorWrites, CommandEncoderDescriptor, CompositeAlphaMode, Device, DeviceDescriptor, Features,
    FragmentState, Instance, InstanceDescriptor, Limits, LoadOp, MultisampleState, Operations,
    PipelineCompilationOptions, PowerPreference, PresentMode, PrimitiveState, PrimitiveTopology,
    Queue, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, ShaderModuleDescriptor, ShaderSource, StoreOp,
    Surface, SurfaceConfiguration, SurfaceTexture, TextureFormat, TextureUsages,
    TextureViewDescriptor, VertexAttribute, VertexBufferLayout, VertexFormat, VertexState,
    VertexStepMode,
};
use winit::{dpi::PhysicalSize, window::Window};

mod types;

pub use types::{DrawPrimitive, Rect, RenderError, Rgba};

const TEXT_CACHE_SOFT_LIMIT: usize = 2_048;
const TEXT_CACHE_HARD_LIMIT: usize = 3_072;
const CELL_TRUNCATE_CHAR_WIDTH_FACTOR: f32 = 0.55;
const APP_MONOSPACE_FONT_CANDIDATES: &[&str] = &[
    "Berkeley Mono",
    "JetBrains Mono",
    "SF Mono",
    "Roboto Mono",
    "Roboto Mono for Powerline",
    "Menlo",
    "Monaco",
];
const TOOLTIP_HEIGHT: f32 = 34.0;
const TOP_BAR_HEIGHT: f32 = FORMULA_BAR_TOP as f32;
const OPEN_BUTTON_X: f32 = 12.0;
const OPEN_BUTTON_Y: f32 = 10.0;
const OPEN_BUTTON_WIDTH: f32 = 132.0;
const OPEN_BUTTON_HEIGHT: f32 = 32.0;
const SORT_BUTTON_X: f32 = 158.0;
const SORT_BUTTON_WIDTH: f32 = 48.0;
const FILTER_BUTTON_X: f32 = 212.0;
const FILTER_BUTTON_WIDTH: f32 = 56.0;
const CLEAR_VIEW_BUTTON_X: f32 = 274.0;
const CLEAR_VIEW_BUTTON_WIDTH: f32 = 46.0;
const ACTION_CHIP_Y: f32 = 11.0;
const ACTION_CHIP_HEIGHT: f32 = 30.0;
const SUMMARY_X: f32 = 336.0;
const SUMMARY_Y: f32 = 11.0;
const SUMMARY_HEIGHT: f32 = 30.0;
const CELL_NAME_X: f32 = 14.0;
const CELL_NAME_WIDTH: f32 = 74.0;
const FORMULA_INPUT_X: f32 = 112.0;
const FORMULA_INPUT_RIGHT_PAD: f32 = 18.0;
const MIN_VISUAL_COLUMN_WIDTH_PX: f32 = 48.0;
const MIN_VISUAL_ROW_HEIGHT_PX: f32 = 18.0;

#[derive(Debug, Clone, PartialEq)]
struct TextLabel<'a> {
    rect: Rect,
    clip: Option<Rect>,
    text: Cow<'a, str>,
    color: TextColor,
    weight: Weight,
    font_size: f32,
    line_height: f32,
}

impl TextLabel<'_> {
    fn clipped(mut self, clip: Rect) -> Self {
        self.clip = Some(clip);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TextStyleKey {
    width: u32,
    height: u32,
    font_size: u32,
    line_height: u32,
    color: u32,
    weight: u16,
}

impl TextStyleKey {
    fn from_label(label: &TextLabel<'_>) -> Self {
        Self {
            width: quantized_pixels(label.rect.width),
            height: quantized_pixels(label.rect.height),
            font_size: quantized_pixels(label.font_size),
            line_height: quantized_pixels(label.line_height),
            color: label.color.0,
            weight: label.weight.0,
        }
    }
}

struct CachedTextBuffer {
    buffer: TextBuffer,
    last_used_frame: u64,
}

struct TextEngine {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    buffer_cache: HashMap<TextStyleKey, HashMap<String, CachedTextBuffer>>,
    frame_index: u64,
}

impl TextEngine {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let mut font_system = FontSystem::new();
        configure_app_fonts(&mut font_system);
        let swash_cache = SwashCache::new();
        let cache = TextCache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer = TextRenderer::new(&mut atlas, device, MultisampleState::default(), None);
        Self {
            font_system,
            swash_cache,
            viewport,
            atlas,
            renderer,
            buffer_cache: HashMap::new(),
            frame_index: 0,
        }
    }

    fn reset(&mut self, device: &Device, queue: &Queue, format: TextureFormat) {
        *self = Self::new(device, queue, format);
    }

    fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        size: PhysicalSize<u32>,
        labels: &[TextLabel<'_>],
    ) -> Result<(), PrepareError> {
        self.frame_index = self.frame_index.saturating_add(1);
        self.viewport.update(
            queue,
            Resolution {
                width: size.width.max(1),
                height: size.height.max(1),
            },
        );
        self.atlas.trim();
        self.prune_buffer_cache();
        for label in labels {
            self.ensure_buffer(label);
        }
        let text_areas = labels
            .iter()
            .filter_map(|label| {
                let style = TextStyleKey::from_label(label);
                let buffer = self
                    .buffer_cache
                    .get(&style)?
                    .get(label.text.as_ref())
                    .map(|entry| &entry.buffer)?;
                text_area(label, buffer)
            })
            .collect::<Vec<_>>();
        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        )?;
        self.prune_buffer_cache();
        Ok(())
    }

    fn trim_atlas(&mut self) {
        self.atlas.trim();
    }

    fn ensure_buffer(&mut self, label: &TextLabel<'_>) {
        let style = TextStyleKey::from_label(label);
        let bucket = self.buffer_cache.entry(style).or_default();
        if let Some(entry) = bucket.get_mut(label.text.as_ref()) {
            entry.last_used_frame = self.frame_index;
            return;
        }

        let mut buffer = TextBuffer::new(
            &mut self.font_system,
            Metrics::new(label.font_size, label.line_height),
        );
        buffer.set_size(
            &mut self.font_system,
            Some(label.rect.width.max(1.0)),
            Some(label.rect.height.max(1.0)),
        );
        buffer.set_wrap(&mut self.font_system, Wrap::None);
        buffer.set_text(
            &mut self.font_system,
            label.text.as_ref(),
            &default_text_attrs(label.color, label.weight),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);
        bucket.insert(
            label.text.as_ref().to_string(),
            CachedTextBuffer {
                buffer,
                last_used_frame: self.frame_index,
            },
        );
    }

    fn hit_test(&mut self, label: &TextLabel<'_>, x: f32, y: f32) -> Option<usize> {
        self.ensure_buffer(label);
        let style = TextStyleKey::from_label(label);
        self.buffer_cache
            .get(&style)?
            .get(label.text.as_ref())?
            .buffer
            .hit(x, y)
            .map(|cursor| cursor.index)
    }

    fn cursor_x(&mut self, label: &TextLabel<'_>, byte_index: usize) -> Option<f32> {
        self.ensure_buffer(label);
        let style = TextStyleKey::from_label(label);
        let buffer = &self
            .buffer_cache
            .get(&style)?
            .get(label.text.as_ref())?
            .buffer;
        text_buffer_cursor_x(buffer, byte_index, label.text.len()).map(|x| label.rect.x + x)
    }

    fn text_width(&mut self, label: &TextLabel<'_>) -> Option<f32> {
        self.cursor_x(label, label.text.len())
            .map(|x| x - label.rect.x)
    }

    fn prune_buffer_cache(&mut self) {
        let total = self.buffer_cache.values().map(HashMap::len).sum::<usize>();
        if total <= TEXT_CACHE_HARD_LIMIT {
            return;
        }

        let remove_count = total.saturating_sub(TEXT_CACHE_SOFT_LIMIT);
        let mut keys = self
            .buffer_cache
            .iter()
            .flat_map(|(style, bucket)| {
                bucket
                    .iter()
                    .map(|(text, entry)| (entry.last_used_frame, *style, text.clone()))
            })
            .collect::<Vec<_>>();
        keys.sort_by_key(|(last_used_frame, _, _)| *last_used_frame);
        for (_, style, text) in keys.into_iter().take(remove_count) {
            if let Some(bucket) = self.buffer_cache.get_mut(&style) {
                bucket.remove(&text);
            }
        }
        self.buffer_cache.retain(|_, bucket| !bucket.is_empty());
    }
}

fn configure_app_fonts(font_system: &mut FontSystem) {
    if let Some(family) = preferred_monospace_family(font_system) {
        font_system.db_mut().set_monospace_family(family);
    }
}

fn preferred_monospace_family(font_system: &FontSystem) -> Option<&'static str> {
    APP_MONOSPACE_FONT_CANDIDATES
        .iter()
        .copied()
        .find(|candidate| {
            font_system.db().faces().any(|face| {
                face.monospaced
                    && face
                        .families
                        .iter()
                        .any(|(name, _)| name.as_str() == *candidate)
            })
        })
}

fn default_text_attrs(color: TextColor, weight: Weight) -> Attrs<'static> {
    Attrs::new()
        .family(Family::Monospace)
        .color(color)
        .weight(weight)
}

fn text_buffer_cursor_x(buffer: &TextBuffer, byte_index: usize, text_len: usize) -> Option<f32> {
    let cursor = Cursor::new(0, byte_index.min(text_len));
    buffer
        .layout_runs()
        .find_map(|run| run.highlight(cursor, cursor))
        .map(|(x, _)| x)
}

fn text_area<'a>(label: &TextLabel<'_>, buffer: &'a TextBuffer) -> Option<TextArea<'a>> {
    let bounds = match label.clip {
        Some(clip) => intersect_rect(label.rect, clip)?,
        None => label.rect,
    };
    let left = bounds.x.round() as i32;
    let top = bounds.y.round() as i32;
    Some(TextArea {
        buffer,
        left: label.rect.x,
        top: label.rect.y,
        scale: 1.0,
        bounds: TextBounds {
            left,
            top,
            right: left.saturating_add(bounds.width.max(1.0).round() as i32),
            bottom: top.saturating_add(bounds.height.max(1.0).round() as i32),
        },
        default_color: label.color,
        custom_glyphs: &[],
    })
}

fn quantized_pixels(value: f32) -> u32 {
    (value.max(0.0) * 64.0).round().min(u32::MAX as f32) as u32
}

fn intersect_rect(rect: Rect, clip: Rect) -> Option<Rect> {
    let left = rect.x.max(clip.x);
    let top = rect.y.max(clip.y);
    let right = (rect.x + rect.width).min(clip.x + clip.width);
    let bottom = (rect.y + rect.height).min(clip.y + clip.height);
    if right <= left || bottom <= top {
        return None;
    }
    Some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

pub struct Renderer {
    window: Arc<Window>,
    size: PhysicalSize<u32>,
    scale_factor: f32,
    surface: Surface<'static>,
    config: SurfaceConfiguration,
    device: Device,
    queue: Queue,
    rect_pipeline: RenderPipeline,
    rect_vertex_buffer: GpuBuffer,
    text_engine: TextEngine,
    clear_color: Rgba,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, RenderError> {
        let size = window.inner_size();
        let scale_factor = window.scale_factor().max(1.0) as f32;
        let mut instance_descriptor = InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = Backends::PRIMARY;
        let instance = Instance::new(instance_descriptor);
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| RenderError::SurfaceCreation(error.to_string()))?;
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|error| RenderError::AdapterRequest(error.to_string()))?;
        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("cellium-device"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|error| RenderError::DeviceRequest(error.to_string()))?;
        let capabilities = surface.get_capabilities(&adapter);
        let surface_format = [TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm]
            .into_iter()
            .find(|format| capabilities.formats.contains(format))
            .or_else(|| {
                capabilities
                    .formats
                    .iter()
                    .copied()
                    .find(TextureFormat::is_srgb)
            })
            .or_else(|| capabilities.formats.first().copied())
            .ok_or(RenderError::NoSurfaceFormats)?;
        let present_mode = capabilities
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == PresentMode::Mailbox)
            .unwrap_or(PresentMode::Fifo);
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(CompositeAlphaMode::Auto);
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let rect_pipeline = create_rect_pipeline(&device, surface_format);
        let text_engine = TextEngine::new(&device, &queue, surface_format);
        let rect_vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("cellium-rect-vertex-buffer"),
            size: 256 * 1024,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            window,
            size,
            scale_factor,
            surface,
            config,
            device,
            queue,
            rect_pipeline,
            rect_vertex_buffer,
            text_engine,
            clear_color: Rgba {
                red: 0.025,
                green: 0.028,
                blue: 0.031,
                alpha: 1.0,
            },
        })
    }

    #[must_use]
    pub fn window(&self) -> &Window {
        &self.window
    }

    #[must_use]
    pub const fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    #[must_use]
    pub const fn scale_factor(&self) -> f64 {
        self.scale_factor as f64
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.size = size;
        self.scale_factor = self.window.scale_factor().max(1.0) as f32;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn hit_test_editor_text(
        &mut self,
        text: &str,
        width: f32,
        height: f32,
        font_size: f32,
        x: f32,
        y: f32,
    ) -> Option<usize> {
        let label = label(
            0.0,
            0.0,
            width.max(1.0),
            height.max(1.0),
            text,
            editor_text_color(),
            font_size.max(1.0),
        );
        self.text_engine.hit_test(&label, x, y)
    }

    pub fn measure_editor_text_width(
        &mut self,
        text: &str,
        width: f32,
        height: f32,
        font_size: f32,
    ) -> Option<f32> {
        let label = label(
            0.0,
            0.0,
            width.max(1.0),
            height.max(1.0),
            text,
            editor_text_color(),
            font_size.max(1.0),
        );
        self.text_engine.text_width(&label)
    }

    pub fn render(&mut self, primitives: &[DrawPrimitive]) -> Result<(), RenderError> {
        let (output, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout => return Err(RenderError::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => return Err(RenderError::Occluded),
            wgpu::CurrentSurfaceTexture::Outdated => return Err(RenderError::Outdated),
            wgpu::CurrentSurfaceTexture::Lost => return Err(RenderError::Lost),
            wgpu::CurrentSurfaceTexture::Validation => return Err(RenderError::Validation),
        };
        self.render_to(output, primitives)?;
        if suboptimal {
            return Err(RenderError::Suboptimal);
        }
        Ok(())
    }

    fn render_to(
        &mut self,
        output: SurfaceTexture,
        primitives: &[DrawPrimitive],
    ) -> Result<(), RenderError> {
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());
        let labels = build_text_labels(
            self.size,
            self.scale_factor,
            primitives,
            Some(&mut self.text_engine),
        );
        if let Err(error) = self
            .text_engine
            .prepare(&self.device, &self.queue, self.size, &labels)
        {
            if error != PrepareError::AtlasFull {
                return Err(RenderError::TextPrepare(error));
            }
            self.text_engine
                .reset(&self.device, &self.queue, self.config.format);
            self.text_engine
                .prepare(&self.device, &self.queue, self.size, &labels)?;
        }
        let vertices = build_frame_vertices(
            self.size,
            self.scale_factor,
            primitives,
            Some(&mut self.text_engine),
        );
        self.queue
            .write_buffer(&self.rect_vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("cellium-render-encoder"),
            });
        {
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("cellium-main-pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(self.clear_color.into()),
                        store: StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            render_pass.set_pipeline(&self.rect_pipeline);
            render_pass.set_vertex_buffer(0, self.rect_vertex_buffer.slice(..));
            render_pass.draw(0..vertices.len() as u32, 0..1);
            self.text_engine.renderer.render(
                &self.text_engine.atlas,
                &self.text_engine.viewport,
                &mut render_pass,
            )?;
        }
        self.queue.submit([encoder.finish()]);
        output.present();
        self.text_engine.trim_atlas();
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RectVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl RectVertex {
    const ATTRIBUTES: [VertexAttribute; 2] = [
        VertexAttribute {
            format: VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        VertexAttribute {
            format: VertexFormat::Float32x4,
            offset: std::mem::size_of::<[f32; 2]>() as u64,
            shader_location: 1,
        },
    ];

    const LAYOUT: VertexBufferLayout<'static> = VertexBufferLayout {
        array_stride: std::mem::size_of::<Self>() as u64,
        step_mode: VertexStepMode::Vertex,
        attributes: &Self::ATTRIBUTES,
    };
}

#[derive(Debug, Clone, Copy)]
struct RenderRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
}

fn create_rect_pipeline(device: &Device, surface_format: TextureFormat) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("cellium-rect-shader"),
        source: ShaderSource::Wgsl(
            r#"
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#
            .into(),
        ),
    });

    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("cellium-rect-pipeline"),
        layout: None,
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[RectVertex::LAYOUT],
            compilation_options: PipelineCompilationOptions::default(),
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: MultisampleState::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format: surface_format,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn build_frame_vertices(
    size: PhysicalSize<u32>,
    scale_factor: f32,
    primitives: &[DrawPrimitive],
    mut text_engine: Option<&mut TextEngine>,
) -> Vec<RectVertex> {
    let mut vertices = Vec::with_capacity(900);
    let width = size.width.max(1) as f32;
    let height = size.height.max(1) as f32;
    let canvas = Canvas {
        width,
        height,
        scale_factor,
    };
    let dashboard_present = primitives
        .iter()
        .any(|primitive| matches!(primitive, DrawPrimitive::DashboardFrame(_)));
    push_rect(
        &mut vertices,
        canvas,
        RenderRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
            color: if dashboard_present {
                rgb(248, 248, 248)
            } else {
                rgb(8, 10, 12)
            },
        },
    );
    if !dashboard_present {
        push_shell(&mut vertices, canvas);
    }
    for primitive in primitives {
        match primitive {
            DrawPrimitive::Rect { rect, color } => push_rect(
                &mut vertices,
                canvas,
                RenderRect {
                    x: canvas.s(rect.x),
                    y: canvas.s(rect.y),
                    width: canvas.s(rect.width),
                    height: canvas.s(rect.height),
                    color: rgba(*color),
                },
            ),
            DrawPrimitive::GridFrame(frame) => push_grid(
                &mut vertices,
                canvas,
                frame.as_ref(),
                text_engine.as_deref_mut(),
            ),
            DrawPrimitive::DashboardFrame(frame) => {
                push_dashboard(&mut vertices, canvas, frame.as_ref());
            }
            DrawPrimitive::Tooltip { x, y, width, .. } => {
                push_tooltip(&mut vertices, canvas, *x, *y, *width);
            }
            DrawPrimitive::Text { .. } => {}
        }
    }
    vertices
}

fn build_text_labels<'a>(
    size: PhysicalSize<u32>,
    scale_factor: f32,
    primitives: &'a [DrawPrimitive],
    mut text_engine: Option<&mut TextEngine>,
) -> Vec<TextLabel<'a>> {
    let canvas = Canvas {
        width: size.width.max(1) as f32,
        height: size.height.max(1) as f32,
        scale_factor,
    };
    let mut labels = Vec::with_capacity(900);

    for primitive in primitives {
        match primitive {
            DrawPrimitive::DashboardFrame(frame) => {
                push_dashboard_labels(&mut labels, canvas, frame.as_ref());
            }
            DrawPrimitive::GridFrame(frame) => {
                push_grid_labels(
                    &mut labels,
                    canvas,
                    frame.as_ref(),
                    text_engine.as_deref_mut(),
                );
            }
            DrawPrimitive::Text { x, y, text } => labels.push(scaled_label(
                canvas,
                LabelSpec {
                    x: *x,
                    y: *y,
                    width: 240.0,
                    height: 22.0,
                    text: text.as_str(),
                    color: primary_text(),
                    font_size: 14.0,
                },
            )),
            DrawPrimitive::Tooltip { x, y, width, text } => labels.push(scaled_label(
                canvas,
                LabelSpec {
                    x: *x + 10.0,
                    y: *y + 8.0,
                    width: (*width - 20.0).max(40.0),
                    height: 18.0,
                    text: text.as_str(),
                    color: primary_text(),
                    font_size: 13.0,
                },
            )),
            DrawPrimitive::Rect { .. } => {}
        }
    }

    labels
}

fn push_grid_labels<'a>(
    labels: &mut Vec<TextLabel<'a>>,
    canvas: Canvas,
    frame: &'a GridFrame,
    mut text_engine: Option<&mut TextEngine>,
) {
    let spec = GridGeometry::for_frame(canvas, frame);
    let window = &frame.visible_window;
    let visible_columns = window.column_count.min(24);
    let column_header_clip = spec.column_header_clip();
    let row_header_clip = spec.row_header_clip();
    let body_clip = spec.body_clip();
    let summary = frame.snapshot.as_ref().map_or_else(
        || "Drop a CSV, Parquet, or Arrow file".to_string(),
        table_summary,
    );
    push_shell_labels(labels, canvas, frame, summary);
    push_formula_labels(labels, canvas, &spec, frame);
    push_sheet_tab_labels(labels, canvas, &spec, frame);

    for column in 0..visible_columns {
        let column_index = window.start_column + column;
        let column_rect = visible_column_rect(window, &spec, column, Some(frame));
        labels.push(
            semibold(label(
                column_rect.x + spec.t(12.0),
                spec.grid_top + spec.t(8.0),
                column_rect.width - spec.t(18.0),
                spec.t(GRID_CELL_TEXT_HEIGHT),
                column_name(column_index),
                header_text(),
                spec.t(GRID_CELL_FONT_SIZE),
            ))
            .clipped(column_header_clip),
        );
        if let Some(marker) = view_column_marker(frame, column_index) {
            labels.push(
                semibold(label(
                    column_rect.x + column_rect.width - spec.t(42.0),
                    spec.grid_top + spec.t(8.0),
                    spec.t(34.0),
                    spec.t(GRID_CELL_TEXT_HEIGHT),
                    marker,
                    accent_text(),
                    spec.t(11.0),
                ))
                .clipped(column_header_clip),
            );
        }
    }

    let max_rows = window.row_count.min(80);
    for row in 0..max_rows {
        let row_number = (window.start_row + u64::from(row) + 1).to_string();
        let row_rect = visible_row_rect(window, &spec, u64::from(row), Some(frame));
        labels.push(
            label(
                spec.t(12.0),
                row_rect.y + spec.t(6.0),
                spec.header_width - spec.t(18.0),
                spec.t(18.0),
                row_number,
                header_text(),
                spec.t(12.0),
            )
            .clipped(row_header_clip),
        );
        if row_rect.y > spec.grid_top + spec.grid_height - spec.scrollbar_thickness {
            break;
        }
    }

    if let Some(snapshot) = &frame.snapshot {
        push_cell_labels(
            labels,
            &spec,
            frame,
            snapshot,
            window,
            body_clip,
            &mut text_engine,
        );
    }
    push_cell_editor_labels(labels, &spec, frame, body_clip, &mut text_engine);
}

fn view_column_marker(frame: &GridFrame, column_index: u32) -> Option<String> {
    let column = frame
        .snapshot
        .as_ref()?
        .columns
        .get(column_index as usize)?;
    let sort = frame
        .view
        .sorts
        .iter()
        .position(|sort| sort.column == *column)
        .map(|index| {
            let direction = match frame.view.sorts[index].direction {
                cellium_ui::SortDirection::Ascending => "^",
                cellium_ui::SortDirection::Descending => "v",
            };
            format!("{}{direction}", index + 1)
        });
    let filtered = frame
        .view
        .filters
        .iter()
        .any(|filter| filter.column == *column);
    match (sort, filtered) {
        (Some(sort), true) => Some(format!("{sort} F")),
        (Some(sort), false) => Some(sort),
        (None, true) => Some("F".to_string()),
        (None, false) => None,
    }
}

fn push_shell_labels<'a>(
    labels: &mut Vec<TextLabel<'a>>,
    canvas: Canvas,
    frame: &GridFrame,
    summary: String,
) {
    let open_color = if matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::OpenButton)
    ) {
        primary_text()
    } else {
        muted_text()
    };
    labels.push(semibold(scaled_label(
        canvas,
        LabelSpec {
            x: OPEN_BUTTON_X + 32.0,
            y: OPEN_BUTTON_Y + 8.0,
            width: OPEN_BUTTON_WIDTH - 42.0,
            height: 18.0,
            text: "Open",
            color: open_color,
            font_size: 13.0,
        },
    )));
    let sort_color = if frame.view.sorts.is_empty() {
        muted_text()
    } else {
        accent_text()
    };
    let filter_color = if frame.view.filters.is_empty() {
        muted_text()
    } else {
        violet_text()
    };
    labels.push(semibold(scaled_label(
        canvas,
        LabelSpec {
            x: SORT_BUTTON_X + 8.0,
            y: ACTION_CHIP_Y + 8.0,
            width: SORT_BUTTON_WIDTH - 12.0,
            height: 16.0,
            text: "SORT",
            color: sort_color,
            font_size: 10.5,
        },
    )));
    labels.push(semibold(scaled_label(
        canvas,
        LabelSpec {
            x: FILTER_BUTTON_X + 8.0,
            y: ACTION_CHIP_Y + 8.0,
            width: FILTER_BUTTON_WIDTH - 12.0,
            height: 16.0,
            text: "FILTER",
            color: filter_color,
            font_size: 10.5,
        },
    )));
    labels.push(semibold(scaled_label(
        canvas,
        LabelSpec {
            x: CLEAR_VIEW_BUTTON_X + 7.0,
            y: ACTION_CHIP_Y + 8.0,
            width: CLEAR_VIEW_BUTTON_WIDTH - 10.0,
            height: 16.0,
            text: "CLEAR",
            color: if frame.view.is_default() {
                subdued_text()
            } else {
                muted_text()
            },
            font_size: 10.5,
        },
    )));
    labels.push(label(
        canvas.s(SUMMARY_X + 16.0),
        canvas.s(SUMMARY_Y + 8.0),
        (canvas.width - canvas.s(SUMMARY_X + 34.0)).max(canvas.s(120.0)),
        canvas.s(17.0),
        truncate_cell(&summary, 120).into_owned(),
        muted_text(),
        canvas.s(13.0),
    ));
}

fn push_formula_labels<'a>(
    labels: &mut Vec<TextLabel<'a>>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &'a GridFrame,
) {
    labels.push(semibold(label(
        spec.s(CELL_NAME_X + 21.0),
        spec.formula_top + spec.s(13.0),
        spec.s(44.0),
        spec.s(18.0),
        active_cell_label(frame.selection.active.row, frame.selection.active.column),
        muted_text(),
        spec.s(13.0),
    )));
    labels.push(label(
        spec.s(FORMULA_INPUT_X + 16.0),
        spec.formula_top + spec.s(13.0),
        (canvas.width - spec.s(FORMULA_INPUT_X + FORMULA_INPUT_RIGHT_PAD + 24.0))
            .max(spec.s(120.0)),
        spec.s(18.0),
        formula_bar_text(frame),
        if matches!(
            frame.chrome.hovered.as_ref(),
            Some(ChromeHoverTarget::FormulaBar)
        ) {
            muted_text()
        } else {
            subdued_text()
        },
        spec.s(13.0),
    ));
}

fn push_sheet_tab_labels<'a>(
    labels: &mut Vec<TextLabel<'a>>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &'a GridFrame,
) {
    let tab_text = frame
        .snapshot
        .as_ref()
        .map_or("Sheet 1", |snapshot| snapshot.source_name.as_str());
    labels.push(semibold(label(
        spec.s(36.0),
        canvas.height - spec.tab_height + spec.s(10.0),
        spec.s(170.0),
        spec.s(18.0),
        truncate_cell(tab_text, 28),
        accent_text(),
        spec.s(12.0),
    )));
    labels.push(label(
        spec.s(238.0),
        canvas.height - spec.tab_height + spec.s(10.0),
        (canvas.width - spec.s(252.0)).max(spec.s(100.0)),
        spec.s(18.0),
        frame.status.as_str(),
        subdued_text(),
        spec.s(12.0),
    ));
}

fn formula_bar_text(frame: &GridFrame) -> Cow<'_, str> {
    if let GridEditState::Editing { buffer, .. } = &frame.edit_state {
        return Cow::Borrowed(buffer.as_str());
    }
    if let Some(text) = frame_cell_text(frame, &frame.selection.active) {
        return sanitize_single_line(text);
    }
    if frame.snapshot.is_some() {
        Cow::Owned(format!(
            "Ready • {}",
            active_cell_label(frame.selection.active.row, frame.selection.active.column)
        ))
    } else {
        Cow::Borrowed("Open a CSV, Parquet, Arrow file, or drop it here")
    }
}

fn editing_cell(edit_state: &GridEditState) -> Option<&CellRef> {
    match edit_state {
        GridEditState::Editing { cell, .. } => Some(cell),
        GridEditState::Idle | GridEditState::Selected { .. } => None,
    }
}

fn editing_parts(edit_state: &GridEditState) -> Option<(&CellRef, &str, TextSelection)> {
    match edit_state {
        GridEditState::Editing {
            cell,
            buffer,
            selection,
            ..
        } => Some((cell, buffer.as_str(), *selection)),
        GridEditState::Idle | GridEditState::Selected { .. } => None,
    }
}

fn editing_overlay_rect(
    frame: &GridFrame,
    spec: &GridGeometry,
    text_engine: &mut Option<&mut TextEngine>,
) -> Option<Rect> {
    let (cell, buffer, _) = editing_parts(&frame.edit_state)?;
    let rect = selection_range_rect(
        &SelectionRange::Cells {
            start: cell.clone(),
            end: cell.clone(),
        },
        &frame.visible_window,
        spec,
    )?;
    Some(cell_editor_rect(rect, buffer, spec, text_engine))
}

fn active_cell_label(row: u32, column: u32) -> String {
    format!("{}{}", column_name(column.saturating_sub(1)), row)
}

fn table_summary(snapshot: &GridSnapshot) -> String {
    let rows = snapshot.row_count.map_or_else(
        || "unknown rows".to_string(),
        |count| format!("{count} rows"),
    );
    format!(
        "{} • {} cols • {rows}",
        snapshot.source_name,
        snapshot.columns.len()
    )
}

fn push_cell_labels<'a>(
    labels: &mut Vec<TextLabel<'a>>,
    spec: &GridGeometry,
    frame: &'a GridFrame,
    snapshot: &'a GridSnapshot,
    window: &VisibleWindow,
    body_clip: Rect,
    text_engine: &mut Option<&mut TextEngine>,
) {
    let snapshot_end = snapshot
        .start_row
        .saturating_add(snapshot.rows.len() as u64);
    let window_end = window.start_row.saturating_add(u64::from(window.row_count));
    let first_row = snapshot.start_row.max(window.start_row);
    let last_row = snapshot_end.min(window_end);
    if first_row >= last_row {
        return;
    }

    let editor_rect = editing_overlay_rect(frame, spec, text_engine);
    let snapshot_skip = first_row.saturating_sub(snapshot.start_row) as usize;
    let visible_row_offset = first_row.saturating_sub(window.start_row) as usize;
    let row_limit = last_row.saturating_sub(first_row) as usize;
    for (row_offset, row) in snapshot
        .rows
        .iter()
        .skip(snapshot_skip)
        .take(row_limit)
        .enumerate()
    {
        let visible_row = visible_row_offset.saturating_add(row_offset);
        let row_rect = visible_row_rect(window, spec, visible_row as u64, Some(frame));
        let text_height = spec.t(GRID_CELL_TEXT_HEIGHT);
        let y = row_rect.y + ((row_rect.height - text_height) * 0.5).max(0.0);
        if y > spec.grid_top + spec.grid_height - spec.t(18.0) {
            break;
        }
        for column in 0..window.column_count.min(24) {
            let column_index = window.start_column + column;
            let Some(value) = projected_cell_value(row, snapshot, column_index) else {
                continue;
            };
            let absolute_row = first_row.saturating_add(row_offset as u64);
            let Some(cell_row) = u32::try_from(absolute_row.saturating_add(1)).ok() else {
                continue;
            };
            let value = frame
                .edited_cells
                .get(&CellRef::new(cell_row, column_index.saturating_add(1)))
                .map_or(value.as_str(), String::as_str);
            let color = if absolute_row < u64::from(snapshot.header_row_count) {
                csv_header_text()
            } else {
                primary_text()
            };
            let cell_rect =
                visible_cell_rect(window, spec, visible_row as u64, column, Some(frame));
            if editor_rect
                .is_some_and(|editor_rect| intersect_rect(cell_rect, editor_rect).is_some())
            {
                continue;
            }
            let Some(cell_clip) = intersect_rect(cell_rect, body_clip) else {
                continue;
            };
            let text = label(
                cell_rect.x + spec.t(10.0),
                y,
                cell_rect.width - spec.t(18.0),
                text_height,
                truncate_cell_to_width(
                    value,
                    (cell_rect.width - spec.t(18.0)).max(spec.t(8.0)),
                    spec.t(GRID_CELL_FONT_SIZE),
                ),
                color,
                spec.t(GRID_CELL_FONT_SIZE),
            )
            .clipped(cell_clip);
            labels.push(if absolute_row < u64::from(snapshot.header_row_count) {
                semibold(text)
            } else {
                text
            });
        }
    }
}

fn push_cell_editor_labels<'a>(
    labels: &mut Vec<TextLabel<'a>>,
    spec: &GridGeometry,
    frame: &'a GridFrame,
    body_clip: Rect,
    text_engine: &mut Option<&mut TextEngine>,
) {
    let Some((cell, buffer, _selection)) = editing_parts(&frame.edit_state) else {
        return;
    };
    let Some(rect) = selection_range_rect(
        &SelectionRange::Cells {
            start: cell.clone(),
            end: cell.clone(),
        },
        &frame.visible_window,
        spec,
    ) else {
        return;
    };
    let editor_rect = cell_editor_rect(rect, buffer, spec, text_engine);
    let Some(clip) = intersect_rect(editor_rect, body_clip) else {
        return;
    };
    labels.push(editor_text_label(editor_rect, buffer, spec).clipped(clip));
}

fn push_cell_editor(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
    mut text_engine: Option<&mut TextEngine>,
) {
    let Some((cell, buffer, selection)) = editing_parts(&frame.edit_state) else {
        return;
    };
    let Some(rect) = selection_range_rect(
        &SelectionRange::Cells {
            start: cell.clone(),
            end: cell.clone(),
        },
        &frame.visible_window,
        spec,
    ) else {
        return;
    };
    let rect = cell_editor_rect(rect, buffer, spec, &mut text_engine);
    let clip = spec.body_clip();
    push_clipped_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color: rgba_f32(13, 18, 20, 0.98),
        },
        clip,
    );
    let selection_start = selection.anchor.min(selection.caret);
    let selection_end = selection.anchor.max(selection.caret);
    if selection_start != selection_end {
        let start_x =
            editor_text_x_for_byte_index(rect, buffer, selection_start, spec, &mut text_engine);
        let end_x =
            editor_text_x_for_byte_index(rect, buffer, selection_end, spec, &mut text_engine);
        let width = (end_x - start_x).max(spec.t(2.0));
        push_clipped_rect(
            vertices,
            canvas,
            RenderRect {
                x: start_x,
                y: rect.y + spec.t(4.0),
                width,
                height: (rect.height - spec.t(8.0)).max(spec.t(8.0)),
                color: rgba_f32(80, 225, 216, 0.22),
            },
            clip,
        );
    }
    push_selection_outline_with(
        vertices,
        canvas,
        spec,
        rect,
        clip,
        rgba_f32(102, 232, 224, 0.92),
        spec.t(1.5).clamp(1.0, 2.5),
    );
    if frame.editor_caret_visible {
        let cursor_x = editor_cursor_x(rect, buffer, selection, frame, spec, &mut text_engine);
        push_clipped_rect(
            vertices,
            canvas,
            RenderRect {
                x: cursor_x,
                y: rect.y + spec.t(5.0),
                width: spec.t(1.5),
                height: (rect.height - spec.t(10.0)).max(spec.t(8.0)),
                color: rgba_f32(207, 250, 246, 0.92),
            },
            clip,
        );
    }
}

fn cell_editor_rect(
    rect: Rect,
    buffer: &str,
    spec: &GridGeometry,
    text_engine: &mut Option<&mut TextEngine>,
) -> Rect {
    let text_width = editor_rect_text_width(buffer, spec, text_engine);
    let body_clip = spec.body_clip();
    let max_width = (body_clip.x + body_clip.width - rect.x).max(rect.width);
    Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width.max(text_width).min(max_width),
        height: rect.height,
    }
}

fn editor_rect_text_width(
    buffer: &str,
    spec: &GridGeometry,
    text_engine: &mut Option<&mut TextEngine>,
) -> f32 {
    let fallback = buffer.chars().count() as f32 * spec.t(7.2) + spec.t(24.0);
    if let Some(text_engine) = text_engine
        && let Some(width) = text_engine.text_width(&editor_measure_label(buffer, spec, fallback))
    {
        return width + spec.t(24.0);
    }
    fallback
}

fn editor_measure_label<'a>(
    buffer: &'a str,
    spec: &GridGeometry,
    fallback_width: f32,
) -> TextLabel<'a> {
    label(
        0.0,
        0.0,
        (fallback_width - spec.t(18.0)).max(spec.t(12.0)),
        spec.t(GRID_CELL_TEXT_HEIGHT),
        buffer,
        editor_text_color(),
        spec.t(GRID_CELL_FONT_SIZE),
    )
}

fn editor_cursor_x(
    rect: Rect,
    buffer: &str,
    selection: TextSelection,
    frame: &GridFrame,
    spec: &GridGeometry,
    text_engine: &mut Option<&mut TextEngine>,
) -> f32 {
    let target_x = editor_text_x_for_byte_index(rect, buffer, selection.caret, spec, text_engine);
    let x = if let Some(animation) = &frame.editor_caret_animation {
        let from_x = editor_text_x_for_byte_index(
            rect,
            &animation.from_buffer,
            animation.from_caret,
            spec,
            text_engine,
        );
        let to_x =
            editor_text_x_for_byte_index(rect, buffer, animation.to_caret, spec, text_engine);
        lerp_f32(from_x, to_x, ease_caret_progress(animation.progress))
    } else {
        target_x
    };
    x.min(rect.x + rect.width - spec.t(6.0))
}

fn editor_text_x_for_byte_index(
    rect: Rect,
    buffer: &str,
    byte_index: usize,
    spec: &GridGeometry,
    text_engine: &mut Option<&mut TextEngine>,
) -> f32 {
    if let Some(text_engine) = text_engine
        && let Some(x) = text_engine.cursor_x(&editor_text_label(rect, buffer, spec), byte_index)
    {
        return x;
    }
    let prefix_chars = buffer
        .get(..byte_index)
        .map_or(buffer.chars().count() as f32, |text| {
            text.chars().count() as f32
        });
    let approx_char_width = spec.t(7.2);
    rect.x + spec.t(10.0) + prefix_chars * approx_char_width
}

fn editor_text_label<'a>(editor_rect: Rect, buffer: &'a str, spec: &GridGeometry) -> TextLabel<'a> {
    label(
        editor_rect.x + spec.t(10.0),
        editor_rect.y
            + ((editor_rect.height - spec.t(GRID_CELL_TEXT_HEIGHT)) * 0.5).max(spec.t(3.0)),
        (editor_rect.width - spec.t(18.0)).max(spec.t(12.0)),
        spec.t(GRID_CELL_TEXT_HEIGHT),
        buffer,
        editor_text_color(),
        spec.t(GRID_CELL_FONT_SIZE),
    )
}

fn frame_cell_text<'a>(frame: &'a GridFrame, cell: &CellRef) -> Option<&'a str> {
    if let Some(text) = frame.edited_cells.get(cell) {
        return Some(text.as_str());
    }
    let snapshot = frame.snapshot.as_ref()?;
    let display_row = u64::from(cell.row.saturating_sub(1));
    let display_column = cell.column.saturating_sub(1);
    if display_row < snapshot.start_row || display_column < snapshot.start_column {
        return None;
    }
    let row_index = display_row.saturating_sub(snapshot.start_row) as usize;
    let column_index = display_column.saturating_sub(snapshot.start_column) as usize;
    snapshot
        .rows
        .get(row_index)
        .and_then(|row| row.get(column_index))
        .map(String::as_str)
}

fn editor_text_color() -> TextColor {
    primary_text()
}

fn projected_cell_value<'a>(
    row: &'a [String],
    snapshot: &GridSnapshot,
    column_index: u32,
) -> Option<&'a String> {
    let projected_index = column_index.checked_sub(snapshot.start_column)?;
    row.get(projected_index as usize)
}

fn truncate_cell(value: &str, max_chars: usize) -> Cow<'_, str> {
    if max_chars == 0 {
        return Cow::Borrowed("");
    }
    let sanitized = sanitize_single_line(value);
    let sanitized_ref = sanitized.as_ref();
    if sanitized_ref.chars().count() <= max_chars {
        return sanitized;
    }
    let mut truncated = sanitized_ref
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    Cow::Owned(truncated)
}

fn truncate_cell_to_width(value: &str, width_px: f32, font_size_px: f32) -> Cow<'_, str> {
    if width_px <= 0.0 || font_size_px <= 0.0 {
        return Cow::Borrowed("");
    }
    let char_width = (font_size_px * CELL_TRUNCATE_CHAR_WIDTH_FACTOR).max(1.0);
    let max_chars = (width_px / char_width).floor().max(1.0) as usize;
    truncate_cell(value, max_chars)
}

fn sanitize_single_line(value: &str) -> Cow<'_, str> {
    if value.chars().all(|character| !character.is_control()) {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(
            value
                .chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .collect(),
        )
    }
}

fn column_name(mut index: u32) -> String {
    let mut chars = Vec::new();
    loop {
        let letter = (b'A' + (index % 26) as u8) as char;
        chars.push(letter);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    chars.iter().rev().collect()
}

fn label<'a>(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    text: impl Into<Cow<'a, str>>,
    color: TextColor,
    font_size: f32,
) -> TextLabel<'a> {
    TextLabel {
        rect: Rect {
            x,
            y,
            width,
            height,
        },
        clip: None,
        text: text.into(),
        color,
        weight: Weight::NORMAL,
        font_size,
        line_height: (font_size + 5.0).max(16.0),
    }
}

fn semibold(mut label: TextLabel<'_>) -> TextLabel<'_> {
    label.weight = Weight::SEMIBOLD;
    label
}

struct LabelSpec<T> {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    text: T,
    color: TextColor,
    font_size: f32,
}

#[derive(Debug, Clone, Copy)]
struct UiRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

fn scaled_label<'a, T>(canvas: Canvas, spec: LabelSpec<T>) -> TextLabel<'a>
where
    T: Into<Cow<'a, str>>,
{
    label(
        canvas.s(spec.x),
        canvas.s(spec.y),
        canvas.s(spec.width),
        canvas.s(spec.height),
        spec.text,
        spec.color,
        canvas.s(spec.font_size),
    )
}

#[derive(Debug, Clone, Copy)]
struct Canvas {
    width: f32,
    height: f32,
    scale_factor: f32,
}

impl Canvas {
    fn s(self, value: f32) -> f32 {
        value * self.scale_factor
    }
}

fn push_shell(vertices: &mut Vec<RectVertex>, canvas: Canvas) {
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: 0.0,
            y: 0.0,
            width: canvas.width,
            height: canvas.s(TOP_BAR_HEIGHT),
            color: rgb(17, 20, 23),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: 0.0,
            y: canvas.s(TOP_BAR_HEIGHT - 1.0),
            width: canvas.width,
            height: canvas.s(1.0),
            color: rgb(44, 50, 54),
        },
    );
}

fn push_button_rect(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: UiRect,
    fill: [f32; 4],
    border: [f32; 4],
) {
    let rect = Rect {
        x: canvas.s(rect.x),
        y: canvas.s(rect.y),
        width: canvas.s(rect.width),
        height: canvas.s(rect.height),
    };
    push_panel_rect_physical(vertices, canvas, rect, fill, border);
}

fn push_panel_rect(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: UiRect,
    fill: [f32; 4],
    border: [f32; 4],
) {
    let rect = Rect {
        x: canvas.s(rect.x),
        y: canvas.s(rect.y),
        width: canvas.s(rect.width),
        height: canvas.s(rect.height),
    };
    push_panel_rect_physical(vertices, canvas, rect, fill, border);
}

fn push_panel_rect_physical(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: Rect,
    fill: [f32; 4],
    border: [f32; 4],
) {
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color: fill,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: canvas.s(1.0),
            color: border,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y + rect.height - canvas.s(1.0),
            width: rect.width,
            height: canvas.s(1.0),
            color: rgba_f32(5, 8, 10, 0.44),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: canvas.s(1.0),
            height: rect.height,
            color: border,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x + rect.width - canvas.s(1.0),
            y: rect.y,
            width: canvas.s(1.0),
            height: rect.height,
            color: rgba_f32(5, 8, 10, 0.32),
        },
    );
}

fn push_dashboard(vertices: &mut Vec<RectVertex>, canvas: Canvas, frame: &DashboardFrame) {
    let tree = dashboard_component_tree(
        frame,
        canvas.width / canvas.scale_factor,
        canvas.height / canvas.scale_factor,
    );
    for node in &tree.nodes {
        push_ui_node(vertices, canvas, node);
    }
}

fn push_dashboard_labels<'a>(
    labels: &mut Vec<TextLabel<'a>>,
    canvas: Canvas,
    frame: &'a DashboardFrame,
) {
    let tree = dashboard_component_tree(
        frame,
        canvas.width / canvas.scale_factor,
        canvas.height / canvas.scale_factor,
    );
    for node in tree.nodes {
        if let UiNode::Text(text) = node {
            let label = scaled_label(
                canvas,
                LabelSpec {
                    x: text.rect.x,
                    y: text.rect.y,
                    width: text.rect.width,
                    height: text.rect.height,
                    text: text.text,
                    color: text_color(text.color),
                    font_size: text.font_size,
                },
            );
            labels.push(if text.weight == UiTextWeight::Semibold {
                semibold(label)
            } else {
                label
            });
        }
    }
}

fn push_ui_node(vertices: &mut Vec<RectVertex>, canvas: Canvas, node: &UiNode) {
    match node {
        UiNode::Panel(panel) => push_panel_rect(
            vertices,
            canvas,
            UiRect {
                x: panel.rect.x,
                y: panel.rect.y,
                width: panel.rect.width,
                height: panel.rect.height,
            },
            ui_color(panel.fill),
            panel.border.map_or(ui_color(panel.fill), ui_color),
        ),
        UiNode::Icon(icon) => push_ui_icon(
            vertices,
            canvas,
            icon.kind,
            icon.rect,
            icon.color,
            icon.stroke_width,
        ),
        UiNode::Text(_) => {}
    }
}

fn push_ui_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    kind: UiIconKind,
    rect: cellium_ui::UiRect,
    color: UiColor,
    stroke_width: f32,
) {
    let color = ui_color(color);
    match kind {
        UiIconKind::CelliumMark => push_cellium_mark(vertices, canvas, rect, color),
        UiIconKind::Plus => push_plus_icon(vertices, canvas, rect, color, stroke_width),
        UiIconKind::Home => push_home_icon(vertices, canvas, rect, color, stroke_width),
        UiIconKind::Star => push_star_icon(vertices, canvas, rect, color, stroke_width),
        UiIconKind::Workbooks => push_workbooks_icon(vertices, canvas, rect, color, stroke_width),
        UiIconKind::Square => push_square_icon(vertices, canvas, rect, color, stroke_width),
        UiIconKind::Table => push_table_icon(vertices, canvas, rect, color, stroke_width),
        UiIconKind::Database => push_database_icon(vertices, canvas, rect, color, stroke_width),
        UiIconKind::MoreHorizontal => push_more_icon(vertices, canvas, rect, color),
        UiIconKind::Search => push_search_icon(vertices, canvas, rect, color, stroke_width),
    }
}

fn push_cellium_mark(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
) {
    let x = canvas.s(rect.x);
    let y = canvas.s(rect.y);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x,
            y,
            width: canvas.s(rect.width),
            height: canvas.s(rect.height),
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + canvas.s(rect.width * 0.34),
            y: y + canvas.s(rect.height * 0.34),
            width: canvas.s(rect.width * 0.66),
            height: canvas.s(rect.height * 0.66),
            color: rgb(10, 11, 13),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + canvas.s(rect.width * 0.72),
            y: y + canvas.s(rect.height * 0.72),
            width: canvas.s(rect.width * 0.28),
            height: canvas.s(rect.height * 0.28),
            color,
        },
    );
}

fn push_plus_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    let stroke = canvas.s(stroke_width).max(1.0);
    let center_x = canvas.s(rect.x + rect.width / 2.0);
    let center_y = canvas.s(rect.y + rect.height / 2.0);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: canvas.s(rect.x),
            y: center_y - stroke / 2.0,
            width: canvas.s(rect.width),
            height: stroke,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: center_x - stroke / 2.0,
            y: canvas.s(rect.y),
            width: stroke,
            height: canvas.s(rect.height),
            color,
        },
    );
}

fn push_home_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    let stroke = canvas.s(stroke_width).max(1.0);
    let x = canvas.s(rect.x);
    let y = canvas.s(rect.y);
    let width = canvas.s(rect.width);
    let height = canvas.s(rect.height);
    let roof_y = y + height * 0.18;
    let body_y = y + height * 0.42;
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x,
            y: roof_y,
            width,
            height: stroke,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + width * 0.14,
            y: body_y,
            width: width * 0.72,
            height: stroke,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + width * 0.14,
            y: body_y,
            width: stroke,
            height: height * 0.46,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + width * 0.86 - stroke,
            y: body_y,
            width: stroke,
            height: height * 0.46,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + width * 0.42,
            y: y + height * 0.68,
            width: width * 0.16,
            height: height * 0.20,
            color,
        },
    );
}

fn push_star_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    let stroke = canvas.s(stroke_width).max(1.0);
    let center_x = canvas.s(rect.x + rect.width / 2.0);
    let center_y = canvas.s(rect.y + rect.height / 2.0);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: center_x - canvas.s(rect.width * 0.30),
            y: center_y - stroke / 2.0,
            width: canvas.s(rect.width * 0.60),
            height: stroke,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: center_x - stroke / 2.0,
            y: center_y - canvas.s(rect.height * 0.30),
            width: stroke,
            height: canvas.s(rect.height * 0.60),
            color,
        },
    );
    let dot = canvas.s(2.0).max(1.0);
    for (x, y) in [
        (rect.x + rect.width * 0.16, rect.y + rect.height * 0.16),
        (rect.x + rect.width * 0.76, rect.y + rect.height * 0.16),
        (rect.x + rect.width * 0.16, rect.y + rect.height * 0.76),
        (rect.x + rect.width * 0.76, rect.y + rect.height * 0.76),
    ] {
        push_rect(
            vertices,
            canvas,
            RenderRect {
                x: canvas.s(x),
                y: canvas.s(y),
                width: dot,
                height: dot,
                color,
            },
        );
    }
}

fn push_workbooks_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    push_square_icon(
        vertices,
        canvas,
        cellium_ui::UiRect::new(
            rect.x,
            rect.y + rect.height * 0.20,
            rect.width * 0.72,
            rect.height * 0.72,
        ),
        color,
        stroke_width,
    );
    push_square_icon(
        vertices,
        canvas,
        cellium_ui::UiRect::new(
            rect.x + rect.width * 0.28,
            rect.y,
            rect.width * 0.72,
            rect.height * 0.72,
        ),
        color,
        stroke_width,
    );
}

fn push_square_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    let stroke = canvas.s(stroke_width).max(1.0);
    let x = canvas.s(rect.x);
    let y = canvas.s(rect.y);
    let width = canvas.s(rect.width);
    let height = canvas.s(rect.height);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x,
            y,
            width,
            height: stroke,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x,
            y: y + height - stroke,
            width,
            height: stroke,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x,
            y,
            width: stroke,
            height,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + width - stroke,
            y,
            width: stroke,
            height,
            color,
        },
    );
}

fn push_table_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    push_square_icon(vertices, canvas, rect, color, stroke_width);
    let stroke = canvas.s(stroke_width).max(1.0);
    for offset in [rect.width / 3.0, rect.width * 2.0 / 3.0] {
        push_rect(
            vertices,
            canvas,
            RenderRect {
                x: canvas.s(rect.x + offset),
                y: canvas.s(rect.y),
                width: stroke,
                height: canvas.s(rect.height),
                color,
            },
        );
    }
}

fn push_database_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    let stroke = canvas.s(stroke_width).max(1.0);
    for y in [
        rect.y,
        rect.y + rect.height * 0.38,
        rect.y + rect.height * 0.76,
    ] {
        push_rect(
            vertices,
            canvas,
            RenderRect {
                x: canvas.s(rect.x),
                y: canvas.s(y),
                width: canvas.s(rect.width),
                height: stroke,
                color,
            },
        );
    }
}

fn push_more_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
) {
    let size = canvas.s(3.0).max(2.0);
    for offset in [0.0, rect.width * 0.42, rect.width * 0.84] {
        push_rect(
            vertices,
            canvas,
            RenderRect {
                x: canvas.s(rect.x + offset),
                y: canvas.s(rect.y + rect.height / 2.0) - size / 2.0,
                width: size,
                height: size,
                color,
            },
        );
    }
}

fn push_search_icon(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    rect: cellium_ui::UiRect,
    color: [f32; 4],
    stroke_width: f32,
) {
    let stroke = canvas.s(stroke_width).max(1.0);
    push_square_icon(
        vertices,
        canvas,
        cellium_ui::UiRect::new(rect.x, rect.y, rect.width * 0.62, rect.height * 0.62),
        color,
        stroke_width,
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: canvas.s(rect.x + rect.width * 0.58),
            y: canvas.s(rect.y + rect.height * 0.58),
            width: canvas.s(rect.width * 0.42),
            height: stroke,
            color,
        },
    );
}

#[derive(Debug, Clone, Copy)]
struct ActionChipSpec {
    x: f32,
    width: f32,
    hovered: bool,
    active: bool,
    fill: [f32; 4],
    border: [f32; 4],
}

fn push_action_chip(vertices: &mut Vec<RectVertex>, canvas: Canvas, spec: ActionChipSpec) {
    let fill = if spec.hovered || spec.active {
        spec.fill
    } else {
        rgba_f32(30, 35, 39, 0.84)
    };
    let border = if spec.hovered || spec.active {
        spec.border
    } else {
        rgba_f32(44, 52, 57, 0.68)
    };
    push_button_rect(
        vertices,
        canvas,
        UiRect {
            x: spec.x,
            y: ACTION_CHIP_Y,
            width: spec.width,
            height: ACTION_CHIP_HEIGHT,
        },
        fill,
        border,
    );
}

fn push_icon_plus(vertices: &mut Vec<RectVertex>, canvas: Canvas, x: f32, y: f32, hovered: bool) {
    let color = if hovered {
        accent_line()
    } else {
        rgb(145, 159, 166)
    };
    let x = canvas.s(x);
    let y = canvas.s(y);
    let size = canvas.s(9.0);
    let thickness = canvas.s(1.5).max(1.0);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x - size * 0.5,
            y: y - thickness * 0.5,
            width: size,
            height: thickness,
            color,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x - thickness * 0.5,
            y: y - size * 0.5,
            width: thickness,
            height: size,
            color,
        },
    );
}

fn push_grid(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    frame: &GridFrame,
    text_engine: Option<&mut TextEngine>,
) {
    let spec = GridGeometry::for_frame(canvas, frame);

    push_shell_controls(vertices, canvas, frame);
    push_formula_bar(vertices, canvas, &spec, frame);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.grid_left,
            y: spec.grid_top,
            width: spec.grid_width,
            height: spec.grid_height,
            color: rgb(12, 14, 16),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.grid_left,
            y: spec.grid_top,
            width: spec.header_width,
            height: spec.grid_height,
            color: rgb(22, 26, 29),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.grid_left,
            y: spec.grid_top,
            width: spec.grid_width,
            height: spec.header_height,
            color: rgb(24, 28, 31),
        },
    );
    push_grid_row_stripes(vertices, canvas, &spec, frame);
    if let Some(snapshot) = &frame.snapshot {
        push_csv_header_row(vertices, canvas, &spec, frame, snapshot);
    }
    push_hover_highlight(vertices, canvas, &spec, frame);
    push_selection_fills(vertices, canvas, &spec, frame);
    push_anchor_cell_marker(vertices, canvas, &spec, frame);
    push_selected_headers(vertices, canvas, &spec, frame);
    push_grid_lines(vertices, canvas, &spec, frame);
    push_column_resize_separator(vertices, canvas, &spec, frame);
    push_row_resize_separator(vertices, canvas, &spec, frame);
    push_selection_outlines(vertices, canvas, &spec, frame);
    push_cell_editor(vertices, canvas, &spec, frame, text_engine);
    push_scrollbars(vertices, canvas, &spec, frame);
    push_bottom_bar(vertices, canvas, &spec, frame);
}

fn push_column_resize_separator(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let Some(animation) = &frame.column_resize_animation else {
        return;
    };
    if !animation.show_separator {
        return;
    }
    let body_right = spec.grid_width - spec.scrollbar_thickness;
    let x = animation
        .separator_x_px
        .clamp(spec.header_width, body_right.max(spec.header_width));
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x - spec.s(1.0),
            y: spec.grid_top,
            width: spec.s(2.0).max(1.0),
            height: spec.grid_height - spec.scrollbar_thickness,
            color: rgba_f32(107, 241, 232, 0.92),
        },
    );
}

fn push_row_resize_separator(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let Some(animation) = &frame.row_resize_animation else {
        return;
    };
    if !animation.show_separator {
        return;
    }
    let body_bottom = spec.grid_top + spec.grid_height - spec.scrollbar_thickness;
    let y = (spec.grid_top + animation.separator_y_px).clamp(
        spec.grid_top + spec.header_height,
        body_bottom.max(spec.grid_top),
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.grid_left,
            y: y - spec.s(1.0),
            width: spec.grid_width - spec.scrollbar_thickness,
            height: spec.s(2.0).max(1.0),
            color: rgba_f32(107, 241, 232, 0.92),
        },
    );
}

fn push_hover_highlight(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    match frame.chrome.hovered.as_ref() {
        Some(ChromeHoverTarget::GridCell(cell)) => {
            let range = SelectionRange::Cells {
                start: cell.clone(),
                end: cell.clone(),
            };
            let Some(rect) = selection_range_rect(&range, &frame.visible_window, spec) else {
                return;
            };
            push_clipped_rect(
                vertices,
                canvas,
                RenderRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                    color: rgba_f32(255, 255, 255, 0.025),
                },
                spec.body_clip(),
            );
        }
        Some(ChromeHoverTarget::RowHeader(row)) => {
            let start = u64::from(row.saturating_sub(1));
            let Some(rect) = row_header_selection_rect(
                start,
                start.saturating_add(1),
                &frame.visible_window,
                spec,
            ) else {
                return;
            };
            push_clipped_rect(
                vertices,
                canvas,
                RenderRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                    color: rgba_f32(255, 255, 255, 0.045),
                },
                spec.row_header_clip(),
            );
        }
        Some(ChromeHoverTarget::ColumnHeader(column)) => {
            let start = column.saturating_sub(1);
            let Some(rect) = column_header_selection_rect(
                start,
                start.saturating_add(1),
                &frame.visible_window,
                spec,
            ) else {
                return;
            };
            push_clipped_rect(
                vertices,
                canvas,
                RenderRect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                    color: rgba_f32(255, 255, 255, 0.055),
                },
                spec.column_header_clip(),
            );
        }
        Some(ChromeHoverTarget::SelectAllCorner) => {
            push_rect(
                vertices,
                canvas,
                RenderRect {
                    x: spec.grid_left,
                    y: spec.grid_top,
                    width: spec.header_width,
                    height: spec.header_height,
                    color: rgba_f32(255, 255, 255, 0.052),
                },
            );
        }
        Some(ChromeHoverTarget::ColumnResize(column)) => {
            let column_index = column.saturating_sub(1);
            let Some(x) = column_edge_x(column_index, &frame.visible_window, spec) else {
                return;
            };
            push_clipped_rect(
                vertices,
                canvas,
                RenderRect {
                    x: x - spec.s(1.0),
                    y: spec.grid_top,
                    width: spec.s(2.0),
                    height: spec.grid_height - spec.scrollbar_thickness,
                    color: accent_line(),
                },
                spec.column_header_clip(),
            );
        }
        Some(ChromeHoverTarget::RowResize(row)) => {
            let row_index = u64::from(row.saturating_sub(1));
            let Some(y) = row_edge_y(row_index, &frame.visible_window, spec) else {
                return;
            };
            push_clipped_rect(
                vertices,
                canvas,
                RenderRect {
                    x: spec.grid_left,
                    y: y - spec.s(1.0),
                    width: spec.grid_width - spec.scrollbar_thickness,
                    height: spec.s(2.0),
                    color: accent_line(),
                },
                spec.row_header_clip(),
            );
        }
        Some(
            ChromeHoverTarget::OpenButton
            | ChromeHoverTarget::FormulaBar
            | ChromeHoverTarget::SortButton
            | ChromeHoverTarget::FilterButton
            | ChromeHoverTarget::ClearViewButton
            | ChromeHoverTarget::SheetTab
            | ChromeHoverTarget::VerticalScrollbar
            | ChromeHoverTarget::HorizontalScrollbar,
        )
        | None => {}
    }
}

fn push_shell_controls(vertices: &mut Vec<RectVertex>, canvas: Canvas, frame: &GridFrame) {
    let open_hovered = matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::OpenButton)
    );
    let sort_hovered = matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::SortButton)
    );
    let filter_hovered = matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::FilterButton)
    );
    let clear_view_hovered = matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::ClearViewButton)
    );

    push_button_rect(
        vertices,
        canvas,
        UiRect {
            x: OPEN_BUTTON_X,
            y: OPEN_BUTTON_Y,
            width: OPEN_BUTTON_WIDTH,
            height: OPEN_BUTTON_HEIGHT,
        },
        if open_hovered {
            rgb(43, 50, 55)
        } else {
            rgb(31, 36, 40)
        },
        if open_hovered {
            rgba_f32(94, 112, 120, 0.95)
        } else {
            rgba_f32(55, 64, 70, 0.9)
        },
    );
    push_icon_plus(
        vertices,
        canvas,
        OPEN_BUTTON_X + 17.0,
        OPEN_BUTTON_Y + 16.0,
        open_hovered,
    );
    push_action_chip(
        vertices,
        canvas,
        ActionChipSpec {
            x: SORT_BUTTON_X,
            width: SORT_BUTTON_WIDTH,
            hovered: sort_hovered,
            active: !frame.view.sorts.is_empty(),
            fill: rgba_f32(120, 185, 255, 0.16),
            border: rgba_f32(120, 185, 255, 0.72),
        },
    );
    push_action_chip(
        vertices,
        canvas,
        ActionChipSpec {
            x: FILTER_BUTTON_X,
            width: FILTER_BUTTON_WIDTH,
            hovered: filter_hovered,
            active: !frame.view.filters.is_empty(),
            fill: rgba_f32(177, 143, 255, 0.16),
            border: rgba_f32(177, 143, 255, 0.72),
        },
    );
    push_action_chip(
        vertices,
        canvas,
        ActionChipSpec {
            x: CLEAR_VIEW_BUTTON_X,
            width: CLEAR_VIEW_BUTTON_WIDTH,
            hovered: clear_view_hovered,
            active: false,
            fill: rgba_f32(255, 255, 255, 0.08),
            border: rgba_f32(150, 160, 168, 0.62),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: canvas.s(260.0),
            y: canvas.s(14.0),
            width: canvas.s(1.0),
            height: canvas.s(24.0),
            color: rgb(47, 54, 59),
        },
    );
    push_panel_rect(
        vertices,
        canvas,
        UiRect {
            x: SUMMARY_X,
            y: SUMMARY_Y,
            width: (canvas.width / canvas.scale_factor - SUMMARY_X - 14.0).max(120.0),
            height: SUMMARY_HEIGHT,
        },
        rgb(25, 29, 33),
        rgba_f32(48, 56, 62, 0.74),
    );
}

fn push_formula_bar(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let formula_hovered = matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::FormulaBar)
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: 0.0,
            y: spec.formula_top,
            width: canvas.width,
            height: spec.formula_height,
            color: rgb(12, 15, 17),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: 0.0,
            y: spec.formula_top,
            width: canvas.width,
            height: spec.s(1.0),
            color: rgb(26, 31, 35),
        },
    );
    push_panel_rect_physical(
        vertices,
        canvas,
        Rect {
            x: spec.s(CELL_NAME_X),
            y: spec.formula_top + spec.s(8.0),
            width: spec.s(CELL_NAME_WIDTH),
            height: spec.s(28.0),
        },
        rgb(29, 35, 40),
        rgba_f32(47, 58, 65, 0.84),
    );
    push_panel_rect_physical(
        vertices,
        canvas,
        Rect {
            x: spec.s(FORMULA_INPUT_X),
            y: spec.formula_top + spec.s(8.0),
            width: (canvas.width - spec.s(FORMULA_INPUT_X + FORMULA_INPUT_RIGHT_PAD))
                .max(spec.s(120.0)),
            height: spec.s(28.0),
        },
        if formula_hovered {
            rgb(22, 27, 31)
        } else {
            rgb(17, 21, 24)
        },
        if formula_hovered {
            rgba_f32(67, 82, 91, 0.92)
        } else {
            rgba_f32(40, 48, 54, 0.84)
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.s(99.0),
            y: spec.formula_top + spec.s(14.0),
            width: spec.s(1.0),
            height: spec.s(16.0),
            color: rgb(54, 64, 70),
        },
    );
}

fn push_bottom_bar(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let sheet_hovered = matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::SheetTab)
    );
    let y = canvas.height - spec.tab_height;
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: 0.0,
            y,
            width: canvas.width,
            height: spec.tab_height,
            color: rgb(15, 18, 21),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: 0.0,
            y,
            width: canvas.width,
            height: spec.s(1.0),
            color: rgb(39, 46, 50),
        },
    );
    push_button_rect(
        vertices,
        canvas,
        UiRect {
            x: 14.0,
            y: y / canvas.scale_factor + 6.0,
            width: 198.0,
            height: 24.0,
        },
        if sheet_hovered {
            rgb(27, 54, 55)
        } else {
            rgb(23, 43, 45)
        },
        rgba_f32(59, 140, 136, 0.68),
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.s(224.0),
            y: y + spec.s(8.0),
            width: spec.s(1.0),
            height: spec.s(20.0),
            color: rgb(43, 50, 55),
        },
    );
}

fn push_grid_row_stripes(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let window = &frame.visible_window;
    let body_clip = spec.body_clip();
    for row in 0..window.row_count.min(80) {
        let absolute_row = window.start_row.saturating_add(u64::from(row));
        if absolute_row % 2 == 0 {
            continue;
        }
        let row_rect = visible_row_rect(window, spec, u64::from(row), Some(frame));
        if row_rect.y > spec.grid_top + spec.grid_height - spec.scrollbar_thickness {
            break;
        }
        push_clipped_rect(
            vertices,
            canvas,
            RenderRect {
                x: body_clip.x,
                y: row_rect.y,
                width: body_clip.width,
                height: row_rect.height,
                color: rgb(16, 19, 22),
            },
            body_clip,
        );
    }
}

fn push_selection_fills(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let body_clip = spec.body_clip();
    for (index, range) in frame.selection.ranges.iter().enumerate() {
        let Some(color) = selection_fill_color(range) else {
            continue;
        };
        let rect = if index + 1 == frame.selection.ranges.len() {
            animated_selection_rect_for_range(frame, range, spec)
                .or_else(|| selection_range_rect(range, &frame.visible_window, spec))
        } else {
            selection_range_rect(range, &frame.visible_window, spec)
        };
        let Some(rect) = rect else {
            continue;
        };
        push_clipped_rect(
            vertices,
            canvas,
            RenderRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color,
            },
            body_clip,
        );
    }
}

fn push_csv_header_row(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
    snapshot: &GridSnapshot,
) {
    let window = &frame.visible_window;
    if snapshot.header_row_count == 0 || window.start_row >= u64::from(snapshot.header_row_count) {
        return;
    }
    let body_clip = spec.body_clip();
    let visible_header_rows =
        (u64::from(snapshot.header_row_count) - window.start_row).min(u64::from(window.row_count));
    push_clipped_rect(
        vertices,
        canvas,
        RenderRect {
            x: body_clip.x,
            y: spec.grid_top + spec.header_height + visible_row_offset_y(window, 0, Some(frame)),
            width: body_clip.width,
            height: row_span_height(window, 0, visible_header_rows, Some(frame)),
            color: rgba_f32(31, 39, 41, 0.82),
        },
        body_clip,
    );
}

fn push_anchor_cell_marker(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let body_clip = spec.body_clip();
    let anchor = match &frame.selection.anchor {
        SelectionAnchor::Cell(cell) => cell.clone(),
        SelectionAnchor::Row(_) | SelectionAnchor::Column(_) | SelectionAnchor::Sheet => {
            frame.selection.active.clone()
        }
    };
    let range = SelectionRange::Cells {
        start: anchor.clone(),
        end: anchor,
    };
    let Some(rect) = selection_range_rect(&range, &frame.visible_window, spec) else {
        return;
    };
    push_clipped_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color: rgba_f32(0, 0, 0, 0.16),
        },
        body_clip,
    );
    push_selection_outline_with(
        vertices,
        canvas,
        spec,
        Rect {
            x: rect.x + spec.t(1.5),
            y: rect.y + spec.t(1.5),
            width: (rect.width - spec.t(3.0)).max(1.0),
            height: (rect.height - spec.t(3.0)).max(1.0),
        },
        body_clip,
        rgba_f32(156, 247, 240, 0.58),
        spec.t(1.0).clamp(1.0, 2.0),
    );
}

fn push_selected_headers(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    for range in &frame.selection.ranges {
        push_selected_row_headers(vertices, canvas, spec, &frame.visible_window, range);
        push_selected_column_headers(vertices, canvas, spec, &frame.visible_window, range);
    }
}

fn push_selected_row_headers(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    window: &VisibleWindow,
    range: &SelectionRange,
) {
    let Some((start, end, strong)) = selected_row_span(range) else {
        return;
    };
    let Some(rect) = row_header_selection_rect(start, end, window, spec) else {
        return;
    };
    push_clipped_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color: if strong {
                rgba_f32(80, 225, 216, 0.18)
            } else {
                rgba_f32(80, 225, 216, 0.095)
            },
        },
        spec.row_header_clip(),
    );
}

fn push_selected_column_headers(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    window: &VisibleWindow,
    range: &SelectionRange,
) {
    let Some((start, end, strong)) = selected_column_span(range) else {
        return;
    };
    let Some(rect) = column_header_selection_rect(start, end, window, spec) else {
        return;
    };
    push_clipped_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color: if strong {
                rgba_f32(80, 225, 216, 0.18)
            } else {
                rgba_f32(80, 225, 216, 0.095)
            },
        },
        spec.column_header_clip(),
    );
}

fn push_selection_outlines(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let body_clip = spec.body_clip();
    for (index, range) in frame.selection.ranges.iter().enumerate() {
        if editing_cell(&frame.edit_state)
            .is_some_and(|cell| matches!(range, SelectionRange::Cells { start, end } if start == end && start == cell))
        {
            continue;
        }
        let Some(rect) = selection_range_rect(range, &frame.visible_window, spec) else {
            continue;
        };
        let rect = if index + 1 == frame.selection.ranges.len() {
            animated_selection_rect_for_range(frame, range, spec).unwrap_or(rect)
        } else {
            rect
        };
        push_selection_outline(vertices, canvas, spec, rect, body_clip);
        if index + 1 == frame.selection.ranges.len() {
            push_selection_handle(vertices, canvas, spec, rect, body_clip);
        }
    }
}

fn animated_selection_rect_for_range(
    frame: &GridFrame,
    range: &SelectionRange,
    spec: &GridGeometry,
) -> Option<Rect> {
    let animation = frame.selection_animation.as_ref()?;
    if animation.to != *range {
        return None;
    }
    animated_selection_rect(animation, &frame.visible_window, spec)
}

fn animated_selection_rect(
    animation: &cellium_ui::SelectionAnimation,
    window: &VisibleWindow,
    spec: &GridGeometry,
) -> Option<Rect> {
    let from = selection_range_rect(&animation.from, window, spec)?;
    let to = selection_range_rect(&animation.to, window, spec)?;
    let progress = ease_selection_progress(animation.progress);
    Some(lerp_rect(from, to, progress))
}

fn ease_selection_progress(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(3)
}

fn ease_caret_progress(progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    1.0 - (1.0 - progress).powi(2)
}

fn lerp_rect(from: Rect, to: Rect, progress: f32) -> Rect {
    Rect {
        x: lerp_f32(from.x, to.x, progress),
        y: lerp_f32(from.y, to.y, progress),
        width: lerp_f32(from.width, to.width, progress).max(1.0),
        height: lerp_f32(from.height, to.height, progress).max(1.0),
    }
}

fn lerp_f32(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

fn selection_fill_color(range: &SelectionRange) -> Option<[f32; 4]> {
    if selection_is_single_cell(range) {
        return None;
    }
    Some(rgba_f32(78, 216, 207, 0.055))
}

fn selection_is_single_cell(range: &SelectionRange) -> bool {
    matches!(range, SelectionRange::Cells { start, end } if start == end)
}

fn selected_row_span(range: &SelectionRange) -> Option<(u64, u64, bool)> {
    match range {
        SelectionRange::Cells { start, end } => Some((
            u64::from(start.row.min(end.row).saturating_sub(1)),
            u64::from(start.row.max(end.row)),
            false,
        )),
        SelectionRange::Rows { start, end } => Some((
            u64::from(start.min(end).saturating_sub(1)),
            u64::from(*start.max(end)),
            true,
        )),
        SelectionRange::Sheet | SelectionRange::Columns { .. } => None,
    }
}

fn selected_column_span(range: &SelectionRange) -> Option<(u32, u32, bool)> {
    match range {
        SelectionRange::Cells { start, end } => Some((
            start.column.min(end.column).saturating_sub(1),
            start.column.max(end.column),
            false,
        )),
        SelectionRange::Columns { start, end } => {
            Some((start.min(end).saturating_sub(1), *start.max(end), true))
        }
        SelectionRange::Sheet | SelectionRange::Rows { .. } => None,
    }
}

fn visible_row_height(window: &VisibleWindow, visible_row: u64, frame: Option<&GridFrame>) -> f32 {
    let height = window
        .row_heights
        .get(visible_row as usize)
        .copied()
        .unwrap_or(0) as f32;
    let row_index = window.start_row.saturating_add(visible_row);
    frame
        .and_then(|frame| frame.row_resize_animation.as_ref())
        .filter(|animation| animation.row == row_index)
        .map_or(height, |animation| {
            animation.visual_row_height_px.max(MIN_VISUAL_ROW_HEIGHT_PX)
        })
}

fn visible_column_width(
    window: &VisibleWindow,
    visible_column: u32,
    frame: Option<&GridFrame>,
) -> f32 {
    let width = window
        .column_widths
        .get(visible_column as usize)
        .copied()
        .unwrap_or(0) as f32;
    let column_index = window.start_column.saturating_add(visible_column);
    frame
        .and_then(|frame| frame.column_resize_animation.as_ref())
        .filter(|animation| animation.column == column_index)
        .map_or(width, |animation| {
            animation
                .visual_column_width_px
                .max(MIN_VISUAL_COLUMN_WIDTH_PX)
        })
}

fn visible_row_offset_y(
    window: &VisibleWindow,
    visible_row: u64,
    frame: Option<&GridFrame>,
) -> f32 {
    let rows = visible_row.min(u64::from(window.row_count));
    (0..rows)
        .map(|row| visible_row_height(window, row, frame))
        .sum::<f32>()
        - window.row_offset_px as f32
}

fn visible_column_offset_x(
    window: &VisibleWindow,
    visible_column: u32,
    frame: Option<&GridFrame>,
) -> f32 {
    let columns = visible_column.min(window.column_count);
    (0..columns)
        .map(|column| visible_column_width(window, column, frame))
        .sum::<f32>()
        - window.column_offset_px as f32
}

fn visible_row_rect(
    window: &VisibleWindow,
    spec: &GridGeometry,
    visible_row: u64,
    frame: Option<&GridFrame>,
) -> Rect {
    Rect {
        x: spec.header_width,
        y: spec.grid_top + spec.header_height + visible_row_offset_y(window, visible_row, frame),
        width: spec.body_clip().width,
        height: visible_row_height(window, visible_row, frame),
    }
}

fn visible_column_rect(
    window: &VisibleWindow,
    spec: &GridGeometry,
    visible_column: u32,
    frame: Option<&GridFrame>,
) -> Rect {
    Rect {
        x: spec.header_width + visible_column_offset_x(window, visible_column, frame),
        y: spec.grid_top + spec.header_height,
        width: visible_column_width(window, visible_column, frame),
        height: spec.body_clip().height,
    }
}

fn visible_cell_rect(
    window: &VisibleWindow,
    spec: &GridGeometry,
    visible_row: u64,
    visible_column: u32,
    frame: Option<&GridFrame>,
) -> Rect {
    Rect {
        x: spec.header_width + visible_column_offset_x(window, visible_column, frame),
        y: spec.grid_top + spec.header_height + visible_row_offset_y(window, visible_row, frame),
        width: visible_column_width(window, visible_column, frame),
        height: visible_row_height(window, visible_row, frame),
    }
}

fn row_span_height(
    window: &VisibleWindow,
    visible_start: u64,
    visible_end: u64,
    frame: Option<&GridFrame>,
) -> f32 {
    (visible_start..visible_end.min(u64::from(window.row_count)))
        .map(|row| visible_row_height(window, row, frame))
        .sum()
}

fn column_span_width(
    window: &VisibleWindow,
    visible_start: u32,
    visible_end: u32,
    frame: Option<&GridFrame>,
) -> f32 {
    (visible_start..visible_end.min(window.column_count))
        .map(|column| visible_column_width(window, column, frame))
        .sum()
}

fn row_header_selection_rect(
    row_start: u64,
    row_end: u64,
    window: &VisibleWindow,
    spec: &GridGeometry,
) -> Option<Rect> {
    let first_row = row_start.max(window.start_row);
    let last_row = row_end.min(window.start_row.saturating_add(u64::from(window.row_count)));
    if first_row >= last_row {
        return None;
    }
    let visible_row = first_row.saturating_sub(window.start_row);
    let visible_end = last_row.saturating_sub(window.start_row);
    Some(Rect {
        x: spec.grid_left,
        y: spec.grid_top + spec.header_height + visible_row_offset_y(window, visible_row, None),
        width: spec.header_width,
        height: row_span_height(window, visible_row, visible_end, None),
    })
}

fn column_header_selection_rect(
    column_start: u32,
    column_end: u32,
    window: &VisibleWindow,
    spec: &GridGeometry,
) -> Option<Rect> {
    let first_column = column_start.max(window.start_column);
    let last_column = column_end.min(window.start_column.saturating_add(window.column_count));
    if first_column >= last_column {
        return None;
    }
    let visible_column = first_column.saturating_sub(window.start_column);
    let visible_end = last_column.saturating_sub(window.start_column);
    Some(Rect {
        x: spec.header_width + visible_column_offset_x(window, visible_column, None),
        y: spec.grid_top,
        width: column_span_width(window, visible_column, visible_end, None),
        height: spec.header_height,
    })
}

fn column_edge_x(column_index: u32, window: &VisibleWindow, spec: &GridGeometry) -> Option<f32> {
    if column_index < window.start_column {
        return None;
    }
    let edge = column_index
        .saturating_sub(window.start_column)
        .saturating_add(1);
    if edge > window.column_count {
        return None;
    }
    Some(spec.header_width + visible_column_offset_x(window, edge, None))
}

fn row_edge_y(row_index: u64, window: &VisibleWindow, spec: &GridGeometry) -> Option<f32> {
    if row_index < window.start_row {
        return None;
    }
    let edge = row_index.saturating_sub(window.start_row).saturating_add(1);
    if edge > u64::from(window.row_count) {
        return None;
    }
    Some(spec.grid_top + spec.header_height + visible_row_offset_y(window, edge, None))
}

fn selection_range_rect(
    range: &SelectionRange,
    window: &VisibleWindow,
    spec: &GridGeometry,
) -> Option<Rect> {
    let window_row_start = window.start_row;
    let window_row_end = window.start_row.saturating_add(u64::from(window.row_count));
    let window_column_start = window.start_column;
    let window_column_end = window.start_column.saturating_add(window.column_count);
    let (row_start, row_end, column_start, column_end) = match range {
        SelectionRange::Cells { start, end } => (
            u64::from(start.row.min(end.row).saturating_sub(1)),
            u64::from(start.row.max(end.row)),
            start.column.min(end.column).saturating_sub(1),
            start.column.max(end.column),
        ),
        SelectionRange::Rows { start, end } => (
            u64::from(start.min(end).saturating_sub(1)),
            u64::from(*start.max(end)),
            window_column_start,
            window_column_end,
        ),
        SelectionRange::Columns { start, end } => (
            window_row_start,
            window_row_end,
            start.min(end).saturating_sub(1),
            *start.max(end),
        ),
        SelectionRange::Sheet => (
            window_row_start,
            window_row_end,
            window_column_start,
            window_column_end,
        ),
    };
    let first_row = row_start.max(window_row_start);
    let last_row = row_end.min(window_row_end);
    let first_column = column_start.max(window_column_start);
    let last_column = column_end.min(window_column_end);
    if first_row >= last_row || first_column >= last_column {
        return None;
    }

    let visible_row = first_row.saturating_sub(window.start_row);
    let visible_column = first_column.saturating_sub(window.start_column);
    let visible_row_end = last_row.saturating_sub(window.start_row);
    let visible_column_end = last_column.saturating_sub(window.start_column);
    Some(Rect {
        x: spec.header_width + visible_column_offset_x(window, visible_column, None),
        y: spec.grid_top + spec.header_height + visible_row_offset_y(window, visible_row, None),
        width: column_span_width(window, visible_column, visible_column_end, None),
        height: row_span_height(window, visible_row, visible_row_end, None),
    })
}

fn push_selection_outline(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    rect: Rect,
    clip: Rect,
) {
    let color = rgba_f32(80, 225, 216, 0.96);
    let thickness = spec.t(2.0).clamp(1.25, 3.0);
    push_selection_outline_with(vertices, canvas, spec, rect, clip, color, thickness);
}

fn push_selection_outline_with(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    _spec: &GridGeometry,
    rect: Rect,
    clip: Rect,
    color: [f32; 4],
    thickness: f32,
) {
    for border in [
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: thickness,
            color,
        },
        RenderRect {
            x: rect.x,
            y: rect.y + rect.height - thickness,
            width: rect.width,
            height: thickness,
            color,
        },
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: thickness,
            height: rect.height,
            color,
        },
        RenderRect {
            x: rect.x + rect.width - thickness,
            y: rect.y,
            width: thickness,
            height: rect.height,
            color,
        },
    ] {
        push_clipped_rect(vertices, canvas, border, clip);
    }
}

fn push_selection_handle(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    rect: Rect,
    clip: Rect,
) {
    let size = spec.t(6.0).clamp(5.0, 8.0);
    push_clipped_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x + rect.width - size * 0.5,
            y: rect.y + rect.height - size * 0.5,
            width: size,
            height: size,
            color: rgba_f32(80, 225, 216, 1.0),
        },
        clip,
    );
}

fn push_scrollbars(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let thickness = frame.viewport.metrics.scrollbar_thickness as f32;
    if thickness <= 0.0 {
        return;
    }

    let track = rgb(18, 22, 25);
    let vertical_thumb = if matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::VerticalScrollbar)
    ) {
        rgb(103, 122, 130)
    } else {
        rgb(70, 82, 88)
    };
    let horizontal_thumb = if matches!(
        frame.chrome.hovered.as_ref(),
        Some(ChromeHoverTarget::HorizontalScrollbar)
    ) {
        rgb(103, 122, 130)
    } else {
        rgb(70, 82, 88)
    };
    let corner = rgb(15, 18, 20);
    let vertical_track = RenderRect {
        x: frame
            .viewport
            .pixel_width
            .saturating_sub(frame.viewport.metrics.scrollbar_thickness) as f32,
        y: spec.grid_top + frame.viewport.metrics.header_height as f32,
        width: thickness,
        height: frame.viewport.body_height() as f32,
        color: track,
    };
    let horizontal_track = RenderRect {
        x: frame.viewport.metrics.header_width as f32,
        y: spec.grid_top
            + frame
                .viewport
                .pixel_height
                .saturating_sub(frame.viewport.metrics.scrollbar_thickness) as f32,
        width: frame.viewport.body_width() as f32,
        height: thickness,
        color: track,
    };
    push_rect(vertices, canvas, vertical_track);
    push_rect(vertices, canvas, horizontal_track);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: horizontal_track.x + horizontal_track.width,
            y: horizontal_track.y,
            width: thickness,
            height: thickness,
            color: corner,
        },
    );

    let Some(snapshot) = &frame.snapshot else {
        return;
    };
    if let Some(layout) = frame.viewport.vertical_scrollbar(snapshot.row_count) {
        push_scrollbar_thumb(vertices, canvas, spec, layout, vertical_thumb);
    }
    if let Some(layout) = frame.viewport.horizontal_scrollbar(snapshot.columns.len()) {
        push_scrollbar_thumb(vertices, canvas, spec, layout, horizontal_thumb);
    }
}

fn push_scrollbar_thumb(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    layout: cellium_ui::ScrollbarLayout,
    color: [f32; 4],
) {
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: layout.thumb_x as f32,
            y: spec.grid_top + layout.thumb_y as f32,
            width: layout.thumb_width as f32,
            height: layout.thumb_height as f32,
            color,
        },
    );
}

fn push_tooltip(vertices: &mut Vec<RectVertex>, canvas: Canvas, x: f32, y: f32, width: f32) {
    let x = canvas.s(x);
    let y = canvas.s(y);
    let width = canvas.s(width);
    let height = canvas.s(TOOLTIP_HEIGHT);
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: x + canvas.s(2.0),
            y: y + canvas.s(2.0),
            width,
            height,
            color: rgb(5, 7, 9),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x,
            y,
            width,
            height,
            color: rgb(34, 39, 44),
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x,
            y,
            width,
            height: canvas.s(1.0),
            color: rgb(78, 92, 99),
        },
    );
}

#[derive(Debug, Clone, Copy)]
struct GridGeometry {
    scale_factor: f32,
    table_scale_factor: f32,
    formula_top: f32,
    formula_height: f32,
    tab_height: f32,
    grid_left: f32,
    grid_top: f32,
    grid_width: f32,
    grid_height: f32,
    header_width: f32,
    header_height: f32,
    scrollbar_thickness: f32,
}

impl GridGeometry {
    fn for_frame(canvas: Canvas, frame: &GridFrame) -> Self {
        let formula_top = canvas.s(FORMULA_BAR_TOP as f32);
        let formula_height = canvas.s(FORMULA_BAR_HEIGHT as f32);
        let tab_height = canvas.s(SHEET_TAB_HEIGHT as f32);
        let grid_top = formula_top + formula_height;
        let metrics = &frame.viewport.metrics;
        Self {
            scale_factor: canvas.scale_factor,
            table_scale_factor: canvas.scale_factor * metrics.zoom as f32,
            formula_top,
            formula_height,
            tab_height,
            grid_left: 0.0,
            grid_top,
            grid_width: canvas.width,
            grid_height: (canvas.height - grid_top - tab_height).max(canvas.s(120.0)),
            header_width: metrics.header_width as f32,
            header_height: metrics.header_height as f32,
            scrollbar_thickness: metrics.scrollbar_thickness as f32,
        }
    }

    fn s(self, value: f32) -> f32 {
        value * self.scale_factor
    }

    fn t(self, value: f32) -> f32 {
        value * self.table_scale_factor
    }

    fn column_header_clip(self) -> Rect {
        Rect {
            x: self.grid_left + self.header_width,
            y: self.grid_top,
            width: (self.grid_width - self.header_width - self.scrollbar_thickness).max(0.0),
            height: self.header_height,
        }
    }

    fn row_header_clip(self) -> Rect {
        Rect {
            x: self.grid_left,
            y: self.grid_top + self.header_height,
            width: self.header_width,
            height: (self.grid_height - self.header_height - self.scrollbar_thickness).max(0.0),
        }
    }

    fn body_clip(self) -> Rect {
        Rect {
            x: self.grid_left + self.header_width,
            y: self.grid_top + self.header_height,
            width: (self.grid_width - self.header_width - self.scrollbar_thickness).max(0.0),
            height: (self.grid_height - self.header_height - self.scrollbar_thickness).max(0.0),
        }
    }
}

fn push_grid_lines(
    vertices: &mut Vec<RectVertex>,
    canvas: Canvas,
    spec: &GridGeometry,
    frame: &GridFrame,
) {
    let line = rgb(36, 41, 44);
    let header_line = rgb(52, 59, 63);
    let data_grid_clip = Rect {
        x: spec.header_width,
        y: spec.grid_top,
        width: (spec.grid_width - spec.header_width - spec.scrollbar_thickness).max(0.0),
        height: (spec.grid_height - spec.scrollbar_thickness).max(0.0),
    };
    let body_clip = Rect {
        x: spec.grid_left,
        y: spec.grid_top + spec.header_height,
        width: (spec.grid_width - spec.scrollbar_thickness).max(0.0),
        height: (spec.grid_height - spec.header_height - spec.scrollbar_thickness).max(0.0),
    };
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.header_width - 1.0,
            y: spec.grid_top,
            width: 1.0,
            height: spec.grid_height,
            color: header_line,
        },
    );
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: spec.grid_left,
            y: spec.grid_top + spec.header_height - 1.0,
            width: spec.grid_width,
            height: 1.0,
            color: header_line,
        },
    );

    let max_columns = frame.visible_window.column_count.min(32);
    for column in 0..=max_columns {
        let x =
            spec.header_width + visible_column_offset_x(&frame.visible_window, column, Some(frame));
        if x > spec.grid_width {
            break;
        }
        push_clipped_rect(
            vertices,
            canvas,
            RenderRect {
                x,
                y: spec.grid_top,
                width: 1.0,
                height: spec.grid_height,
                color: line,
            },
            data_grid_clip,
        );
    }

    let max_rows = frame.visible_window.row_count.min(80);
    for row in 0..=max_rows {
        let y = spec.grid_top
            + spec.header_height
            + visible_row_offset_y(&frame.visible_window, u64::from(row), Some(frame));
        if y > spec.grid_top + spec.grid_height {
            break;
        }
        push_clipped_rect(
            vertices,
            canvas,
            RenderRect {
                x: spec.grid_left,
                y,
                width: spec.grid_width,
                height: 1.0,
                color: line,
            },
            body_clip,
        );
    }
}

fn push_clipped_rect(vertices: &mut Vec<RectVertex>, canvas: Canvas, rect: RenderRect, clip: Rect) {
    let color = rect.color;
    let Some(rect) = intersect_rect(
        Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        },
        clip,
    ) else {
        return;
    };
    push_rect(
        vertices,
        canvas,
        RenderRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            color,
        },
    );
}

fn push_rect(vertices: &mut Vec<RectVertex>, canvas: Canvas, rect: RenderRect) {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }
    let left = to_clip_x(rect.x, canvas.width);
    let right = to_clip_x(rect.x + rect.width, canvas.width);
    let top = to_clip_y(rect.y, canvas.height);
    let bottom = to_clip_y(rect.y + rect.height, canvas.height);
    let color = rect.color;
    vertices.extend_from_slice(&[
        RectVertex {
            position: [left, top],
            color,
        },
        RectVertex {
            position: [left, bottom],
            color,
        },
        RectVertex {
            position: [right, bottom],
            color,
        },
        RectVertex {
            position: [left, top],
            color,
        },
        RectVertex {
            position: [right, bottom],
            color,
        },
        RectVertex {
            position: [right, top],
            color,
        },
    ]);
}

fn to_clip_x(x: f32, width: f32) -> f32 {
    x / width * 2.0 - 1.0
}

fn to_clip_y(y: f32, height: f32) -> f32 {
    1.0 - y / height * 2.0
}

fn rgb(red: u8, green: u8, blue: u8) -> [f32; 4] {
    [
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        1.0,
    ]
}

fn rgba_f32(red: u8, green: u8, blue: u8, alpha: f32) -> [f32; 4] {
    [
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
        alpha,
    ]
}

fn rgba(color: Rgba) -> [f32; 4] {
    [
        color.red as f32,
        color.green as f32,
        color.blue as f32,
        color.alpha as f32,
    ]
}

fn ui_color(color: UiColor) -> [f32; 4] {
    [
        f32::from(color.red) / 255.0,
        f32::from(color.green) / 255.0,
        f32::from(color.blue) / 255.0,
        color.alpha,
    ]
}

fn text_color(color: UiColor) -> TextColor {
    TextColor::rgb(color.red, color.green, color.blue)
}

fn primary_text() -> TextColor {
    TextColor::rgb(228, 234, 238)
}

fn muted_text() -> TextColor {
    TextColor::rgb(172, 184, 190)
}

fn subdued_text() -> TextColor {
    TextColor::rgb(112, 124, 130)
}

fn header_text() -> TextColor {
    TextColor::rgb(150, 164, 170)
}

fn csv_header_text() -> TextColor {
    TextColor::rgb(190, 204, 210)
}

fn accent_text() -> TextColor {
    TextColor::rgb(95, 221, 211)
}

fn violet_text() -> TextColor {
    TextColor::rgb(188, 163, 245)
}

fn accent_line() -> [f32; 4] {
    rgba_f32(80, 225, 216, 0.95)
}

#[cfg(test)]
mod tests;
