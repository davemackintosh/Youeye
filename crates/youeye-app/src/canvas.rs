//! Vello-backed design canvas.
//!
//! Renders into an offscreen `Rgba8Unorm` texture which egui displays via
//! [`egui_wgpu::Renderer::register_native_texture`]. Rendering runs *before*
//! `egui_ctx.run`, using the camera + rect recorded by the previous frame.
//! That one-frame lag is invisible at 60fps and keeps the flow clean: no
//! re-entrancy between vello and egui.

use anyhow::Result;
use kurbo::{
    Affine, BezPath, Ellipse as KEllipse, Line, Point, Rect as KRect, Shape, Stroke, Vec2,
};
use vello::peniko::color::{AlphaColor, Srgb};
use vello::peniko::{Brush, Color};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use youeye_doc::{
    Color as DocColor, Document, Ellipse, Fill, Frame, Node, NodeBase, Paint, Rect,
    RulerOrientation, Text,
};

use crate::modifiers::{Modifier, held};
use crate::ui::Tool;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// World position drawn at the canvas's top-left pixel. Screen → world is
    /// `(screen - translate) / scale`.
    pub translate: Vec2,
    pub scale: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            translate: Vec2::ZERO,
            scale: 1.0,
        }
    }
}

struct Target {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    egui_id: egui::TextureId,
    size_px: [u32; 2],
}

/// In-flight pointer drag.
enum Drag {
    /// Dragging out the bounds of a new shape at doc root.
    Creating {
        kind: CreateKind,
        start: Vec2,
        current: Vec2,
    },
    /// Translating a selected node live; doc is mutated each tick.
    Moving,
    /// Resizing a selected shape via one of its handles; doc is mutated each
    /// tick.
    Resizing {
        handle: Handle,
        start_world: Vec2,
        start_bounds: ShapeBounds,
    },
}

#[derive(Copy, Clone)]
enum CreateKind {
    Rect,
    Ellipse,
    Frame,
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Handle {
    Nw,
    N,
    Ne,
    E,
    Se,
    S,
    Sw,
    W,
}

#[derive(Copy, Clone)]
struct ShapeBounds {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

pub struct Canvas {
    renderer: Renderer,
    scene: Scene,
    target: Option<Target>,
    camera: Camera,
    /// Pixel size requested by the last `ui()` call — what the next `render()`
    /// should produce.
    pending_size_px: [u32; 2],
    /// egui's pixels-per-point captured on the last `ui()` call. Camera
    /// coordinates and pointer input live in *logical* pixels; the render
    /// transform scales by this at the end to end up in texture (physical)
    /// pixel space.
    pending_ppp: f64,
    /// Whether Space is currently held. Tracked across frames because
    /// drag-start doesn't re-read modifier state.
    space_held: bool,
    drag: Option<Drag>,
}

impl Canvas {
    pub fn new(device: &wgpu::Device) -> Result<Self> {
        let renderer = Renderer::new(device, RendererOptions::default())
            .map_err(|e| anyhow::anyhow!("vello renderer init: {e:?}"))?;
        Ok(Self {
            renderer,
            scene: Scene::new(),
            target: None,
            camera: Camera::default(),
            pending_size_px: [0, 0],
            pending_ppp: 1.0,
            space_held: false,
            drag: None,
        })
    }

    /// Render the scene to the offscreen texture using the latest camera +
    /// size. Called before `egui_ctx.run`. If `doc` is `Some`, its node tree
    /// is composed on top of the background grid. The currently-selected
    /// node (if any) is outlined and decorated with resize handles.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        egui_renderer: &mut egui_wgpu::Renderer,
        doc: Option<&Document>,
        selection: Option<&[usize]>,
    ) -> Result<()> {
        let [w, h] = self.pending_size_px;
        if w == 0 || h == 0 {
            return Ok(());
        }

        let need_recreate = self
            .target
            .as_ref()
            .is_none_or(|t| t.size_px != self.pending_size_px);
        if need_recreate {
            if let Some(old) = self.target.take() {
                egui_renderer.free_texture(&old.egui_id);
            }
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vello canvas target"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let egui_id =
                egui_renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear);
            self.target = Some(Target {
                _texture: texture,
                view,
                egui_id,
                size_px: self.pending_size_px,
            });
        }

