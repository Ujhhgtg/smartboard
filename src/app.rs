use crate::egui_tools::EguiRenderer;
use egui::{Color32, Pos2, Shape, Stroke};
use egui_wgpu::wgpu::{ExperimentalFeatures, SurfaceError};
use egui_wgpu::{ScreenDescriptor, wgpu};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

pub struct AppState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub scale_factor: f32,
    pub egui_renderer: EguiRenderer,
}

// 动态画笔模式
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DynamicBrushMode {
    Disabled,   // 禁用
    BrushTip,   // 模拟笔锋
    SpeedBased, // 基于速度
}

// 工具类型
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,       // 选择
    Brush,        // 画笔
    ObjectEraser, // 对象橡皮擦
    PixelEraser,  // 像素橡皮擦
    Insert,       // 插入
    Background,   // 背景
}

// 插入的图片数据结构
pub struct InsertedImage {
    pub texture: egui::TextureHandle,
    pub pos: Pos2,
    pub size: egui::Vec2,
    pub aspect_ratio: f32,
}

// 插入的文本数据结构
pub struct InsertedText {
    pub text: String,
    pub pos: Pos2,
    pub color: Color32,
    pub font_size: f32,
}

// 被选择的对象
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectedObject {
    Stroke(usize),
    Image(usize),
    Text(usize),
}

// 绘图数据结构
#[derive(Clone)]
pub struct DrawingStroke {
    pub points: Vec<Pos2>,
    pub widths: Vec<f32>, // 每个点的宽度（用于动态画笔）
    pub color: Color32,
    pub base_width: f32,
}

pub struct DrawingState {
    pub strokes: Vec<DrawingStroke>,
    pub images: Vec<InsertedImage>,
    pub texts: Vec<InsertedText>,
    pub current_stroke: Option<Vec<Pos2>>,
    pub current_stroke_widths: Option<Vec<f32>>, // 当前笔画的宽度
    pub current_stroke_times: Option<Vec<f64>>,  // 每个点的时间戳（用于速度计算）
    pub stroke_start_time: Option<Instant>,      // 笔画开始时间
    pub is_drawing: bool,
    pub brush_color: Color32,
    pub brush_width: f32,
    pub dynamic_brush_mode: DynamicBrushMode,
    pub stroke_smoothing: bool, // 笔画平滑选项
    pub current_tool: Tool,
    pub eraser_size: f32,          // 橡皮擦大小
    pub background_color: Color32, // 背景颜色
    pub selected_object: Option<SelectedObject>,
    pub drag_start_pos: Option<Pos2>,
    pub show_size_preview: bool,
    pub size_preview_pos: Pos2,
    pub size_preview_size: f32,
}

impl AppState {
    async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        window: &Window,
        width: u32,
        height: u32,
    ) -> Self {
        let power_pref = wgpu::PowerPreference::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: power_pref,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("Failed to find an appropriate adapter");

        let features = wgpu::Features::empty();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: None,
                required_features: features,
                required_limits: Default::default(),
                memory_hints: Default::default(),
                trace: Default::default(),
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
            .expect("failed to select proper surface texture format!");

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: *swapchain_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 0,
            alpha_mode: swapchain_capabilities.alpha_modes[0],
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

    fn resize_surface(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }
}

pub struct App {
    instance: wgpu::Instance,
    state: Option<AppState>,
    window: Option<Arc<Window>>,
    drawing_state: DrawingState,
    should_quit: bool,
    show_text_dialog: bool,
    new_text_content: String,
}

impl App {
    // 检查点是否与笔画相交（用于对象橡皮擦）
    fn point_intersects_stroke(pos: Pos2, stroke: &DrawingStroke, eraser_size: f32) -> bool {
        let eraser_radius = eraser_size / 2.0;
        for i in 0..stroke.points.len() - 1 {
            let p1 = stroke.points[i];
            let p2 = stroke.points[i + 1];
            let stroke_width = if i < stroke.widths.len() {
                stroke.widths[i].max(
                    stroke
                        .widths
                        .get(i + 1)
                        .copied()
                        .unwrap_or(stroke.widths[i]),
                )
            } else {
                stroke.widths[0]
            };

            // 计算点到线段的距离
            let dist = Self::point_to_line_segment_distance(pos, p1, p2);
            if dist <= eraser_radius + stroke_width / 2.0 {
                return true;
            }
        }
        false
    }

    // 计算点到线段的最短距离
    fn point_to_line_segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
        let ab = Pos2::new(b.x - a.x, b.y - a.y);
        let ap = Pos2::new(p.x - a.x, p.y - a.y);
        let ab_sq = ab.x * ab.x + ab.y * ab.y;

