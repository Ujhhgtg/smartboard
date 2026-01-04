use crate::render::RenderState;
use crate::state::{
    AppState, CanvasImage, CanvasObject, CanvasObjectOps, CanvasShape, CanvasShapeType,
    CanvasState, CanvasText, CanvasTool, DynamicBrushWidthMode, FONT, ICON, OptimizationPolicy,
    PersistentState, StartupAnimation, ThemeMode, WindowMode,
};
use crate::{UserEvent, utils};
use core::f32;
use egui::{Color32, Pos2, Shape, Stroke};
use egui_wgpu::wgpu::SurfaceError;
use egui_wgpu::{ScreenDescriptor, wgpu, wgpu::PresentMode};
use image::GenericImageView;
use std::sync::Arc;
use std::time::Instant;
use tray_icon::TrayIconBuilder;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, Touch, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::WindowExtWindows;
use winit::window::{Fullscreen, Window, WindowId};

// 启动动画
include!(concat!(env!("OUT_DIR"), "/startup_frames.rs"));
pub const STARTUP_AUDIO: &[u8] = include_bytes!("../assets/startup_animation/audio.wav");

pub struct App {
    gpu_instance: wgpu::Instance,
    render_state: Option<RenderState>,
    window: Option<Arc<Window>>,
    state: AppState,
    // start: Instant,

    // bg_tex: Option<egui::TextureId>,
    // logo_tex: Option<egui::TextureId>,
    // button_tex: Option<egui::TextureId>,
}

impl App {
    pub fn new() -> Self {
        let gpu_instance = egui_wgpu::wgpu::Instance::default();
        let mut state = AppState::default();

        // init
        if !state.persistent.show_welcome_window_on_start {
            state.show_welcome_window = false
        }
        if state.persistent.show_startup_animation {
            state.startup_animation =
                Some(StartupAnimation::new(30.0, STARTUP_FRAMES, STARTUP_AUDIO));
        }

        Self {
            gpu_instance,
            render_state: None,
            window: None,
            state,
            // start: Instant::now(),
            // bg_tex: None,
            // logo_tex: None,
            // button_tex: None,
        }
    }

    // fn load_embedded_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureId {
    //     let image = image::load_from_memory(bytes)
    //         .expect("invalid image")
    //         .to_rgba8();

    //     let size = [image.width() as usize, image.height() as usize];
    //     let pixels = image
    //         .pixels()
    //         .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
    //         .collect();

    //     ctx.load_texture(
    //         name,
    //         egui::ColorImage {
    //             size,
    //             pixels,
    //             source_size: Vec2 {
    //                 x: image.width() as f32,
    //                 y: image.height() as f32,
    //             },
    //         },
    //         egui::TextureOptions::LINEAR,
    //     )
    //     .id()
    // }

