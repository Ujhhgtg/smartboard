use crate::assets::ICON;
use crate::render::RenderState;
use crate::state::{AppState, CanvasObject, CanvasTool};
use crate::utils::stroke::{brush_stroke_add_point, brush_stroke_end, brush_stroke_start};
use crate::utils::ui::{apply_theme_mode_and_canvas_color, apply_window_mode};
use crate::{UserEvent, ui};
use core::f32;
use egui::Pos2;
use egui_wgpu::{ScreenDescriptor, wgpu};
use image::GenericImageView;
use std::sync::Arc;
use tray_icon::TrayIconBuilder;
use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, Touch, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

#[cfg(feature = "startup_animation")]
use crate::state::StartupAnimation;

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
            state.startup_animation = Some(StartupAnimation::new(
                30.0,
                crate::assets::STARTUP_FRAMES,
                crate::assets::STARTUP_AUDIO,
            ));
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
        if let Some(monitor) = monitor {
            self.state.fullscreen_video_modes = monitor.video_modes().collect();
        } else {
            eprintln!(
                "
error: failed to get monitor
       this is expected behaviour on wayland & web, do not switch to exclusive fullscreen mode"
            )
        }

        // 设置窗口模式
        apply_window_mode(&mut self.state, &window);

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
        apply_theme_mode_and_canvas_color(
            ctx,
            self.state.persistent.theme_mode,
            self.state.persistent.canvas_color,
        );

        self.window.get_or_insert(window);
        self.render_state.get_or_insert(state);
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.state.persistent.save_to_file() {
            eprintln!("failed to save settings: {}", err);
        }
        self.state.tray.take(); // closes tray
        event_loop.exit();
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

        // fix a borrow checker error
        // egui::Context is merely an Arc, and cloning is cheap
        let ctx = &(render_state.egui_renderer.context().clone());

        #[cfg(feature = "startup_animation")]
        if let Some(anim) = &mut self.state.startup_animation {
            if !anim.is_finished() {
                anim.update(ctx);
                anim.draw_fullscreen(ctx);
                ctx.request_repaint(); // ensure smooth playback
            }
        }

        self.state.toasts.show(ctx);

        // --- ui ---

        if self.state.show_welcome_window {
            ui::ui_welcome(&mut self.state, ctx);
        }

        ui::ui_toolbar(&mut self.state, ctx, window, render_state);

        ui::ui_pages_nav(&mut self.state, ctx);

        if self.state.show_page_management_window {
            ui::ui_pages_manager(&mut self.state, ctx);
        }

        ui::ui_canvas(&mut self.state, ctx);

        // --- end ui

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

        self.state.canvas.objects.retain(|obj| {
            if let CanvasObject::Image(img) = obj {
                !img.marked_for_deletion
            } else {
                true
            }
        });

        if self.state.persistent.show_fps {
            _ = self.state.fps_counter.update();
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(Window::default_attributes())
            .unwrap();
        pollster::block_on(self.set_window(window));
        // redraw on window creation
        self.window.as_ref().unwrap().request_redraw();
    }

    // redraw if egui requests repaint
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if self.state.should_quit {
            return;
        }

        if let Some(render_state) = self.render_state.as_ref() {
            if render_state.egui_renderer.context().has_requested_repaint() {
                self.window.as_ref().unwrap().request_redraw();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::TrayIconEvent(event) => {
                if let tray_icon::TrayIconEvent::Click { .. } = event {
                    let window = self.window.as_ref().unwrap();
                    window.set_visible(true);
                    window.focus_window();
                    if let Some(tray) = &self.state.tray {
                        let _ = tray.set_visible(false);
                    }
                    // redraw on tray click
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

        // redraw only on input
        // don't pass RedrawRequested to egui's input handler,
        // it's not input and would make egui request a repaint, causing an infinite redraw loop
        if self.state.persistent.force_redraw_every_frame
            || !matches!(event, WindowEvent::RedrawRequested)
        {
            let egui_needs_repaint = self
                .render_state
                .as_mut()
                .unwrap()
                .egui_renderer
                .handle_input(self.window.as_ref().unwrap(), &event);

            if self.state.persistent.force_redraw_every_frame || egui_needs_repaint {
                self.window.as_ref().unwrap().request_redraw();
            }
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
                self.state.touch_used = true;

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