        if ab_sq < 0.0001 {
            // a 和 b 几乎重合
            return (p.x - a.x).hypot(p.y - a.y);
        }

        let t = ((ap.x * ab.x + ap.y * ab.y) / ab_sq).max(0.0).min(1.0);
        let closest = Pos2::new(a.x + t * ab.x, a.y + t * ab.y);
        (p.x - closest.x).hypot(p.y - closest.y)
    }

    // 计算动态画笔宽度
    fn calculate_dynamic_width(
        base_width: f32,
        mode: DynamicBrushMode,
        point_index: usize,
        total_points: usize,
        speed: Option<f32>,
    ) -> f32 {
        match mode {
            DynamicBrushMode::Disabled => base_width,

            DynamicBrushMode::BrushTip => {
                // 模拟笔锋：在笔画末尾逐渐缩小
                let progress = point_index as f32 / total_points.max(1) as f32;
                // 在最后 30% 的笔画中逐渐缩小到 40% 的宽度
                if progress > 0.7 {
                    let shrink_progress = (progress - 0.7) / 0.3; // 0.0 到 1.0
                    base_width * (1.0 - shrink_progress * 0.6) // 从 100% 缩小到 40%
                } else {
                    base_width
                }
            }

            DynamicBrushMode::SpeedBased => {
                // 基于速度：速度快时变细，速度慢时变粗
                if let Some(speed_val) = speed {
                    // 速度范围假设：0-500 像素/秒
                    // 速度越快，宽度越小（最小到 50%）
                    // 速度越慢，宽度越大（最大到 150%）
                    let normalized_speed = (speed_val / 500.0).min(1.0);
                    base_width * (1.5 - normalized_speed) // 从 150% 到 50%
                } else {
                    base_width
                }
            }
        }
    }

    // 笔画平滑算法 - 使用移动平均和曲线拟合来减少抖动并添加圆角
    fn apply_stroke_smoothing(points: &[Pos2]) -> Vec<Pos2> {
        if points.len() < 2 {
            return points.to_vec();
        }

        // 第一步：应用移动平均滤波器减少抖动
        let mut smoothed_points = Vec::with_capacity(points.len());

        // 窗口大小（调整此值以控制平滑强度）
        let window_size = 3; // 使用3点移动平均

        for i in 0..points.len() {
            let start_idx = i.saturating_sub(window_size / 2);
            let end_idx = (i + window_size / 2).min(points.len() - 1);

            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut count = 0;

            for j in start_idx..=end_idx {
                sum_x += points[j].x;
                sum_y += points[j].y;
                count += 1;
            }

            let avg_x = sum_x / count as f32;
            let avg_y = sum_y / count as f32;
            smoothed_points.push(Pos2::new(avg_x, avg_y));
        }

        // 第二步：添加圆角到起始和结束部分
        // if smoothed_points.len() >= 2 {
        //     // 添加起始圆角
        //     let start_point = smoothed_points[0];
        //     let second_point = smoothed_points[1];
        //     let start_dir = (second_point - start_point).normalized();

        //     // 添加几个点来创建圆角效果
        //     let num_cap_points = 3;
        //     for i in 1..=num_cap_points {
        //         let angle = std::f32::consts::PI / 2.0 * (i as f32 / (num_cap_points + 1) as f32);
        //         let offset_x = start_dir.x * 2.0 * angle.cos() - start_dir.y * 2.0 * angle.sin();
        //         let offset_y = start_dir.y * 2.0 * angle.cos() + start_dir.x * 2.0 * angle.sin();
        //         smoothed_points.insert(0, Pos2::new(start_point.x + offset_x, start_point.y + offset_y));
        //     }

        //     // 添加结束圆角
        //     let end_point = smoothed_points[smoothed_points.len() - 1];
        //     let second_last_point = smoothed_points[smoothed_points.len() - 2];
        //     let end_dir = (end_point - second_last_point).normalized();

        //     for i in 1..=num_cap_points {
        //         let angle = std::f32::consts::PI / 2.0 * (i as f32 / (num_cap_points + 1) as f32);
        //         let offset_x = end_dir.x * 2.0 * angle.cos() + end_dir.y * 2.0 * angle.sin();
        //         let offset_y = end_dir.y * 2.0 * angle.cos() - end_dir.x * 2.0 * angle.sin();
        //         smoothed_points.push(Pos2::new(end_point.x + offset_x, end_point.y + offset_y));
        //     }
        // }

        smoothed_points
    }

    pub fn new() -> Self {
        let instance = egui_wgpu::wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        Self {
            instance,
            state: None,
            window: None,
            drawing_state: DrawingState {
                strokes: Vec::new(),
                images: Vec::new(),
                texts: Vec::new(),
                current_stroke: None,
                current_stroke_widths: None,
                current_stroke_times: None,
                stroke_start_time: None,
                is_drawing: false,
                brush_color: Color32::WHITE,
                brush_width: 5.0,
                dynamic_brush_mode: DynamicBrushMode::Disabled,
                stroke_smoothing: true,
                current_tool: Tool::Brush,
                eraser_size: 10.0,
                background_color: Color32::from_rgb(16, 80, 60),
                selected_object: None,
                drag_start_pos: None,
                show_size_preview: false,
                size_preview_pos: Pos2::new(50.0, 50.0),
                size_preview_size: 5.0,
            },
            should_quit: false,
            show_text_dialog: false,
            new_text_content: String::from(""),
        }
    }

    async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);

        // 设置全屏模式
        let monitor = window.current_monitor();
        window.set_fullscreen(Some(Fullscreen::Borderless(monitor)));

        // 获取全屏后的实际尺寸
        let size = window.inner_size();
        let initial_width = size.width;
        let initial_height = size.height;

        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("Failed to create surface!");

        let state = AppState::new(
            &self.instance,
            surface,
            &window,
            initial_width,
            initial_height,
        )
        .await;

        self.window.get_or_insert(window);
        self.state.get_or_insert(state);
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.state.as_mut().unwrap().resize_surface(width, height);
        }
    }

    fn handle_redraw(&mut self) {
        // Attempt to handle minimizing window
        if let Some(window) = self.window.as_ref() {
            if let Some(min) = window.is_minimized() {
                if min {
                    println!("Window is minimized");
                    return;
                }
            }
        }

        let state = self.state.as_mut().unwrap();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [state.surface_config.width, state.surface_config.height],
            pixels_per_point: self.window.as_ref().unwrap().scale_factor() as f32
                * state.scale_factor,
        };

        let surface_texture = state.surface.get_current_texture();

        match surface_texture {
            Err(SurfaceError::Outdated) => {
                // Ignoring outdated to allow resizing and minimization
                println!("wgpu surface outdated");
                return;
            }
            Err(_) => {
                surface_texture.expect("Failed to acquire next swap chain texture");
                return;
            }
            Ok(_) => {}
        };

        let surface_texture = surface_texture.unwrap();

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let window = self.window.as_ref().unwrap();

        {
            state.egui_renderer.begin_frame(window);
            let ctx = state.egui_renderer.context();

            // 工具栏窗口 - 使用 pivot 锚定在底部中央，使用实际窗口大小
            let content_rect = ctx.available_rect();
            let margin = 20.0; // 底部边距

            egui::Window::new("工具栏")
                .resizable(false)
                .pivot(egui::Align2::CENTER_BOTTOM)
                .default_pos([content_rect.center().x, content_rect.max.y - margin])
                .show(ctx, |ui| {
                    // 工具选择
                    ui.horizontal(|ui| {
                        ui.label("工具:");
                        // TODO: egui doesn't support rendering fonts with colors
                        let old_tool = self.drawing_state.current_tool;
                        if ui
                            .selectable_value(
                                &mut self.drawing_state.current_tool,
                                Tool::Select,
                                "选择",
                            )
                            .changed()
                            || ui
                                .selectable_value(
                                    &mut self.drawing_state.current_tool,
                                    Tool::Brush,
                                    "画笔",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.drawing_state.current_tool,
                                    Tool::ObjectEraser,
                                    "对象橡皮擦",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.drawing_state.current_tool,
                                    Tool::PixelEraser,
                                    "像素橡皮擦",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.drawing_state.current_tool,
                                    Tool::Insert,
                                    "插入",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.drawing_state.current_tool,
                                    Tool::Background,
                                    "🎨 背景",
                                )
                                .changed()
                        {
                            if self.drawing_state.current_tool != old_tool {
                                self.drawing_state.selected_object = None;
                            }
                        }
                    });

                    ui.separator();

                    // 画笔相关设置
                    if self.drawing_state.current_tool == Tool::Brush {
                        ui.horizontal(|ui| {
                            ui.label("颜色:");
                            let old_color = self.drawing_state.brush_color;
                            if ui
                                .color_edit_button_srgba(&mut self.drawing_state.brush_color)
                                .changed()
                            {
                                // 颜色改变时，如果正在绘制，结束当前笔画（使用旧颜色）
                                if self.drawing_state.is_drawing {
                                    if let Some(points) = self.drawing_state.current_stroke.take() {
                                        if let Some(widths) =
                                            self.drawing_state.current_stroke_widths.take()
                                        {
                                            if points.len() > 1 {
                                                self.drawing_state.strokes.push(DrawingStroke {
                                                    points,
                                                    widths,
                                                    color: old_color,
                                                    base_width: self.drawing_state.brush_width,
                                                });
                                            }
                                        }
                                    }
                                    self.drawing_state.current_stroke_times = None;
                                    self.drawing_state.stroke_start_time = None;
                                    self.drawing_state.is_drawing = false;
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("画笔宽度:");
                            let slider_response = ui.add(egui::Slider::new(
                                &mut self.drawing_state.brush_width,
                                1.0..=20.0,
                            ));

                            // 显示大小预览
                            if slider_response.dragged() || slider_response.hovered() {
                                self.drawing_state.show_size_preview = true;
                                self.drawing_state.size_preview_size =
                                    self.drawing_state.brush_width;
                                // 使用屏幕中心位置
                                let content_rect = ui.ctx().available_rect();
                                self.drawing_state.size_preview_pos = content_rect.center();
                            } else if !slider_response.dragged() && !slider_response.hovered() {
                                self.drawing_state.show_size_preview = false;
                            }
                        });

                        ui.separator();

                        ui.horizontal(|ui| {
                            ui.label("动态画笔宽度微调:");
                            ui.selectable_value(
                                &mut self.drawing_state.dynamic_brush_mode,
                                DynamicBrushMode::Disabled,
                                "禁用",
                            );
                            ui.selectable_value(
                                &mut self.drawing_state.dynamic_brush_mode,
                                DynamicBrushMode::BrushTip,
                                "模拟笔锋",
                            );
                            ui.selectable_value(
                                &mut self.drawing_state.dynamic_brush_mode,
                                DynamicBrushMode::SpeedBased,
                                "基于速度",
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("笔迹平滑:");
                            ui.checkbox(&mut self.drawing_state.stroke_smoothing, "启用");
                        });
                    }

                    // 橡皮擦相关设置
                    if self.drawing_state.current_tool == Tool::ObjectEraser
                        || self.drawing_state.current_tool == Tool::PixelEraser
                    {
                        ui.horizontal(|ui| {
                            ui.label("橡皮擦大小:");
                            let slider_response = ui.add(egui::Slider::new(
                                &mut self.drawing_state.eraser_size,
                                5.0..=50.0,
                            ));

                            ui.separator();

                            // 显示大小预览
                            if slider_response.dragged() || slider_response.hovered() {
                                self.drawing_state.show_size_preview = true;
                                self.drawing_state.size_preview_size =
                                    self.drawing_state.eraser_size;
                                // 使用屏幕中心位置
                                let content_rect = ui.ctx().available_rect();
                                self.drawing_state.size_preview_pos = content_rect.center();
                            } else if !slider_response.dragged() && !slider_response.hovered() {
                                self.drawing_state.show_size_preview = false;
                            }

                            if ui.button("清空画布").clicked() {
                                self.drawing_state.strokes.clear();
                                self.drawing_state.images.clear();
                                self.drawing_state.texts.clear();
                                self.drawing_state.current_stroke = None;
                                self.drawing_state.is_drawing = false;
                                self.drawing_state.selected_object = None;
                            }
                        });
                    }

                    // 插入工具相关设置
                    if self.drawing_state.current_tool == Tool::Insert {
                        ui.horizontal(|ui| {
                            if ui.button("图片").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter(
                                        "图片",
                                        &[
                                            "png", "jpg", "jpeg", "bmp", "gif", "tiff", "pnm",
                                            "webp", "tga", "dds", "ico", "hdr", "avif", "qoi",
                                        ],
                                    )
                                    .pick_file()
                                {
                                    if let Ok(img) = image::open(path) {
                                        let img = img.to_rgba8();
                                        let (width, height) = img.dimensions();
                                        let aspect_ratio = width as f32 / height as f32;

                                        // 默认大小
                                        let target_width = 300.0f32;
                                        let target_height = target_width / aspect_ratio;

                                        let ctx = ui.ctx();
                                        let texture = ctx.load_texture(
                                            "inserted_image",
                                            egui::ColorImage::from_rgba_unmultiplied(
                                                [width as usize, height as usize],
                                                &img,
                                            ),
                                            egui::TextureOptions::LINEAR,
                                        );

                                        self.drawing_state.images.push(InsertedImage {
                                            texture,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: egui::vec2(target_width, target_height),
                                            aspect_ratio,
                                        });
                                    }
                                }
                            }
                            if ui.button("文本").clicked() {
                                self.show_text_dialog = true;
                            }
                        });

                        if self.show_text_dialog {
                            // 计算屏幕中心位置
                            let content_rect = ctx.available_rect();
                            let center_pos = content_rect.center();

                            egui::Window::new("插入文本")
                                .collapsible(false)
                                .resizable(false)
                                .pivot(egui::Align2::CENTER_CENTER)
                                .default_pos([center_pos.x, center_pos.y])
                                .show(ctx, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("文本内容:");
                                        ui.text_edit_singleline(&mut self.new_text_content);
                                    });

                                    ui.horizontal(|ui| {
                                        if ui.button("确认").clicked() {
                                            self.drawing_state.texts.push(InsertedText {
                                                text: self.new_text_content.clone(),
                                                pos: Pos2::new(100.0, 100.0),
                                                color: Color32::WHITE,
                                                font_size: 16.0,
                                            });
                                            self.show_text_dialog = false;
                                            self.new_text_content.clear();
                                        }

                                        if ui.button("取消").clicked() {
                                            self.show_text_dialog = false;
                                            self.new_text_content.clear();
                                        }
                                    });
                                });
                        }
                    }

                    // 背景工具相关设置
                    if self.drawing_state.current_tool == Tool::Background {
                        ui.horizontal(|ui| {
                            ui.label("背景颜色:");
                            ui.color_edit_button_srgba(&mut self.drawing_state.background_color);
                        });
                    }

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("退出").clicked() {
                            self.should_quit = true;
                        }
                    });
                });

            // 主画布区域
            egui::CentralPanel::default().show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

                let painter = ui.painter();

                // 绘制背景
                painter.rect_filled(rect, 0.0, self.drawing_state.background_color);

                // 绘制所有图片
                for (i, img) in self.drawing_state.images.iter().enumerate() {
                    let img_rect = egui::Rect::from_min_size(img.pos, img.size);
                    painter.image(
                        img.texture.id(),
                        img_rect,
                        egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );

                    // 如果被选中，绘制边框
                    if let Some(SelectedObject::Image(selected_idx)) =
                        self.drawing_state.selected_object
                    {
                        if i == selected_idx {
                            painter.rect_stroke(
                                img_rect,
                                0.0,
                                Stroke::new(2.0, Color32::BLUE),
                                egui::StrokeKind::Outside,
                            );
                        }
                    }
                }

                // 绘制所有文本
                for (i, text) in self.drawing_state.texts.iter().enumerate() {
                    // Draw text using egui's text rendering
                    painter.text(
                        text.pos,
                        egui::Align2::LEFT_TOP,
                        &text.text,
                        egui::FontId::proportional(text.font_size),
                        text.color,
                    );

                    if let Some(SelectedObject::Text(selected_idx)) =
                        self.drawing_state.selected_object
                    {
                        if i == selected_idx {
                            let text_size = painter
                                .text(
                                    Pos2::new(0.0, 0.0),
                                    egui::Align2::LEFT_TOP,
                                    &text.text,
                                    egui::FontId::proportional(text.font_size),
                                    text.color,
                                )
                                .size();

                            let text_rect = egui::Rect::from_min_size(text.pos, text_size);
                            painter.rect_stroke(
                                text_rect,
                                0.0,
                                Stroke::new(2.0, Color32::BLUE),
                                egui::StrokeKind::Outside,
                            );
                        }
                    }
                }

                // 绘制所有已完成的笔画 - 支持动态宽度
                for (i, stroke) in self.drawing_state.strokes.iter().enumerate() {
                    if stroke.points.len() < 2 {
                        continue;
                    }

                    let color = if let Some(SelectedObject::Stroke(selected_idx)) =
                        self.drawing_state.selected_object
                    {
                        if i == selected_idx {
                            Color32::BLUE
                        } else {
                            stroke.color
                        }
                    } else {
                        stroke.color
                    };

                    // 如果所有宽度相同，使用简单路径
                    let all_same_width =
                        stroke.widths.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01);

                    if all_same_width && stroke.points.len() == 2 {
                        // 只有两个点且宽度相同，直接画线段
                        painter.line_segment(
                            [stroke.points[0], stroke.points[1]],
                            Stroke::new(stroke.widths[0], color),
                        );
                    } else if all_same_width {
                        // 多个点但宽度相同，使用路径
                        let path = egui::epaint::PathShape::line(
                            stroke.points.clone(),
                            Stroke::new(stroke.widths[0], color),
                        );
                        painter.add(Shape::Path(path));
                    } else {
                        // 宽度不同，分段绘制
                        for i in 0..stroke.points.len() - 1 {
                            let avg_width = (stroke.widths[i] + stroke.widths[i + 1]) / 2.0;
                            painter.line_segment(
                                [stroke.points[i], stroke.points[i + 1]],
                                Stroke::new(avg_width, color),
                            );
                        }
                    }
                }

                // 绘制当前正在绘制的笔画 - 支持动态宽度
                if let Some(ref points) = self.drawing_state.current_stroke {
                    if let Some(ref widths) = self.drawing_state.current_stroke_widths {
                        if points.len() >= 2 && widths.len() == points.len() {
                            // 检查是否所有宽度相同
                            let all_same_width =
                                widths.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01);

                            if all_same_width && points.len() == 2 {
                                // 只有两个点且宽度相同
                                painter.line_segment(
                                    [points[0], points[1]],
                                    Stroke::new(widths[0], self.drawing_state.brush_color),
                                );
                            } else if all_same_width {
                                // 多个点但宽度相同
                                let path = egui::epaint::PathShape::line(
                                    points.clone(),
                                    Stroke::new(widths[0], self.drawing_state.brush_color),
                                );
                                painter.add(Shape::Path(path));
                            } else {
                                // 宽度不同，分段绘制
                                for i in 0..points.len() - 1 {
                                    let avg_width = (widths[i] + widths[i + 1]) / 2.0;
                                    painter.line_segment(
                                        [points[i], points[i + 1]],
                                        Stroke::new(avg_width, self.drawing_state.brush_color),
                                    );
                                }
                            }
                        }
                    }
                }

                // 绘制大小预览圆圈
                if self.drawing_state.show_size_preview {
                    const PREVIEW_BORDER_WIDTH: f32 = 2.0;

                    let preview_pos = self.drawing_state.size_preview_pos;
                    let preview_size = self.drawing_state.size_preview_size;
                    let radius = preview_size / PREVIEW_BORDER_WIDTH;

                    // 绘制白色填充的圆
                    painter.circle_filled(preview_pos, radius, Color32::WHITE);

                    // 绘制黑色边框
                    painter.circle_stroke(
                        preview_pos,
                        radius,
                        Stroke::new(PREVIEW_BORDER_WIDTH, Color32::BLACK),
                    );
                }

                // 处理鼠标输入
                let pointer_pos = response.interact_pointer_pos();

                match self.drawing_state.current_tool {
                    Tool::Select => {
                        if response.drag_started() {
                            if let Some(pos) = pointer_pos {
                                self.drawing_state.drag_start_pos = Some(pos);
                                self.drawing_state.selected_object = None;

                                // 检查图片
                                for (i, img) in self.drawing_state.images.iter().enumerate().rev() {
                                    let img_rect = egui::Rect::from_min_size(img.pos, img.size);
                                    if img_rect.contains(pos) {
                                        self.drawing_state.selected_object =
                                            Some(SelectedObject::Image(i));
                                        break;
                                    }
                                }

                                // 检查文本
                                for (i, text) in self.drawing_state.texts.iter().enumerate().rev() {
                                    // 使用 painter 来计算文本大小
                                    let text_size = painter
                                        .text(
                                            Pos2::new(0.0, 0.0),
                                            egui::Align2::LEFT_TOP,
                                            &text.text,
                                            egui::FontId::proportional(text.font_size),
                                            text.color,
                                        )
                                        .size();

                                    let text_rect = egui::Rect::from_min_size(text.pos, text_size);
                                    if text_rect.contains(pos) {
                                        self.drawing_state.selected_object =
                                            Some(SelectedObject::Text(i));
                                        break;
                                    }
                                }

                                // 检查笔画
                                if self.drawing_state.selected_object.is_none() {
                                    for (i, stroke) in
                                        self.drawing_state.strokes.iter().enumerate().rev()
                                    {
                                        if Self::point_intersects_stroke(pos, stroke, 10.0) {
                                            self.drawing_state.selected_object =
                                                Some(SelectedObject::Stroke(i));
                                            break;
                                        }
                                    }
                                }
                            }
                        } else if response.clicked() {
                            // 点击非对象区域时取消选择
                            if let Some(pos) = pointer_pos {
                                let mut hit = false;
                                for img in &self.drawing_state.images {
                                    if egui::Rect::from_min_size(img.pos, img.size).contains(pos) {
                                        hit = true;
                                        break;
                                    }
                                }
                                if !hit {
                                    for stroke in &self.drawing_state.strokes {
                                        if Self::point_intersects_stroke(pos, stroke, 10.0) {
                                            hit = true;
                                            break;
                                        }
                                    }
                                }
                                if !hit {
                                    self.drawing_state.selected_object = None;
                                }
                            }
                        } else if response.dragged() {
                            if let (Some(pos), Some(start_pos)) =
                                (pointer_pos, self.drawing_state.drag_start_pos)
                            {
                                let delta = pos - start_pos;
                                self.drawing_state.drag_start_pos = Some(pos);

                                match self.drawing_state.selected_object {
                                    Some(SelectedObject::Image(idx)) => {
                                        if let Some(img) = self.drawing_state.images.get_mut(idx) {
                                            img.pos += delta;
                                        }
                                    }
                                    Some(SelectedObject::Stroke(idx)) => {
                                        if let Some(stroke) =
                                            self.drawing_state.strokes.get_mut(idx)
                                        {
                                            for p in &mut stroke.points {
                                                *p += delta;
                                            }
                                        }
                                    }
                                    Some(SelectedObject::Text(idx)) => {
                                        if let Some(text) = self.drawing_state.texts.get_mut(idx) {
                                            text.pos += delta;
                                        }
                                    }
                                    None => {}
                                }
                            }
                        }
                    }

                    Tool::Insert | Tool::Background => {
                        // 插入工具和背景工具通过 UI 按钮触发，这里不处理画布交互
                    }

                    Tool::ObjectEraser => {
                        // 对象橡皮擦：点击或拖拽时删除相交的整个笔画
                        if response.drag_started() || response.clicked() || response.dragged() {
                            if let Some(pos) = pointer_pos {
                                // 从后往前删除，避免索引问题
                                let mut to_remove = Vec::new();
                                for (i, stroke) in
                                    self.drawing_state.strokes.iter().enumerate().rev()
                                {
                                    if Self::point_intersects_stroke(
                                        pos,
                                        stroke,
                                        self.drawing_state.eraser_size,
                                    ) {
                                        to_remove.push(i);
                                    }
                                }
                                for i in to_remove {
                                    self.drawing_state.strokes.remove(i);
                                }
                            }
                        }
                    }

                    Tool::PixelEraser => {
                        // 像素橡皮擦：从笔画中移除被擦除的点
                        if response.drag_started() {
                            if let Some(pos) = pointer_pos {
                                self.drawing_state.is_drawing = true;
                                self.drawing_state.current_stroke = Some(vec![pos]);
                            }
                        } else if response.dragged() {
                            if self.drawing_state.is_drawing {
                                if let Some(pos) = pointer_pos {
                                    if let Some(ref mut points) = self.drawing_state.current_stroke
                                    {
                                        if points.is_empty()
                                            || points.last().unwrap().distance(pos) > 1.0
                                        {
                                            points.push(pos);
                                        }
                                    }

                                    // 从所有笔画中移除被橡皮擦覆盖的点
                                    let eraser_radius = self.drawing_state.eraser_size / 2.0;
                                    for stroke in &mut self.drawing_state.strokes {
                                        let mut new_points = Vec::new();
                                        let mut new_widths = Vec::new();

                                        for (i, point) in stroke.points.iter().enumerate() {
                                            let dist = (point.x - pos.x).hypot(point.y - pos.y);
                                            if dist > eraser_radius {
                                                new_points.push(*point);
                                                if i < stroke.widths.len() {
                                                    new_widths.push(stroke.widths[i]);
                                                }
                                            }
                                        }

                                        stroke.points = new_points;
                                        stroke.widths = new_widths;
                                    }

                                    // 移除空的笔画
                                    self.drawing_state.strokes.retain(|s| s.points.len() >= 2);
                                }
                            }
                        } else if response.drag_stopped() {
                            self.drawing_state.is_drawing = false;
                            self.drawing_state.current_stroke = None;
                        }
                    }

                    Tool::Brush => {
                        // 画笔工具：原有逻辑
                        if response.drag_started() {
                            // 开始新的笔画
                            if let Some(pos) = pointer_pos {
                                if pos.x >= rect.min.x
                                    && pos.x <= rect.max.x
                                    && pos.y >= rect.min.y
                                    && pos.y <= rect.max.y
                                {
                                    self.drawing_state.is_drawing = true;
                                    self.drawing_state.current_stroke = Some(vec![pos]);
                                    let start_time = Instant::now();
                                    self.drawing_state.stroke_start_time = Some(start_time);
                                    self.drawing_state.current_stroke_times = Some(vec![0.0]);
                                    let width = Self::calculate_dynamic_width(
                                        self.drawing_state.brush_width,
                                        self.drawing_state.dynamic_brush_mode,
                                        0,
                                        1,
                                        None,
                                    );
                                    self.drawing_state.current_stroke_widths = Some(vec![width]);
                                }
                            }
                        } else if response.dragged() {
                            // 继续绘制
                            if self.drawing_state.is_drawing {
                                if let Some(pos) = pointer_pos {
                                    if let Some(ref mut points) = self.drawing_state.current_stroke
                                    {
                                        if let Some(ref mut widths) =
                                            self.drawing_state.current_stroke_widths
                                        {
                                            if let Some(ref mut times) =
                                                self.drawing_state.current_stroke_times
                                            {
                                                // 只添加与上一个点距离足够远的点，避免点太密集
                                                if points.is_empty()
                                                    || points.last().unwrap().distance(pos) > 1.0
                                                {
                                                    let current_time = if let Some(start) =
                                                        self.drawing_state.stroke_start_time
                                                    {
                                                        start.elapsed().as_secs_f64()
                                                    } else {
                                                        0.0
                                                    };

                                                    // 计算速度（像素/秒）
                                                    let speed = if points.len() > 0
                                                        && times.len() > 0
                                                    {
                                                        let last_time = times.last().unwrap();
                                                        let time_delta =
                                                            ((current_time - last_time) as f32)
                                                                .max(0.001); // 避免除零
                                                        let distance =
                                                            points.last().unwrap().distance(pos);
                                                        Some(distance / time_delta)
                                                    } else {
                                                        None
                                                    };

                                                    points.push(pos);
                                                    times.push(current_time);

                                                    // 计算动态宽度
                                                    let width = Self::calculate_dynamic_width(
                                                        self.drawing_state.brush_width,
                                                        self.drawing_state.dynamic_brush_mode,
                                                        points.len() - 1,
                                                        points.len(),
                                                        speed,
                                                    );
                                                    widths.push(width);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else if response.drag_stopped() {
                            // 结束当前笔画
                            if self.drawing_state.is_drawing {
                                if let Some(points) = self.drawing_state.current_stroke.take() {
                                    if let Some(widths) =
                                        self.drawing_state.current_stroke_widths.take()
                                    {
                                        if points.len() > 1 && widths.len() == points.len() {
                                            // 应用笔画平滑
                                            let final_points =
                                                if self.drawing_state.stroke_smoothing {
                                                    Self::apply_stroke_smoothing(&points)
                                                } else {
                                                    points
                                                };

                                            self.drawing_state.strokes.push(DrawingStroke {
                                                points: final_points,
                                                widths,
                                                color: self.drawing_state.brush_color,
                                                base_width: self.drawing_state.brush_width,
                                            });
                                        }
                                    }
                                }
                                self.drawing_state.current_stroke_times = None;
                                self.drawing_state.stroke_start_time = None;
                                self.drawing_state.is_drawing = false;
                            }
                        }

                        // 如果鼠标在画布内移动且正在绘制，也添加点（用于平滑绘制）
                        if response.hovered() && self.drawing_state.is_drawing {
                            if let Some(pos) = pointer_pos {
                                if let Some(ref mut points) = self.drawing_state.current_stroke {
                                    if let Some(ref mut widths) =
                                        self.drawing_state.current_stroke_widths
                                    {
                                        if let Some(ref mut times) =
                                            self.drawing_state.current_stroke_times
                                        {
                                            if points.is_empty()
                                                || points.last().unwrap().distance(pos) > 1.0
                                            {
                                                let current_time = if let Some(start) =
                                                    self.drawing_state.stroke_start_time
                                                {
                                                    start.elapsed().as_secs_f64()
                                                } else {
                                                    0.0
                                                };

                                                // 计算速度
                                                let speed = if points.len() > 0 && times.len() > 0 {
                                                    let last_time = times.last().unwrap();
                                                    let time_delta = ((current_time - last_time)
                                                        as f32)
                                                        .max(0.001);
                                                    let distance =
                                                        points.last().unwrap().distance(pos);
                                                    Some(distance / time_delta)
                                                } else {
                                                    None
                                                };

                                                points.push(pos);
                                                times.push(current_time);

                                                let width = Self::calculate_dynamic_width(
                                                    self.drawing_state.brush_width,
                                                    self.drawing_state.dynamic_brush_mode,
                                                    points.len() - 1,
                                                    points.len(),
                                                    speed,
                                                );
                                                widths.push(width);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            });

            state.egui_renderer.end_frame_and_draw(
                &state.device,
                &state.queue,
                &mut encoder,
                window,
                &surface_view,
                screen_descriptor,
            );
        }

        state.queue.submit(Some(encoder.finish()));
        surface_texture.present();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();
        pollster::block_on(self.set_window(window));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        // 检查是否需要退出
        if self.should_quit {
            println!("Quit button was pressed; exiting");
            event_loop.exit();
            return;
        }

        // let egui render to process the event first
        self.state
            .as_mut()
            .unwrap()
            .egui_renderer
            .handle_input(self.window.as_ref().unwrap(), &event);

        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: winit::event::ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                println!("Escape key pressed; exiting");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();

                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::Resized(new_size) => {
                self.handle_resized(new_size.width, new_size.height);
            }
            _ => (),
        }
    }
}