    pub async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);

        let image = image::load_from_memory(ICON).expect("invalid icon data");

        let rgba = image.to_rgba8().to_vec();
        let (width, height) = image.dimensions();

        // 设置标题
        window.set_title("smartboard");
        let winit_icon = Some(
            winit::window::Icon::from_rgba(rgba.clone(), width, height).expect("invalid icon data"),
        );
        window.set_window_icon(winit_icon.clone());
        window.set_taskbar_icon(winit_icon);

        // 获取显示模式
        self.state.available_video_modes = window
            .current_monitor()
            .expect("no monitor found")
            .video_modes()
            .collect();

        // 设置窗口模式
        self.apply_window_mode(&window);

        // 创建托盘图标
        let tray = TrayIconBuilder::new()
            .with_icon(tray_icon::Icon::from_rgba(rgba, width, height).expect("invalid icon data"))
            .with_tooltip("smartboard")
            .build()
            .unwrap();
        std::mem::forget(tray);

        // 获取窗口尺寸
        let size = window.inner_size();
        let initial_width = size.width;
        let initial_height = size.height;

        let surface = self
            .gpu_instance
            .create_surface(window.clone())
            .expect("Failed to create surface");

        let state = RenderState::new(
            &self.gpu_instance,
            surface,
            &window,
            initial_width,
            initial_height,
            self.state.persistent.optimization_policy,
        )
        .await;

        self.window.get_or_insert(window);
        self.render_state.get_or_insert(state);
    }

    fn exit(&self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.state.persistent.save_to_file() {
            eprintln!("Failed to save settings: {}", err)
        }
        event_loop.exit();
    }

    fn apply_window_mode(&self, window: &Arc<Window>) {
        match self.state.persistent.window_mode {
            WindowMode::Windowed => {
                // 窗口模式
                window.set_fullscreen(None);
            }
            WindowMode::Fullscreen => {
                // 全屏模式
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
                        .get(0)
                        .expect("no video mode available")
                        .clone(),
                )));
            }
            WindowMode::BorderlessFullscreen => {
                // 无边框全屏模式
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

    // Convert text to strokes
    // pub fn rasterize_text_to_strokes(text: &CanvasText) -> Vec<CanvasStroke> {
    //     let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default()).unwrap();

    //     // let mut strokes = Vec::new();

    //     // let mut cursor_x = 0.0;

    //     // for ch in text.text.chars() {
    //     //     let (metrics, bitmap) = font.rasterize(ch, text.font_size);

    //     //     let width = metrics.width as usize;
    //     //     let height = metrics.height as usize;

    //     //     for y in 0..height {
    //     //         let mut points = Vec::new();
    //     //         let mut widths = Vec::new();

    //     //         for x in 0..width {
    //     //             let alpha = bitmap[x + y * width];

    //     //             if alpha > 0 {
    //     //                 let px = text.pos.x + cursor_x + (x as f32 + metrics.xmin as f32);
    //     //                 let py = text.pos.y + (y as f32 + metrics.ymin as f32);

    //     //                 points.push(Pos2::new(px, py));
    //     //                 widths.push(alpha as f32 / 255.0);
    //     //             } else if !points.is_empty() {
    //     //                 strokes.push(CanvasStroke {
    //     //                     points,
    //     //                     widths,
    //     //                     color: text.color,
    //     //                     base_width: text.font_size,
    //     //                 });
    //     //                 points = Vec::new();
    //     //                 widths = Vec::new();
    //     //             }
    //     //         }

    //     //         if !points.is_empty() {
    //     //             strokes.push(CanvasStroke {
    //     //                 points,
    //     //                 widths,
    //     //                 color: text.color,
    //     //                 base_width: text.font_size,
    //     //             });
    //     //         }
    //     //     }

    //     //     cursor_x += metrics.advance_width;
    //     // }

    //     // strokes

    //     let mut strokes = Vec::with_capacity(text.text.len() * 4);
    //     let mut cursor_x = 0.0;
    //     let inv_255 = 1.0 / 255.0;

    //     let mut points = Vec::with_capacity(32);
    //     let mut widths = Vec::with_capacity(32);

    //     for ch in text.text.chars() {
    //         let (metrics, bitmap) = font.rasterize(ch, text.font_size);

    //         if metrics.width == 0 || metrics.height == 0 {
    //             cursor_x += metrics.advance_width;
    //             continue;
    //         }

    //         let width = metrics.width as usize;
    //         let height = metrics.height as usize;

    //         let base_x = text.pos.x + cursor_x + metrics.xmin as f32;
    //         let base_y = text.pos.y + metrics.ymin as f32;

    //         for y in 0..height {
    //             let row_start = y * width;
    //             let row = &bitmap[row_start..row_start + width];

    //             if row.iter().all(|&a| a == 0) {
    //                 continue;
    //             }

    //             points.clear();
    //             widths.clear();

    //             for (x, &alpha) in row.iter().enumerate() {
    //                 if alpha > 0 {
    //                     points.push(Pos2::new(base_x + x as f32, base_y + y as f32));
    //                     widths.push(alpha as f32 * inv_255);
    //                 } else if points.len() > 1 {
    //                     strokes.push(CanvasStroke {
    //                         points: std::mem::take(&mut points),
    //                         widths: std::mem::take(&mut widths),
    //                         color: text.color,
    //                         base_width: text.font_size,
    //                     });
    //                     points.clear();
    //                     widths.clear();
    //                 }
    //             }

    //             if points.len() > 1 {
    //                 strokes.push(CanvasStroke {
    //                     points: std::mem::take(&mut points),
    //                     widths: std::mem::take(&mut widths),
    //                     color: text.color,
    //                     base_width: text.font_size,
    //                 });
    //             }
    //         }

    //         cursor_x += metrics.advance_width;
    //     }

    //     strokes
    // }

    fn handle_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.render_state
                .as_mut()
                .unwrap()
                .resize_surface(width, height);
        }
    }

    fn handle_redraw(&mut self) {
        // if let Some(window) = self.window.as_ref() {
        //     if let Some(min) = window.is_minimized() {
        //         if min {
        //             println!("Window is minimized");
        //             return;
        //         }
        //     }
        // }

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

        match surface_texture {
            Err(SurfaceError::Lost) => {
                println!("wgpu surface lost");
                return;
            }
            Err(SurfaceError::Outdated) => {
                // Ignoring outdated to allow resizing and minimization
                println!("wgpu surface outdated");
                return;
            }
            Err(SurfaceError::Timeout) => {
                println!("wgpu surface timeout");
                return;
            }
            Err(SurfaceError::OutOfMemory) => {
                panic!("out of memory");
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

        let mut encoder = render_state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let window = self.window.as_ref().unwrap();

        render_state.egui_renderer.begin_frame(window);
        let ctx = render_state.egui_renderer.context();

        // 应用主题设置
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

        if let Some(anim) = &mut self.state.startup_animation {
            if !anim.is_finished() {
                anim.update(ctx);
                anim.draw_fullscreen(ctx);
                ctx.request_repaint(); // ensure smooth playback
            }
        }

        self.state.toasts.show(ctx);

        // let time = self.start.elapsed().as_secs_f32();

        // 欢迎窗口
        if self.state.show_welcome_window {
            let content_rect = ctx.available_rect();
            let center_pos = content_rect.center();

            // if !self.state.persistent.easter_egg_yuzu_welcome {
            egui::Window::new("欢迎")
                .resizable(false)
                .collapsible(false)
                .movable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos([center_pos.x, center_pos.y])
                .order(egui::Order::Foreground)
                .enabled(if let Some(anim) = &self.state.startup_animation {
                    anim.is_finished()
                } else {
                    true
                })
                .show(ctx, |ui| {
                    ui.heading("欢迎使用 smartboard");
                    ui.separator();

                    ui.label("这是一个功能强大的数字画板应用程序，您可以：");
                    ui.label("• 绘制和涂鸦");
                    ui.label("• 使用各种工具进行编辑");
                    ui.label("• 插入图片、文本和形状");
                    ui.label("• 自定义画板设置");
                    ui.separator();

                    if ui.button("新建画布").clicked() {
                        self.state.show_welcome_window = false;
                    }
                    if ui.button("加载画布").clicked() {
                        match CanvasState::load_from_file_with_dialog() {
                            Ok(canvas) => {
                                self.state.canvas = canvas;
                                self.state.show_welcome_window = false;
                                self.state.toasts.success("成功加载画布!")
                            }
                            Err(err) => self.state.toasts.error(format!("画布加载失败: {}!", err)),
                        };
                    }

                    ui.separator();

                    ui.checkbox(
                        &mut self.state.persistent.show_welcome_window_on_start,
                        "启动时显示欢迎",
                    );
                });
            // } else {
            //     if self.bg_tex.is_none() {
            //         self.bg_tex = Some(Self::load_embedded_texture(
            //             ctx,
            //             "bg",
            //             include_bytes!("../assets/images/welcome/bg.png"),
            //         ));

            //         self.logo_tex = Some(Self::load_embedded_texture(
            //             ctx,
            //             "logo",
            //             include_bytes!("../assets/images/welcome/logo.png"),
            //         ));

            //         self.button_tex = Some(Self::load_embedded_texture(
            //             ctx,
            //             "btn",
            //             include_bytes!("../assets/images/welcome/new.png"),
            //         ));
            //     }

            //     // egui::Window::new("欢迎")
            //     //     .resizable(false)
            //     //     .movable(false)
            //     //     .collapsible(false)
            //     //     .title_bar(false)
            //     //     .fixed_rect(content_rect)
            //     //     .order(egui::Order::Foreground)
            //     //     .show(ctx, |ui| {
            //     //         ui.set_min_size(content_rect.size());

            //     let avail = content_rect.size();

            //     // ===== virtual 1920x1080 =====
            //     let logical_w = 1920.0;
            //     let logical_h = 1080.0;

            //     let scale = (avail.x / logical_w).min(avail.y / logical_h);
            //     let draw_w = logical_w * scale;
            //     let draw_h = logical_h * scale;

            //     let offset_x = (avail.x - draw_w) * 0.5;
            //     let offset_y = (avail.y - draw_h) * 0.5;

            //     let painter = ctx.layer_painter(egui::LayerId::new(
            //         egui::Order::Foreground,
            //         egui::Id::new("yuzu_welcome"),
            //     ));

            //     // ===========================
            //     // BACKGROUND (animated)
            //     // ===========================
            //     let bg_delay = 0.0;
            //     let bg_dur = 1.1;

            //     let mut bg_x = -64.0;
            //     let mut bg_y = -36.0;
            //     let mut bg_scale = 1.067;

            //     if time > bg_delay {
            //         let t = ((time - bg_delay) / bg_dur).clamp(0.0, 1.0);
            //         let e = utils::exp_ease(0.1, t);

            //         bg_x = -64.0 + 64.0 * e;
            //         bg_y = -36.0 + 36.0 * e;
            //         bg_scale = 1.067 - 0.067 * e;
            //     }

            //     let bg_pos = egui::pos2(offset_x + bg_x * scale, offset_y + bg_y * scale);
            //     let bg_size = egui::vec2(1920.0 * bg_scale * scale, 1080.0 * bg_scale * scale);

            //     painter.image(
            //         self.bg_tex.unwrap(),
            //         egui::Rect::from_min_size(bg_pos, bg_size),
            //         egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            //         egui::Color32::WHITE,
            //     );

            //     // ===========================
            //     // LOGO (animated)
            //     // ===========================
            //     let logo_delay = 0.3;
            //     let logo_dur = 0.6;

            //     let mut logo_scale = 1.1;

            //     if time > logo_delay {
            //         let t = ((time - logo_delay) / logo_dur).clamp(0.0, 1.0);
            //         let e = utils::exp_ease(0.1, t);
            //         logo_scale = 1.1 - 0.1 * e;
            //     }

            //     let logo_pos = egui::pos2(offset_x + 40.0 * scale, offset_y + 60.0 * scale);
            //     let logo_size = egui::vec2(400.0 * logo_scale * scale, 160.0 * logo_scale * scale);

            //     painter.image(
            //         self.logo_tex.unwrap(),
            //         egui::Rect::from_min_size(logo_pos, logo_size),
            //         egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            //         egui::Color32::WHITE,
            //     );

            //     // ===========================
            //     // BUTTON (Minecraft-style)
            //     // ===========================
            //     let btn_x = 60.0;
            //     let btn_y = 360.0;

            //     let btn_rect = egui::Rect::from_min_size(
            //         egui::pos2(offset_x + btn_x * scale, offset_y + btn_y * scale),
            //         egui::vec2(300.0 * scale, 60.0 * scale),
            //     );

            //     // let response =
            //     //     ui.interact(btn_rect, ui.id().with("new_game"), egui::Sense::click());

            //     painter.image(
            //         self.button_tex.unwrap(),
            //         btn_rect,
            //         egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            //         egui::Color32::WHITE,
            //     );

            //     // if response.clicked() {
            //     //     self.state.show_welcome_window = false;
            //     // }
            //     // });
            // }
        }

        // 工具栏窗口
        // if !self.state.show_welcome_window {
        let content_rect = ctx.available_rect();
        let margin = 20.0; // 底部边距

        egui::Window::new("工具栏")
            .resizable(false)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .default_pos([content_rect.center().x, content_rect.max.y - margin])
            .enabled(
                !self.state.show_welcome_window
                    && if let Some(anim) = &self.state.startup_animation {
                        anim.is_finished()
                    } else {
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
                                        // 获取文本对象副本以避免借用冲突
                                        if let Some(text_obj) =
                                            self.state.canvas.objects.get(selected_idx).cloned()
                                        {
                                            if let CanvasObject::Text(text) = &text_obj {
                                                // Save state to history before modification
                                                self.state.history.save_state(&self.state.canvas);

                                                // 转换文本为笔画
                                                let strokes =
                                                    crate::utils::rasterize_text(text, FONT);
                                                for stroke in strokes {
                                                    self.state
                                                        .canvas
                                                        .objects
                                                        .push(CanvasObject::Stroke(stroke));
                                                }

                                                // 删除原文本对象
                                                self.state.canvas.objects.remove(selected_idx);
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
                                        crate::state::CanvasStroke {
                                            points: active_stroke.points,
                                            widths: active_stroke.widths,
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
                            self.state.history.save_state(&self.state.canvas);
                            self.state.canvas.objects.clear();
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
                                    let target_width = 300.0f32;
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
                                    self.state.history.save_state(&self.state.canvas);
                                    self.state.canvas.objects.push(CanvasObject::Image(
                                        CanvasImage {
                                            texture: texture,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: egui::vec2(target_width, target_height),
                                            aspect_ratio,
                                            marked_for_deletion: false,
                                            rot: 0.0,
                                        },
                                    ));

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
                                    ui.text_edit_singleline(&mut self.state.new_text_content);
                                });

                                ui.horizontal(|ui| {
                                    if ui.button("确认").clicked() {
                                        // Save state to history before modification
                                        self.state.history.save_state(&self.state.canvas);
                                        self.state.canvas.objects.push(CanvasObject::Text(
                                            CanvasText {
                                                text: self.state.new_text_content.clone(),
                                                pos: Pos2::new(100.0, 100.0),
                                                color: Color32::WHITE,
                                                font_size: 16.0,
                                                rot: 0.0,
                                            },
                                        ));
                                        self.state.current_tool = CanvasTool::Select;
                                        self.state.show_insert_text_dialog = false;
                                        self.state.new_text_content.clear();
                                    }

                                    if ui.button("取消").clicked() {
                                        self.state.show_insert_text_dialog = false;
                                        self.state.new_text_content.clear();
                                    }
                                });
                            });
                    }

                    if self.state.show_insert_shape_dialog {
                        // 计算屏幕中心位置
                        let content_rect = ctx.available_rect();
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
                                        self.state.history.save_state(&self.state.canvas);
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Line,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("箭头").clicked() {
                                        // Save state to history before modification
                                        self.state.history.save_state(&self.state.canvas);
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Arrow,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("矩形").clicked() {
                                        // Save state to history before modification
                                        self.state.history.save_state(&self.state.canvas);
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Rectangle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }
                                    if ui.button("三角形").clicked() {
                                        // Save state to history before modification
                                        self.state.history.save_state(&self.state.canvas);
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Triangle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_insert_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("圆形").clicked() {
                                        // Save state to history before modification
                                        self.state.history.save_state(&self.state.canvas);
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Circle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
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
                            ui.selectable_value(
                                &mut self.state.persistent.theme_mode,
                                ThemeMode::System,
                                "跟随系统",
                            );
                            ui.selectable_value(
                                &mut self.state.persistent.theme_mode,
                                ThemeMode::Light,
                                "浅色模式",
                            );
                            ui.selectable_value(
                                &mut self.state.persistent.theme_mode,
                                ThemeMode::Dark,
                                "深色模式",
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("启动时显示欢迎:");
                            ui.checkbox(
                                &mut self.state.persistent.show_welcome_window_on_start,
                                "",
                            );
                        });

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
                                match CanvasState::load_from_file_with_dialog() {
                                    Ok(canvas) => {
                                        self.state.canvas = canvas;
                                        self.state.show_welcome_window = false;
                                        self.state.toasts.success("成功加载画布!");
                                    }
                                    Err(err) => {
                                        self.state.toasts.error(format!("画布加载失败: {}!", err));
                                    }
                                };
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
                            let content_rect = ctx.available_rect();
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

                                let mode = &self.state.available_video_modes[current_selection];
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
                                let stress_color = Color32::from_rgb(255, 0, 0); // 红色
                                let stress_width = 3.0;

                                // 添加1000条笔画
                                for i in 0..1000 {
                                    let mut points = Vec::new();
                                    let mut widths = Vec::new();

                                    let num_points = 100;

                                    // 生成笔画位置
                                    let start_x = (i as f32 % 20.0) * 50.0;
                                    let start_y = ((i as f32 / 20.0).floor() % 15.0) * 50.0;

                                    // 生成笔画方向和长度
                                    for j in 0..num_points {
                                        let x = start_x + (j as f32 * 10.0);
                                        let y = start_y + (j as f32 * 5.0);

                                        points.push(Pos2::new(x, y));
                                        widths.push(stress_width);
                                    }

                                    // 创建笔画对象
                                    let stroke = crate::state::CanvasStroke {
                                        points,
                                        widths,
                                        color: stress_color,
                                        base_width: stress_width,
                                        rot: 0.0,
                                    };

                                    self.state.canvas.objects.push(CanvasObject::Stroke(stroke));
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("保存设置:");
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

                        ui.horizontal(|ui| {
                            ui.label("???:");
                            ui.checkbox(&mut self.state.persistent.easter_egg_yuzu_welcome, "");
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
                        ui.label(format!(
                            "FPS: {}",
                            self.state.fps_counter.current_fps.to_string()
                        ));
                    }
                });
            });

        // 主画布区域
        egui::CentralPanel::default().show(ctx, |ui| {
            // egui::Window::new("画布")
            //     .resizable(false)
            //     .movable(false)
            //     .title_bar(false)
            //     .pivot(egui::Align2::LEFT_TOP)
            //     .movable(false)
            //     .fixed_pos(Pos2::new(0.0, 0.0))
            //     .fixed_rect(content_rect)
            //     .order(egui::Order::Background)
            //     .show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(
                ui.available_size(),
                if self.state.persistent.low_latency_mode {
                    egui::Sense::drag()
                } else {
                    egui::Sense::click_and_drag()
                },
            );

            let painter = ui.painter();

            // 绘制背景
            painter.rect_filled(rect, 0.0, self.state.persistent.background_color);

            // 绘制所有对象
            for (i, object) in self.state.canvas.objects.iter().enumerate() {
                let selected = self.state.selected_object == Some(i);
                object.paint(painter, selected);
            }

            // 绘制当前正在绘制的笔画
            for (_touch_id, active_stroke) in &self.state.active_strokes {
                if active_stroke.widths.len() == active_stroke.points.len() {
                    // 检查是否所有宽度相同
                    let all_same_width = active_stroke
                        .widths
                        .windows(2)
                        .all(|w| (w[0] - w[1]).abs() < 0.01);

                    if all_same_width && active_stroke.points.len() == 2 {
                        // 只有两个点且宽度相同
                        painter.line_segment(
                            [active_stroke.points[0], active_stroke.points[1]],
                            Stroke::new(active_stroke.widths[0], self.state.brush_color),
                        );
                    } else if all_same_width {
                        // 多个点但宽度相同
                        let path = egui::epaint::PathShape::line(
                            active_stroke.points.clone(),
                            Stroke::new(active_stroke.widths[0], self.state.brush_color),
                        );
                        painter.add(Shape::Path(path));
                    } else {
                        // 宽度不同，分段绘制
                        for i in 0..active_stroke.points.len() - 1 {
                            let avg_width =
                                (active_stroke.widths[i] + active_stroke.widths[i + 1]) / 2.0;
                            painter.line_segment(
                                [active_stroke.points[i], active_stroke.points[i + 1]],
                                Stroke::new(avg_width, self.state.brush_color),
                            );
                        }
                    }
                }
            }

            // 绘制大小预览圆圈
            if self.state.show_size_preview {
                let content_rect = ui.ctx().available_rect();
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
                    painter.circle_stroke(*pos, 15.0, Stroke::new(2.0, Color32::BLUE));

                    // 绘制触控ID
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

            // 检查是否有窗口正在捕获输入
            // let egui_wants_pointer = ctx.wants_pointer_input();
            // println!("{}, {}", egui_wants_pointer, ctx.is_using_pointer());

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

                            // Check if we're dragging a resize handle
                            if let Some(selected_idx) = self.state.selected_object {
                                if let Some(object) = self.state.canvas.objects.get(selected_idx) {
                                    let bbox = object.bounding_box();
                                    if let Some(handle) =
                                        utils::get_transform_handle_at_pos(bbox, pos)
                                    {
                                        self.state.dragged_handle = Some(handle);
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
                                    // Resize operation - save state before modification
                                    let old_object =
                                        self.state.canvas.objects[selected_idx].clone();
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
                                    self.state.history.save_modify_object(
                                        selected_idx,
                                        old_object,
                                        self.state.canvas.objects[selected_idx].clone(),
                                    );
                                } else {
                                    // Move operation - save state before modification
                                    let old_object =
                                        self.state.canvas.objects[selected_idx].clone();
                                    if let Some(object) =
                                        self.state.canvas.objects.get_mut(selected_idx)
                                    {
                                        CanvasObject::move_object(object, delta);
                                    }
                                    self.state.history.save_modify_object(
                                        selected_idx,
                                        old_object,
                                        self.state.canvas.objects[selected_idx].clone(),
                                    );
                                }
                            }

                            // Update drag start position for continuous dragging
                            self.state.drag_start_pos = Some(current_pos);
                        }
                    }

                    // Handle drag stop: clear drag state
                    if response.drag_stopped() {
                        self.state.drag_start_pos = None;
                        self.state.dragged_handle = None;
                    }
                }

                CanvasTool::ObjectEraser => {
                    // 对象橡皮擦：点击或拖拽时删除相交的整个对象
                    // if egui_wants_pointer {
                    //     return;
                    // }
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

                            // 如果有对象要删除，保存状态到历史记录
                            if !to_remove.is_empty() {
                                self.state.history.save_state(&self.state.canvas);
                            }

                            // 删除对象
                            for i in to_remove {
                                self.state.canvas.objects.remove(i);
                            }
                        }
                    }
                }

                CanvasTool::PixelEraser => {
                    // 像素橡皮擦：从笔画中移除被擦除的段落，并将笔画分割为多个部分
                    // if egui_wants_pointer {
                    //     return;
                    // }
                    if response.dragged() || response.clicked() {
                        if let Some(pos) = pointer_pos {
                            // 绘制指针
                            utils::draw_size_preview(painter, pos, self.state.eraser_size);

                            // 从所有笔画中移除被橡皮擦覆盖的段落
                            let eraser_radius = self.state.eraser_size / 2.0;

                            // 我们需要收集所有新的笔画，因为我们可能需要将一个笔画分割为多个
                            let mut new_strokes = Vec::new();

                            for object in &self.state.canvas.objects {
                                if let CanvasObject::Stroke(stroke) = object {
                                    if stroke.points.len() < 2 {
                                        continue;
                                    }

                                    let mut current_points = Vec::new();
                                    let mut current_widths = Vec::new();

                                    // 添加第一个点
                                    current_points.push(stroke.points[0]);
                                    if !stroke.widths.is_empty() {
                                        current_widths.push(stroke.widths[0]);
                                    }

                                    // 检查每个段落
                                    for i in 0..stroke.points.len() - 1 {
                                        let p1 = stroke.points[i];
                                        let p2 = stroke.points[i + 1];
                                        let segment_width = if i < stroke.widths.len() {
                                            stroke.widths[i]
                                        } else {
                                            stroke.widths[0]
                                        };

                                        // 计算点到线段的距离
                                        let dist =
                                            utils::point_to_line_segment_distance(pos, p1, p2);

                                        // 如果段落不被橡皮擦覆盖，保留第二个点
                                        if dist > eraser_radius + segment_width / 2.0 {
                                            current_points.push(p2);
                                            if i + 1 < stroke.widths.len() {
                                                current_widths.push(stroke.widths[i + 1]);
                                            } else if !stroke.widths.is_empty() {
                                                current_widths
                                                    .push(stroke.widths[stroke.widths.len() - 1]);
                                            }
                                        } else {
                                            // 段落被擦除，如果当前笔画有足够的点，保存它
                                            if current_points.len() >= 2 {
                                                new_strokes.push(crate::state::CanvasStroke {
                                                    points: current_points.clone(),
                                                    widths: current_widths.clone(),
                                                    color: stroke.color,
                                                    base_width: stroke.base_width,
                                                    rot: 0.0,
                                                });
                                            }
                                            // 开始新的笔画段落
                                            current_points = Vec::new();
                                            current_widths = Vec::new();
                                        }
                                    }

                                    // 添加最后一个笔画段落
                                    if current_points.len() >= 2 {
                                        new_strokes.push(crate::state::CanvasStroke {
                                            points: current_points,
                                            widths: current_widths,
                                            color: stroke.color,
                                            base_width: stroke.base_width,
                                            rot: 0.0,
                                        });
                                    }
                                } else {
                                    // 非笔画对象保留原样
                                    if let CanvasObject::Stroke(stroke) = object {
                                        new_strokes.push(stroke.clone());
                                    }
                                }
                            }

                            // 如果有笔画被修改，保存状态到历史记录
                            let original_stroke_count = self
                                .state
                                .canvas
                                .objects
                                .iter()
                                .filter(|obj| matches!(obj, CanvasObject::Stroke(_)))
                                .count();
                            let new_stroke_count = new_strokes.len();
                            if original_stroke_count != new_stroke_count {
                                self.state.history.save_state(&self.state.canvas);
                            }

                            // 替换所有笔画
                            self.state.canvas.objects = self
                                .state
                                .canvas
                                .objects
                                .iter()
                                .filter_map(|obj| {
                                    if let CanvasObject::Stroke(_) = obj {
                                        None
                                    } else {
                                        Some(obj.clone())
                                    }
                                })
                                .collect();

                            // 添加处理后的笔画
                            for stroke in new_strokes {
                                self.state.canvas.objects.push(CanvasObject::Stroke(stroke));
                            }
                        }
                    }
                }

                CanvasTool::Brush => {
                    // 画笔工具
                    // if egui_wants_pointer {
                    //     return;
                    // }
                    if response.drag_started() {
                        // 开始新的笔画
                        if let Some(pos) = pointer_pos {
                            if pos.x >= rect.min.x
                                && pos.x <= rect.max.x
                                && pos.y >= rect.min.y
                                && pos.y <= rect.max.y
                            {
                                self.state.is_drawing = true;
                                let start_time = Instant::now();
                                let width = utils::calculate_dynamic_width(
                                    self.state.brush_width,
                                    self.state.dynamic_brush_width_mode,
                                    0,
                                    1,
                                    None,
                                );

                                // 使用特殊的 touch_id 0 表示鼠标输入
                                let touch_id = 0;
                                self.state.active_strokes.insert(
                                    touch_id,
                                    crate::state::ActiveStroke {
                                        points: vec![pos],
                                        widths: vec![width],
                                        times: vec![0.0],
                                        start_time,
                                        last_movement_time: start_time,
                                    },
                                );
                            }
                        }
                    } else if response.dragged() {
                        // 继续绘制
                        if self.state.is_drawing {
                            if let Some(pos) = pointer_pos {
                                // 使用特殊的 touch_id 0 表示鼠标输入
                                let touch_id = 0;
                                if let Some(active_stroke) =
                                    self.state.active_strokes.get_mut(&touch_id)
                                {
                                    let current_time =
                                        active_stroke.start_time.elapsed().as_secs_f64();

                                    // 只添加与上一个点距离足够远的点，避免点太密集
                                    if active_stroke.points.is_empty()
                                        || active_stroke.points.last().unwrap().distance(pos) > 1.0
                                    {
                                        // 计算速度（像素/秒）
                                        let speed = if active_stroke.points.len() > 0
                                            && active_stroke.times.len() > 0
                                        {
                                            let last_time = active_stroke.times.last().unwrap();
                                            let time_delta =
                                                ((current_time - last_time) as f32).max(0.001); // 避免除零
                                            let distance =
                                                active_stroke.points.last().unwrap().distance(pos);
                                            Some(distance / time_delta)
                                        } else {
                                            None
                                        };

                                        active_stroke.points.push(pos);
                                        active_stroke.times.push(current_time);

                                        // 计算动态宽度
                                        let width = utils::calculate_dynamic_width(
                                            self.state.brush_width,
                                            self.state.dynamic_brush_width_mode,
                                            active_stroke.points.len() - 1,
                                            active_stroke.points.len(),
                                            speed,
                                        );
                                        active_stroke.widths.push(width);

                                        // 更新最后移动时间
                                        active_stroke.last_movement_time = Instant::now();
                                    }
                                }
                            }
                        }
                    } else if response.drag_stopped() {
                        // 结束当前笔画
                        if self.state.is_drawing {
                            // 使用特殊的 touch_id 0 表示鼠标输入
                            let touch_id = 0;
                            if let Some(active_stroke) = self.state.active_strokes.remove(&touch_id)
                            {
                                if active_stroke.widths.len() == active_stroke.points.len() {
                                    // 应用笔画平滑
                                    let final_points = if self.state.persistent.stroke_smoothing {
                                        utils::apply_stroke_smoothing(&active_stroke.points)
                                    } else {
                                        active_stroke.points
                                    };

                                    // 应用插值
                                    let (interpolated_points, interpolated_widths) =
                                        utils::apply_point_interpolation(
                                            &final_points,
                                            &active_stroke.widths,
                                            self.state.persistent.interpolation_frequency,
                                        );

                                    // Save state to history before modification
                                    self.state.history.save_state(&self.state.canvas);
                                    self.state.canvas.objects.push(CanvasObject::Stroke(
                                        crate::state::CanvasStroke {
                                            points: interpolated_points,
                                            widths: interpolated_widths,
                                            color: self.state.brush_color,
                                            base_width: self.state.brush_width,
                                            rot: 0.0,
                                        },
                                    ));
                                }
                            }

                            // 检查是否还有其他正在绘制的笔画
                            self.state.is_drawing = !self.state.active_strokes.is_empty();
                        }
                    } else if response.clicked() {
                        // 处理单击事件 - 绘制单个点
                        if let Some(pos) = pointer_pos {
                            if pos.x >= rect.min.x
                                && pos.x <= rect.max.x
                                && pos.y >= rect.min.y
                                && pos.y <= rect.max.y
                            {
                                // Save state to history before modification
                                self.state.history.save_state(&self.state.canvas);
                                self.state.canvas.objects.push(CanvasObject::Stroke(
                                    crate::state::CanvasStroke {
                                        points: vec![pos],
                                        widths: vec![self.state.brush_width],
                                        color: self.state.brush_color,
                                        base_width: self.state.brush_width,
                                        rot: 0.0,
                                    },
                                ));
                            }
                        }
                    }

                    // 如果鼠标在画布内移动且正在绘制，也添加点（用于平滑绘制）
                    if response.hovered() && self.state.is_drawing {
                        if let Some(pos) = pointer_pos {
                            // 使用特殊的 touch_id 0 表示鼠标输入
                            let touch_id = 0;
                            if let Some(active_stroke) =
                                self.state.active_strokes.get_mut(&touch_id)
                            {
                                let current_time = active_stroke.start_time.elapsed().as_secs_f64();

                                if self.state.persistent.stroke_straightening {
                                    // 检查是否停留超过 0.5 秒
                                    let time_since_last_movement =
                                        active_stroke.last_movement_time.elapsed().as_secs_f32();
                                    if time_since_last_movement > 0.5 {
                                        // 拉直笔画
                                        let straightened_points = utils::straighten_stroke(
                                            &active_stroke.points,
                                            self.state.persistent.stroke_straightening_tolerance,
                                        );

                                        // 只有在点数量实际改变时才更新宽度数组
                                        if straightened_points.len() != active_stroke.points.len() {
                                            active_stroke.points = straightened_points;

                                            // 更新宽度数组以匹配新的点数量
                                            if !active_stroke.widths.is_empty() {
                                                let first_width = active_stroke.widths[0];
                                                let last_width =
                                                    *active_stroke.widths.last().unwrap();
                                                active_stroke.widths =
                                                    if active_stroke.points.len() == 1 {
                                                        vec![first_width]
                                                    } else {
                                                        vec![first_width, last_width]
                                                    };
                                            }
                                        }

                                        // 更新最后移动时间
                                        active_stroke.last_movement_time = Instant::now();
                                    }
                                }

                                if active_stroke.points.is_empty()
                                    || active_stroke.points.last().unwrap().distance(pos) > 1.0
                                {
                                    // 计算速度
                                    let speed = if active_stroke.points.len() > 0
                                        && active_stroke.times.len() > 0
                                    {
                                        let last_time = active_stroke.times.last().unwrap();
                                        let time_delta =
                                            ((current_time - last_time) as f32).max(0.001);
                                        let distance =
                                            active_stroke.points.last().unwrap().distance(pos);
                                        Some(distance / time_delta)
                                    } else {
                                        None
                                    };

                                    active_stroke.points.push(pos);
                                    active_stroke.times.push(current_time);

                                    let width = utils::calculate_dynamic_width(
                                        self.state.brush_width,
                                        self.state.dynamic_brush_width_mode,
                                        active_stroke.points.len() - 1,
                                        active_stroke.points.len(),
                                        speed,
                                    );
                                    active_stroke.widths.push(width);

                                    // 更新最后移动时间
                                    active_stroke.last_movement_time = Instant::now();
                                }
                            }
                        }
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
        if self.state.window_mode_changed {
            if let Some(window) = self.window.as_ref() {
                self.apply_window_mode(window);
                self.state.window_mode_changed = false;
            }
        }

        // 应用垂直同步模式更改
        if self.state.present_mode_changed {
            self.apply_present_mode();
            self.state.present_mode_changed = false;
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();
        pollster::block_on(self.set_window(window));
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::TrayIconEvent(event) => match event {
                tray_icon::TrayIconEvent::Click { .. } => {
                    let window = self.window.as_ref().expect("no window??");
                    window.set_visible(true);
                    window.focus_window();
                }
                _ => {}
            },
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        // 检查是否需要退出
        if self.state.should_quit {
            println!("Quit button was pressed; exiting");
            self.exit(event_loop);
            return;
        }

        // 让 egui 先处理事件
        self.render_state
            .as_mut()
            .unwrap()
            .egui_renderer
            .handle_input(self.window.as_ref().unwrap(), &event);

        match event {
            WindowEvent::CloseRequested => {
                println!("Window close was requested; exiting");
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
                println!("Escape key pressed; exiting");
                self.exit(event_loop);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
                self.window.as_ref().unwrap().request_redraw();
            }
            WindowEvent::Resized(new_size) => {
                self.handle_resized(new_size.width, new_size.height);
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
                            let start_time = Instant::now();
                            let width = utils::calculate_dynamic_width(
                                self.state.brush_width,
                                self.state.dynamic_brush_width_mode,
                                0,
                                1,
                                None,
                            );

                            self.state.active_strokes.insert(
                                id,
                                crate::state::ActiveStroke {
                                    points: vec![logical_pos],
                                    widths: vec![width],
                                    times: vec![0.0],
                                    start_time,
                                    last_movement_time: start_time,
                                },
                            );
                        }
                    }
                    TouchPhase::Moved => {
                        self.state.touch_points.insert(id, logical_pos);

                        // 如果当前工具是画笔，继续绘制
                        if self.state.current_tool == CanvasTool::Brush {
                            if let Some(active_stroke) = self.state.active_strokes.get_mut(&id) {
                                let current_time = active_stroke.start_time.elapsed().as_secs_f64();

                                // 只添加与上一个点距离足够远的点，避免点太密集
                                if active_stroke.points.is_empty()
                                    || active_stroke.points.last().unwrap().distance(logical_pos)
                                        > 1.0
                                {
                                    // 计算速度（像素/秒）
                                    let speed = if active_stroke.points.len() > 0
                                        && active_stroke.times.len() > 0
                                    {
                                        let last_time = active_stroke.times.last().unwrap();
                                        let time_delta =
                                            ((current_time - last_time) as f32).max(0.001); // 避免除零
                                        let distance = active_stroke
                                            .points
                                            .last()
                                            .unwrap()
                                            .distance(logical_pos);
                                        Some(distance / time_delta)
                                    } else {
                                        None
                                    };

                                    active_stroke.points.push(logical_pos);
                                    active_stroke.times.push(current_time);

                                    // 计算动态宽度
                                    let width = utils::calculate_dynamic_width(
                                        self.state.brush_width,
                                        self.state.dynamic_brush_width_mode,
                                        active_stroke.points.len() - 1,
                                        active_stroke.points.len(),
                                        speed,
                                    );
                                    active_stroke.widths.push(width);

                                    // 更新最后移动时间
                                    active_stroke.last_movement_time = Instant::now();
                                }
                            }
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.state.touch_points.remove(&id);

                        // 如果当前工具是画笔，结束笔画
                        if self.state.current_tool == CanvasTool::Brush {
                            if let Some(active_stroke) = self.state.active_strokes.remove(&id) {
                                if active_stroke.widths.len() == active_stroke.points.len() {
                                    // 应用笔画平滑
                                    let final_points = if self.state.persistent.stroke_smoothing {
                                        utils::apply_stroke_smoothing(&active_stroke.points)
                                    } else {
                                        active_stroke.points
                                    };

                                    // 应用插值
                                    let (interpolated_points, interpolated_widths) =
                                        utils::apply_point_interpolation(
                                            &final_points,
                                            &active_stroke.widths,
                                            self.state.persistent.interpolation_frequency,
                                        );

                                    self.state.canvas.objects.push(CanvasObject::Stroke(
                                        crate::state::CanvasStroke {
                                            points: interpolated_points,
                                            widths: interpolated_widths,
                                            color: self.state.brush_color,
                                            base_width: self.state.brush_width,
                                            rot: 0.0,
                                        },
                                    ));
                                }
                            }

                            // 检查是否还有其他正在绘制的笔画
                            self.state.is_drawing = !self.state.active_strokes.is_empty();
                        }
                    }
                }

                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}