        self.build_scene(doc, selection);

        let target = self.target.as_ref().expect("target present");
        self.renderer
            .render_to_texture(
                device,
                queue,
                &self.scene,
                &target.view,
                &RenderParams {
                    base_color: srgb(0x18, 0x18, 0x1b, 0xff),
                    width: w,
                    height: h,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| anyhow::anyhow!("vello render: {e:?}"))?;
        Ok(())
    }

    /// Draw the canvas into the current egui `Ui` and process input. Returns
    /// `true` if the document was mutated this frame.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        doc: Option<&mut Document>,
        selection: &mut Option<Vec<usize>>,
        tool: Tool,
    ) -> bool {
        let rect = ui.available_rect_before_wrap();
        let ppp = ui.ctx().pixels_per_point();
        self.pending_ppp = ppp as f64;
        self.pending_size_px = [
            (rect.width() * ppp).round().max(1.0) as u32,
            (rect.height() * ppp).round().max(1.0) as u32,
        ];

        let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());

        ui.input(|i| {
            self.space_held = i.key_down(egui::Key::Space);
        });

        let (scroll_delta, modifiers, pointer) =
            ui.input(|i| (i.smooth_scroll_delta, i.modifiers, i.pointer.hover_pos()));

        let pointer_world =
            pointer.map(|p| self.screen_to_world(Vec2::new(p.x as f64, p.y as f64), rect));

        let pan_requested = self.space_held || tool == Tool::Hand;
        let panning = (pan_requested && response.dragged_by(egui::PointerButton::Primary))
            || response.dragged_by(egui::PointerButton::Middle);

        let mut mutated = false;

        if panning {
            let d = response.drag_delta();
            self.camera.translate += Vec2::new(d.x as f64, d.y as f64);
        } else {
            mutated |= self.handle_tool_interaction(&response, pointer_world, tool, doc, selection);
        }

        if held(&modifiers, Modifier::Command)
            && scroll_delta.y.abs() > 0.1
            && let Some(p_screen) = pointer
        {
            let p_canvas = p_screen - rect.min;
            let canvas_pt = Vec2::new(p_canvas.x as f64, p_canvas.y as f64);
            let world_before = (canvas_pt - self.camera.translate) / self.camera.scale;
            let factor = (scroll_delta.y as f64 * 0.01).exp();
            self.camera.scale = (self.camera.scale * factor).clamp(0.05, 50.0);
            let world_after = (canvas_pt - self.camera.translate) / self.camera.scale;
            self.camera.translate += (world_before - world_after) * self.camera.scale;
        }

        if let Some(target) = self.target.as_ref() {
            let size = egui::vec2(
                target.size_px[0] as f32 / ppp,
                target.size_px[1] as f32 / ppp,
            );
            egui::Image::new(egui::load::SizedTexture::new(target.egui_id, size))
                .paint_at(ui, rect);
        }

