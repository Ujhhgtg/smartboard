use crate::render::RenderState;
use crate::state::{
    AppState, CanvasImage, CanvasObject, CanvasShape, CanvasShapeType, CanvasState, CanvasText,
    CanvasTool, DynamicBrushWidthMode, OptimizationPolicy, PersistentState, ResizeAnchor,
    ResizeOperation, RotationOperation, StartupAnimation, ThemeMode, WindowMode,
};
use crate::utils::AppUtils;
use core::f32;
use egui::{Color32, Pos2, Shape, Stroke};
use egui_wgpu::wgpu::SurfaceError;
use egui_wgpu::{ScreenDescriptor, wgpu, wgpu::PresentMode};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, Touch, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

// 启动动画
include!(concat!(env!("OUT_DIR"), "/startup_frames.rs"));
pub const STARTUP_AUDIO: &[u8] = include_bytes!("../assets/startup_animation/audio.wav");

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
        }
    }

    pub async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);

        // 设置标题
        window.set_title("smartboard");

        // 设置窗口模式
        self.apply_window_mode(&window);

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
                if let Some(monitor) = window.current_monitor() {
                    // 使用选中的视频模式，如果没有选中则使用第一个可用的
                    // if let Some(selected_index) = self.state.selected_video_mode_index {
                    //     if selected_index < self.state.available_video_modes.len() {
                    //         if let Some(mode) = self.state.available_video_modes.get(selected_index)
                    //         {
                    //             window.set_fullscreen(Some(Fullscreen::Exclusive(mode.clone())));
                    //             return;
                    //         }
                    //     }
                    // }

                    // 回退到第一个可用的视频模式
                    let video_mode = monitor.video_modes().next();
                    if let Some(mode) = video_mode {
                        window.set_fullscreen(Some(Fullscreen::Exclusive(mode)));
                    }
                }
            }
            WindowMode::BorderlessFullscreen => {
                // 无边框全屏模式
                if let Some(monitor) = window.current_monitor() {
                    window.set_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));
                }
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
        if let Some(window) = self.window.as_ref() {
            if let Some(min) = window.is_minimized() {
                if min {
                    println!("Window is minimized");
                    return;
                }
            }
        }

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

        // 更新可用的视频模式
        // let window_ref = self.window.as_ref().unwrap();
        // if let Some(monitor) = window_ref.current_monitor() {
        //     self.state.available_video_modes = monitor.video_modes().collect();

        //     // 如果没有选中的视频模式，默认选择第一个
        //     if self.state.selected_video_mode_index.is_none()
        //         && !self.state.available_video_modes.is_empty()
        //     {
        //         self.state.selected_video_mode_index = Some(0);
        //     }
        // }

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
            let content_rect = ctx.available_rect();
            let center_pos = content_rect.center();

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
                                "对象橡皮擦",
                            )
                            .changed()
                        || ui
                            .selectable_value(
                                &mut self.state.current_tool,
                                CanvasTool::PixelEraser,
                                "像素橡皮擦",
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
                            let color_name = if color.r() == 255 && color.g() == 0 && color.b() == 0
                            {
                                "红"
                            } else if color.r() == 255 && color.g() == 255 && color.b() == 0 {
                                "黄"
                            } else if color.r() == 0 && color.g() == 255 && color.b() == 0 {
                                "绿"
                            } else if color.r() == 0 && color.g() == 0 && color.b() == 255 {
                                "蓝"
                            } else if color.r() == 0 && color.g() == 0 && color.b() == 0 {
                                "黑"
                            } else if color.r() == 255 && color.g() == 255 && color.b() == 255 {
                                "白"
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

                                    self.state.canvas.objects.push(CanvasObject::Image(
                                        CanvasImage {
                                            texture: texture,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: egui::vec2(target_width, target_height),
                                            aspect_ratio,
                                            marked_for_deletion: false,
                                        },
                                    ));
                                }
                            }
                        }
                        if ui.button("文本").clicked() {
                            self.state.show_text_dialog = true;
                        }
                        if ui.button("形状").clicked() {
                            self.state.show_shape_dialog = true;
                        }
                    });

                    if self.state.show_text_dialog {
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
                                        self.state.canvas.objects.push(CanvasObject::Text(
                                            CanvasText {
                                                text: self.state.new_text_content.clone(),
                                                pos: Pos2::new(100.0, 100.0),
                                                color: Color32::WHITE,
                                                font_size: 16.0,
                                            },
                                        ));
                                        self.state.show_text_dialog = false;
                                        self.state.new_text_content.clear();
                                    }

                                    if ui.button("取消").clicked() {
                                        self.state.show_text_dialog = false;
                                        self.state.new_text_content.clear();
                                    }
                                });
                            });
                    }

                    if self.state.show_shape_dialog {
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
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Line,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("箭头").clicked() {
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Arrow,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("矩形").clicked() {
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Rectangle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }
                                    if ui.button("三角形").clicked() {
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Triangle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }

                                    if ui.button("圆形").clicked() {
                                        self.state.canvas.objects.push(CanvasObject::Shape(
                                            CanvasShape {
                                                shape_type: CanvasShapeType::Circle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            },
                                        ));
                                        self.state.show_shape_dialog =
                                            self.state.persistent.keep_insertion_window_open;
                                    }
                                });

                                ui.horizontal(|ui| {
                                    if ui.button("取消").clicked() {
                                        self.state.show_shape_dialog = false;
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
                                            self.state.persistent.quick_colors = vec![
                                                Color32::from_rgb(255, 0, 0),     // 红色
                                                Color32::from_rgb(255, 255, 0),   // 黄色
                                                Color32::from_rgb(0, 255, 0),     // 绿色
                                                Color32::from_rgb(0, 0, 0),       // 黑色
                                                Color32::from_rgb(255, 255, 255), // 白色
                                            ]
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
                        // ui.horizontal(|ui| {
                        //     ui.label("显示模式:");

                        //     // 创建一个可变引用用于选择
                        //     let mut current_selection: usize =
                        //         self.state.selected_video_mode_index.unwrap_or(0);

                        //     // 显示当前选择的视频模式
                        //     // if self.state.persistent_state.window_mode == WindowMode::Fullscreen {
                        //     let video_modes = self.state.available_video_modes.clone();

                        //     if let Some(mode) = video_modes.get(current_selection) {
                        //         let mode_text = format!(
                        //             "{}x{} @ {}Hz",
                        //             mode.size().width,
                        //             mode.size().height,
                        //             mode.refresh_rate_millihertz() as f32 / 1000.0
                        //         );
                        //         ui.label(mode_text);
                        //     }

                        //     egui::ComboBox::from_id_salt("video_mode_selection").show_ui(
                        //         ui,
                        //         |ui| {
                        //             // 显示所有可用的视频模式
                        //             for (index, mode) in video_modes.iter().enumerate() {
                        //                 let mode_text = format!(
                        //                     "{}x{} @ {}Hz",
                        //                     mode.size().width,
                        //                     mode.size().height,
                        //                     mode.refresh_rate_millihertz() as f32 / 1000.0
                        //                 );
                        //                 ui.selectable_value(
                        //                     &mut current_selection,
                        //                     index,
                        //                     mode_text,
                        //                 );
                        //             }
                        //         },
                        //     );
                        //     // } else {
                        //     //     // 非全屏模式下显示当前模式但不允许更改
                        //     //     if let Some(mode) =
                        //     //         self.state.available_video_modes.get(current_selection)
                        //     //     {
                        //     //         let mode_text = format!(
                        //     //             "{}x{} @ {}Hz",
                        //     //             mode.size().width,
                        //     //             mode.size().height,
                        //     //             mode.refresh_rate_millihertz() as f32 / 1000.0
                        //     //         );
                        //     //         ui.label(mode_text);
                        //     //     }
                        //     // }

                        //     // 更新选择
                        //     self.state.selected_video_mode_index = Some(current_selection);

                        //     // 如果在全屏模式下更改了显示模式，立即应用更改
                        //     if self.state.persistent_state.window_mode == WindowMode::Fullscreen {
                        //         self.state.persistent_state.window_mode_changed = true;
                        //     }
                        // });

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
                    });
                }

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("退出").clicked() {
                        self.state.should_quit = true;
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
            let (rect, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

            let painter = ui.painter();

            // 绘制背景
            painter.rect_filled(rect, 0.0, self.state.persistent.background_color);

            // 绘制所有对象
            for (i, object) in self.state.canvas.objects.iter().enumerate() {
                let selected = self.state.selected_object == Some(i);
                object.draw(painter, selected);
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
                AppUtils::draw_size_preview(
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
            // 绘制多点触控点

            // 绘制调整大小和旋转锚点
            if let Some(selected_idx) = self.state.selected_object {
                if let Some(object) = self.state.canvas.objects.get(selected_idx) {
                    let object_rect = match object {
                        CanvasObject::Image(img) => egui::Rect::from_min_size(img.pos, img.size),
                        CanvasObject::Text(text) => {
                            let text_galley = painter.layout_no_wrap(
                                text.text.clone(),
                                egui::FontId::proportional(text.font_size),
                                text.color,
                            );
                            let text_size = text_galley.size();
                            egui::Rect::from_min_size(text.pos, text_size)
                        }
                        CanvasObject::Shape(shape) => AppUtils::calculate_shape_bounding_box(shape),
                        CanvasObject::Stroke(_) => {
                            // 笔画不支持调整大小和旋转
                            return;
                        }
                    };

                    AppUtils::draw_resize_and_rotation_anchors(
                        &painter,
                        object_rect,
                        self.state.resize_anchor_hovered,
                        self.state.rotation_anchor_hovered,
                    );
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
                    // if egui_wants_pointer {
                    //     return;
                    // }
                    // 检测锚点悬停状态
                    if let Some(pos) = pointer_pos {
                        // 检查是否有对象被选中
                        if let Some(selected_idx) = self.state.selected_object {
                            // 获取选中对象的边界框
                            let object_rect =
                                if let Some(object) = self.state.canvas.objects.get(selected_idx) {
                                    match object {
                                        CanvasObject::Image(img) => {
                                            Some(egui::Rect::from_min_size(img.pos, img.size))
                                        }
                                        CanvasObject::Text(text) => {
                                            let text_galley = painter.layout_no_wrap(
                                                text.text.clone(),
                                                egui::FontId::proportional(text.font_size),
                                                text.color,
                                            );
                                            let text_size = text_galley.size();
                                            Some(egui::Rect::from_min_size(text.pos, text_size))
                                        }
                                        CanvasObject::Shape(shape) => {
                                            Some(AppUtils::calculate_shape_bounding_box(shape))
                                        }
                                        CanvasObject::Stroke(_) => None, // 笔画不支持调整大小和旋转
                                    }
                                } else {
                                    None
                                };

                            if let Some(rect) = object_rect {
                                // 检查调整大小锚点悬停
                                let resize_anchors = [
                                    (ResizeAnchor::TopLeft, rect.left_top()),
                                    (ResizeAnchor::TopRight, rect.right_top()),
                                    (ResizeAnchor::BottomLeft, rect.left_bottom()),
                                    (ResizeAnchor::BottomRight, rect.right_bottom()),
                                    (ResizeAnchor::Top, Pos2::new(rect.center().x, rect.min.y)),
                                    (ResizeAnchor::Bottom, Pos2::new(rect.center().x, rect.max.y)),
                                    (ResizeAnchor::Left, Pos2::new(rect.min.x, rect.center().y)),
                                    (ResizeAnchor::Right, Pos2::new(rect.max.x, rect.center().y)),
                                ];

                                let mut found_resize_anchor = None;
                                for (anchor_type, anchor_pos) in resize_anchors {
                                    if pos.distance(anchor_pos) <= 15.0 {
                                        found_resize_anchor = Some(anchor_type);
                                        break;
                                    }
                                }

                                self.state.resize_anchor_hovered = found_resize_anchor;

                                // 检查旋转锚点悬停
                                let rotation_anchor_pos =
                                    Pos2::new(rect.center().x, rect.min.y - 30.0);
                                self.state.rotation_anchor_hovered =
                                    pos.distance(rotation_anchor_pos) <= 15.0;
                            } else {
                                self.state.resize_anchor_hovered = None;
                                self.state.rotation_anchor_hovered = false;
                            }
                        } else {
                            self.state.resize_anchor_hovered = None;
                            self.state.rotation_anchor_hovered = false;
                        }
                    } else {
                        self.state.resize_anchor_hovered = None;
                        self.state.rotation_anchor_hovered = false;
                    }

                    if response.drag_started() {
                        if let Some(pos) = pointer_pos {
                            self.state.drag_start_pos = Some(pos);

                            let mut hit = false;
                            for object in &self.state.canvas.objects {
                                if let CanvasObject::Image(img) = object {
                                    if egui::Rect::from_min_size(img.pos, img.size).contains(pos) {
                                        hit = true;
                                        break;
                                    }
                                }
                            }
                            if !hit {
                                for object in &self.state.canvas.objects {
                                    if let CanvasObject::Stroke(stroke) = object {
                                        if AppUtils::point_intersects_stroke(pos, stroke, 10.0) {
                                            hit = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            if !hit {
                                self.state.selected_object = None;
                            }

                            // 如果已经有对象被选中，检查是否点击了锚点
                            if let Some(selected_idx) = self.state.selected_object {
                                if let Some(object) = self.state.canvas.objects.get(selected_idx) {
                                    let object_rect = match object {
                                        CanvasObject::Image(img) => {
                                            Some(egui::Rect::from_min_size(img.pos, img.size))
                                        }
                                        CanvasObject::Text(text) => {
                                            let text_galley = painter.layout_no_wrap(
                                                text.text.clone(),
                                                egui::FontId::proportional(text.font_size),
                                                text.color,
                                            );
                                            let text_size = text_galley.size();
                                            Some(egui::Rect::from_min_size(text.pos, text_size))
                                        }
                                        CanvasObject::Shape(shape) => {
                                            Some(AppUtils::calculate_shape_bounding_box(shape))
                                        }
                                        CanvasObject::Stroke(_) => None,
                                    };

                                    if let Some(rect) = object_rect {
                                        // 检查是否点击了调整大小锚点
                                        if let Some(anchor) = self.state.resize_anchor_hovered {
                                            self.state.resize_operation = Some(ResizeOperation {
                                                anchor,
                                                start_pos: pos,
                                                start_size: rect.size(),
                                                start_object_pos: rect.min,
                                            });
                                        }
                                        // 检查是否点击了旋转锚点
                                        else if self.state.rotation_anchor_hovered {
                                            self.state.rotation_operation =
                                                Some(RotationOperation {
                                                    start_pos: pos,
                                                    start_angle: 0.0, // 当前角度，需要从对象中获取
                                                    center: rect.center(),
                                                });

                                            // 设置初始角度
                                            if let Some(CanvasObject::Shape(shape)) =
                                                self.state.canvas.objects.get(selected_idx)
                                            {
                                                if let Some(op) =
                                                    self.state.rotation_operation.as_mut()
                                                {
                                                    op.start_angle = shape.rotation;
                                                }
                                            }
                                        }
                                        // 否则检查是否点击了对象本身（用于拖动）
                                        else if rect.contains(pos) {
                                            // 已经选中，什么都不做
                                        }
                                        // 点击了非对象区域，取消选择
                                        else {
                                            self.state.selected_object = None;
                                        }
                                    }
                                }
                            }
                            // 没有对象被选中，尝试选择新对象
                            else {
                                self.state.selected_object = None;

                                // 检查所有对象
                                for (i, object) in
                                    self.state.canvas.objects.iter().enumerate().rev()
                                {
                                    match object {
                                        CanvasObject::Image(img) => {
                                            let img_rect =
                                                egui::Rect::from_min_size(img.pos, img.size);
                                            if img_rect.contains(pos) {
                                                self.state.selected_object = Some(i);
                                                break;
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
                                                self.state.selected_object = Some(i);
                                                break;
                                            }
                                        }
                                        CanvasObject::Shape(shape) => {
                                            let shape_rect =
                                                AppUtils::calculate_shape_bounding_box(shape);
                                            if shape_rect.contains(pos) {
                                                self.state.selected_object = Some(i);
                                                break;
                                            }
                                        }
                                        CanvasObject::Stroke(stroke) => {
                                            if AppUtils::point_intersects_stroke(pos, stroke, 10.0)
                                            {
                                                self.state.selected_object = Some(i);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if response.clicked() {
                        // 点击非对象区域时取消选择
                        if let Some(pos) = pointer_pos {
                            let mut hit = false;
                            for object in &self.state.canvas.objects {
                                if let CanvasObject::Image(img) = object {
                                    if egui::Rect::from_min_size(img.pos, img.size).contains(pos) {
                                        hit = true;
                                        break;
                                    }
                                }
                            }
                            if !hit {
                                for object in &self.state.canvas.objects {
                                    if let CanvasObject::Stroke(stroke) = object {
                                        if AppUtils::point_intersects_stroke(pos, stroke, 10.0) {
                                            hit = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            if !hit {
                                self.state.selected_object = None;
                            }
                        }
                    } else if response.dragged() {
                        if let Some(pos) = pointer_pos {
                            // 处理调整大小操作
                            if let Some(resize_op) = self.state.resize_operation {
                                if let Some(selected_idx) = self.state.selected_object {
                                    if let Some(object) =
                                        self.state.canvas.objects.get_mut(selected_idx)
                                    {
                                        let delta = pos - resize_op.start_pos;

                                        match object {
                                            CanvasObject::Image(img) => {
                                                let mut new_size = resize_op.start_size;
                                                let mut new_pos = resize_op.start_object_pos;

                                                // 根据锚点类型调整大小和位置
                                                match resize_op.anchor {
                                                    ResizeAnchor::TopLeft => {
                                                        new_size.x = (resize_op.start_size.x
                                                            - delta.x)
                                                            .max(20.0);
                                                        new_size.y = (resize_op.start_size.y
                                                            - delta.y)
                                                            .max(20.0);
                                                        new_pos.x =
                                                            resize_op.start_object_pos.x + delta.x;
                                                        new_pos.y =
                                                            resize_op.start_object_pos.y + delta.y;
                                                    }
                                                    ResizeAnchor::TopRight => {
                                                        new_size.x = (resize_op.start_size.x
                                                            + delta.x)
                                                            .max(20.0);
                                                        new_size.y = (resize_op.start_size.y
                                                            - delta.y)
                                                            .max(20.0);
                                                        new_pos.y =
                                                            resize_op.start_object_pos.y + delta.y;
                                                    }
                                                    ResizeAnchor::BottomLeft => {
                                                        new_size.x = (resize_op.start_size.x
                                                            - delta.x)
                                                            .max(20.0);
                                                        new_size.y = (resize_op.start_size.y
                                                            + delta.y)
                                                            .max(20.0);
                                                        new_pos.x =
                                                            resize_op.start_object_pos.x + delta.x;
                                                    }
                                                    ResizeAnchor::BottomRight => {
                                                        new_size.x = (resize_op.start_size.x
                                                            + delta.x)
                                                            .max(20.0);
                                                        new_size.y = (resize_op.start_size.y
                                                            + delta.y)
                                                            .max(20.0);
                                                    }
                                                    ResizeAnchor::Top => {
                                                        new_size.y = (resize_op.start_size.y
                                                            - delta.y)
                                                            .max(20.0);
                                                        new_pos.y =
                                                            resize_op.start_object_pos.y + delta.y;
                                                    }
                                                    ResizeAnchor::Bottom => {
                                                        new_size.y = (resize_op.start_size.y
                                                            + delta.y)
                                                            .max(20.0);
                                                    }
                                                    ResizeAnchor::Left => {
                                                        new_size.x = (resize_op.start_size.x
                                                            - delta.x)
                                                            .max(20.0);
                                                        new_pos.x =
                                                            resize_op.start_object_pos.x + delta.x;
                                                    }
                                                    ResizeAnchor::Right => {
                                                        new_size.x = (resize_op.start_size.x
                                                            + delta.x)
                                                            .max(20.0);
                                                    }
                                                }

                                                // 保持纵横比（仅适用于图片）
                                                if img.aspect_ratio > 0.0 {
                                                    let target_aspect = img.aspect_ratio;
                                                    let current_aspect = new_size.x / new_size.y;

                                                    if current_aspect.abs() > 0.01 {
                                                        if current_aspect > target_aspect {
                                                            // 太宽，调整宽度
                                                            new_size.x = new_size.y * target_aspect;
                                                        } else {
                                                            // 太高，调整高度
                                                            new_size.y = new_size.x / target_aspect;
                                                        }
                                                    }
                                                }

                                                img.pos = new_pos;
                                                img.size = new_size;
                                            }
                                            CanvasObject::Text(text) => {
                                                // 文本调整大小比较复杂，暂时只支持移动
                                                // 可以考虑调整字体大小
                                                match resize_op.anchor {
                                                    ResizeAnchor::TopLeft
                                                    | ResizeAnchor::BottomRight => {
                                                        text.font_size = (resize_op.start_size.x
                                                            + delta.x)
                                                            .max(8.0);
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            CanvasObject::Shape(shape) => {
                                                let delta = pos - resize_op.start_pos;

                                                match resize_op.anchor {
                                                    ResizeAnchor::TopLeft
                                                    | ResizeAnchor::BottomRight => {
                                                        shape.size = (resize_op.start_size.x
                                                            + delta.x)
                                                            .max(10.0);
                                                    }
                                                    ResizeAnchor::TopRight
                                                    | ResizeAnchor::BottomLeft => {
                                                        shape.size = (resize_op.start_size.x
                                                            - delta.x)
                                                            .max(10.0);
                                                    }
                                                    ResizeAnchor::Top | ResizeAnchor::Bottom => {
                                                        shape.size = (resize_op.start_size.y
                                                            + delta.y)
                                                            .max(10.0);
                                                    }
                                                    ResizeAnchor::Left | ResizeAnchor::Right => {
                                                        shape.size = (resize_op.start_size.x
                                                            + delta.x)
                                                            .max(10.0);
                                                    }
                                                }
                                            }
                                            CanvasObject::Stroke(_) => {}
                                        }
                                    }
                                }
                            }
                            // 处理旋转操作
                            else if let Some(rotate_op) = self.state.rotation_operation {
                                if let Some(selected_idx) = self.state.selected_object {
                                    if let Some(object) =
                                        self.state.canvas.objects.get_mut(selected_idx)
                                    {
                                        // 计算当前角度
                                        let center = rotate_op.center;
                                        let current_dir = pos - center;
                                        let start_dir = rotate_op.start_pos - center;

                                        let current_angle = current_dir.y.atan2(current_dir.x);
                                        let start_angle = start_dir.y.atan2(start_dir.x);

                                        let angle_delta = current_angle - start_angle;

                                        match object {
                                            CanvasObject::Shape(shape) => {
                                                shape.rotation =
                                                    rotate_op.start_angle + angle_delta;
                                            }
                                            _ => {
                                                // 其他对象类型暂时不支持旋转
                                            }
                                        }
                                    }
                                }
                            }
                            // 处理普通拖动
                            else if let (Some(start_pos), Some(selected_idx)) =
                                (self.state.drag_start_pos, self.state.selected_object)
                            {
                                let delta = pos - start_pos;
                                self.state.drag_start_pos = Some(pos);

                                if let Some(object) =
                                    self.state.canvas.objects.get_mut(selected_idx)
                                {
                                    match object {
                                        CanvasObject::Image(img) => {
                                            img.pos += delta;
                                        }
                                        CanvasObject::Stroke(stroke) => {
                                            for p in &mut stroke.points {
                                                *p += delta;
                                            }
                                        }
                                        CanvasObject::Text(text) => {
                                            text.pos += delta;
                                        }
                                        CanvasObject::Shape(shape) => {
                                            shape.pos += delta;
                                        }
                                    }
                                }
                            }
                        }
                    } else if response.drag_stopped() {
                        // 结束调整大小或旋转操作
                        self.state.resize_operation = None;
                        self.state.rotation_operation = None;
                        self.state.drag_start_pos = None;
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
                            AppUtils::draw_size_preview(painter, pos, self.state.eraser_size);

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
                                        let shape_rect =
                                            AppUtils::calculate_shape_bounding_box(shape);
                                        if shape_rect.contains(pos) {
                                            to_remove.push(i);
                                        }
                                    }
                                    CanvasObject::Stroke(stroke) => {
                                        if AppUtils::point_intersects_stroke(
                                            pos,
                                            stroke,
                                            self.state.eraser_size,
                                        ) {
                                            to_remove.push(i);
                                        }
                                    }
                                }
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
                            AppUtils::draw_size_preview(painter, pos, self.state.eraser_size);

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
                                            AppUtils::point_to_line_segment_distance(pos, p1, p2);

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
                                        });
                                    }
                                } else {
                                    // 非笔画对象保留原样
                                    if let CanvasObject::Stroke(stroke) = object {
                                        new_strokes.push(stroke.clone());
                                    }
                                }
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
                                let width = AppUtils::calculate_dynamic_width(
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
                                        let width = AppUtils::calculate_dynamic_width(
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
                                        AppUtils::apply_stroke_smoothing(&active_stroke.points)
                                    } else {
                                        active_stroke.points
                                    };

                                    // 应用插值
                                    let (interpolated_points, interpolated_widths) =
                                        AppUtils::apply_point_interpolation(
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
                                self.state.canvas.objects.push(CanvasObject::Stroke(
                                    crate::state::CanvasStroke {
                                        points: vec![pos],
                                        widths: vec![self.state.brush_width],
                                        color: self.state.brush_color,
                                        base_width: self.state.brush_width,
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
                                        let straightened_points = AppUtils::straighten_stroke(
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

                                    let width = AppUtils::calculate_dynamic_width(
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

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();
        pollster::block_on(self.set_window(window));
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
                            let width = AppUtils::calculate_dynamic_width(
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
                                    let width = AppUtils::calculate_dynamic_width(
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
                                        AppUtils::apply_stroke_smoothing(&active_stroke.points)
                                    } else {
                                        active_stroke.points
                                    };

                                    // 应用插值
                                    let (interpolated_points, interpolated_widths) =
                                        AppUtils::apply_point_interpolation(
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
