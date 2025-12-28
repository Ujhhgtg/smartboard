use egui::{Context, FontData, FontDefinitions, FontFamily};
use egui_wgpu::wgpu::{CommandEncoder, Device, Queue, StoreOp, TextureFormat, TextureView};
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor, wgpu};
use egui_winit::State;
use winit::event::WindowEvent;
use winit::window::Window;
use std::fs;
use std::sync::Arc;

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
        
        // 配置支持 CJK 的字体
        Self::setup_cjk_fonts(&mut egui_context);

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

    fn setup_cjk_fonts(ctx: &mut Context) {
        let mut fonts = FontDefinitions::default();
        
        // 尝试加载系统 CJK 字体
        let cjk_font_paths = [
            // Windows 常见 CJK 字体路径
            "C:\\Windows\\Fonts\\msyh.ttc",           // 微软雅黑
            "C:\\Windows\\Fonts\\simhei.ttf",         // 黑体
            "C:\\Windows\\Fonts\\simsun.ttc",         // 宋体
            "C:\\Windows\\Fonts\\msyhbd.ttc",         // 微软雅黑 Bold
            // Linux 常见路径
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            // macOS 常见路径
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
        ];

        let mut cjk_font_loaded = false;
        
        for font_path in &cjk_font_paths {
            if let Ok(font_data) = fs::read(font_path) {
                fonts.font_data.insert(
                    "cjk_font".to_owned(),
                    Arc::new(FontData::from_owned(font_data)),
                );
                
                // 将 CJK 字体添加到比例字体族的最前面
                fonts
                    .families
                    .get_mut(&FontFamily::Proportional)
                    .unwrap()
                    .insert(0, "cjk_font".to_owned());
                
                // 也添加到等宽字体族
                fonts
                    .families
                    .get_mut(&FontFamily::Monospace)
                    .unwrap()
                    .insert(0, "cjk_font".to_owned());
                
                cjk_font_loaded = true;
                break;
            }
        }

        // 如果系统字体加载失败，尝试使用 fontdb 查找
        if !cjk_font_loaded {
            let mut font_db = fontdb::Database::new();
            font_db.load_system_fonts();
            
            // 查找支持 CJK 的字体
            let cjk_font_names = [
                "Microsoft YaHei",
                "微软雅黑",
                "SimHei",
                "黑体",
                "SimSun",
                "宋体",
                "PingFang SC",
                "Noto Sans CJK",
                "Source Han Sans",
            ];
            
            for font_name in &cjk_font_names {
                if let Some(face_id) = font_db.query(&fontdb::Query {
                    families: &[fontdb::Family::Name(font_name)],
                    weight: fontdb::Weight::NORMAL,
                    stretch: fontdb::Stretch::Normal,
                    style: fontdb::Style::Normal,
                }) {
                    if let Some(font_data) = font_db.with_face_data(face_id, |data, _| {
                        Some(data.to_vec())
                    }) {
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
                            
                            cjk_font_loaded = true;
                            break;
                        }
                    }
                }
            }
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
            panic!("begin_frame must be called before end_frame_and_draw can be called!");
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
