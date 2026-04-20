//! Application shell: winit event loop, wgpu surface, egui integration, menus.

use std::sync::Arc;

use egui::ViewportId;
use egui_wgpu::ScreenDescriptor;
use tracing::{debug, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use youeye_doc::Document;

use crate::canvas::Canvas;
use crate::menu::{self, MenuAction, MenuBar};

/// Custom events pushed into the winit loop from sources outside winit itself.
///
/// Right now only muda (menu clicks on macOS / Windows) uses this channel; on
/// Linux the variant is declared but never constructed because muda isn't
/// compiled in.
#[derive(Debug, Clone)]
pub enum UserEvent {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    MenuEvent(muda::MenuEvent),
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    menu: Box<dyn MenuBar>,
    state: Option<AppState>,
    ui: crate::ui::UiState,
    doc_state: Option<DocumentState>,
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy,
            menu: menu::create(),
            state: None,
            ui: crate::ui::UiState::default(),
            doc_state: None,
        }
    }
}

/// An open document plus editor-session metadata.
pub struct DocumentState {
    pub doc: Document,
    pub path: Option<std::path::PathBuf>,
    pub dirty: bool,
}

impl DocumentState {
    pub fn new(doc: Document, path: Option<std::path::PathBuf>) -> Self {
        Self {
            doc,
            path,
            dirty: false,
        }
    }
}

/// All state that depends on having a live window + GPU context.
struct AppState {
    window: Arc<Window>,
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    _adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_config: wgpu::SurfaceConfiguration,

    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    canvas: Canvas,
}

impl AppState {
    fn new(event_loop: &ActiveEventLoop) -> anyhow::Result<Self> {
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title("youeye")
                    .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0)),
            )?,
        );

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))?;

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("youeye device"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            }))?;

        let max_dim = device.limits().max_texture_dimension_2d;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.clamp(1, max_dim),
            height: size.height.clamp(1, max_dim),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let egui_ctx = egui::Context::default();
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            ViewportId::ROOT,
            &*window,
            Some(window.scale_factor() as f32),
            None,
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let egui_renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

        let canvas = Canvas::new(&device)?;

        Ok(Self {
            window,
            _instance: instance,
            surface,
            _adapter: adapter,
            device,
            queue,
            surface_config,
            egui_ctx,
            egui_state,
            egui_renderer,
            canvas,
        })
    }

    fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        let max_dim = self.device.limits().max_texture_dimension_2d;
        self.surface_config.width = new_size.width.min(max_dim);
        self.surface_config.height = new_size.height.min(max_dim);
        self.surface.configure(&self.device, &self.surface_config);
    }

    fn render(
        &mut self,
        ui: &mut crate::ui::UiState,
        menu: &mut dyn MenuBar,
        pending_actions: &mut Vec<MenuAction>,
        doc_state: Option<&mut DocumentState>,
    ) -> anyhow::Result<()> {
        // Canvas wants an immutable view of the doc; the inspector may then
        // mutate it. Take the immutable borrow in a narrow scope so the
        // mutable one can move into the egui closure afterwards.
        {
            let canvas_doc: Option<&Document> = doc_state.as_deref().map(|s| &s.doc);
            if let Err(e) = self.canvas.render(
                &self.device,
                &self.queue,
                &mut self.egui_renderer,
                canvas_doc,
            ) {
                warn!("canvas render: {e:?}");
            }
        }

        let raw_input = self.egui_state.take_egui_input(&*self.window);
        let canvas = &mut self.canvas;
        let mut doc_state = doc_state;
        let output = self.egui_ctx.clone().run(raw_input, |ctx| {
            menu.draw_egui(ctx, pending_actions);
            ui.draw(ctx, pending_actions, canvas, doc_state.as_deref_mut());
        });
        self.egui_state
            .handle_platform_output(&*self.window, output.platform_output);

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            Err(e) => {
                warn!("surface acquire: {e:?}");
                return Ok(());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let pixels_per_point = self.window.scale_factor() as f32;
        let paint_jobs = self.egui_ctx.tessellate(output.shapes, pixels_per_point);
        let screen = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point,
        };

        for (id, image_delta) in &output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("youeye encoder"),
            });
        let cmds = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );

        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.08,
                                g: 0.08,
                                b: 0.09,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.egui_renderer.render(&mut rpass, &paint_jobs, &screen);
        }

        self.queue
            .submit(cmds.into_iter().chain([encoder.finish()]));
        self.window.pre_present_notify();
        frame.present();

        for id in &output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        Ok(())
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match AppState::new(event_loop) {
            Ok(state) => {
                self.menu.attach(&state.window, &self.proxy);
                info!(
                    "window ready at {}x{}",
                    state.surface_config.width, state.surface_config.height
                );
                self.state = Some(state);
            }
            Err(e) => {
                tracing::error!("failed to initialise app state: {e:?}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        let response = state.egui_state.on_window_event(&*state.window, &event);
        if response.repaint {
            state.window.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => {
                info!("close requested");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                state.resize(size);
                state.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let mut actions = Vec::new();
                if let Err(e) = state.render(
                    &mut self.ui,
                    &mut *self.menu,
                    &mut actions,
                    self.doc_state.as_mut(),
                ) {
                    warn!("render error: {e:?}");
                }
                self.drain_actions(&actions, event_loop);
            }
            _ => {}
        }
    }

    #[cfg_attr(
        not(any(target_os = "macos", target_os = "windows")),
        allow(unused_variables, unreachable_code)
    )]
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        if self.state.is_none() {
            return;
        }
        #[cfg_attr(
            not(any(target_os = "macos", target_os = "windows")),
            allow(unused_mut)
        )]
        let mut actions = Vec::new();
        match event {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            UserEvent::MenuEvent(ev) => {
                self.menu.handle_native_event(&ev, &mut actions);
            }
        }
        self.drain_actions(&actions, event_loop);
        if let Some(state) = self.state.as_mut() {
            state.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // With ControlFlow::Wait we only redraw on demand; nothing to do here.
    }
}

