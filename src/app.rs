use crate::assets::ICON;
use crate::render::RenderState;
#[cfg(feature = "startup_animation")]
use crate::state::StartupAnimation;
use crate::state::{
    AppState, CanvasObject, CanvasObjectOps, CanvasTool, PointerInteraction, PointerState,
};
use crate::ui;
use crate::utils;
use crate::utils::stroke::{brush_stroke_add_point, brush_stroke_end, brush_stroke_start};
use crate::utils::ui::{apply_theme_mode_and_canvas_color, apply_window_mode};
use core::f32;
use egui::{Pos2, Vec2};
use egui_wgpu::{ScreenDescriptor, wgpu};
use image::GenericImageView;
use std::sync::Arc;
use wgpu::{Backends, TexelCopyTextureInfo};
use wgpu::{CurrentSurfaceTexture, InstanceDescriptor, TexelCopyBufferInfo, TexelCopyBufferLayout};
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, Touch, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

pub struct App {
    gpu_instance: wgpu::Instance,
    render_state: Option<RenderState>,
    window: Option<Arc<Window>>,
    state: AppState,
}

impl App {
    pub fn new() -> Self {
        let mut state = AppState::default();
        let gpu_instance = wgpu::Instance::new(InstanceDescriptor {
            backends: if cfg!(target_os = "windows") { // on windows, using vulkan results in a hang after resizing the window, so we default to dx12 which is more stable
                Backends::DX12
            } else { state.persistent.graphics_api.to_backends() },
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

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
        window.set_title("uwu");
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
            .or_else(|| window.primary_monitor())
            .or_else(|| window.available_monitors().next());
        if let Some(monitor) = monitor {
            self.state.fullscreen_video_modes = monitor.video_modes().collect();
        } else {
            eprintln!("error: failed to get monitor, exclusive fullscreen mode will be unavailable")
        }

        // 设置窗口模式
        apply_window_mode(&mut self.state, &window);

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

        self.state.active_backend = Some(state.device.adapter_info().backend);

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
        event_loop.exit();
    }

    fn handle_resized(&mut self, width: u32, height: u32) {
        self.render_state
            .as_mut()
            .unwrap()
            .resize_surface(width, height);
    }

    #[cfg_attr(feature = "profiling", profiling::function)]
    fn handle_redraw(&mut self) {
        #[cfg(feature = "profiling")]
        profiling::scope!("handle_redraw::setup");

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
            CurrentSurfaceTexture::Suboptimal(surface) => {
                println!("warning: wgpu surface suboptimal");
                surface
            }
            val => {
                println!("warning: wgpu surface {:?}", val);
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

        // --- ui ---
        {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::ui");

            // fixes a borrow checker bug
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

            #[cfg(feature = "profiling")]
            puffin_egui::profiler_window(ctx);

            if self.state.show_welcome_window {
                ui::ui_welcome(&mut self.state, ctx);
            }

            ui::ui_toolbar(&mut self.state, ctx, window);

            ui::ui_pages_nav(&mut self.state, ctx);

            if self.state.show_page_management_window {
                ui::ui_pages_manager(&mut self.state, ctx);
            }

            ui::ui_canvas(&mut self.state, ctx);
        }

        // access this value in next redraw before ui to ensure that all ui has become invisible
        let screenshot_path = self.state.screenshot_path.clone();
        // --- end ui

        // egui render pass
        {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::render_pass");

            render_state.egui_renderer.end_frame_and_draw(
                &render_state.device,
                &render_state.queue,
                &mut encoder,
                window,
                &surface_view,
                screen_descriptor,
            );
        }

        // submit & present texture
        if let Some(path) = screenshot_path {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::screenshot");

            let width = render_state.surface_config.width;
            let height = render_state.surface_config.height;

            let bytes_per_pixel = 4;
            let unpadded_bytes_per_row = width * bytes_per_pixel;

            // wgpu requires 256-byte alignment
            const ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(ALIGN) * ALIGN;

            let buffer_size = (padded_bytes_per_row * height) as u64;

            let output_buffer = render_state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("screenshot buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });

            encoder.copy_texture_to_buffer(
                TexelCopyTextureInfo {
                    texture: &surface_texture.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                TexelCopyBufferInfo {
                    buffer: &output_buffer,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(padded_bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );

            render_state.queue.submit(Some(encoder.finish()));

            let buffer_slice = output_buffer.slice(..);

            buffer_slice.map_async(wgpu::MapMode::Read, |_| {});

            // ensure gpu work is done
            let _ = render_state.device.poll(wgpu::wgt::PollType::Wait {
                submission_index: None,
                timeout: None,
            });

            let data = buffer_slice.get_mapped_range();

            let mut pixels = vec![0u8; (width * height * 4) as usize];

            for y in 0..height as usize {
                let src_offset = y * padded_bytes_per_row as usize;
                let dst_offset = y * unpadded_bytes_per_row as usize;

                pixels[dst_offset..dst_offset + unpadded_bytes_per_row as usize].copy_from_slice(
                    &data[src_offset..src_offset + unpadded_bytes_per_row as usize],
                );
            }

            // pixels
            //     .chunks_exact(width as usize * 4)
            //     .collect::<Vec<_>>()
            //     .into_iter()
            //     .rev()
            //     .flatten()
            //     .copied()
            //     .collect::<Vec<u8>>();

            for chunk in pixels.chunks_exact_mut(4) {
                chunk.swap(0, 2); // B ↔ R
            }

            match image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8) {
                Ok(_) => {
                    self.state.toasts.success("成功导出为图片!");
                }
                Err(err) => {
                    self.state.toasts.error(format!("画布导出失败: {}!", err));
                }
            }

            drop(data);
            output_buffer.unmap();

            self.state.screenshot_path = None;
        } else {
            render_state.queue.submit(Some(encoder.finish()));
        }

        {
            #[cfg(feature = "profiling")]
            profiling::scope!("handle_redraw::gc");

            self.state.canvas.objects.retain(|obj| {
                if let CanvasObject::Image(img) = obj {
                    !img.marked_for_deletion
                } else {
                    true
                }
            });
        }

        surface_texture.present();

        if self.state.present_mode_changed {
            render_state.set_present_mode(self.state.persistent.present_mode);
            self.state.present_mode_changed = false;
        }

        if self.state.persistent.show_fps {
            _ = self.state.fps_counter.update();
        }

        #[cfg(feature = "profiling")]
        profiling::finish_frame!();
    }
}

impl ApplicationHandler<()> for App {
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
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
                // TODO: issue #1 is caused by get_current_texture() lagging after surface reconfigure
                //       although we haven't fix that, we avoid the reconfigure on minimize/resume to prevent that bug from occurring
                //       when that actual cause is fixed, we can probably remove this guard
                let surface_config = &self.render_state.as_ref().unwrap().surface_config;
                if (new_size.width != surface_config.width
                    || new_size.height != surface_config.height)
                    && new_size.width > 0
                    && new_size.height > 0
                {
                    self.handle_resized(new_size.width, new_size.height);
                    self.window.as_ref().unwrap().request_redraw();
                }
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
                let pos = Pos2::new(
                    location.x as f32 / scale_factor,
                    location.y as f32 / scale_factor,
                );

                match phase {
                    TouchPhase::Started => match self.state.current_tool {
                        CanvasTool::Brush => {
                            brush_stroke_start(&mut self.state, id, pos);
                        }
                        CanvasTool::Select
                            if !self.state.pointers.values().any(|p| {
                                matches!(p.interaction, PointerInteraction::Selecting { .. })
                            }) =>
                        {
                            // Hit-test objects (last to first for z-order)
                            for (i, object) in self.state.canvas.objects.iter().enumerate().rev() {
                                if object.bounding_box().contains(pos) {
                                    self.state.selected_object_index = Some(i);
                                    break;
                                }
                            }

                            let (dragged_handle, drag_original_transform) = if let Some(idx) =
                                self.state.selected_object_index
                                && idx < self.state.canvas.objects.len()
                            {
                                let object = &self.state.canvas.objects[idx];
                                let bbox = object.bounding_box();
                                let handle = utils::get_transform_handle_at_pos(bbox, pos);
                                let transform = handle.is_some().then(|| object.get_transform());
                                (handle, transform)
                            } else {
                                (None, None)
                            };

                            self.state.pointers.insert(
                                id,
                                PointerState {
                                    id,
                                    pos,
                                    interaction: PointerInteraction::Selecting {
                                        drag_start: pos,
                                        dragged_handle,
                                        drag_original_transform,
                                        drag_accumulated_delta: Vec2::ZERO,
                                    },
                                },
                            );
                        }
                        CanvasTool::ObjectEraser | CanvasTool::PixelEraser => {
                            self.state.pointers.insert(
                                id,
                                PointerState {
                                    id,
                                    pos,
                                    interaction: PointerInteraction::Erasing,
                                },
                            );
                        }
                        _ => {}
                    },
                    TouchPhase::Moved => match self.state.current_tool {
                        CanvasTool::Brush => {
                            brush_stroke_add_point(&mut self.state, id, pos, false);
                        }
                        CanvasTool::Select => {
                            if let Some(pointer) = self.state.pointers.get_mut(&id) {
                                pointer.pos = pos;

                                if let PointerInteraction::Selecting {
                                    ref mut drag_start,
                                    dragged_handle,
                                    ref mut drag_accumulated_delta,
                                    ..
                                } = pointer.interaction
                                {
                                    let delta = pos - *drag_start;

                                    if let Some(idx) = self.state.selected_object_index
                                        && idx < self.state.canvas.objects.len()
                                    {
                                        if let Some(handle) = dragged_handle {
                                            if let Some(object) =
                                                self.state.canvas.objects.get_mut(idx)
                                            {
                                                object.transform(handle, delta, *drag_start, pos);
                                            }
                                        } else {
                                            if let Some(object) =
                                                self.state.canvas.objects.get_mut(idx)
                                            {
                                                CanvasObject::move_object(object, delta);
                                            }
                                            *drag_accumulated_delta += delta;
                                        }
                                    }

                                    *drag_start = pos;
                                }
                            }
                        }
                        CanvasTool::ObjectEraser | CanvasTool::PixelEraser => {
                            if let Some(pointer) = self.state.pointers.get_mut(&id) {
                                pointer.pos = pos;
                            }
                        }
                        _ => {}
                    },
                    TouchPhase::Ended | TouchPhase::Cancelled => match self.state.current_tool {
                        CanvasTool::Brush => {
                            brush_stroke_end(&mut self.state, id);
                        }
                        CanvasTool::Select => {
                            if let Some(pointer) = self.state.pointers.get(&id) {
                                if let PointerInteraction::Selecting {
                                    drag_accumulated_delta,
                                    drag_original_transform,
                                    ..
                                } = &pointer.interaction
                                {
                                    if let Some(sel_idx) = self.state.selected_object_index {
                                        if *drag_accumulated_delta != Vec2::ZERO {
                                            self.state.history.save_move_object(
                                                sel_idx,
                                                -*drag_accumulated_delta,
                                                *drag_accumulated_delta,
                                            );
                                        }
                                    }
                                    if let Some(original) = drag_original_transform.clone() {
                                        if let Some(sel_idx) = self.state.selected_object_index
                                            && sel_idx < self.state.canvas.objects.len()
                                        {
                                            let new_transform =
                                                self.state.canvas.objects[sel_idx].get_transform();
                                            self.state.history.save_transform_object(
                                                sel_idx,
                                                original,
                                                new_transform,
                                            );
                                        }
                                    }
                                }
                            }
                            self.state.pointers.remove(&id);
                        }
                        CanvasTool::ObjectEraser | CanvasTool::PixelEraser => {
                            self.state.pointers.remove(&id);
                        }
                        _ => {}
                    },
                }

                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}
