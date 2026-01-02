use egui::{Context, FontData, FontDefinitions, FontFamily};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::ExperimentalFeatures;
use egui_wgpu::wgpu::{CommandEncoder, Device, Queue, StoreOp, TextureFormat, TextureView};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State;
use std::sync::Arc;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::state::OptimizationPolicy;

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
    ) -> Self {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: match optimization_policy {
                    OptimizationPolicy::Performance => wgpu::PowerPreference::HighPerformance,
                    OptimizationPolicy::ResourceUsage => wgpu::PowerPreference::LowPower,
                },
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: match optimization_policy {
                    OptimizationPolicy::Performance => wgpu::MemoryHints::Performance,
                    OptimizationPolicy::ResourceUsage => wgpu::MemoryHints::MemoryUsage,
                },
                trace: wgpu::Trace::Off,
                experimental_features: ExperimentalFeatures::default(),
            })
            .await
            .expect("Failed to create device");

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let selected_format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let swapchain_format = swapchain_capabilities
            .formats
            .iter()
            .find(|d| **d == selected_format)
            .expect("Failed to select proper surface texture format");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: *swapchain_format,
            width,
            height,
            present_mode: crate::state::WGPU_PRESENTMODE_AUTOVSYNC,
            desired_maximum_frame_latency: 0,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
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

        // 配置字体
        Self::setup_fonts(&mut egui_context);

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
                msaa_samples: msaa_samples,
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

    fn setup_fonts(ctx: &mut Context) {
        let mut fonts = FontDefinitions::default();

        // fonts.font_data.insert(
        //     "notosans_cjk_sc".to_owned(),
        //     Arc::new(egui::FontData::from_static(include_bytes!(
        //         "../assets/fonts/NotoSans-CJK-SC/NotoSansCJKsc-Regular.otf"
        //     ))),
        // );

        // fonts
        //     .families
        //     .entry(egui::FontFamily::Proportional)
        //     .or_default()
        //     .insert(0, "notosans_cjk_sc".to_owned());

        let mut font_db = fontdb::Database::new();
        font_db.load_system_fonts();

        let cjk_font_names = [
            "Noto Sans CJK SC",
            "Noto Sans CJK",
            "Microsoft YaHei",
            "微软雅黑",
        ];

        let mut font_loaded = false;

        for font_name in &cjk_font_names {
            if let Some(face_id) = font_db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(font_name)],
                weight: fontdb::Weight::NORMAL,
                stretch: fontdb::Stretch::Normal,
                style: fontdb::Style::Normal,
            }) {
                if let Some(font_data) =
                    font_db.with_face_data(face_id, |data, _| Some(data.to_vec()))
                {
                    if let Some(font_bytes) = font_data {
                        fonts.font_data.insert(
                            "cjk_font".to_owned(),
                            Arc::new(FontData::from_owned(font_bytes)),
                        );

                        fonts
                            .families
                            .get_mut(&FontFamily::Proportional)
                            .unwrap()
                            .insert(0, "cjk_font".to_owned());

                        fonts
                            .families
                            .get_mut(&FontFamily::Monospace)
                            .unwrap()
                            .insert(0, "cjk_font".to_owned());

                        font_loaded = true;

                        break;
                    }
                }
            }
        }

        if !font_loaded {
            panic!("cannot find cjk font")
        }

        ctx.set_fonts(fonts);
    }

    pub fn handle_input(&mut self, window: &Window, event: &WindowEvent) {
        let _ = self.state.on_window_event(window, event);
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
            panic!("begin_frame must be called before end_frame_and_draw can be called");
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
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: window_surface_view,
                resolve_target: None,
                ops: egui_wgpu::wgpu::Operations {
                    load: egui_wgpu::wgpu::LoadOp::Load,
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            label: Some("egui main render pass"),
            occlusion_query_set: None,
        });

        self.renderer
            .render(&mut rpass.forget_lifetime(), &tris, &screen_descriptor);
        for x in &full_output.textures_delta.free {
            self.renderer.free_texture(x)
        }

        self.frame_started = false;
    }
}
