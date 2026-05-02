use crate::render::RenderState;
use crate::state::{
    ActiveStroke, AppState, CanvasImage, CanvasObject, CanvasObjectOps, CanvasShape,
    CanvasShapeType, CanvasState, CanvasStroke, CanvasText, CanvasTool, DynamicBrushWidthMode,
    History, ICON, OptimizationPolicy, PageState, PersistentState, StrokeWidth, ThemeMode,
    WindowMode,
};
use crate::{UserEvent, utils};
use core::f32;
use egui::{Color32, Pos2, Stroke};
use egui_wgpu::{ScreenDescriptor, wgpu, wgpu::PresentMode};
use image::GenericImageView;
use std::sync::Arc;
use std::time::Instant;
use tray_icon::TrayIconBuilder;
use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, Touch, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

#[cfg(feature = "startup_animation")]
use crate::state::StartupAnimation;
#[cfg(feature = "startup_animation")]
include!(concat!(env!("OUT_DIR"), "/startup_frames.rs"));
#[cfg(feature = "startup_animation")]
const STARTUP_AUDIO: &[u8] = include_bytes!("../assets/startup_animation/audio.wav");

enum PageAction {
    None,
    Previous,
    Next,
    New,
    Delete,
}

fn switch_to_page_state(state: &mut AppState, page_index: usize) {
    let old = state.current_page;
    if old != page_index {
        std::mem::swap(&mut state.canvas, &mut state.pages[old].canvas);
        std::mem::swap(&mut state.history, &mut state.pages[old].history);
        state.current_page = page_index;
        std::mem::swap(&mut state.canvas, &mut state.pages[page_index].canvas);
        std::mem::swap(&mut state.history, &mut state.pages[page_index].history);
    }
    state.selected_object = None;
    state.drag_start_pos = None;
    state.dragged_handle = None;
    state.drag_move_accumulated_delta = egui::Vec2::ZERO;
    state.drag_original_transform = None;
    state.active_strokes.clear();
    state.is_drawing = false;
}

fn add_new_page_state(state: &mut AppState) {
    let old = state.current_page;
    state.pages[old].canvas = std::mem::take(&mut state.canvas);
    state.pages[old].history = std::mem::take(&mut state.history);
    state.pages.push(PageState::default());
    let new_idx = state.pages.len() - 1;
    state.current_page = new_idx;
    state.selected_object = None;
    state.drag_start_pos = None;
    state.dragged_handle = None;
    state.drag_move_accumulated_delta = egui::Vec2::ZERO;
    state.drag_original_transform = None;
    state.active_strokes.clear();
    state.is_drawing = false;
}

fn load_canvas_from_file(state: &mut AppState) {
    match CanvasState::load_from_file_with_dialog() {
        Ok(canvas) => {
            let page = PageState {
                canvas,
                history: History::default(),
            };
            let new_idx = state.pages.len();
            state.pages.push(page.clone());
            state.current_page = new_idx;
            state.canvas = page.canvas;
            state.history = page.history;
            state.selected_object = None;
            state.show_welcome_window = false;
            state.toasts.success("成功加载画布!");
        }
        Err(err) => {
            state.toasts.error(format!("画布加载失败: {}!", err));
        }
    };
}

fn apply_page_action(state: &mut AppState, action: PageAction) {
    match action {
        PageAction::Previous if state.current_page > 0 => {
            switch_to_page_state(state, state.current_page - 1);
        }
        PageAction::Next if state.current_page + 1 < state.pages.len() => {
            switch_to_page_state(state, state.current_page + 1);
        }
        PageAction::New => {
            add_new_page_state(state);
        }
        PageAction::Delete if state.pages.len() > 1 => {
            let i = state.current_page;
            state.pages.remove(i);
            if i >= state.pages.len() {
                state.current_page = state.pages.len() - 1;
            }
            state.canvas = std::mem::take(&mut state.pages[state.current_page].canvas);
            state.history = std::mem::take(&mut state.pages[state.current_page].history);
            state.selected_object = None;
            state.drag_start_pos = None;
            state.dragged_handle = None;
            state.drag_move_accumulated_delta = egui::Vec2::ZERO;
            state.drag_original_transform = None;
            state.active_strokes.clear();
            state.is_drawing = false;
        }
        _ => {}
    }
}

pub struct App {
    gpu_instance: wgpu::Instance,
    render_state: Option<RenderState>,
    window: Option<Arc<Window>>,
    state: AppState,
}

impl App {
    pub fn new() -> Self {
        let gpu_instance = egui_wgpu::wgpu::Instance::default();
        let mut state = AppState::default();

        if !state.persistent.show_welcome_window_on_start {
            state.show_welcome_window = false
        }

        #[cfg(feature = "startup_animation")]
        if state.persistent.show_startup_animation {
            state.startup_animation =
                Some(StartupAnimation::new(30.0, STARTUP_FRAMES, STARTUP_AUDIO));
        }

        Self {
            gpu_instance,
            render_state: None,
            window: None,
            state,
        }
    }

