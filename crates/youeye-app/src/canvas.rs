//! Vello-backed design canvas.
//!
//! Renders into an offscreen `Rgba8Unorm` texture which egui displays via
//! [`egui_wgpu::Renderer::register_native_texture`]. Rendering runs *before*
//! `egui_ctx.run`, using the camera + rect recorded by the previous frame.
//! That one-frame lag is invisible at 60fps and keeps the flow clean: no
//! re-entrancy between vello and egui.

use anyhow::Result;
use kurbo::{Affine, Circle, Line, Point, Rect, Stroke, Vec2};
use vello::peniko::color::{AlphaColor, Srgb};
use vello::peniko::{Brush, Color, Fill};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

use crate::modifiers::{Modifier, held};

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

pub struct Canvas {
    renderer: Renderer,
    scene: Scene,
    target: Option<Target>,
    camera: Camera,
    /// Pixel size requested by the last `ui()` call — what the next `render()`
    /// should produce.
    pending_size_px: [u32; 2],
    /// Whether Space is currently held. Tracked across frames because
    /// drag-start doesn't re-read modifier state.
    space_held: bool,
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
            space_held: false,
        })
    }

    /// Render the scene to the offscreen texture using the latest camera +
    /// size. Called before `egui_ctx.run`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        egui_renderer: &mut egui_wgpu::Renderer,
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

        self.build_scene();

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

    /// Draw the canvas into the current egui `Ui` and process input.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let ppp = ui.ctx().pixels_per_point();
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

        let panning = (self.space_held && response.dragged())
            || response.dragged_by(egui::PointerButton::Middle);
        if panning {
            let d = response.drag_delta();
            self.camera.translate += Vec2::new(d.x as f64, d.y as f64);
        }

        if held(&modifiers, Modifier::Command) && scroll_delta.y.abs() > 0.1 {
            if let Some(p_screen) = pointer {
                let p_canvas = p_screen - rect.min;
                let canvas_pt = Vec2::new(p_canvas.x as f64, p_canvas.y as f64);
                let world_before = (canvas_pt - self.camera.translate) / self.camera.scale;
                let factor = (scroll_delta.y as f64 * 0.01).exp();
                self.camera.scale = (self.camera.scale * factor).clamp(0.05, 50.0);
                let world_after = (canvas_pt - self.camera.translate) / self.camera.scale;
                self.camera.translate += (world_before - world_after) * self.camera.scale;
            }
        }

        if let Some(target) = self.target.as_ref() {
            let size = egui::vec2(
                target.size_px[0] as f32 / ppp,
                target.size_px[1] as f32 / ppp,
            );
            egui::Image::new(egui::load::SizedTexture::new(target.egui_id, size))
                .paint_at(ui, rect);
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

    /// Placeholder content so pan/zoom is visibly working. Real scene building
    /// lands in phase 2 when the document model is in place.
    fn build_scene(&mut self) {
        self.scene.reset();
        let xform = Affine::translate(self.camera.translate) * Affine::scale(self.camera.scale);

        // Grid so zoom is visible.
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

        // Origin crosshair.
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

        // Test content.
        self.scene.fill(
            Fill::NonZero,
            xform,
            &Brush::Solid(srgb(0x00, 0x52, 0xcc, 0xff)),
            None,
            &Rect::new(0.0, 0.0, 200.0, 120.0),
        );
        self.scene.fill(
            Fill::NonZero,
            xform,
            &Brush::Solid(srgb(0xff, 0x57, 0x22, 0xff)),
            None,
            &Circle::new(Point::new(320.0, 220.0), 70.0),
        );
    }
}

fn srgb(r: u8, g: u8, b: u8, a: u8) -> Color {
    AlphaColor::<Srgb>::from_rgba8(r, g, b, a)
}