        mutated
    }

    fn screen_to_world(&self, screen: Vec2, rect: egui::Rect) -> Vec2 {
        let canvas = screen - Vec2::new(rect.min.x as f64, rect.min.y as f64);
        (canvas - self.camera.translate) / self.camera.scale
    }

    fn handle_tool_interaction(
        &mut self,
        response: &egui::Response,
        pointer_world: Option<Vec2>,
        tool: Tool,
        mut doc: Option<&mut Document>,
        selection: &mut Option<Vec<usize>>,
    ) -> bool {
        let mut mutated = false;

        // Begin a drag.
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(world) = pointer_world
        {
            self.drag = self.start_drag(world, tool, doc.as_deref(), selection);
        }

        // Track / apply drag progress.
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(world) = pointer_world
        {
            match self.drag.as_mut() {
                Some(Drag::Creating { current, .. }) => {
                    *current = world;
                }
                Some(Drag::Moving) => {
                    if let (Some(doc), Some(path)) = (doc.as_deref_mut(), selection.as_deref()) {
                        let delta = Vec2::new(
                            response.drag_delta().x as f64 / self.camera.scale,
                            response.drag_delta().y as f64 / self.camera.scale,
                        );
                        if translate_node_at(doc, path, delta) {
                            mutated = true;
                        }
                    }
                }
                Some(Drag::Resizing {
                    handle,
                    start_world,
                    start_bounds,
                }) => {
                    let delta = world - *start_world;
                    let new_bounds = resize_bounds(*start_bounds, *handle, delta);
                    if let (Some(doc), Some(path)) = (doc.as_deref_mut(), selection.as_deref())
                        && set_bounds(doc, path, new_bounds)
                    {
                        mutated = true;
                    }
                }
                None => {}
            }
        }

        // End the drag.
        if response.drag_stopped()
            && let Some(drag) = self.drag.take()
            && let Drag::Creating {
                kind,
                start,
                current,
            } = drag
            && let Some(doc) = doc.as_deref_mut()
        {
            let bounds = normalize_rect(start, current);
            if bounds.w > 0.5 && bounds.h > 0.5 {
                let node = create_shape(kind, bounds);
                doc.children.push(node);
                *selection = Some(vec![doc.children.len() - 1]);
                mutated = true;
            }
        }

        // Plain click (no drag) handlers — selection hit-test for Select,
        // and single-click placement for the Text tool.
        if response.clicked_by(egui::PointerButton::Primary)
            && self.drag.is_none()
            && let Some(world) = pointer_world
        {
            match tool {
                Tool::Select => {
                    if let Some(doc) = doc.as_deref() {
                        *selection = hit_test(doc, world);
                    }
                }
                Tool::Text => {
                    if let Some(doc) = doc.as_deref_mut() {
                        let text = Text {
                            base: NodeBase {
                                fill: Some(Fill {
                                    paint: Paint::Solid(DocColor {
                                        r: 0.92,
                                        g: 0.92,
                                        b: 0.95,
                                        a: 1.0,
                                    }),
                                    opacity: None,
                                }),
                                ..Default::default()
                            },
                            x: world.x,
                            y: world.y,
                            content: "Text".into(),
                            font_family: None,
                            font_size: Some(24.0),
                        };
                        doc.children.push(Node::Text(text));
                        *selection = Some(vec![doc.children.len() - 1]);
                        mutated = true;
                    }
                }
                _ => {}
            }
        }

        mutated
    }

    /// Decide what kind of drag (if any) starts at `world`.
    fn start_drag(
        &self,
        world: Vec2,
        tool: Tool,
        doc: Option<&Document>,
        selection: &mut Option<Vec<usize>>,
    ) -> Option<Drag> {
        match tool {
            Tool::Rect => Some(Drag::Creating {
                kind: CreateKind::Rect,
                start: world,
                current: world,
            }),
            Tool::Ellipse => Some(Drag::Creating {
                kind: CreateKind::Ellipse,
                start: world,
                current: world,
            }),
            Tool::Frame => Some(Drag::Creating {
                kind: CreateKind::Frame,
                start: world,
                current: world,
            }),
            Tool::Select => {
                let doc = doc?;
                // If a shape is already selected and the pointer is on one of
                // its handles, start a resize.
                if let Some(path) = selection.as_deref()
                    && let Some(bounds) = world_bounds_of(doc, path)
                    && let Some(handle) = hit_handle(bounds, world, self.camera.scale)
                {
                    return Some(Drag::Resizing {
                        handle,
                        start_world: world,
                        start_bounds: bounds,
                    });
                }
                // Otherwise: hit-test to pick a shape, start moving it.
                let hit = hit_test(doc, world);
                *selection = hit.clone();
                hit.map(|_| Drag::Moving)
            }
            _ => None,
        }
    }

    #[allow(dead_code)] // Wired up in a follow-up alongside menu actions.
    pub fn zoom_to_fit(&mut self) {
        self.camera = Camera::default();
    }

    #[allow(dead_code)] // Wired up in a follow-up alongside menu actions.
    pub fn zoom_actual(&mut self) {
        self.camera.scale = 1.0;
    }

    /// Build the frame's scene: the background grid + crosshair, then the
    /// document tree on top when a doc is loaded, plus any drag preview and
    /// selection decorations.
    fn build_scene(&mut self, doc: Option<&Document>, selection: Option<&[usize]>) {
        self.scene.reset();
        // Camera is in logical pixels; the final scale(ppp) converts to the
        // texture's physical pixel space.
        let xform = Affine::scale(self.pending_ppp)
            * Affine::translate(self.camera.translate)
            * Affine::scale(self.camera.scale);

        let grid = Stroke::new(1.0 / self.camera.scale.max(0.001));
        let grid_brush = Brush::Solid(srgb(0x28, 0x28, 0x2e, 0xff));
        for i in -20..=20 {
            let x = (i * 50) as f64;
            let line = Line::new(Point::new(x, -1000.0), Point::new(x, 1000.0));
            self.scene.stroke(&grid, xform, &grid_brush, None, &line);
            let y = (i * 50) as f64;
            let line = Line::new(Point::new(-1000.0, y), Point::new(1000.0, y));
            self.scene.stroke(&grid, xform, &grid_brush, None, &line);
        }

        let axis = Stroke::new(1.5 / self.camera.scale.max(0.001));
        self.scene.stroke(
            &axis,
            xform,
            &Brush::Solid(srgb(0x44, 0x44, 0x4a, 0xff)),
            None,
            &Line::new(Point::new(-1000.0, 0.0), Point::new(1000.0, 0.0)),
        );
        self.scene.stroke(
            &axis,
            xform,
            &Brush::Solid(srgb(0x44, 0x44, 0x4a, 0xff)),
            None,
            &Line::new(Point::new(0.0, -1000.0), Point::new(0.0, 1000.0)),
        );

        if let Some(doc) = doc {
            youeye_render::build(&mut self.scene, doc, xform);

            if let Some(path) = selection
                && let Some(b) = world_bounds_of(doc, path)
            {
                self.draw_selection_decor(b, xform);
            }
        }

        if let Some(Drag::Creating {
            kind,
            start,
            current,
        }) = &self.drag
        {
            self.draw_create_preview(*kind, normalize_rect(*start, *current), xform);
        }
    }

    fn draw_selection_decor(&mut self, b: ShapeBounds, xform: Affine) {
        let scale = self.camera.scale.max(0.001);
        let outline_stroke = Stroke::new(1.5 / scale);
        let brush = Brush::Solid(srgb(0x57, 0x9f, 0xff, 0xff));
        let outline = KRect::new(b.x, b.y, b.x + b.w, b.y + b.h);
        self.scene
            .stroke(&outline_stroke, xform, &brush, None, &outline);

        let handle_size = 8.0 / scale;
        for (cx, cy) in handle_centers(b) {
            let h = KRect::new(
                cx - handle_size / 2.0,
                cy - handle_size / 2.0,
                cx + handle_size / 2.0,
                cy + handle_size / 2.0,
            );
            self.scene
                .fill(vello::peniko::Fill::NonZero, xform, &brush, None, &h);
        }
    }

    fn draw_create_preview(&mut self, kind: CreateKind, b: ShapeBounds, xform: Affine) {
        let scale = self.camera.scale.max(0.001);
        let mut stroke = Stroke::new(1.5 / scale);
        stroke.dash_pattern.push(6.0 / scale);
        stroke.dash_pattern.push(4.0 / scale);
        let brush = Brush::Solid(srgb(0x57, 0x9f, 0xff, 0xff));
        match kind {
            CreateKind::Rect | CreateKind::Frame => {
                let shape = KRect::new(b.x, b.y, b.x + b.w, b.y + b.h);
                self.scene.stroke(&stroke, xform, &brush, None, &shape);
            }
            CreateKind::Ellipse => {
                let shape = KEllipse::new(
                    (b.x + b.w / 2.0, b.y + b.h / 2.0),
                    (b.w / 2.0, b.h / 2.0),
                    0.0,
                );
                self.scene.stroke(&stroke, xform, &brush, None, &shape);
            }
        }
    }
}

