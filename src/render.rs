use egui::Context;
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::ExperimentalFeatures;
use egui_wgpu::wgpu::{CommandEncoder, Device, Queue, StoreOp, TextureFormat, TextureView};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State;
use wgpu::TextureUsages;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::state::OptimizationPolicy;
use crate::utils;

pub struct RenderState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub scale_factor: f32,
    pub egui_renderer: EguiRenderer,
}

impl RenderState {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        window: &Window,
        width: u32,
        height: u32,
        optimization_policy: OptimizationPolicy,
        present_mode: wgpu::PresentMode,
    ) -> Self {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("failed to find an appropriate adapter");

        let info = adapter.get_info();
        println!("using gpu device: {}", info.name);
        println!("using render backend: {}", info.backend);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::default(),
                required_limits: wgpu::Limits::default(),
                memory_hints: match optimization_policy {
                    OptimizationPolicy::Performance => wgpu::MemoryHints::Performance,
                    OptimizationPolicy::ResourceUsage => wgpu::MemoryHints::MemoryUsage,
                },
                trace: wgpu::Trace::Off,
                experimental_features: ExperimentalFeatures::default(),
            })
            .await
            .expect("failed to create device");

        const TEXTURE_FORMAT: TextureFormat = TextureFormat::Bgra8UnormSrgb;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            format: TEXTURE_FORMAT,
            width,
            height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: if cfg!(not(target_os = "windows")) {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else {
                wgpu::CompositeAlphaMode::Opaque // uhh
            },
            view_formats: vec![TEXTURE_FORMAT],
        };

        surface.configure(&device, &surface_config);

        let egui_renderer = EguiRenderer::new(&device, surface_config.format, None, 1, window);

        let scale_factor = 1.0;

        Self {
            device,
            queue,
            surface,
            surface_config,
            egui_renderer,
            scale_factor,
        }
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub fn set_present_mode(&mut self, present_mode: wgpu::PresentMode) {
        self.surface_config.present_mode = present_mode;
        self.surface.configure(&self.device, &self.surface_config);
    }
}

pub struct EguiRenderer {
    state: State,
    renderer: Renderer,
    frame_started: bool,
}

impl EguiRenderer {
    pub fn context(&self) -> &Context {
        self.state.egui_ctx()
    }

    pub fn new(
        device: &Device,
        output_color_format: TextureFormat,
        output_depth_format: Option<TextureFormat>,
        msaa_samples: u32,
        window: &Window,
    ) -> EguiRenderer {
        let mut egui_context = Context::default();

        utils::ui::setup_fonts(&mut egui_context);

        let egui_state = egui_winit::State::new(
            egui_context,
            egui::viewport::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(2 * 1024), // default dimension is 2048
        );
        let egui_renderer = Renderer::new(
            device,
            output_color_format,
            RendererOptions {
                depth_stencil_format: output_depth_format,
                msaa_samples,
                dithering: true,
                predictable_texture_filtering: false,
            },
        );

        EguiRenderer {
            state: egui_state,
            renderer: egui_renderer,
            frame_started: false,
        }
    }

    pub fn handle_input(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).repaint
    }

    pub fn ppp(&mut self, v: f32) {
        self.context().set_pixels_per_point(v);
    }

    pub fn begin_frame(&mut self, window: &Window) {
        let raw_input = self.state.take_egui_input(window);
        self.state.egui_ctx().begin_pass(raw_input);
        self.frame_started = true;
    }

    pub fn end_frame_and_draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        window: &Window,
        window_surface_view: &TextureView,
        screen_descriptor: ScreenDescriptor,
    ) {
        if !self.frame_started {
            panic!("begin_frame must be called before end_frame_and_draw is called");
        }

        self.ppp(screen_descriptor.pixels_per_point);

        let full_output = self.state.egui_ctx().end_pass();

        self.state
            .handle_platform_output(window, full_output.platform_output);

        let tris = self
            .state
            .egui_ctx()
            .tessellate(full_output.shapes, self.state.egui_ctx().pixels_per_point());
        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }
        self.renderer
            .update_buffers(device, queue, encoder, &tris, &screen_descriptor);
        let rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui main render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: window_surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0_f64,
                        g: 0.0_f64,
                        b: 0.0_f64,
                        a: 0.0_f64,
                    }),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });

        self.renderer
            .render(&mut rpass.forget_lifetime(), &tris, &screen_descriptor);
        for x in &full_output.textures_delta.free {
            self.renderer.free_texture(x)
        }

        self.frame_started = false;
    }
}