impl App {
    fn drain_actions(&mut self, actions: &[MenuAction], event_loop: &ActiveEventLoop) {
        for action in actions {
            debug!(?action, "menu action");
            match action {
                MenuAction::Quit => event_loop.exit(),
                MenuAction::OpenProject => self.open_file_dialog(),
                MenuAction::Save => self.save(),
                MenuAction::SaveAs => self.save_as_dialog(),
                MenuAction::NewProject => {
                    self.doc_state = Some(DocumentState::new(Document::default(), None));
                    self.request_redraw();
                }
                _ => {}
            }
        }
    }

    fn request_redraw(&self) {
        if let Some(state) = self.state.as_ref() {
            state.window.request_redraw();
        }
    }

    fn open_file_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match youeye_io::from_svg(&text) {
                Ok(doc) => {
                    info!(?path, "opened document");
                    self.doc_state = Some(DocumentState::new(doc, Some(path)));
                    self.request_redraw();
                }
                Err(e) => warn!("parse {path:?}: {e:?}"),
            },
            Err(e) => warn!("read {path:?}: {e:?}"),
        }
    }

    fn save(&mut self) {
        let Some(ds) = self.doc_state.as_mut() else {
            return;
        };
        let Some(path) = ds.path.clone() else {
            return self.save_as_dialog();
        };
        let text = youeye_io::to_svg(&ds.doc);
        match std::fs::write(&path, text) {
            Ok(()) => {
                ds.dirty = false;
                info!(?path, "saved document");
            }
            Err(e) => warn!("write {path:?}: {e:?}"),
        }
    }

    fn save_as_dialog(&mut self) {
        let Some(ds) = self.doc_state.as_mut() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .save_file()
        else {
            return;
        };
        let text = youeye_io::to_svg(&ds.doc);
        match std::fs::write(&path, text) {
            Ok(()) => {
                ds.path = Some(path.clone());
                ds.dirty = false;
                info!(?path, "saved document");
            }
            Err(e) => warn!("write {path:?}: {e:?}"),
        }
    }
}