// ---- shape helpers ----

fn create_shape(kind: CreateKind, b: ShapeBounds) -> Node {
    match kind {
        CreateKind::Rect => Node::Rect(Rect {
            base: NodeBase {
                fill: Some(default_fill(0.33, 0.53, 0.98)),
                ..Default::default()
            },
            x: b.x,
            y: b.y,
            width: b.w,
            height: b.h,
            rx: 0.0,
            ry: 0.0,
        }),
        CreateKind::Ellipse => Node::Ellipse(Ellipse {
            base: NodeBase {
                fill: Some(default_fill(1.0, 0.55, 0.2)),
                ..Default::default()
            },
            cx: b.x + b.w / 2.0,
            cy: b.y + b.h / 2.0,
            rx: b.w / 2.0,
            ry: b.h / 2.0,
        }),
        CreateKind::Frame => Node::Frame(Frame {
            x: b.x,
            y: b.y,
            width: b.w,
            height: b.h,
            ..Default::default()
        }),
    }
}

fn default_fill(r: f32, g: f32, b: f32) -> Fill {
    Fill {
        paint: Paint::Solid(DocColor { r, g, b, a: 1.0 }),
        opacity: None,
    }
}

fn normalize_rect(a: Vec2, b: Vec2) -> ShapeBounds {
    ShapeBounds {
        x: a.x.min(b.x),
        y: a.y.min(b.y),
        w: (a.x - b.x).abs(),
        h: (a.y - b.y).abs(),
    }
}

