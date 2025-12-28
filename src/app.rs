use crate::render::RenderState;
use crate::state::{
    AppState, DynamicBrushMode, InsertedImage, InsertedShape, InsertedText, SelectedObject,
    ShapeType, Tool,
};
use crate::utils::AppUtils;
use egui::{Color32, Pos2, Shape, Stroke};
use egui_wgpu::wgpu::SurfaceError;
use egui_wgpu::{ScreenDescriptor, wgpu};
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{KeyEvent, Touch, TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

pub struct App {
    instance: wgpu::Instance,
    render_state: Option<RenderState>,
    window: Option<Arc<Window>>,
    state: AppState,
}

impl App {
    pub fn new() -> Self {
        let instance = egui_wgpu::wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        Self {
            instance,
            render_state: None,
            window: None,
            state: AppState::default(),
        }
    }

    pub async fn set_window(&mut self, window: Window) {
        let window = Arc::new(window);

        // 设置标题
        window.set_title("smartboard");

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

        let state = RenderState::new(
            &self.instance,
            surface,
            &window,
            initial_width,
            initial_height,
        )
        .await;

        self.window.get_or_insert(window);
        self.render_state.get_or_insert(state);
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

        let state = self.render_state.as_mut().unwrap();

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

            // 工具栏窗口
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
                        let old_tool = self.state.current_tool;
                        if ui
                            .selectable_value(&mut self.state.current_tool, Tool::Select, "选择")
                            .changed()
                            || ui
                                .selectable_value(&mut self.state.current_tool, Tool::Brush, "画笔")
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.state.current_tool,
                                    Tool::ObjectEraser,
                                    "对象橡皮擦",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.state.current_tool,
                                    Tool::PixelEraser,
                                    "像素橡皮擦",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.state.current_tool,
                                    Tool::Insert,
                                    "插入",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.state.current_tool,
                                    Tool::Background,
                                    "背景",
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.state.current_tool,
                                    Tool::Settings,
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
                    if self.state.current_tool == Tool::Brush {
                        ui.horizontal(|ui| {
                            ui.label("颜色:");
                            let old_color = self.state.brush_color;
                            if ui
                                .color_edit_button_srgba(&mut self.state.brush_color)
                                .changed()
                            {
                                // 颜色改变时，如果正在绘制，结束当前笔画（使用旧颜色）
                                if self.state.is_drawing {
                                    if let Some(points) = self.state.current_stroke.take() {
                                        if let Some(widths) =
                                            self.state.current_stroke_widths.take()
                                        {
                                            if points.len() > 1 {
                                                self.state.strokes.push(
                                                    crate::state::DrawingStroke {
                                                        points,
                                                        widths,
                                                        color: old_color,
                                                        base_width: self.state.brush_width,
                                                    },
                                                );
                                            }
                                        }
                                    }
                                    self.state.current_stroke_times = None;
                                    self.state.stroke_start_time = None;
                                    self.state.is_drawing = false;
                                }
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("画笔宽度:");
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

                        ui.separator();

                        ui.horizontal(|ui| {
                            ui.label("动态画笔宽度微调:");
                            ui.selectable_value(
                                &mut self.state.dynamic_brush_mode,
                                DynamicBrushMode::Disabled,
                                "禁用",
                            );
                            ui.selectable_value(
                                &mut self.state.dynamic_brush_mode,
                                DynamicBrushMode::BrushTip,
                                "模拟笔锋",
                            );
                            ui.selectable_value(
                                &mut self.state.dynamic_brush_mode,
                                DynamicBrushMode::SpeedBased,
                                "基于速度",
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label("笔迹平滑:");
                            ui.checkbox(&mut self.state.stroke_smoothing, "启用");
                        });
                    }

                    // 橡皮擦相关设置
                    if self.state.current_tool == Tool::ObjectEraser
                        || self.state.current_tool == Tool::PixelEraser
                    {
                        ui.horizontal(|ui| {
                            ui.label("橡皮擦大小:");
                            let slider_response =
                                ui.add(egui::Slider::new(&mut self.state.eraser_size, 5.0..=50.0));

                            ui.separator();

                            // 显示大小预览
                            if slider_response.dragged() || slider_response.hovered() {
                                self.state.show_size_preview = true;
                                // 使用屏幕中心位置
                            } else if !slider_response.dragged() && !slider_response.hovered() {
                                self.state.show_size_preview = false;
                            }

                            if ui.button("清空画布").clicked() {
                                self.state.strokes.clear();
                                self.state.images.clear();
                                self.state.texts.clear();
                                self.state.current_stroke = None;
                                self.state.is_drawing = false;
                                self.state.selected_object = None;
                            }
                        });
                    }

                    // 插入工具相关设置
                    if self.state.current_tool == Tool::Insert {
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

                                        self.state.images.push(InsertedImage {
                                            texture,
                                            pos: Pos2::new(100.0, 100.0),
                                            size: egui::vec2(target_width, target_height),
                                            aspect_ratio,
                                            marked_for_deletion: false,
                                        });
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
                                            self.state.texts.push(InsertedText {
                                                text: self.state.new_text_content.clone(),
                                                pos: Pos2::new(100.0, 100.0),
                                                color: Color32::WHITE,
                                                font_size: 16.0,
                                            });
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
                                            self.state.shapes.push(InsertedShape {
                                                shape_type: ShapeType::Line,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            });
                                            self.state.show_shape_dialog = false;
                                        }

                                        if ui.button("箭头").clicked() {
                                            self.state.shapes.push(InsertedShape {
                                                shape_type: ShapeType::Arrow,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            });
                                            self.state.show_shape_dialog = false;
                                        }

                                        if ui.button("矩形").clicked() {
                                            self.state.shapes.push(InsertedShape {
                                                shape_type: ShapeType::Rectangle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            });
                                            self.state.show_shape_dialog = false;
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        if ui.button("三角形").clicked() {
                                            self.state.shapes.push(InsertedShape {
                                                shape_type: ShapeType::Triangle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            });
                                            self.state.show_shape_dialog = false;
                                        }

                                        if ui.button("圆形").clicked() {
                                            self.state.shapes.push(InsertedShape {
                                                shape_type: ShapeType::Circle,
                                                pos: Pos2::new(100.0, 100.0),
                                                size: 100.0,
                                                color: Color32::WHITE,
                                                rotation: 0.0,
                                            });
                                            self.state.show_shape_dialog = false;
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        if ui.button("取消").clicked() {
                                            self.state.show_shape_dialog = false;
                                        }
                                    });
                                });
                        }
                    }

                    // 背景工具相关设置
                    if self.state.current_tool == Tool::Background {
                        ui.horizontal(|ui| {
                            ui.label("颜色:");
                            ui.color_edit_button_srgba(&mut self.state.background_color);
                        });
                    }

                    // 设置工具相关设置
                    if self.state.current_tool == Tool::Settings {
                        ui.horizontal(|ui| {
                            ui.label("显示 FPS:");
                            ui.checkbox(&mut self.state.show_fps, "启用");
                            if ui.button("调试: 引发异常").clicked() {
                                panic!("test panic")
                            }
                        });
                    }

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("退出").clicked() {
                            self.state.should_quit = true;
                        }
                        if self.state.show_fps {
                            ui.label(format!(
                                "FPS: {}",
                                self.state.fps_counter.current_fps.to_string()
                            ));
                        }
                    });
                });

            // 主画布区域
            egui::CentralPanel::default().show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());

                let painter = ui.painter();

                // 绘制背景
                painter.rect_filled(rect, 0.0, self.state.background_color);

                // 绘制所有图片（跳过已标记为删除的图片）
                for (i, img) in self.state.images.iter().enumerate() {
                    if img.marked_for_deletion {
                        continue;
                    }

                    let img_rect = egui::Rect::from_min_size(img.pos, img.size);
                    painter.image(
                        img.texture.id(),
                        img_rect,
                        egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );

                    // 如果被选中，绘制边框
                    if let Some(SelectedObject::Image(selected_idx)) = self.state.selected_object {
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
                for (i, text) in self.state.texts.iter().enumerate() {
                    // Draw text using egui's text rendering
                    let text_galley = painter.layout_no_wrap(
                        text.text.clone(),
                        egui::FontId::proportional(text.font_size),
                        text.color,
                    );
                    let text_shape = egui::epaint::TextShape {
                        pos: text.pos,
                        galley: text_galley.clone(),
                        underline: egui::Stroke::NONE,
                        override_text_color: None,
                        angle: 0.0,
                        fallback_color: text.color,
                        opacity_factor: 1.0,
                    };
                    painter.add(text_shape);

                    if let Some(SelectedObject::Text(selected_idx)) = self.state.selected_object {
                        if i == selected_idx {
                            let text_size = text_galley.size();
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

                // 绘制所有形状
                for (i, shape) in self.state.shapes.iter().enumerate() {
                    // 绘制形状本身
                    match shape.shape_type {
                        ShapeType::Line => {
                            let end_point = Pos2::new(shape.pos.x + shape.size, shape.pos.y);
                            painter.line_segment(
                                [shape.pos, end_point],
                                Stroke::new(2.0, shape.color),
                            );
                        }
                        ShapeType::Arrow => {
                            let end_point = Pos2::new(shape.pos.x + shape.size, shape.pos.y);
                            painter.line_segment(
                                [shape.pos, end_point],
                                Stroke::new(2.0, shape.color),
                            );

                            // 绘制箭头头部
                            let arrow_size = shape.size * 0.1;
                            let arrow_angle = std::f32::consts::PI / 6.0; // 30度
                            let arrow_point1 = Pos2::new(
                                end_point.x - arrow_size * arrow_angle.cos(),
                                end_point.y - arrow_size * arrow_angle.sin(),
                            );
                            let arrow_point2 = Pos2::new(
                                end_point.x - arrow_size * arrow_angle.cos(),
                                end_point.y + arrow_size * arrow_angle.sin(),
                            );

                            painter.line_segment(
                                [end_point, arrow_point1],
                                Stroke::new(2.0, shape.color),
                            );
                            painter.line_segment(
                                [end_point, arrow_point2],
                                Stroke::new(2.0, shape.color),
                            );
                        }
                        ShapeType::Rectangle => {
                            let rect = egui::Rect::from_min_size(
                                shape.pos,
                                egui::vec2(shape.size, shape.size),
                            );
                            painter.rect_stroke(
                                rect,
                                0.0,
                                Stroke::new(2.0, shape.color),
                                egui::StrokeKind::Outside,
                            );
                        }
                        ShapeType::Triangle => {
                            let half_size = shape.size / 2.0;
                            let points = [
                                shape.pos,
                                Pos2::new(shape.pos.x + shape.size, shape.pos.y),
                                Pos2::new(shape.pos.x + half_size, shape.pos.y + half_size),
                            ];
                            painter.add(egui::Shape::convex_polygon(
                                points.to_vec(),
                                shape.color,
                                Stroke::new(2.0, shape.color),
                            ));
                        }
                        ShapeType::Circle => {
                            painter.circle_stroke(
                                shape.pos,
                                shape.size / 2.0,
                                Stroke::new(2.0, shape.color),
                            );
                        }
                    }

                    // 如果被选中，绘制边框
                    if let Some(SelectedObject::Shape(selected_idx)) = self.state.selected_object {
                        if i == selected_idx {
                            let shape_rect = AppUtils::calculate_shape_bounding_box(shape);

                            painter.rect_stroke(
                                shape_rect,
                                0.0,
                                Stroke::new(2.0, Color32::BLUE),
                                egui::StrokeKind::Outside,
                            );
                        }
                    }
                }

                // 绘制所有已完成的笔画 - 支持动态宽度
                for (i, stroke) in self.state.strokes.iter().enumerate() {
                    if stroke.points.len() < 2 {
                        continue;
                    }

                    let color = if let Some(SelectedObject::Stroke(selected_idx)) =
                        self.state.selected_object
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
                if let Some(ref points) = self.state.current_stroke {
                    if let Some(ref widths) = self.state.current_stroke_widths {
                        if points.len() >= 2 && widths.len() == points.len() {
                            // 检查是否所有宽度相同
                            let all_same_width =
                                widths.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01);

                            if all_same_width && points.len() == 2 {
                                // 只有两个点且宽度相同
                                painter.line_segment(
                                    [points[0], points[1]],
                                    Stroke::new(widths[0], self.state.brush_color),
                                );
                            } else if all_same_width {
                                // 多个点但宽度相同
                                let path = egui::epaint::PathShape::line(
                                    points.clone(),
                                    Stroke::new(widths[0], self.state.brush_color),
                                );
                                painter.add(Shape::Path(path));
                            } else {
                                // 宽度不同，分段绘制
                                for i in 0..points.len() - 1 {
                                    let avg_width = (widths[i] + widths[i + 1]) / 2.0;
                                    painter.line_segment(
                                        [points[i], points[i + 1]],
                                        Stroke::new(avg_width, self.state.brush_color),
                                    );
                                }
                            }
                        }
                    }
                }

                // 绘制大小预览圆圈
                if self.state.show_size_preview {
                    let content_rect = ui.ctx().available_rect();
                    let pos = content_rect.center();
                    AppUtils::draw_size_preview(painter, pos, self.state.brush_width);
                }

                // 绘制多点触控点
                for (id, pos) in &self.state.touch_points {
                    // 绘制触控点
                    painter.circle_filled(*pos, 15.0, Color32::from_rgba_unmultiplied(255, 255, 255, 180));
                    painter.circle_stroke(*pos, 15.0, Stroke::new(2.0, Color32::BLUE));

                    // 绘制触控ID
                    let text_galley = painter.layout_no_wrap(
                        format!("{}", id),
                        egui::FontId::proportional(14.0),
                        Color32::BLACK,
                    );
                    let text_pos = Pos2::new(pos.x - text_galley.size().x / 2.0, pos.y - text_galley.size().y / 2.0);
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

                // 处理鼠标输入
                let pointer_pos = response.interact_pointer_pos();

                match self.state.current_tool {
                    Tool::Insert | Tool::Background | Tool::Settings => {}

                    Tool::Select => {
                        if response.drag_started() {
                            if let Some(pos) = pointer_pos {
                                self.state.drag_start_pos = Some(pos);
                                self.state.selected_object = None;

                                // 检查图片
                                for (i, img) in self.state.images.iter().enumerate().rev() {
                                    let img_rect = egui::Rect::from_min_size(img.pos, img.size);
                                    if img_rect.contains(pos) {
                                        self.state.selected_object =
                                            Some(crate::state::SelectedObject::Image(i));
                                        break;
                                    }
                                }

                                // 检查文本
                                for (i, text) in self.state.texts.iter().enumerate().rev() {
                                    // 使用 layout_no_wrap 来计算文本大小（不渲染）
                                    let text_galley = painter.layout_no_wrap(
                                        text.text.clone(),
                                        egui::FontId::proportional(text.font_size),
                                        text.color,
                                    );
                                    let text_size = text_galley.size();

                                    let text_rect = egui::Rect::from_min_size(text.pos, text_size);
                                    if text_rect.contains(pos) {
                                        self.state.selected_object =
                                            Some(crate::state::SelectedObject::Text(i));
                                        break;
                                    }
                                }

                                // 检查形状
                                if self.state.selected_object.is_none() {
                                    for (i, shape) in self.state.shapes.iter().enumerate().rev() {
                                        let shape_rect =
                                            AppUtils::calculate_shape_bounding_box(shape);

                                        if shape_rect.contains(pos) {
                                            self.state.selected_object =
                                                Some(crate::state::SelectedObject::Shape(i));
                                            break;
                                        }
                                    }
                                }

                                // 检查笔画
                                if self.state.selected_object.is_none() {
                                    for (i, stroke) in self.state.strokes.iter().enumerate().rev() {
                                        if AppUtils::point_intersects_stroke(pos, stroke, 10.0) {
                                            self.state.selected_object =
                                                Some(crate::state::SelectedObject::Stroke(i));
                                            break;
                                        }
                                    }
                                }
                            }
                        } else if response.clicked() {
                            // 点击非对象区域时取消选择
                            if let Some(pos) = pointer_pos {
                                let mut hit = false;
                                for img in &self.state.images {
                                    if egui::Rect::from_min_size(img.pos, img.size).contains(pos) {
                                        hit = true;
                                        break;
                                    }
                                }
                                if !hit {
                                    for stroke in &self.state.strokes {
                                        if AppUtils::point_intersects_stroke(pos, stroke, 10.0) {
                                            hit = true;
                                            break;
                                        }
                                    }
                                }
                                if !hit {
                                    self.state.selected_object = None;
                                }
                            }
                        } else if response.dragged() {
                            if let (Some(pos), Some(start_pos)) =
                                (pointer_pos, self.state.drag_start_pos)
                            {
                                let delta = pos - start_pos;
                                self.state.drag_start_pos = Some(pos);

                                match self.state.selected_object {
                                    Some(crate::state::SelectedObject::Image(idx)) => {
                                        if let Some(img) = self.state.images.get_mut(idx) {
                                            img.pos += delta;
                                        }
                                    }
                                    Some(crate::state::SelectedObject::Stroke(idx)) => {
                                        if let Some(stroke) = self.state.strokes.get_mut(idx) {
                                            for p in &mut stroke.points {
                                                *p += delta;
                                            }
                                        }
                                    }
                                    Some(crate::state::SelectedObject::Text(idx)) => {
                                        if let Some(text) = self.state.texts.get_mut(idx) {
                                            text.pos += delta;
                                        }
                                    }
                                    Some(crate::state::SelectedObject::Shape(idx)) => {
                                        if let Some(shape) = self.state.shapes.get_mut(idx) {
                                            shape.pos += delta;
                                        }
                                    }
                                    None => {}
                                }
                            }
                        }
                    }

                    Tool::ObjectEraser => {
                        // 对象橡皮擦：点击或拖拽时删除相交的整个对象
                        if response.drag_started() || response.clicked() || response.dragged() {
                            if let Some(pos) = pointer_pos {
                                // 绘制指针
                                AppUtils::draw_size_preview(painter, pos, self.state.eraser_size);

                                // 从后往前删除，避免索引问题

                                // 标记图片为删除（延迟删除以避免Vulkan资源冲突）
                                for img in &mut self.state.images {
                                    let img_rect = egui::Rect::from_min_size(img.pos, img.size);
                                    if img_rect.contains(pos) {
                                        img.marked_for_deletion = true;
                                    }
                                }

                                // 删除文本
                                let mut to_remove_texts = Vec::new();
                                for (i, text) in self.state.texts.iter().enumerate().rev() {
                                    // 使用 layout_no_wrap 来计算文本大小（不渲染）
                                    let text_galley = painter.layout_no_wrap(
                                        text.text.clone(),
                                        egui::FontId::proportional(text.font_size),
                                        text.color,
                                    );
                                    let text_size = text_galley.size();
                                    let text_rect = egui::Rect::from_min_size(text.pos, text_size);
                                    if text_rect.contains(pos) {
                                        to_remove_texts.push(i);
                                    }
                                }
                                for i in to_remove_texts {
                                    self.state.texts.remove(i);
                                }

                                // 删除形状
                                let mut to_remove_shapes = Vec::new();
                                for (i, shape) in self.state.shapes.iter().enumerate().rev() {
                                    let shape_rect = AppUtils::calculate_shape_bounding_box(shape);

                                    if shape_rect.contains(pos) {
                                        to_remove_shapes.push(i);
                                    }
                                }
                                for i in to_remove_shapes {
                                    self.state.shapes.remove(i);
                                }

                                // 删除笔画
                                let mut to_remove = Vec::new();
                                for (i, stroke) in self.state.strokes.iter().enumerate().rev() {
                                    if AppUtils::point_intersects_stroke(
                                        pos,
                                        stroke,
                                        self.state.eraser_size,
                                    ) {
                                        to_remove.push(i);
                                    }
                                }
                                for i in to_remove {
                                    self.state.strokes.remove(i);
                                }
                            }
                        }
                    }

                    Tool::PixelEraser => {
                        // 像素橡皮擦：从笔画中移除被擦除的点
                        if response.drag_started() {
                            if let Some(pos) = pointer_pos {
                                self.state.is_drawing = true;
                                self.state.current_stroke = Some(vec![pos]);
                            }
                        } else if response.dragged() {
                            if self.state.is_drawing {
                                if let Some(pos) = pointer_pos {
                                    // 绘制指针
                                    AppUtils::draw_size_preview(
                                        painter,
                                        pos,
                                        self.state.eraser_size,
                                    );

                                    if let Some(ref mut points) = self.state.current_stroke {
                                        if points.is_empty()
                                            || points.last().unwrap().distance(pos) > 1.0
                                        {
                                            points.push(pos);
                                        }
                                    }

                                    // 从所有笔画中移除被橡皮擦覆盖的点
                                    let eraser_radius = self.state.eraser_size / 2.0;
                                    for stroke in &mut self.state.strokes {
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
                                    self.state.strokes.retain(|s| s.points.len() >= 2);
                                }
                            }
                        } else if response.drag_stopped() {
                            self.state.is_drawing = false;
                            self.state.current_stroke = None;
                        }
                    }

                    Tool::Brush => {
                        // 画笔工具
                        if response.drag_started() {
                            // 开始新的笔画
                            if let Some(pos) = pointer_pos {
                                if pos.x >= rect.min.x
                                    && pos.x <= rect.max.x
                                    && pos.y >= rect.min.y
                                    && pos.y <= rect.max.y
                                {
                                    self.state.is_drawing = true;
                                    self.state.current_stroke = Some(vec![pos]);
                                    let start_time = Instant::now();
                                    self.state.stroke_start_time = Some(start_time);
                                    self.state.current_stroke_times = Some(vec![0.0]);
                                    let width = AppUtils::calculate_dynamic_width(
                                        self.state.brush_width,
                                        self.state.dynamic_brush_mode,
                                        0,
                                        1,
                                        None,
                                    );
                                    self.state.current_stroke_widths = Some(vec![width]);
                                }
                            }
                        } else if response.dragged() {
                            // 继续绘制
                            if self.state.is_drawing {
                                if let Some(pos) = pointer_pos {
                                    if let Some(ref mut points) = self.state.current_stroke {
                                        if let Some(ref mut widths) =
                                            self.state.current_stroke_widths
                                        {
                                            if let Some(ref mut times) =
                                                self.state.current_stroke_times
                                            {
                                                // 只添加与上一个点距离足够远的点，避免点太密集
                                                if points.is_empty()
                                                    || points.last().unwrap().distance(pos) > 1.0
                                                {
                                                    let current_time = if let Some(start) =
                                                        self.state.stroke_start_time
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
                                                    let width = AppUtils::calculate_dynamic_width(
                                                        self.state.brush_width,
                                                        self.state.dynamic_brush_mode,
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
                            if self.state.is_drawing {
                                if let Some(points) = self.state.current_stroke.take() {
                                    if let Some(widths) = self.state.current_stroke_widths.take() {
                                        if points.len() > 1 && widths.len() == points.len() {
                                            // 应用笔画平滑
                                            let final_points = if self.state.stroke_smoothing {
                                                AppUtils::apply_stroke_smoothing(&points)
                                            } else {
                                                points
                                            };

                                            self.state.strokes.push(crate::state::DrawingStroke {
                                                points: final_points,
                                                widths,
                                                color: self.state.brush_color,
                                                base_width: self.state.brush_width,
                                            });
                                        }
                                    }
                                }
                                self.state.current_stroke_times = None;
                                self.state.stroke_start_time = None;
                                self.state.is_drawing = false;
                            }
                        }

                        // 如果鼠标在画布内移动且正在绘制，也添加点（用于平滑绘制）
                        if response.hovered() && self.state.is_drawing {
                            if let Some(pos) = pointer_pos {
                                if let Some(ref mut points) = self.state.current_stroke {
                                    if let Some(ref mut widths) = self.state.current_stroke_widths {
                                        if let Some(ref mut times) = self.state.current_stroke_times
                                        {
                                            if points.is_empty()
                                                || points.last().unwrap().distance(pos) > 1.0
                                            {
                                                let current_time = if let Some(start) =
                                                    self.state.stroke_start_time
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

                                                let width = AppUtils::calculate_dynamic_width(
                                                    self.state.brush_width,
                                                    self.state.dynamic_brush_mode,
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

        // 清理已标记为删除的图片（在帧结束时安全删除）
        self.state.images.retain(|img| !img.marked_for_deletion);

        // 如果启用了 FPS 显示，更新 FPS
        if self.state.show_fps {
            _ = self.state.fps_counter.update();
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
            event_loop.exit();
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
                    }
                    TouchPhase::Moved => {
                        self.state.touch_points.insert(id, logical_pos);
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.state.touch_points.remove(&id);
                    }
                }

                // Request redraw to update touch visualization
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => (),
        }
    }
}
