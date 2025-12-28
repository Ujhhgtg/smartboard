use crate::egui_tools::EguiRenderer;
use egui_wgpu::wgpu::{ExperimentalFeatures, SurfaceError};
use egui_wgpu::{ScreenDescriptor, wgpu};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};
use egui::{Color32, Pos2, Shape, Stroke};

pub struct AppState {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface: wgpu::Surface<'static>,
    pub scale_factor: f32,
    pub egui_renderer: EguiRenderer,
}

// 绘图数据结构
#[derive(Clone)]
pub struct DrawingStroke {
    pub points: Vec<Pos2>,
    pub color: Color32,
    pub width: f32,
}

pub struct DrawingState {
    pub strokes: Vec<DrawingStroke>,
    pub current_stroke: Option<Vec<Pos2>>,
    pub is_drawing: bool,
    pub brush_color: Color32,
    pub brush_width: f32,
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
}

impl App {
    pub fn new() -> Self {
        let instance = egui_wgpu::wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        Self {
            instance,
            state: None,
            window: None,
            drawing_state: DrawingState {
                strokes: Vec::new(),
                current_stroke: None,
                is_drawing: false,
                brush_color: Color32::BLACK,
                brush_width: 2.0,
            },
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
                    ui.horizontal(|ui| {
                        ui.label("颜色:");
                        let old_color = self.drawing_state.brush_color;
                        if ui.color_edit_button_srgba(&mut self.drawing_state.brush_color).changed() {
                            // 颜色改变时，如果正在绘制，结束当前笔画（使用旧颜色）
                            if self.drawing_state.is_drawing {
                                if let Some(points) = self.drawing_state.current_stroke.take() {
                                    if points.len() > 1 {
                                        self.drawing_state.strokes.push(DrawingStroke {
                                            points,
                                            color: old_color,
                                            width: self.drawing_state.brush_width,
                                        });
                                    }
                                }
                                self.drawing_state.is_drawing = false;
                            }
                        }
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("画笔宽度:");
                        ui.add(egui::Slider::new(&mut self.drawing_state.brush_width, 1.0..=20.0));
                    });

                    ui.separator();

                    if ui.button("清除画布").clicked() {
                        self.drawing_state.strokes.clear();
                        self.drawing_state.current_stroke = None;
                        self.drawing_state.is_drawing = false;
                    }
                });

            // 主画布区域
            egui::CentralPanel::default().show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(
                    ui.available_size(),
                    egui::Sense::click_and_drag(),
                );

                let painter = ui.painter();
                
                // 绘制背景
                painter.rect_filled(rect, 0.0, Color32::WHITE);

                // 绘制所有已完成的笔画 - 使用路径避免黑色边框
                for stroke in &self.drawing_state.strokes {
                    if stroke.points.len() < 2 {
                        continue;
                    }
                    if stroke.points.len() == 2 {
                        // 只有两个点，直接画线段
                        painter.line_segment(
                            [stroke.points[0], stroke.points[1]],
                            Stroke::new(stroke.width, stroke.color),
                        );
                    } else {
                        // 多个点，使用路径绘制平滑曲线
                        let path = egui::epaint::PathShape::line(stroke.points.clone(), Stroke::new(stroke.width, stroke.color));
                        painter.add(Shape::Path(path));
                    }
                }

                // 绘制当前正在绘制的笔画 - 使用路径避免黑色边框
                if let Some(ref points) = self.drawing_state.current_stroke {
                    if points.len() >= 2 {
                        if points.len() == 2 {
                            // 只有两个点，直接画线段
                            painter.line_segment(
                                [points[0], points[1]],
                                Stroke::new(
                                    self.drawing_state.brush_width,
                                    self.drawing_state.brush_color,
                                ),
                            );
                        } else {
                            // 多个点，使用路径绘制平滑曲线
                            let path = egui::epaint::PathShape::line(points.clone(), Stroke::new(
                                self.drawing_state.brush_width,
                                self.drawing_state.brush_color,
                            ));
                            painter.add(Shape::Path(path));
                        }
                    }
                }

                // 处理鼠标输入
                let pointer_pos = response.interact_pointer_pos();
                
                if response.drag_started() {
                    // 开始新的笔画
                    if let Some(pos) = pointer_pos {
                        if pos.x >= rect.min.x && pos.x <= rect.max.x 
                            && pos.y >= rect.min.y && pos.y <= rect.max.y {
                            self.drawing_state.is_drawing = true;
                            self.drawing_state.current_stroke = Some(vec![pos]);
                        }
                    }
                } else if response.dragged() {
                    // 继续绘制
                    if self.drawing_state.is_drawing {
                        if let Some(pos) = pointer_pos {
                            if let Some(ref mut points) = self.drawing_state.current_stroke {
                                // 只添加与上一个点距离足够远的点，避免点太密集
                                if points.is_empty() || 
                                    points.last().unwrap().distance(pos) > 1.0 {
                                    points.push(pos);
                                }
                            }
                        }
                    }
                } else if response.drag_stopped() {
                    // 结束当前笔画
                    if self.drawing_state.is_drawing {
                        if let Some(points) = self.drawing_state.current_stroke.take() {
                            if points.len() > 1 {
                                self.drawing_state.strokes.push(DrawingStroke {
                                    points,
                                    color: self.drawing_state.brush_color,
                                    width: self.drawing_state.brush_width,
                                });
                            }
                        }
                        self.drawing_state.is_drawing = false;
                    }
                }

                // 如果鼠标在画布内移动且正在绘制，也添加点（用于平滑绘制）
                if response.hovered() && self.drawing_state.is_drawing {
                    if let Some(pos) = pointer_pos {
                        if let Some(ref mut points) = self.drawing_state.current_stroke {
                            if points.is_empty() || 
                                points.last().unwrap().distance(pos) > 1.0 {
                                points.push(pos);
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