// ---- hit testing ----

fn hit_test(doc: &Document, world: Vec2) -> Option<Vec<usize>> {
    let mut result = None;
    hit_test_impl(&doc.children, world, &mut Vec::new(), &mut result);
    result
}

fn hit_test_impl(
    children: &[Node],
    local: Vec2,
    path: &mut Vec<usize>,
    result: &mut Option<Vec<usize>>,
) {
    for (i, child) in children.iter().enumerate() {
        path.push(i);
        match child {
            Node::Rect(r) => {
                if inside_rect(local, r.x, r.y, r.width, r.height) {
                    *result = Some(path.clone());
                }
            }
            Node::Ellipse(e) => {
                if e.rx > 0.0 && e.ry > 0.0 {
                    let dx = (local.x - e.cx) / e.rx;
                    let dy = (local.y - e.cy) / e.ry;
                    if dx * dx + dy * dy <= 1.0 {
                        *result = Some(path.clone());
                    }
                }
            }
            Node::Path(p) => {
                let b = p.data.bounding_box();
                if inside_rect(local, b.x0, b.y0, b.width(), b.height()) {
                    *result = Some(path.clone());
                }
            }
            Node::Text(t) => {
                let (bx, by, bw, bh) = text_bbox(t);
                if inside_rect(local, bx, by, bw, bh) {
                    *result = Some(path.clone());
                }
            }
            Node::Frame(f) => {
                if inside_rect(local, f.x, f.y, f.width, f.height) {
                    *result = Some(path.clone());
                }
                let frame_local = local - Vec2::new(f.x, f.y);
                hit_test_impl(&f.children, frame_local, path, result);
            }
            Node::Group(g) => {
                hit_test_impl(&g.children, local, path, result);
            }
            _ => {}
        }
        path.pop();
    }
}