    pub async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);

        let icon = image::load_from_memory(ICON).expect("invalid icon data");

        let rgba = icon.to_rgba8().to_vec();
        let (width, height) = icon.dimensions();

        // 设置标题
        window.set_title("smartboard");
        let winit_icon = Some(
            winit::window::Icon::from_rgba(rgba.clone(), width, height).expect("invalid icon data"),
        );
        window.set_window_icon(winit_icon.clone());
        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowExtWindows;
            window.set_taskbar_icon(winit_icon);
        }

        // 获取显示模式
        let monitor = window
            .current_monitor()
            .or_else(|| window.primary_monitor());

        self.state.available_video_modes = monitor
            .map(|m| m.video_modes().collect())
            .unwrap_or_else(|| {
                eprintln!("warning: no monitor available yet");
                Vec::new()
            });

        // 设置窗口模式
        self.apply_window_mode(&window);

        // 创建托盘图标
        let tray = TrayIconBuilder::new()
            .with_icon(tray_icon::Icon::from_rgba(rgba, width, height).expect("invalid icon data"))
            .with_tooltip("smartboard")
            .build()
            .unwrap();
        let _ = tray.set_visible(false);
        self.state.tray = Some(tray);

        // 获取窗口尺寸
        let size = window.inner_size();
        let initial_width = size.width;
        let initial_height = size.height;

        let surface = self
            .gpu_instance
            .create_surface(window.clone())
            .expect("failed to create surface");

        let state = RenderState::new(
            &self.gpu_instance,
            surface,
            &window,
            initial_width,
            initial_height,
            self.state.persistent.optimization_policy,
            self.state.persistent.present_mode,
        )
        .await;

        let ctx = state.egui_renderer.context();

        // 设置主题模式
        match self.state.persistent.theme_mode {
            ThemeMode::System => {
                ctx.set_visuals(egui::Visuals {
                    panel_fill: self.state.persistent.background_color,
                    ..Default::default()
                });
            }
            ThemeMode::Light => {
                ctx.set_visuals(egui::Visuals {
                    panel_fill: self.state.persistent.background_color,
                    ..egui::Visuals::light()
                });
            }
            ThemeMode::Dark => {
                ctx.set_visuals(egui::Visuals {
                    panel_fill: self.state.persistent.background_color,
                    ..egui::Visuals::dark()
                });
            }
        }

        self.window.get_or_insert(window);
        self.render_state.get_or_insert(state);
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.state.persistent.save_to_file() {
            eprintln!("failed to save settings: {}", err)
        }
        self.state.tray.take(); // closes tray
        event_loop.exit();
    }

    fn apply_window_mode(&self, window: &Arc<Window>) {
        match self.state.persistent.window_mode {
            WindowMode::Windowed => {
                // 窗口化
                window.set_fullscreen(None);
            }
            WindowMode::Fullscreen => {
                // 全屏
                // 使用选中的视频模式
                if let Some(selected_index) = self.state.selected_video_mode_index {
                    if selected_index < self.state.available_video_modes.len() {
                        if let Some(mode) = self.state.available_video_modes.get(selected_index) {
                            window.set_fullscreen(Some(Fullscreen::Exclusive(mode.clone())));
                            return;
                        }
                    }
                }

                // 回退到第一个可用的视频模式
                window.set_fullscreen(Some(Fullscreen::Exclusive(
                    self.state
                        .available_video_modes
                        .first()
                        .expect("no video mode available")
                        .clone(),
                )));
            }
            WindowMode::BorderlessFullscreen => {
                // 无边框全屏
                window.set_fullscreen(Some(Fullscreen::Borderless(window.current_monitor())));
            }
        }
    }

    fn apply_present_mode(&mut self) {
        let wgpu_present_mode = self.state.persistent.present_mode;

        if let Some(render_state) = self.render_state.as_mut() {
            render_state.set_present_mode(wgpu_present_mode);
        }
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.render_state
                .as_mut()
                .unwrap()
                .resize_surface(width, height);
        }
    }

    fn handle_redraw(&mut self) {
        let render_state = self.render_state.as_mut().unwrap();

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [
                render_state.surface_config.width,
                render_state.surface_config.height,
            ],
            pixels_per_point: self.window.as_ref().unwrap().scale_factor() as f32
                * render_state.scale_factor,
        };

        let surface_texture = render_state.surface.get_current_texture();

        let surface_texture = match surface_texture {
            CurrentSurfaceTexture::Success(surface) => surface,
            CurrentSurfaceTexture::Lost => {
                println!("wgpu surface lost");
                return;
            }
            CurrentSurfaceTexture::Outdated => {
                println!("wgpu surface outdated");
                return;
            }
            CurrentSurfaceTexture::Timeout => {
                println!("wgpu surface timeout");
                return;
            }
            val => {
                println!("{:?}", val);
                return;
            }
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = render_state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let window = self.window.as_ref().unwrap();

        render_state.egui_renderer.begin_frame(window);
        let ctx = render_state.egui_renderer.context();

        #[cfg(feature = "startup_animation")]
        if let Some(anim) = &mut self.state.startup_animation {
            if !anim.is_finished() {
                anim.update(ctx);
                anim.draw_fullscreen(ctx);
                ctx.request_repaint(); // ensure smooth playback
            }
        }

        self.state.toasts.show(ctx);

        // 欢迎窗口
        if self.state.show_welcome_window {
            let content_rect = ctx.content_rect();
            let center_pos = content_rect.center();

            egui::Window::new("欢迎")
                .resizable(false)
                .collapsible(false)
                .movable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .current_pos(center_pos)
                .order(egui::Order::Foreground)
                .enabled({
                    #[cfg(feature = "startup_animation")]
                    if let Some(anim) = &self.state.startup_animation {
                        anim.is_finished()
                    } else {
                        true
                    }
                    #[cfg(not(feature = "startup_animation"))]
                    true
                })
                .show(ctx, |ui| {
                    ui.heading("欢迎使用 smartboard");
                    ui.separator();

                    ui.label("这是一个功能强大的数字画板应用，您可以：");
                    ui.label("• 绘制和涂鸦");
                    ui.label("• 使用各种工具进行编辑");
                    ui.label("• 插入图片、文本和形状");
                    ui.label("• 自定义画板设置");
                    ui.label("• 保存和加载画布到文件");
                    ui.label("• 享受超快的启动速度与超高的流畅度");
                    ui.separator();

                    if ui.button("新建画布").clicked() {
                        let default_page = PageState::default();
                        self.state.pages = vec![default_page.clone()];
                        self.state.current_page = 0;
                        self.state.canvas = default_page.canvas;
                        self.state.history = default_page.history;
                        self.state.selected_object = None;
                        self.state.show_welcome_window = false;
                    }
                    if ui.button("加载画布").clicked() {
                        load_canvas_from_file(&mut self.state);
                    }

                    ui.separator();

                    ui.checkbox(
                        &mut self.state.persistent.show_welcome_window_on_start,
                        "启动时显示欢迎",
                    );
                });
        }

        // 工具栏窗口
        let content_rect = ctx.content_rect();
        let margin = 20.0; // 底部边距

        egui::Window::new("工具栏")
            .resizable(false)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .default_pos([content_rect.center().x, content_rect.max.y - margin])
            .enabled(
                !self.state.show_welcome_window && {
                    #[cfg(feature = "startup_animation")]
                    if let Some(anim) = &self.state.startup_animation {
                        anim.is_finished()
                    } else {
                        true
                    }
                    #[cfg(not(feature = "startup_animation"))]
                    true
                },
            )
            .show(ctx, |ui| {
                // 工具选择
                ui.horizontal(|ui| {
                    ui.label("工具:");
                    // TODO: egui doesn't support rendering fonts with colors
                    let old_tool = self.state.current_tool;
                    if ui
                        .selectable_value(&mut self.state.current_tool, CanvasTool::Select, "选择")
                        .changed()
                        || ui
                            .selectable_value(
                                &mut self.state.current_tool,
                                CanvasTool::Brush,
                                "画笔",
                            )
                            .changed()
                        || ui
                            .selectable_value(
                                &mut self.state.current_tool,
                                CanvasTool::ObjectEraser,
                                "对象擦",
                            )
                            .changed()
                        || ui
                            .selectable_value(
                                &mut self.state.current_tool,
                                CanvasTool::PixelEraser,
                                "像素擦",
                            )
                            .changed()
                        || ui
                            .selectable_value(
                                &mut self.state.current_tool,
                                CanvasTool::Insert,
                                "插入",
                            )
                            .changed()
                        || ui
                            .selectable_value(
                                &mut self.state.current_tool,
                                CanvasTool::Settings,
                                "设置",
                            )
                            .changed()
                    {
                        if self.state.current_tool != old_tool {
                            self.state.selected_object = None;
                        }
                    }
                });

                ui.separator();

                // 选择工具相关设置
                if self.state.current_tool == CanvasTool::Select {
                    if self.state.selected_object.is_some() {
                        ui.horizontal(|ui| {
                            ui.label("对象操作:");
                            if ui.button("删除").clicked() {
                                if let Some(selected_idx) = self.state.selected_object {
                                    // Save state to history before modification
                                    let removed_object =
                                        self.state.canvas.objects.remove(selected_idx);
                                    self.state
                                        .history
                                        .save_remove_object(selected_idx, removed_object);
                                    self.state.selected_object = None;
                                    self.state.toasts.success("对象已删除!");
                                }
                            }
                            if ui.button("置顶").clicked() {
                                if let Some(selected_idx) = self.state.selected_object {
                                    if selected_idx < self.state.canvas.objects.len() - 1 {
                                        // Save state to history before modification
                                        let object = self.state.canvas.objects.remove(selected_idx);
                                        // Actually move the object to the top (end of the array)
                                        self.state.canvas.objects.push(object);
                                        self.state.history.save_add_object(
                                            self.state.canvas.objects.len() - 1,
                                            self.state.canvas.objects.last().unwrap().clone(),
                                        );
                                        self.state.selected_object =
                                            Some(self.state.canvas.objects.len() - 1);
                                        self.state.toasts.success("对象已移至顶部!");
                                    }
                                }
                            }
                            if ui.button("置底").clicked() {
                                if let Some(selected_idx) = self.state.selected_object {
                                    if selected_idx > 0 {
                                        // Save state to history before modification
                                        let object = self.state.canvas.objects.remove(selected_idx);
                                        // Actually move the object to the bottom (beginning of the array)
                                        self.state.canvas.objects.insert(0, object);
                                        self.state.history.save_add_object(
                                            0,
                                            self.state.canvas.objects.first().unwrap().clone(),
                                        );
                                        self.state.selected_object = Some(0);
                                        self.state.toasts.success("对象已移至底部!");
                                    }
                                }
                            }

                            // 检查是否选中了文本对象
                            if let Some(selected_idx) = self.state.selected_object {
                                if let Some(CanvasObject::Text(_)) =
                                    self.state.canvas.objects.get(selected_idx)
                                {
                                    if ui.button("栅格化").clicked() {
                                        if let Some(text_obj) =
                                            self.state.canvas.objects.get(selected_idx).cloned()
                                        {
                                            if let CanvasObject::Text(text) = &text_obj {
                                                // 转换文本为笔画
                                                let strokes = crate::utils::rasterize_text(
                                                    text,
                                                    utils::font_bytes(),
                                                );

                                                // 删除原文本对象
                                                self.state.canvas.objects.remove(selected_idx);

                                                // FIXME: this might be inefficient
                                                // Add all new strokes
                                                for stroke in strokes {
                                                    let stroke_obj = CanvasObject::Stroke(stroke);
                                                    self.state
                                                        .canvas
                                                        .objects
                                                        .push(stroke_obj.clone());

                                                    self.state.history.save_add_object(
                                                        self.state.canvas.objects.len() - 1,
                                                        stroke_obj,
                                                    );
                                                }

                                                self.state
                                                    .history
                                                    .save_remove_object(selected_idx, text_obj);

                                                self.state.selected_object = None;
                                                self.state.toasts.success("已转换为笔画!");
                                            }
                                        }
                                    }
                                }
                            }
                        });
                    } else {
                        ui.label(egui::RichText::new("(未选中对象)").italics());
                    }
                }

                // 画笔相关设置
                if self.state.current_tool == CanvasTool::Brush {
                    ui.horizontal(|ui| {
                        ui.label("颜色:");
                        let old_color = self.state.brush_color;
                        if ui
                            .color_edit_button_srgba(&mut self.state.brush_color)
                            .changed()
                        {
                            // 颜色改变时，如果正在绘制，结束所有当前笔画
                            if self.state.is_drawing {
                                for (_touch_id, active_stroke) in self.state.active_strokes.drain()
                                {
                                    self.state.canvas.objects.push(CanvasObject::Stroke(
                                        CanvasStroke {
                                            points: active_stroke.points,
                                            width: active_stroke.width,
                                            color: old_color,
                                            base_width: self.state.brush_width,
                                            rot: 0.0,
                                        },
                                    ));
                                }
                                self.state.is_drawing = false;
                            }
                        }
                    });

                    // 颜色快捷按钮
                    ui.horizontal(|ui| {
                        ui.label("快捷颜色:");
                        for color in &self.state.persistent.quick_colors {
                            let color_name = if color.r() == 0 && color.g() == 0 && color.b() == 0 {
                                "黑"
                            } else if color.r() == 255 && color.g() == 255 && color.b() == 255 {
                                "白"
                            } else if color.r() == 0 && color.g() == 100 && color.b() == 255 {
                                "蓝"
                            } else if color.r() == 220 && color.g() == 20 && color.b() == 60 {
                                "红"
                            } else if color.r() == 34 && color.g() == 139 && color.b() == 34 {
                                "绿"
                            } else if color.r() == 255 && color.g() == 140 && color.b() == 0 {
                                "橙"
                            } else {
                                "自定义"
                            };
                            if ui
                                .add(egui::Button::new(
                                    egui::RichText::new(color_name).color(*color),
                                ))
                                .clicked()
                            {
                                self.state.brush_color = *color;
                            }
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("宽度:");
                        let slider_response =
                            ui.add(egui::Slider::new(&mut self.state.brush_width, 1.0..=20.0));

                        // 显示大小预览
                        if slider_response.dragged() || slider_response.hovered() {
                            self.state.show_size_preview = true;
                            // 使用屏幕中心位置
                        } else if !slider_response.dragged() && !slider_response.hovered() {
                            self.state.show_size_preview = false;
                        }
                    });

                    // 画笔宽度快捷按钮
                    ui.horizontal(|ui| {
                        ui.label("快捷宽度:");
                        if ui.button("小").clicked() {
                            self.state.brush_width = 1.0;
                        }
                        if ui.button("中").clicked() {
                            self.state.brush_width = 3.0;
                        }
                        if ui.button("大").clicked() {
                            self.state.brush_width = 5.0;
                        }
                    });
                }

                // 橡皮擦相关设置
                if self.state.current_tool == CanvasTool::ObjectEraser
                    || self.state.current_tool == CanvasTool::PixelEraser
                {
                    ui.horizontal(|ui| {
                        ui.label("大小:");
                        let slider_response =
                            ui.add(egui::Slider::new(&mut self.state.eraser_size, 5.0..=50.0));

                        // 显示大小预览
                        if slider_response.dragged() || slider_response.hovered() {
                            self.state.show_size_preview = true;
                        } else if !slider_response.dragged() && !slider_response.hovered() {
                            self.state.show_size_preview = false;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("清空:");
                        if ui.button("OK").clicked() {
                            // Save state to history before modification
                            let old_objects = std::mem::take(&mut self.state.canvas.objects);
                            self.state.history.save_clear_objects(old_objects);
                            self.state.active_strokes.clear();
                            self.state.is_drawing = false;
                            self.state.selected_object = None;
                            self.state.current_tool = CanvasTool::Brush;
                        }
                    });
                }

                // 插入工具相关设置
                if self.state.current_tool == CanvasTool::Insert {
                    ui.horizontal(|ui| {
                        if ui.button("图片").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter(
                                    "图片",
                                    &[
                                        "png", "jpg", "jpeg", "bmp", "gif", "tiff", "pnm", "webp",
                                        "tga", "dds", "ico", "hdr", "avif", "qoi",
                                    ],
                                )
                                .pick_file()
                            {
                                if let Ok(img) = image::open(path) {
                                    // 最大纹理大小限制（通常为 2048x2048）
                                    const MAX_TEXTURE_SIZE: u32 = 2048;

                                    // 如果图像太大，调整大小以适应纹理限制
                                    let img = if img.width() > MAX_TEXTURE_SIZE
                                        || img.height() > MAX_TEXTURE_SIZE
                                    {
                                        crate::utils::resize_image_for_texture(
                                            img,
                                            MAX_TEXTURE_SIZE,
                                        )
                                    } else {
                                        img
                                    };

                                    let img_rgba = img.to_rgba8();
                                    let (width, height) = img_rgba.dimensions();
                                    let aspect_ratio = width as f32 / height as f32;

                                    // 默认大小
                                    let target_width = 300.0_f32;
                                    let target_height = target_width / aspect_ratio;

                                    let ctx = ui.ctx();
                                    let texture = ctx.load_texture(
                                        "inserted_image",
                                        egui::ColorImage::from_rgba_unmultiplied(
                                            [width as usize, height as usize],
                                            &img_rgba,
                                        ),
                                        egui::TextureOptions::LINEAR,
                                    );

                                    // Save state to history before modification
                                    let new_image = CanvasImage {
                                        texture,
                                        pos: Pos2::new(100.0, 100.0),
                                        size: egui::vec2(target_width, target_height),
                                        aspect_ratio,
                                        marked_for_deletion: false,
                                        rot: 0.0,
                                    };
                                    let index = self.state.canvas.objects.len();
                                    self.state.history.save_add_object(
                                        index,
                                        CanvasObject::Image(new_image.clone()),
                                    );
                                    self.state
                                        .canvas
                                        .objects
                                        .push(CanvasObject::Image(new_image));

                                    self.state.current_tool = CanvasTool::Select;
                                }
                            }
                        }
                        if ui.button("文本").clicked() {
                            self.state.show_insert_text_dialog = true;
                        }
                        if ui.button("形状").clicked() {
                            self.state.show_insert_shape_dialog = true;
                        }
                    });

                    if self.state.show_insert_text_dialog {
                        // 计算屏幕中心位置
                        let content_rect = ctx.content_rect();
                        let center_pos = content_rect.center();

                        egui::Window::new("插入文本")
                            .collapsible(false)
                            .resizable(false)
                            .pivot(egui::Align2::CENTER_CENTER)
                            .default_pos([center_pos.x, center_pos.y])
                            .show(ctx, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("文本内容:");
                                    ui.text_edit_singleline(&mut self.state.new_text_content);
                                });

                                ui.horizontal(|ui| {
                                    if ui.button("确认").clicked() {
                                        // Save state to history before modification
                                        let new_text = CanvasText {
                                            text: self.state.new_text_content.clone(),
                                            pos: Pos2::new(100.0, 100.0),
                                            color: Color32::WHITE,
                                            font_size: 16.0,
                                            rot: 0.0,
                                        };
                                        let index = self.state.canvas.objects.len();
                                        self.state.history.save_add_object(
                                            index,
                                            CanvasObject::Text(new_text.clone()),
                                        );
                                        self.state
                                            .canvas
                                            .objects
                                            .push(CanvasObject::Text(new_text));
                                        self.state.current_tool = CanvasTool::Select;
                                        self.state.show_insert_text_dialog = false;
                                        self.state.new_text_content.clear();
                                    }

                                    if ui.button("取消").clicked() {
                                        self.state.show_insert_text_dialog = false;
                                        self.state.new_text_content.clear();
                                    }

                                    #[cfg(target_os = "windows")]
                                    {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                let keyboard_btn = ui.button("屏幕键盘");
                                                if keyboard_btn.clicked() {
                                                    let _ =
                                                        crate::windows_utils::show_touch_keyboard(
                                                            None,
                                                        );
                                                }
                                            },
                                        );
                                    }
                                });
                            });
                    }

                    if self.state.show_insert_shape_dialog {
                        // 计算屏幕中心位置
                        let content_rect = ctx.content_rect();
                        let center_pos = content_rect.center();

                        egui::Window::new("插入形状")
                            .collapsible(false)
                            .resizable(false)
                            .pivot(egui::Align2::CENTER_CENTER)
                            .default_pos([center_pos.x, center_pos.y])
                            .show(ctx, |ui| {
                                ui.label("选择要插入的形状:");

                                ui.horizontal(|ui| {
                                    if ui.button("线").clicked() {
                                        // Save state to history before modification
                                        let new_shape = CanvasShape {
                                            shape_type: CanvasShapeType::Line,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: 100.0,
                                            color: Color32::WHITE,
                                            rotation: 0.0,
                                        };
                                        let index = self.state.canvas.objects.len();
                                        self.state.history.save_add_object(
                                            index,
                                            CanvasObject::Shape(new_shape.clone()),
                                        );
                                        self.state
                                            .canvas
                                            .objects
                                            .push(CanvasObject::Shape(new_shape));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("箭头").clicked() {
                                        // Save state to history before modification
                                        let new_shape = CanvasShape {
                                            shape_type: CanvasShapeType::Arrow,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: 100.0,
                                            color: Color32::WHITE,
                                            rotation: 0.0,
                                        };
                                        let index = self.state.canvas.objects.len();
                                        self.state.history.save_add_object(
                                            index,
                                            CanvasObject::Shape(new_shape.clone()),
                                        );
                                        self.state
                                            .canvas
                                            .objects
                                            .push(CanvasObject::Shape(new_shape));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("矩形").clicked() {
                                        // Save state to history before modification
                                        let new_shape = CanvasShape {
                                            shape_type: CanvasShapeType::Rectangle,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: 100.0,
                                            color: Color32::WHITE,
                                            rotation: 0.0,
                                        };
                                        let index = self.state.canvas.objects.len();
                                        self.state.history.save_add_object(
                                            index,
                                            CanvasObject::Shape(new_shape.clone()),
                                        );
                                        self.state
                                            .canvas
                                            .objects
                                            .push(CanvasObject::Shape(new_shape));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }
                                    if ui.button("三角形").clicked() {
                                        // Save state to history before modification
                                        let new_shape = CanvasShape {
                                            shape_type: CanvasShapeType::Triangle,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: 100.0,
                                            color: Color32::WHITE,
                                            rotation: 0.0,
                                        };
                                        let index = self.state.canvas.objects.len();
                                        self.state.history.save_add_object(
                                            index,
                                            CanvasObject::Shape(new_shape.clone()),
                                        );
                                        self.state
                                            .canvas
                                            .objects
                                            .push(CanvasObject::Shape(new_shape));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("圆形").clicked() {
                                        // Save state to history before modification
                                        let new_shape = CanvasShape {
                                            shape_type: CanvasShapeType::Circle,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: 100.0,
                                            color: Color32::WHITE,
                                            rotation: 0.0,
                                        };
                                        let index = self.state.canvas.objects.len();
                                        self.state.history.save_add_object(
                                            index,
                                            CanvasObject::Shape(new_shape.clone()),
                                        );
                                        self.state
                                            .canvas
                                            .objects
                                            .push(CanvasObject::Shape(new_shape));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    if ui.button("取消").clicked() {
                                        self.state.show_insert_shape_dialog = false;
                                    }
                                    ui.checkbox(
                                        &mut self.state.persistent.keep_insertion_window_open,
                                        "保持窗口开启",
                                    );
                                });
                            });
                    }
                }

                // 设置工具相关设置
                if self.state.current_tool == CanvasTool::Settings {
                    ui.collapsing("外观", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("背景颜色:");
                            ui.color_edit_button_srgba(&mut self.state.persistent.background_color);
                        });

                        ui.horizontal(|ui| {
                            ui.label("主题模式:");
                            if ui
                                .selectable_value(
                                    &mut self.state.persistent.theme_mode,
                                    ThemeMode::System,
                                    "跟随系统",
                                )
                                .clicked()
                            {
                                ctx.set_visuals(egui::Visuals {
                                    panel_fill: self.state.persistent.background_color,
                                    ..Default::default()
                                });
                            }
                            if ui
                                .selectable_value(
                                    &mut self.state.persistent.theme_mode,
                                    ThemeMode::Light,
                                    "浅色模式",
                                )
                                .clicked()
                            {
                                ctx.set_visuals(egui::Visuals {
                                    panel_fill: self.state.persistent.background_color,
                                    ..egui::Visuals::light()
                                });
                            }
                            if ui
                                .selectable_value(
                                    &mut self.state.persistent.theme_mode,
                                    ThemeMode::Dark,
                                    "深色模式",
                                )
                                .clicked()
                            {
                                ctx.set_visuals(egui::Visuals {
                                    panel_fill: self.state.persistent.background_color,
                                    ..egui::Visuals::dark()
                                });
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("启动时显示欢迎:");
                            ui.checkbox(
                                &mut self.state.persistent.show_welcome_window_on_start,
                                "",
                            );
                        });

                        #[cfg(feature = "startup_animation")]
                        ui.horizontal(|ui| {
                            ui.label("显示启动动画:");
                            ui.checkbox(&mut self.state.persistent.show_startup_animation, "");
                        });

                        ui.horizontal(|ui| {
                            ui.label("窗口透明度");
                            ui.add(egui::Slider::new(
                                &mut self.state.persistent.window_opacity,
                                0.0..=1.0,
                            ));
                        });
                    });

                    ui.collapsing("绘制", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("画布持久化:");
                            if ui.button("加载").clicked() {
                                load_canvas_from_file(&mut self.state);
                            }
                            if ui.button("保存").clicked() {
                                match self.state.canvas.save_to_file_with_dialog() {
                                    Ok(_) => {
                                        self.state.toasts.success("成功保存画布!");
                                    }
                                    Err(err) => {
                                        self.state.toasts.error(format!("画布保存失败: {}!", err));
                                    }
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("动态画笔宽度微调:");
                            ui.selectable_value(
                                &mut self.state.dynamic_brush_width_mode,
                                DynamicBrushWidthMode::Disabled,
                                "禁用",
                            );
                            ui.selectable_value(
                                &mut self.state.dynamic_brush_width_mode,
                                DynamicBrushWidthMode::BrushTip,
                                "模拟笔锋",
                            );
                            ui.selectable_value(
                                &mut self.state.dynamic_brush_width_mode,
                                DynamicBrushWidthMode::SpeedBased,
                                "基于速度",
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("笔迹平滑:");
                            ui.checkbox(&mut self.state.persistent.stroke_smoothing, "");
                        });

                        ui.horizontal(|ui| {
                            ui.label("直线停留拉直:");
                            ui.checkbox(&mut self.state.persistent.stroke_straightening, "启用");
                            if self.state.persistent.stroke_straightening {
                                ui.add(egui::Slider::new(
                                    &mut self.state.persistent.stroke_straightening_tolerance,
                                    1.0..=50.0,
                                ));
                                ui.label("灵敏度");
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("插值频率:");
                            ui.add(egui::Slider::new(
                                &mut self.state.persistent.interpolation_frequency,
                                0.0..=1.0,
                            ));
                        });

                        ui.horizontal(|ui| {
                            ui.label("低延迟模式:");
                            ui.checkbox(&mut self.state.persistent.low_latency_mode, "");
                        });

                        ui.horizontal(|ui| {
                            ui.label("编辑快捷颜色:");
                            if ui.button("OK").clicked() {
                                self.state.show_quick_color_editor = true;
                            }
                        });

                        // 快捷颜色编辑器窗口
                        if self.state.show_quick_color_editor {
                            let content_rect = ctx.content_rect();
                            let center_pos = content_rect.center();

                            egui::Window::new("编辑快捷颜色")
                                .collapsible(false)
                                .resizable(false)
                                .movable(false)
                                .pivot(egui::Align2::CENTER_CENTER)
                                .default_pos([center_pos.x, center_pos.y])
                                .show(ctx, |ui| {
                                    ui.label("当前快捷颜色:");
                                    ui.separator();

                                    // 显示当前快捷颜色列表
                                    let mut color_index_to_remove = None;
                                    for (index, color) in
                                        self.state.persistent.quick_colors.iter().enumerate()
                                    {
                                        ui.horizontal(|ui| {
                                            // 创建一个临时可变副本用于颜色编辑器
                                            let mut temp_color = *color;
                                            ui.color_edit_button_srgba(&mut temp_color);
                                            if ui.button("删除").clicked() {
                                                color_index_to_remove = Some(index);
                                            }
                                        });
                                    }

                                    // 处理删除操作
                                    if let Some(index) = color_index_to_remove {
                                        self.state.persistent.quick_colors.remove(index);
                                    }

                                    ui.separator();

                                    // 添加新颜色
                                    ui.horizontal(|ui| {
                                        ui.label("新颜色:");
                                        ui.color_edit_button_srgba(&mut self.state.new_quick_color);
                                        if ui.button("添加").clicked() {
                                            self.state
                                                .persistent
                                                .quick_colors
                                                .push(self.state.new_quick_color);
                                            self.state.new_quick_color = Color32::WHITE;
                                        }
                                    });

                                    ui.separator();

                                    ui.horizontal(|ui| {
                                        if ui.button("完成").clicked() {
                                            self.state.show_quick_color_editor = false;
                                        }
                                        if ui.button("重置").clicked() {
                                            self.state.show_quick_color_editor = false;
                                            // 重置为默认颜色
                                            self.state.persistent.quick_colors =
                                                utils::get_default_quick_colors();
                                        }
                                    });
                                });
                        }
                    });

                    ui.collapsing("性能", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("窗口模式:");
                            let old_mode = self.state.persistent.window_mode;
                            let mode_changed = ui
                                .selectable_value(
                                    &mut self.state.persistent.window_mode,
                                    WindowMode::Windowed,
                                    "窗口化",
                                )
                                .changed()
                                || ui
                                    .selectable_value(
                                        &mut self.state.persistent.window_mode,
                                        WindowMode::Fullscreen,
                                        "全屏",
                                    )
                                    .changed()
                                || ui
                                    .selectable_value(
                                        &mut self.state.persistent.window_mode,
                                        WindowMode::BorderlessFullscreen,
                                        "无边框全屏",
                                    )
                                    .changed();

                            if mode_changed && self.state.persistent.window_mode != old_mode {
                                self.state.window_mode_changed = true;
                            }
                        });

                        // 显示模式选择（仅在全屏模式下可用）
                        ui.horizontal(|ui| {
                            ui.label("显示模式:");

                            // 显示当前选择的视频模式
                            if self.state.persistent.window_mode == WindowMode::Fullscreen {
                                let mut current_selection =
                                    self.state.selected_video_mode_index.unwrap_or(0);

                                let mode = &self
                                    .state
                                    .available_video_modes
                                    .get(current_selection)
                                    .expect("no video mode available");

                                let mode_text = format!(
                                    "{}x{} @ {}Hz",
                                    mode.size().width,
                                    mode.size().height,
                                    mode.refresh_rate_millihertz() as f32 / 1000.0
                                );

                                egui::ComboBox::from_id_salt("video_mode_selection")
                                    .selected_text(mode_text)
                                    .show_ui(ui, |ui| {
                                        for (index, mode) in
                                            self.state.available_video_modes.iter().enumerate()
                                        {
                                            let mode_text = format!(
                                                "{}x{} @ {}Hz",
                                                mode.size().width,
                                                mode.size().height,
                                                mode.refresh_rate_millihertz() as f32 / 1000.0
                                            );
                                            if ui
                                                .selectable_value(
                                                    &mut current_selection,
                                                    index,
                                                    mode_text,
                                                )
                                                .changed()
                                            {
                                                // 更新选择
                                                self.state.selected_video_mode_index =
                                                    Some(current_selection);
                                                self.state.window_mode_changed = true;
                                            }
                                        }
                                    });
                            } else {
                                ui.label(egui::RichText::new("(仅在全屏模式下可切换)").italics());
                            }
                        });

                        // 垂直同步模式选择
                        ui.horizontal(|ui| {
                            ui.label("垂直同步:");
                            let old_present_mode = self.state.persistent.present_mode;
                            let present_mode_changed = ui
                                .selectable_value(
                                    &mut self.state.persistent.present_mode,
                                    PresentMode::AutoVsync,
                                    "开 (自动) | AutoVsync",
                                )
                                .changed()
                                || ui
                                    .selectable_value(
                                        &mut self.state.persistent.present_mode,
                                        PresentMode::AutoNoVsync,
                                        "关 (自动) | AutoNoVsync",
                                    )
                                    .changed()
                                || ui
                                    .selectable_value(
                                        &mut self.state.persistent.present_mode,
                                        PresentMode::Fifo,
                                        "开 | Fifo",
                                    )
                                    .changed()
                                || ui
                                    .selectable_value(
                                        &mut self.state.persistent.present_mode,
                                        PresentMode::FifoRelaxed,
                                        "自适应 | FifoRelaxed",
                                    )
                                    .changed()
                                || ui
                                    .selectable_value(
                                        &mut self.state.persistent.present_mode,
                                        PresentMode::Immediate,
                                        "关 | Immediate",
                                    )
                                    .changed()
                                || ui
                                    .selectable_value(
                                        &mut self.state.persistent.present_mode,
                                        PresentMode::Mailbox,
                                        "开 (快速) | Mailbox",
                                    )
                                    .changed();

                            if present_mode_changed
                                && self.state.persistent.present_mode != old_present_mode
                            {
                                self.state.present_mode_changed = true;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("优化策略 [需重启以应用]:");
                            ui.selectable_value(
                                &mut self.state.persistent.optimization_policy,
                                OptimizationPolicy::Performance,
                                "性能",
                            );
                            ui.selectable_value(
                                &mut self.state.persistent.optimization_policy,
                                OptimizationPolicy::ResourceUsage,
                                "资源用量",
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("强制每帧重绘:");
                            ui.checkbox(&mut self.state.persistent.force_redraw_every_frame, "");
                        });
                    });

                    ui.collapsing("调试", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("引发异常:");
                            if ui.button("OK").clicked() {
                                panic!("test panic")
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("显示 FPS:");
                            ui.checkbox(&mut self.state.persistent.show_fps, "");
                        });

                        ui.horizontal(|ui| {
                            ui.label("显示触控点:");
                            ui.checkbox(&mut self.state.show_touch_points, "");
                        });

                        #[cfg(target_os = "windows")]
                        {
                            ui.horizontal(|ui| {
                                ui.label("显示终端 [仅 Windows]:");
                                let old_show_console = self.state.show_console;
                                if ui.checkbox(&mut self.state.show_console, "").changed() {
                                    use windows::Win32::System::Console::AllocConsole;
                                    use windows::Win32::System::Console::FreeConsole;

                                    if self.state.show_console && !old_show_console {
                                        // 启用控制台
                                        unsafe {
                                            let _ = AllocConsole();
                                        }
                                    } else if !self.state.show_console && old_show_console {
                                        // 禁用控制台
                                        unsafe {
                                            let _ = FreeConsole();
                                        }
                                    }
                                }
                            });
                        }

                        ui.horizontal(|ui| {
                            ui.label("压力测试:");
                            if ui.button("OK").clicked() {
                                // 使用固定颜色和宽度
                                const STRESS_COLOR: Color32 = Color32::from_rgb(255, 0, 0); // 红色
                                const STRESS_WIDTH: f32 = 3.0;

                                // 添加 1000 条笔画
                                for i in 0..1000 {
                                    let mut points = Vec::new();

                                    const NUM_POINTS: i32 = 100;

                                    // 生成笔画位置
                                    let start_x = (i as f32 % 20.0) * 50.0;
                                    let start_y = ((i as f32 / 20.0).floor() % 15.0) * 50.0;

                                    // 生成笔画方向和长度
                                    for j in 0..NUM_POINTS {
                                        let x = start_x + (j as f32 * 10.0);
                                        let y = start_y + (j as f32 * 5.0);

                                        points.push(Pos2::new(x, y));
                                    }

                                    // 创建笔画对象
                                    let stroke = CanvasStroke {
                                        points,
                                        width: STRESS_WIDTH.into(),
                                        color: STRESS_COLOR,
                                        base_width: STRESS_WIDTH,
                                        rot: 0.0,
                                    };

                                    self.state.canvas.objects.push(CanvasObject::Stroke(stroke));
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("立即保存设置:");
                            if ui.button("OK").clicked() {
                                if let Err(err) = self.state.persistent.save_to_file() {
                                    self.state.toasts.error(format!("设置保存失败: {}!", err));
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("重置设置:");
                            if ui.button("OK").clicked() {
                                self.state.persistent = PersistentState::default();
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("???:");
                            ui.checkbox(&mut self.state.persistent.easter_egg_redo, "");
                        });
                    });
                }

                ui.separator();

                // 历史记录操作
                ui.horizontal(|ui| {
                    ui.label("历史记录:");
                    if ui.button("撤销").clicked() {
                        if self.state.history.undo(&mut self.state.canvas) {
                            self.state.toasts.success("成功撤销操作!");
                        } else {
                            self.state.toasts.error("无法撤销，没有更多历史记录!");
                        }
                    }
                    if ui
                        .button(if !self.state.persistent.easter_egg_redo {
                            "重做"
                        } else {
                            "Redo!"
                        })
                        .clicked()
                    {
                        if self.state.history.redo(&mut self.state.canvas) {
                            self.state.toasts.success("成功重做操作!");
                        } else {
                            self.state.toasts.error("无法重做，没有更多历史记录!");
                        }
                    }
                });

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("退出").clicked() {
                        self.state.should_quit = true;
                    }

                    if ui.button("最小化").clicked() {
                        self.window
                            .as_ref()
                            .expect("no window??")
                            .set_visible(false);
                    }

                    if self.state.persistent.show_fps {
                        ui.label(format!("FPS: {}", self.state.fps_counter.current_fps));
                    }
                });
            });

        {
            let content_rect = ctx.content_rect();
            let margin = 8.0;
            let total_pages = self.state.pages.len();
            let current = self.state.current_page;
            let enabled = !self.state.show_welcome_window && {
                #[cfg(feature = "startup_animation")]
                if let Some(anim) = &self.state.startup_animation {
                    anim.is_finished()
                } else {
                    true
                }
                #[cfg(not(feature = "startup_animation"))]
                true
            };

            if enabled {
                let mut action = PageAction::None;

                let build_page_nav = |ui: &mut egui::Ui, action: &mut PageAction| {
                    let btn_style = |text: &str| {
                        egui::Button::new(egui::RichText::new(text).size(20.0))
                            .min_size(egui::vec2(36.0, 28.0))
                    };
                    ui.horizontal(|ui| {
                        if ui.add_enabled(current > 0, btn_style("<")).clicked() {
                            *action = PageAction::Previous;
                        }

                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("{}/{}", current + 1, total_pages))
                                    .size(20.0),
                            )
                            // although most of the labels in this apps should be unselectable, i'm too lazy to do that,
                            // so i'm only applying that to page indicator which is frequently clicked on
                            .selectable(false),
                        );

                        if ui.add_enabled(total_pages > 1, btn_style("X")).clicked() {
                            *action = PageAction::Delete;
                        }

                        let is_last = current == total_pages - 1;
                        if is_last {
                            if ui.add(btn_style("+")).clicked() {
                                *action = PageAction::New;
                            }
                        } else if ui.add(btn_style(">")).clicked() {
                            *action = PageAction::Next;
                        }
                    });
                };

                // left-bottom window
                egui::Window::new("##page_nav_left")
                    .resizable(false)
                    .collapsible(false)
                    .movable(false)
                    .title_bar(false)
                    .pivot(egui::Align2::LEFT_BOTTOM)
                    .current_pos(Pos2::new(
                        content_rect.min.x + margin,
                        content_rect.max.y - margin,
                    ))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        let mut a = PageAction::None;
                        build_page_nav(ui, &mut a);
                        if !matches!(a, PageAction::None) {
                            action = a;
                        }
                    });

                // right-bottom window
                egui::Window::new("##page_nav_right")
                    .resizable(false)
                    .collapsible(false)
                    .movable(false)
                    .title_bar(false)
                    .pivot(egui::Align2::RIGHT_BOTTOM)
                    .current_pos(Pos2::new(
                        content_rect.max.x - margin,
                        content_rect.max.y - margin,
                    ))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        let mut a = PageAction::None;
                        build_page_nav(ui, &mut a);
                        if !matches!(a, PageAction::None) {
                            action = a;
                        }
                    });

                apply_page_action(&mut self.state, action);
            }
        }

        // 主画布区域
        #[allow(deprecated)] // very complicated; since it works, i'm not going to fix it
        egui::CentralPanel::default().show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(
                ui.available_size(),
                if self.state.persistent.low_latency_mode {
                    egui::Sense::drag()
                } else {
                    egui::Sense::click_and_drag()
                },
            );

            let painter = ui.painter();

            // 绘制所有对象
            for (i, object) in self.state.canvas.objects.iter().enumerate() {
                let selected = self.state.selected_object == Some(i);
                object.paint(painter, selected);
            }

            // 绘制当前正在绘制的笔画
            // TODO: unify with CanvasStroke::paint()
            for active_stroke in self.state.active_strokes.values() {
                if let StrokeWidth::Dynamic(v) = &active_stroke.width {
                    if v.len() != active_stroke.points.len() {
                        continue;
                    }
                }
                painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                    active_stroke.points[0],
                    active_stroke.width.first() / 2.0,
                    self.state.brush_color,
                )));
                if active_stroke.points.len() >= 2 {
                    painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                        active_stroke.points[active_stroke.points.len() - 1],
                        active_stroke.width.last() / 2.0,
                        self.state.brush_color,
                    )));
                    for i in 0..active_stroke.points.len() - 1 {
                        let avg_width =
                            (active_stroke.width.get(i) + active_stroke.width.get(i + 1)) / 2.0;
                        painter.line_segment(
                            [active_stroke.points[i], active_stroke.points[i + 1]],
                            Stroke::new(avg_width, self.state.brush_color),
                        );
                    }
                }
            }

            // 绘制大小预览圆圈
            if self.state.show_size_preview {
                let content_rect = ui.ctx().content_rect();
                let pos = content_rect.center();
                utils::draw_size_preview(
                    painter,
                    pos,
                    match self.state.current_tool {
                        CanvasTool::Brush => self.state.brush_width,
                        CanvasTool::ObjectEraser | CanvasTool::PixelEraser => {
                            self.state.eraser_size
                        }
                        _ => unreachable!("should not happen"),
                    },
                );
            }

            // 绘制触控点
            if self.state.show_touch_points {
                for (id, pos) in &self.state.touch_points {
                    painter.circle_filled(
                        *pos,
                        15.0,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 180),
                    );
                    painter.circle_stroke(*pos, 15.0, Stroke::new(2.0_f32, Color32::BLUE));

                    // 绘制触控 ID
                    let text_galley = painter.layout_no_wrap(
                        format!("{}", id),
                        egui::FontId::proportional(14.0),
                        Color32::BLACK,
                    );
                    let text_pos = Pos2::new(
                        pos.x - text_galley.size().x / 2.0,
                        pos.y - text_galley.size().y / 2.0,
                    );
                    let text_shape = egui::epaint::TextShape {
                        pos: text_pos,
                        galley: text_galley,
                        underline: egui::Stroke::NONE,
                        override_text_color: None,
                        angle: 0.0,
                        fallback_color: Color32::BLACK,
                        opacity_factor: 1.0,
                    };
                    painter.add(text_shape);
                }
            }

            // 处理鼠标输入
            let pointer_pos = response.interact_pointer_pos();

            match self.state.current_tool {
                CanvasTool::Insert | CanvasTool::Settings => {}

                CanvasTool::Select => {
                    // Select tool: click to select objects, drag to move/resize selected object

                    // Handle click: iterate through objects from last to first, check bounding boxes
                    if response.clicked() {
                        if let Some(click_pos) = pointer_pos {
                            // Iterate from last to first (top to bottom in z-order)
                            let mut found_selection = false;
                            for (i, object) in self.state.canvas.objects.iter().enumerate().rev() {
                                if object.bounding_box().contains(click_pos) {
                                    self.state.selected_object = Some(i);
                                    found_selection = true;
                                    break;
                                }
                            }

                            // If no object was clicked, deselect
                            if !found_selection {
                                self.state.selected_object = None;
                            }
                        }
                    }

                    // Handle drag start: record the drag start position and check for resize handles
                    if response.drag_started() {
                        if let Some(pos) = pointer_pos {
                            self.state.drag_start_pos = Some(pos);
                            self.state.dragged_handle = None;
                            self.state.drag_move_accumulated_delta = egui::Vec2::ZERO;

                            // Check if we're dragging a resize handle
                            if let Some(selected_idx) = self.state.selected_object {
                                if let Some(object) = self.state.canvas.objects.get(selected_idx) {
                                    let bbox = object.bounding_box();
                                    if let Some(handle) =
                                        utils::get_transform_handle_at_pos(bbox, pos)
                                    {
                                        self.state.dragged_handle = Some(handle);
                                        self.state.drag_original_transform =
                                            Some(object.get_transform());
                                    }
                                }
                            }
                        }
                    }

                    // Handle dragging: move or resize the selected object
                    if response.dragged() && self.state.selected_object.is_some() {
                        if let (Some(drag_start), Some(current_pos)) =
                            (self.state.drag_start_pos, pointer_pos)
                        {
                            let delta = current_pos - drag_start;

                            if let Some(selected_idx) = self.state.selected_object {
                                if let Some(dragged_handle) = self.state.dragged_handle {
                                    // Resize operation — history saved on drag_stopped
                                    if let Some(object) =
                                        self.state.canvas.objects.get_mut(selected_idx)
                                    {
                                        object.transform(
                                            dragged_handle,
                                            delta,
                                            drag_start,
                                            current_pos,
                                        );
                                    }
                                } else {
                                    // Move operation
                                    if let Some(object) =
                                        self.state.canvas.objects.get_mut(selected_idx)
                                    {
                                        CanvasObject::move_object(object, delta);
                                    }
                                    self.state.drag_move_accumulated_delta += delta;
                                }
                            }

                            // Update drag start position for continuous dragging
                            self.state.drag_start_pos = Some(current_pos);
                        }
                    }

                    // Handle drag stop: save move/resize to history and clear state
                    if response.drag_stopped() {
                        if self.state.drag_move_accumulated_delta != egui::Vec2::ZERO {
                            if let Some(selected_idx) = self.state.selected_object {
                                self.state.history.save_move_object(
                                    selected_idx,
                                    -self.state.drag_move_accumulated_delta,
                                    self.state.drag_move_accumulated_delta,
                                );
                            }
                        } else if let Some(original_transform) =
                            self.state.drag_original_transform.take()
                        {
                            if let Some(selected_idx) = self.state.selected_object {
                                if let Some(object) = self.state.canvas.objects.get(selected_idx) {
                                    let new_transform = object.get_transform();
                                    self.state.history.save_transform_object(
                                        selected_idx,
                                        original_transform,
                                        new_transform,
                                    );
                                }
                            }
                        }
                        self.state.drag_start_pos = None;
                        self.state.dragged_handle = None;
                    }
                }

                CanvasTool::ObjectEraser => {
                    // 对象橡皮擦：点击或拖拽时删除相交的整个对象
                    if response.drag_started() || response.clicked() || response.dragged() {
                        if let Some(pos) = pointer_pos {
                            // 绘制指针
                            utils::draw_size_preview(painter, pos, self.state.eraser_size);

                            // 从后往前删除，避免索引问题
                            let mut to_remove = Vec::new();

                            // 检查所有对象
                            for (i, object) in self.state.canvas.objects.iter().enumerate().rev() {
                                match object {
                                    CanvasObject::Image(img) => {
                                        let img_rect = egui::Rect::from_min_size(img.pos, img.size);
                                        if img_rect.contains(pos) {
                                            to_remove.push(i);
                                        }
                                    }
                                    CanvasObject::Text(text) => {
                                        let text_galley = painter.layout_no_wrap(
                                            text.text.clone(),
                                            egui::FontId::proportional(text.font_size),
                                            text.color,
                                        );
                                        let text_size = text_galley.size();
                                        let text_rect =
                                            egui::Rect::from_min_size(text.pos, text_size);
                                        if text_rect.contains(pos) {
                                            to_remove.push(i);
                                        }
                                    }
                                    CanvasObject::Shape(shape) => {
                                        let shape_rect = shape.bounding_box();
                                        if shape_rect.contains(pos) {
                                            to_remove.push(i);
                                        }
                                    }
                                    CanvasObject::Stroke(stroke) => {
                                        if utils::point_intersects_stroke(
                                            pos,
                                            stroke,
                                            self.state.eraser_size,
                                        ) {
                                            to_remove.push(i);
                                        }
                                    }
                                }
                            }

                            // 删除对象，逐个记录到历史
                            for i in to_remove {
                                let object = self.state.canvas.objects.remove(i);
                                self.state.history.save_remove_object(i, object);
                            }
                        }
                    }
                }

                CanvasTool::PixelEraser => {
                    // 像素橡皮擦：从笔画中移除被擦除的段落，并将笔画分割为多个部分
                    if response.dragged() || response.clicked() {
                        if let Some(pos) = pointer_pos {
                            // 绘制指针
                            utils::draw_size_preview(painter, pos, self.state.eraser_size);

                            let eraser_radius = self.state.eraser_size / 2.0;
                            let eraser_rect = egui::Rect::from_center_size(
                                pos,
                                egui::vec2(self.state.eraser_size, self.state.eraser_size),
                            );

                            let mut new_strokes = Vec::new();
                            let mut strokes_modified = false;

                            for object in &self.state.canvas.objects {
                                if let CanvasObject::Stroke(stroke) = object {
                                    if stroke.points.len() < 2 {
                                        new_strokes.push(stroke.clone());
                                        continue;
                                    }

                                    // Quick bounding box filter — skip strokes far from eraser
                                    if !stroke.bounding_box().intersects(eraser_rect) {
                                        new_strokes.push(stroke.clone());
                                        continue;
                                    }

                                    strokes_modified = true;

                                    let mut current_points = Vec::new();
                                    let mut current_widths = Vec::new();

                                    current_points.push(stroke.points[0]);
                                    current_widths.push(stroke.width.first());

                                    for i in 0..stroke.points.len() - 1 {
                                        let p1 = stroke.points[i];
                                        let p2 = stroke.points[i + 1];
                                        let segment_width = stroke.width.get(i);

                                        let dist =
                                            utils::point_to_line_segment_distance(pos, p1, p2);

                                        if dist > eraser_radius + segment_width / 2.0 {
                                            current_points.push(p2);
                                            current_widths.push(stroke.width.get(i + 1));
                                        } else {
                                            if current_points.len() >= 2 {
                                                new_strokes.push(CanvasStroke {
                                                    points: current_points.clone(),
                                                    width: current_widths.clone().into(),
                                                    color: stroke.color,
                                                    base_width: stroke.base_width,
                                                    rot: 0.0,
                                                });
                                            }
                                            current_points = Vec::new();
                                            current_widths = Vec::new();
                                        }
                                    }

                                    if current_points.len() >= 2 {
                                        new_strokes.push(CanvasStroke {
                                            points: current_points,
                                            width: current_widths.into(),
                                            color: stroke.color,
                                            base_width: stroke.base_width,
                                            rot: 0.0,
                                        });
                                    }
                                }
                            }

                            if strokes_modified {
                                let original_stroke_count = self
                                    .state
                                    .canvas
                                    .objects
                                    .iter()
                                    .filter(|obj| matches!(obj, CanvasObject::Stroke(_)))
                                    .count();
                                let new_stroke_count = new_strokes.len();
                                if original_stroke_count != new_stroke_count {
                                    let non_strokes: Vec<_> = self
                                        .state
                                        .canvas
                                        .objects
                                        .iter()
                                        .filter(|obj| !matches!(obj, CanvasObject::Stroke(_)))
                                        .cloned()
                                        .collect();
                                    let old_objects =
                                        std::mem::take(&mut self.state.canvas.objects);
                                    self.state.history.save_clear_objects(old_objects);
                                    self.state.canvas.objects = non_strokes;
                                } else {
                                    self.state
                                        .canvas
                                        .objects
                                        .retain(|obj| !matches!(obj, CanvasObject::Stroke(_)));
                                }

                                for stroke in new_strokes {
                                    self.state.canvas.objects.push(CanvasObject::Stroke(stroke));
                                }
                            }
                        }
                    }
                }

                CanvasTool::Brush => {
                    // 画笔工具
                    if response.drag_started() {
                        if let Some(pos) = pointer_pos
                            && pos.x >= rect.min.x
                            && pos.x <= rect.max.x
                            && pos.y >= rect.min.y
                            && pos.y <= rect.max.y
                        {
                            self.state.is_drawing = true;
                            brush_stroke_start(&mut self.state, 0, pos);
                        }
                    } else if response.dragged() {
                        if self.state.is_drawing
                            && let Some(pos) = pointer_pos
                        {
                            brush_stroke_add_point(&mut self.state, 0, pos, false);
                        }
                    } else if response.drag_stopped() {
                        if self.state.is_drawing {
                            brush_stroke_end(&mut self.state, 0);
                        }
                    } else if response.clicked() {
                        // 处理单击事件 - 绘制单个点
                        if let Some(pos) = pointer_pos
                            && pos.x >= rect.min.x
                            && pos.x <= rect.max.x
                            && pos.y >= rect.min.y
                            && pos.y <= rect.max.y
                        {
                            // Save state to history before modification
                            let new_stroke = CanvasStroke {
                                points: vec![pos],
                                width: StrokeWidth::Fixed(self.state.brush_width),
                                color: self.state.brush_color,
                                base_width: self.state.brush_width,
                                rot: 0.0,
                            };
                            let index = self.state.canvas.objects.len();
                            self.state
                                .history
                                .save_add_object(index, CanvasObject::Stroke(new_stroke.clone()));
                            self.state
                                .canvas
                                .objects
                                .push(CanvasObject::Stroke(new_stroke));
                        }
                    }

                    // 如果鼠标在画布内移动且正在绘制，也添加点（用于平滑绘制）
                    if response.hovered()
                        && self.state.is_drawing
                        && let Some(pos) = pointer_pos
                    {
                        brush_stroke_add_point(&mut self.state, 0, pos, true);
                    }
                }
            }
        });

        render_state.egui_renderer.end_frame_and_draw(
            &render_state.device,
            &render_state.queue,
            &mut encoder,
            window,
            &surface_view,
            screen_descriptor,
        );

        render_state.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        // 清理已标记为删除的图片（在帧结束时安全删除）
        self.state.canvas.objects.retain(|obj| {
            if let CanvasObject::Image(img) = obj {
                !img.marked_for_deletion
            } else {
                true
            }
        });

        // 如果启用了 FPS 显示，更新 FPS
        if self.state.persistent.show_fps {
            _ = self.state.fps_counter.update();
        }

        // 应用窗口模式更改
        if self.state.window_mode_changed
            && let Some(window) = self.window.as_ref()
        {
            self.apply_window_mode(window);
            self.state.window_mode_changed = false;
        }

        // 应用垂直同步模式更改
        if self.state.present_mode_changed {
            self.apply_present_mode();
            self.state.present_mode_changed = false;
        }
    }
}