fn inside_rect(p: Vec2, x: f64, y: f64, w: f64, h: f64) -> bool {
    p.x >= x && p.x <= x + w && p.y >= y && p.y <= y + h
}

// ---- world bounds of a selected node ----

fn world_bounds_of(doc: &Document, path: &[usize]) -> Option<ShapeBounds> {
    let mut origin = Vec2::ZERO;
    let mut children = &doc.children;
    for (i, idx) in path.iter().enumerate() {
        let child = children.get(*idx)?;
        if i == path.len() - 1 {
            return Some(local_bounds(child, origin));
        }
        match child {
            Node::Frame(f) => {
                origin += Vec2::new(f.x, f.y);
                children = &f.children;
            }
            Node::Group(g) => {
                children = &g.children;
            }
            _ => return None,
        }
    }
    None
}

fn local_bounds(node: &Node, origin: Vec2) -> ShapeBounds {
    match node {
        Node::Rect(r) => ShapeBounds {
            x: origin.x + r.x,
            y: origin.y + r.y,
            w: r.width,
            h: r.height,
        },
        Node::Ellipse(e) => ShapeBounds {
            x: origin.x + e.cx - e.rx,
            y: origin.y + e.cy - e.ry,
            w: e.rx * 2.0,
            h: e.ry * 2.0,
        },
        Node::Frame(f) => ShapeBounds {
            x: origin.x + f.x,
            y: origin.y + f.y,
            w: f.width,
            h: f.height,
        },
        Node::Path(p) => {
            let b = p.data.bounding_box();
            ShapeBounds {
                x: origin.x + b.x0,
                y: origin.y + b.y0,
                w: b.width(),
                h: b.height(),
            }
        }
        Node::Text(t) => {
            let (bx, by, bw, bh) = text_bbox(t);
            ShapeBounds {
                x: origin.x + bx,
                y: origin.y + by,
                w: bw,
                h: bh,
            }
        }
        _ => ShapeBounds {
            x: origin.x,
            y: origin.y,
            w: 0.0,
            h: 0.0,
        },
    }
}

/// Rough bounding box for a Text node in its parent's coord space. Width
/// uses the crude `0.55 * font_size * char_count` heuristic and `(x, y)`
/// in SVG is the *baseline* of the first glyph, so the top of the bbox
/// sits above that by the ascent (approx. 0.85 * font_size).
fn text_bbox(t: &youeye_doc::Text) -> (f64, f64, f64, f64) {
    let size = t.font_size.unwrap_or(16.0);
    let chars = t.content.chars().count().max(1) as f64;
    let w = (0.55 * size * chars).max(size * 0.5);
    let h = size * 1.2;
    let top = t.y - size * 0.85;
    (t.x, top, w, h)
}

// ---- translate + resize ----

fn translate_node_at(doc: &mut Document, path: &[usize], delta: Vec2) -> bool {
    let Some(node) = doc.node_at_mut(path) else {
        return false;
    };
    translate_node(node, delta);
    true
}

fn translate_node(node: &mut Node, d: Vec2) {
    match node {
        Node::Rect(r) => {
            r.x += d.x;
            r.y += d.y;
        }
        Node::Ellipse(e) => {
            e.cx += d.x;
            e.cy += d.y;
        }
        Node::Frame(f) => {
            f.x += d.x;
            f.y += d.y;
        }
        Node::Path(p) => {
            let mut buf = BezPath::new();
            std::mem::swap(&mut buf, &mut p.data);
            buf.apply_affine(Affine::translate(d));
            p.data = buf;
        }
        Node::Ruler(r) => match r.orientation {
            RulerOrientation::Horizontal => r.position += d.y,
            RulerOrientation::Vertical => r.position += d.x,
        },
        Node::Use(u) => {
            u.x += d.x;
            u.y += d.y;
        }
        Node::Text(t) => {
            t.x += d.x;
            t.y += d.y;
        }
        Node::Group(_) | Node::Component(_) => {}
    }
}