fn brush_stroke_start(state: &mut AppState, touch_id: u64, pos: Pos2) {
    let start_time = Instant::now();
    let width = utils::calculate_dynamic_width(
        state.brush_width,
        state.dynamic_brush_width_mode,
        0,
        1,
        None,
    );
    state.active_strokes.insert(
        touch_id,
        ActiveStroke {
            points: vec![pos],
            width,
            times: vec![0.0],
            start_time,
            last_movement_time: start_time,
        },
    );
}

fn brush_stroke_add_point(
    state: &mut AppState,
    touch_id: u64,
    pos: Pos2,
    apply_straightening: bool,
) {
    if let Some(active_stroke) = state.active_strokes.get_mut(&touch_id) {
        let current_time = active_stroke.start_time.elapsed().as_secs_f64();

        if apply_straightening && state.persistent.stroke_straightening {
            let time_since_last_movement = active_stroke.last_movement_time.elapsed().as_secs_f32();
            if time_since_last_movement > 0.5 {
                let straightened_points = utils::straighten_stroke(
                    &active_stroke.points,
                    state.persistent.stroke_straightening_tolerance,
                );
                if straightened_points.len() != active_stroke.points.len() {
                    let has_dynamic_mode =
                        state.dynamic_brush_width_mode != DynamicBrushWidthMode::Disabled;
                    active_stroke.points = straightened_points;
                    if let StrokeWidth::Dynamic(v) = &active_stroke.width {
                        if !v.is_empty() {
                            let first_width = v[0];
                            let last_width = *v.last().unwrap();
                            active_stroke.width =
                                if active_stroke.points.len() == 1 && !has_dynamic_mode {
                                    StrokeWidth::Fixed(first_width)
                                } else {
                                    StrokeWidth::Dynamic(vec![first_width, last_width])
                                };
                        }
                    }
                }
                active_stroke.last_movement_time = Instant::now();
            }
        }

        if active_stroke.points.is_empty()
            || active_stroke.points.last().unwrap().distance(pos) > 1.0
        {
            let speed = if !active_stroke.points.is_empty() && !active_stroke.times.is_empty() {
                let last_time = active_stroke.times.last().unwrap();
                let time_delta = ((current_time - last_time) as f32).max(0.001);
                let distance = active_stroke.points.last().unwrap().distance(pos);
                Some(distance / time_delta)
            } else {
                None
            };

            active_stroke.points.push(pos);
            active_stroke.times.push(current_time);

            if state.dynamic_brush_width_mode != DynamicBrushWidthMode::Disabled {
                let stroke_width = utils::calculate_dynamic_width(
                    state.brush_width,
                    state.dynamic_brush_width_mode,
                    active_stroke.points.len() - 1,
                    active_stroke.points.len(),
                    speed,
                );
                active_stroke.width.push(stroke_width.first());
            }

            active_stroke.last_movement_time = Instant::now();
        }
    }
}