fn set_bounds(doc: &mut Document, path: &[usize], b: ShapeBounds) -> bool {
    // Translate the world-space bounds back into the node's local space by
    // walking the path again to accumulate the parent origin.
    let mut origin = Vec2::ZERO;
    {
        let mut children: &[Node] = &doc.children;
        for idx in &path[..path.len().saturating_sub(1)] {
            let Some(child) = children.get(*idx) else {
                return false;
            };
            match child {
                Node::Frame(f) => {
                    origin += Vec2::new(f.x, f.y);
                    children = &f.children;
                }
                Node::Group(g) => {
                    children = &g.children;
                }
                _ => return false,
            }
        }
    }
    let local_x = b.x - origin.x;
    let local_y = b.y - origin.y;
    let Some(node) = doc.node_at_mut(path) else {
        return false;
    };
    match node {
        Node::Rect(r) => {
            r.x = local_x;
            r.y = local_y;
            r.width = b.w;
            r.height = b.h;
        }
        Node::Ellipse(e) => {
            e.rx = (b.w / 2.0).max(0.0);
            e.ry = (b.h / 2.0).max(0.0);
            e.cx = local_x + e.rx;
            e.cy = local_y + e.ry;
        }
        Node::Frame(f) => {
            f.x = local_x;
            f.y = local_y;
            f.width = b.w;
            f.height = b.h;
        }
        _ => return false,
    }
    true
}

// ---- resize handles ----

fn handle_centers(b: ShapeBounds) -> [(f64, f64); 8] {
    let (l, t) = (b.x, b.y);
    let (r, bo) = (b.x + b.w, b.y + b.h);
    let (cx, cy) = (b.x + b.w / 2.0, b.y + b.h / 2.0);
    [
        (l, t),
        (cx, t),
        (r, t),
        (r, cy),
        (r, bo),
        (cx, bo),
        (l, bo),
        (l, cy),
    ]
}

fn hit_handle(b: ShapeBounds, world: Vec2, camera_scale: f64) -> Option<Handle> {
    let tolerance = 10.0 / camera_scale.max(0.001);
    let centers = handle_centers(b);
    let handles = [
        Handle::Nw,
        Handle::N,
        Handle::Ne,
        Handle::E,
        Handle::Se,
        Handle::S,
        Handle::Sw,
        Handle::W,
    ];
    for (i, (cx, cy)) in centers.iter().enumerate() {
        if (world.x - cx).abs() <= tolerance && (world.y - cy).abs() <= tolerance {
            return Some(handles[i]);
        }
    }
    None
}

fn resize_bounds(start: ShapeBounds, handle: Handle, delta: Vec2) -> ShapeBounds {
    let mut left = start.x;
    let mut top = start.y;
    let mut right = start.x + start.w;
    let mut bottom = start.y + start.h;
    match handle {
        Handle::Nw => {
            left += delta.x;
            top += delta.y;
        }
        Handle::N => {
            top += delta.y;
        }
        Handle::Ne => {
            right += delta.x;
            top += delta.y;
        }
        Handle::E => {
            right += delta.x;
        }
        Handle::Se => {
            right += delta.x;
            bottom += delta.y;
        }
        Handle::S => {
            bottom += delta.y;
        }
        Handle::Sw => {
            left += delta.x;
            bottom += delta.y;
        }
        Handle::W => {
            left += delta.x;
        }
    }
    // Prevent the shape from flipping through zero — clamp on each axis.
    if right < left {
        std::mem::swap(&mut left, &mut right);
    }
    if bottom < top {
        std::mem::swap(&mut top, &mut bottom);
    }
    ShapeBounds {
        x: left,
        y: top,
        w: right - left,
        h: bottom - top,
    }
}

fn srgb(r: u8, g: u8, b: u8, a: u8) -> Color {
    AlphaColor::<Srgb>::from_rgba8(r, g, b, a)
}