fn brush_stroke_end(state: &mut AppState, touch_id: u64) {
    if let Some(active_stroke) = state.active_strokes.remove(&touch_id) {
        if let StrokeWidth::Dynamic(v) = &active_stroke.width {
            if v.len() != active_stroke.points.len() {
                return;
            }
        }

        let mut final_points = if state.persistent.stroke_smoothing {
            utils::apply_stroke_smoothing(&active_stroke.points)
        } else {
            active_stroke.points
        };

        let width = utils::apply_point_interpolation_in_place(
            &mut final_points,
            &active_stroke.width,
            state.persistent.interpolation_frequency,
        );

        let new_stroke = CanvasStroke {
            points: final_points,
            width,
            color: state.brush_color,
            base_width: state.brush_width,
            rot: 0.0,
        };
        let index = state.canvas.objects.len();
        state
            .history
            .save_add_object(index, CanvasObject::Stroke(new_stroke.clone()));
        state.canvas.objects.push(CanvasObject::Stroke(new_stroke));
    }
    state.is_drawing = !state.active_strokes.is_empty();
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();
        pollster::block_on(self.set_window(window));
        // Update UI reactively
        self.window.as_ref().unwrap().request_redraw();
    }

    // Update UI reactively
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.state.should_quit {
            return;
        }

        if let Some(render_state) = self.render_state.as_ref() {
            if render_state.egui_renderer.context().has_requested_repaint() {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::TrayIconEvent(event) => {
                if let tray_icon::TrayIconEvent::Click { .. } = event {
                    let window = self.window.as_ref().expect("no window??");
                    window.set_visible(true);
                    window.focus_window();
                    if let Some(tray) = &self.state.tray {
                        let _ = tray.set_visible(false);
                    }
                    // Update UI reactively
                    window.request_redraw();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        // 检查是否需要退出
        if self.state.should_quit {
            println!("quit button was pressed; exiting");
            self.exit(event_loop);
            return;
        }

        // Update UI reactively
        // Don't pass RedrawRequested to egui's input handler,
        // it's not input and would make egui request a repaint, causing an infinite loop
        if !self.state.persistent.force_redraw_every_frame {
            if !matches!(event, WindowEvent::RedrawRequested) {
                let egui_needs_repaint = self
                    .render_state
                    .as_mut()
                    .unwrap()
                    .egui_renderer
                    .handle_input(self.window.as_ref().unwrap(), &event);

                if egui_needs_repaint {
                    self.window.as_ref().unwrap().request_redraw();
                }
            }
        } else {
            self.render_state
                .as_mut()
                .unwrap()
                .egui_renderer
                .handle_input(self.window.as_ref().unwrap(), &event);
            self.window.as_ref().unwrap().request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => {
                self.exit(event_loop);
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
                self.exit(event_loop);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
            }
            WindowEvent::Resized(new_size) => {
                self.handle_resized(new_size.width, new_size.height);
                // Update UI reactively
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::Touch(Touch {
                phase,
                location,
                id,
                ..
            }) => {
                // Convert touch location to logical coordinates
                let window = self.window.as_ref().unwrap();
                let scale_factor = window.scale_factor() as f32;
                let logical_pos = Pos2::new(
                    location.x as f32 / scale_factor,
                    location.y as f32 / scale_factor,
                );

                // Store touch point in state for rendering
                match phase {
                    TouchPhase::Started => {
                        self.state.touch_points.insert(id, logical_pos);

                        // 如果当前工具是画笔，开始新的笔画
                        if self.state.current_tool == CanvasTool::Brush {
                            self.state.is_drawing = true;
                            brush_stroke_start(&mut self.state, id, logical_pos);
                        }
                    }
                    TouchPhase::Moved => {
                        self.state.touch_points.insert(id, logical_pos);

                        // 如果当前工具是画笔，继续绘制
                        if self.state.current_tool == CanvasTool::Brush {
                            brush_stroke_add_point(&mut self.state, id, logical_pos, false);
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.state.touch_points.remove(&id);

                        // 如果当前工具是画笔，结束笔画
                        if self.state.current_tool == CanvasTool::Brush {
                            brush_stroke_end(&mut self.state, id);
                        }
                    }
                }

                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}
