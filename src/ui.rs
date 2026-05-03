use std::sync::Arc;

use egui::{Color32, Context, Pos2, Stroke, Ui};
use wgpu::{Backend, PresentMode};
use winit::window::Window;

use crate::{
    state::{
        AppState, CanvasImage, CanvasObject, CanvasObjectOps, CanvasShape, CanvasShapeType,
        CanvasStroke, CanvasText, CanvasTool, DynamicBrushWidthMode, GraphicsApi,
        OptimizationPolicy, PageState, PersistentState, StrokeWidth, ThemeMode, WindowMode,
    },
    utils::{
        self,
        stroke::{brush_stroke_add_point, brush_stroke_end, brush_stroke_start},
        ui::{
            PageAction, add_new_page_state, apply_theme_mode_and_canvas_color, apply_window_mode,
            clear_interaction_state, load_canvas_from_file, save_canvas_to_file,
            switch_to_page_state,
        },
    },
};

pub fn ui_welcome(state: &mut AppState, ctx: &Context) {
    let content_rect = ctx.content_rect();
    let center_pos = content_rect.center();

    egui::Window::new("欢迎")
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .current_pos(center_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.heading("欢迎使用 smartboard");
            ui.separator();

            ui.label("这是一个功能强大的数字画板应用，您可以：");
            ui.label("• 绘制和涂鸦");
            ui.label("• 使用各种工具进行编辑");
            ui.label("• 插入图片、文本和形状");
            ui.label("• 自定义画板设置");
            ui.label("• 保存与加载画布以保存你的工作");
            ui.label("• 导出画布为图片");
            ui.label("• 享受超快的启动速度与超高的流畅度");
            ui.separator();

            if ui.button("新建画布").clicked() {
                let default_page = PageState::default();
                state.pages = vec![default_page.clone()];
                state.current_page = 0;
                state.canvas = default_page.canvas;
                state.history = default_page.history;
                clear_interaction_state(state);
                state.show_welcome_window = false;
            }
            if ui.button("加载画布").clicked() {
                load_canvas_from_file(state);
            }

            ui.separator();

            ui.checkbox(
                &mut state.persistent.show_welcome_window_on_start,
                "启动时显示欢迎",
            );
        });
}

pub fn ui_toolbar_settings(state: &mut AppState, ctx: &Context, ui: &mut Ui, window: &Arc<Window>) {
    ui.collapsing("外观", |ui| {
        ui.horizontal(|ui| {
            ui.label("画布颜色:");
            if ui
                .color_edit_button_srgba(&mut state.persistent.canvas_color)
                .changed()
            {
                apply_theme_mode_and_canvas_color(
                    ctx,
                    state.persistent.theme_mode,
                    state.persistent.canvas_color,
                );
            }
            if ui.button("重置").clicked() {
                state.persistent.canvas_color = utils::get_default_canvas_color();
                apply_theme_mode_and_canvas_color(
                    ctx,
                    state.persistent.theme_mode,
                    state.persistent.canvas_color,
                );
            }
        });

        ui.horizontal(|ui| {
            ui.label("主题模式:");
            if ui
                .selectable_value(
                    &mut state.persistent.theme_mode,
                    ThemeMode::System,
                    "跟随系统",
                )
                .clicked()
                || ui
                    .selectable_value(
                        &mut state.persistent.theme_mode,
                        ThemeMode::Light,
                        "浅色模式",
                    )
                    .clicked()
                || ui
                    .selectable_value(
                        &mut state.persistent.theme_mode,
                        ThemeMode::Dark,
                        "深色模式",
                    )
                    .clicked()
            {
                apply_theme_mode_and_canvas_color(
                    ctx,
                    state.persistent.theme_mode,
                    state.persistent.canvas_color,
                );
            }
        });

        ui.horizontal(|ui| {
            ui.label("启动时显示欢迎:");
            ui.checkbox(&mut state.persistent.show_welcome_window_on_start, "");
        });

        #[cfg(feature = "startup_animation")]
        ui.horizontal(|ui| {
            ui.label("显示启动动画:");
            ui.checkbox(&mut state.persistent.show_startup_animation, "");
        });

        ui.horizontal(|ui| {
            ui.label("窗口透明度");
            ui.add(egui::Slider::new(
                &mut state.persistent.window_opacity,
                0.0..=1.0,
            ));
        });
    });

    ui.collapsing("绘制", |ui| {
        ui.horizontal(|ui| {
            ui.label("画布持久化:");
            if ui.button("加载").clicked() {
                load_canvas_from_file(state);
            }
            if ui.button("保存").clicked() {
                save_canvas_to_file(&mut state.toasts, &state.canvas);
            }
        });

        ui.horizontal(|ui| {
            ui.label("画布转换:");
            if ui.button("导出为图片").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("画布文件", IMAGE_FILE_EXTS)
                    .set_file_name("canvas.bmp")
                    .save_file()
                {
                    state.screenshot_path = Some(path);
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("动态画笔宽度微调:");
            ui.selectable_value(
                &mut state.dynamic_brush_width_mode,
                DynamicBrushWidthMode::Disabled,
                "禁用",
            );
            ui.selectable_value(
                &mut state.dynamic_brush_width_mode,
                DynamicBrushWidthMode::BrushTip,
                "模拟笔锋",
            );
            ui.selectable_value(
                &mut state.dynamic_brush_width_mode,
                DynamicBrushWidthMode::SpeedBased,
                "基于速度",
            );
        });

        ui.horizontal(|ui| {
            ui.label("笔迹平滑:");
            ui.checkbox(&mut state.persistent.stroke_smoothing, "");
        });

        ui.horizontal(|ui| {
            ui.label("直线停留拉直:");
            ui.checkbox(&mut state.persistent.stroke_straightening, "启用");
            if state.persistent.stroke_straightening {
                ui.add(egui::Slider::new(
                    &mut state.persistent.stroke_straightening_tolerance,
                    1.0..=50.0,
                ));
                ui.label("灵敏度");
            }
        });

        ui.horizontal(|ui| {
            ui.label("插值频率:");
            ui.add(egui::Slider::new(
                &mut state.persistent.interpolation_frequency,
                0.0..=1.0,
            ));
        });

        ui.horizontal(|ui| {
            ui.label("低延迟模式:");
            ui.checkbox(&mut state.persistent.low_latency_mode, "");
        });

        ui.horizontal(|ui| {
            ui.label("编辑快捷颜色:");
            if ui.button("OK").clicked() {
                state.show_quick_color_edit_window = true;
            }
        });

        // 快捷颜色编辑器窗口
        if state.show_quick_color_edit_window {
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
                    for (index, color) in state.persistent.quick_colors.iter().enumerate() {
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
                        state.persistent.quick_colors.remove(index);
                    }

                    ui.separator();

                    // 添加新颜色
                    ui.horizontal(|ui| {
                        ui.label("新颜色:");
                        ui.color_edit_button_srgba(&mut state.new_quick_color);
                        if ui.button("添加").clicked() {
                            state.persistent.quick_colors.push(state.new_quick_color);
                            state.new_quick_color = Color32::WHITE;
                        }
                    });

                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("完成").clicked() {
                            state.show_quick_color_edit_window = false;
                        }
                        if ui.button("重置").clicked() {
                            state.show_quick_color_edit_window = false;
                            state.persistent.quick_colors = utils::get_default_quick_colors();
                        }
                    });
                });
        }
    });

    ui.collapsing("性能", |ui| {
        ui.horizontal(|ui| {
            ui.label("窗口模式:");
            if ui
                .selectable_value(
                    &mut state.persistent.window_mode,
                    WindowMode::Windowed,
                    "窗口化",
                )
                .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.window_mode,
                        WindowMode::Fullscreen,
                        "全屏",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.window_mode,
                        WindowMode::BorderlessFullscreen,
                        "无边框全屏",
                    )
                    .changed()
            {
                apply_window_mode(state, window);
            }
        });

        // 显示模式选择（仅在全屏模式下可用）
        ui.horizontal(|ui| {
            ui.label("显示模式:");

            // 显示当前选择的视频模式
            if state.persistent.window_mode == WindowMode::Fullscreen {
                let mut current_selection = state.selected_video_mode_index.unwrap_or(0);

                let mode = state
                    .fullscreen_video_modes
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
                        for (index, mode) in state.fullscreen_video_modes.clone().iter().enumerate()
                        {
                            let mode_text = format!(
                                "{}x{} @ {}Hz",
                                mode.size().width,
                                mode.size().height,
                                mode.refresh_rate_millihertz() as f32 / 1000.0
                            );
                            if ui
                                .selectable_value(&mut current_selection, index, mode_text)
                                .changed()
                            {
                                state.selected_video_mode_index = Some(current_selection);
                                apply_window_mode(state, window);
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
            if ui
                .selectable_value(
                    &mut state.persistent.present_mode,
                    PresentMode::AutoVsync,
                    "开 (自动) | AutoVsync",
                )
                .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::AutoNoVsync,
                        "关 (自动) | AutoNoVsync",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::Fifo,
                        "开 | Fifo",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::FifoRelaxed,
                        "自适应 | FifoRelaxed",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::Immediate,
                        "关 | Immediate",
                    )
                    .changed()
                || ui
                    .selectable_value(
                        &mut state.persistent.present_mode,
                        PresentMode::Mailbox,
                        "开 (快速) | Mailbox",
                    )
                    .changed()
            {
                state.present_mode_changed = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("优化策略 [需重启以应用]:");
            ui.selectable_value(
                &mut state.persistent.optimization_policy,
                OptimizationPolicy::Performance,
                "性能",
            );
            ui.selectable_value(
                &mut state.persistent.optimization_policy,
                OptimizationPolicy::ResourceUsage,
                "资源用量",
            );
        });

        let current_backend = state.active_backend.unwrap_or(Backend::Noop);
        ui.horizontal(|ui| {
            ui.label("图形 API [需重启以应用]:");
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Auto,
                "自动",
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Vulkan,
                if current_backend == Backend::Vulkan {
                    "Vulkan (当前)"
                } else {
                    "Vulkan"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Dx12,
                if current_backend == Backend::Dx12 {
                    "Dx12 (当前)"
                } else {
                    "Dx12"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Metal,
                if current_backend == Backend::Metal {
                    "Metal (当前)"
                } else {
                    "Metal"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::WebGpu,
                if current_backend == Backend::BrowserWebGpu {
                    "WebGPU (当前)"
                } else {
                    "WebGPU"
                },
            );
            ui.selectable_value(
                &mut state.persistent.graphics_api,
                GraphicsApi::Gl,
                if current_backend == Backend::Gl {
                    "Gl (当前)"
                } else {
                    "Gl"
                },
            );
        });

        ui.horizontal(|ui| {
            ui.label("强制每帧重绘:");
            ui.checkbox(&mut state.persistent.force_redraw_every_frame, "");
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
            ui.checkbox(&mut state.persistent.show_fps, "");
        });

        ui.horizontal(|ui| {
            ui.label("显示触控点:");
            ui.checkbox(&mut state.show_touch_points, "");
        });

        #[cfg(target_os = "windows")]
        {
            ui.horizontal(|ui| {
                ui.label("显示终端 [仅 Windows]:");
                let old_show_console = state.show_console;
                if ui.checkbox(&mut state.show_console, "").changed() {
                    use windows::Win32::System::Console::AllocConsole;
                    use windows::Win32::System::Console::FreeConsole;

                    if state.show_console && !old_show_console {
                        // 启用控制台
                        unsafe {
                            let _ = AllocConsole();
                        }
                    } else if !state.show_console && old_show_console {
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

                    state.canvas.objects.push(CanvasObject::Stroke(stroke));
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("立即保存设置:");
            if ui.button("OK").clicked() {
                if let Err(err) = state.persistent.save_to_file() {
                    state.toasts.error(format!("设置保存失败: {}!", err));
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("重置设置:");
            if ui.button("OK").clicked() {
                clear_interaction_state(state);
                state.persistent = PersistentState::default();
                apply_theme_mode_and_canvas_color(
                    ctx,
                    state.persistent.theme_mode,
                    state.persistent.canvas_color,
                );
                state.present_mode_changed = true;
                apply_window_mode(state, window);
            }
        });

        ui.horizontal(|ui| {
            ui.label("???:");
            ui.checkbox(&mut state.persistent.easter_egg_redo, "");
        });
    });
}

pub fn ui_history(state: &mut AppState, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label("历史记录:");
        if ui.button("撤销").clicked() {
            state.selected_object_index = None; // prevent selecting phantom object
            if state.history.undo(&mut state.canvas) {
                state.toasts.success("成功撤销操作!");
            } else {
                state.toasts.error("无法撤销，没有更多历史记录!");
            }
        }
        if ui
            .button(if !state.persistent.easter_egg_redo {
                "重做"
            } else {
                "Redo!"
            })
            .clicked()
        {
            state.selected_object_index = None; // prevent selecting phantom object
            if state.history.redo(&mut state.canvas) {
                state.toasts.success("成功重做操作!");
            } else {
                state.toasts.error("无法重做，没有更多历史记录!");
            }
        }
    });
}

pub fn ui_window_controls(state: &mut AppState, ui: &mut Ui, window: &Arc<Window>) {
    ui.horizontal(|ui| {
        if ui.button("退出").clicked() {
            state.should_quit = true;
        }

        if ui.button("最小化").clicked() {
            window.set_visible(false);
        }

        if state.persistent.show_fps {
            ui.label(format!("FPS: {}", state.fps_counter.current_fps));
        }
    });
}

pub fn ui_pages_nav(state: &mut AppState, ctx: &Context) {
    if state.screenshot_path.is_some() {
        return;
    }

    let content_rect = ctx.content_rect();
    let margin = 8.0;
    let total_pages = state.pages.len();
    let current = state.current_page;
    let enabled = !state.show_welcome_window;

    if enabled {
        let mut action = PageAction::None;

        let build_page_nav =
            |ui: &mut Ui, action: &mut PageAction, show_management_window: &mut bool| {
                let btn_style = |text: &str| {
                    egui::Button::new(egui::RichText::new(text).size(20.0))
                        .min_size(egui::vec2(36.0, 28.0))
                };
                ui.horizontal(|ui| {
                    if ui.add_enabled(current > 0, btn_style("<")).clicked() {
                        *action = PageAction::Previous;
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("{}/{}", current + 1, total_pages))
                                    .size(20.0),
                            )
                            .min_size(egui::vec2(48.0, 28.0)),
                        )
                        .clicked()
                    {
                        *show_management_window = true;
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
                build_page_nav(ui, &mut a, &mut state.show_page_management_window);
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
                build_page_nav(ui, &mut a, &mut state.show_page_management_window);
                if !matches!(a, PageAction::None) {
                    action = a;
                }
            });

        apply_page_action(state, action);
    }
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
        // PageAction::Delete if state.pages.len() > 1 => {
        //     let i = state.current_page;
        //     state.pages.remove(i);
        //     if i >= state.pages.len() {
        //         state.current_page = state.pages.len() - 1;
        //     }
        //     state.canvas = std::mem::take(&mut state.pages[state.current_page].canvas);
        //     state.history = std::mem::take(&mut state.pages[state.current_page].history);
        //     state.selected_object = None;
        //     state.drag_start_pos = None;
        //     state.dragged_handle = None;
        //     state.drag_move_accumulated_delta = egui::Vec2::ZERO;
        //     state.drag_original_transform = None;
        //     state.active_strokes.clear();
        //     state.is_drawing = false;
        // }
        _ => {}
    }
}

pub fn ui_pages_manager(state: &mut AppState, ctx: &Context) {
    let content_rect = ctx.content_rect();
    let center_pos = content_rect.center();
    let total_pages = state.pages.len();

    egui::Window::new(format!("页面管理 (共 {} 页)", total_pages))
        .id("page_man".into())
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .current_pos(center_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let mut pages_to_remove: Vec<usize> = Vec::new();

            let scroll_height = (total_pages as f32 * 50.0).min(300.0);
            egui::ScrollArea::vertical()
                .max_height(scroll_height)
                .show(ui, |ui| {
                    let mut dnd_from: Option<usize> = None;
                    let mut dnd_to: Option<usize> = None;

                    let zone_frame = egui::Frame::NONE.inner_margin(4.0);
                    let (_, dropped_payload) = ui.dnd_drop_zone::<usize, ()>(zone_frame, |ui| {
                        ui.set_min_width(ui.available_width());

                        let mut i = 0;
                        while i < state.pages.len() {
                            let is_current = i == state.current_page;

                            let row_frame = egui::Frame::NONE
                                .fill(ui.visuals().window_fill)
                                .inner_margin(egui::Margin::symmetric(8, 3));

                            let row_response = row_frame
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.set_min_height(36.0);

                                        let handle_id = egui::Id::new(("page_drag_handle", i));
                                        let _ = ui.dnd_drag_source(handle_id, i, |ui| {
                                            ui.label(egui::RichText::new("-").size(16.0));
                                        });

                                        if is_current {
                                            ui.label(
                                                egui::RichText::new(format!("第 {} 页", i + 1))
                                                    .strong(),
                                            );
                                        } else {
                                            ui.label(format!("第 {} 页", i + 1));
                                        }

                                        if ui.button("✓ 保存").clicked() {
                                            save_canvas_to_file(
                                                &mut state.toasts,
                                                &state.pages[i].canvas,
                                            );
                                        }

                                        if ui
                                            .add_enabled(
                                                total_pages > 1,
                                                egui::Button::new("X 删除"),
                                            )
                                            .clicked()
                                        {
                                            pages_to_remove.push(i);
                                        }

                                        if ui
                                            .add_enabled(
                                                !is_current,
                                                egui::Button::new(if !is_current {
                                                    "→ 跳转"
                                                } else {
                                                    "⊙ 当前"
                                                }),
                                            )
                                            .clicked()
                                        {
                                            switch_to_page_state(state, i);
                                        }
                                    });
                                })
                                .response;

                            if let (Some(pointer), Some(hovered_payload)) = (
                                ui.input(|i| i.pointer.interact_pos()),
                                row_response.dnd_hover_payload::<usize>(),
                            ) {
                                let rect = row_response.rect;
                                let stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
                                if *hovered_payload == i {
                                    ui.painter().hline(rect.x_range(), rect.center().y, stroke);
                                } else if pointer.y < rect.center().y {
                                    ui.painter().hline(rect.x_range(), rect.top(), stroke);
                                } else {
                                    ui.painter().hline(rect.x_range(), rect.bottom(), stroke);
                                }

                                if let Some(dragged_payload) =
                                    row_response.dnd_release_payload::<usize>()
                                {
                                    let insert_row_idx = if pointer.y < rect.center().y {
                                        i
                                    } else {
                                        i + 1
                                    };
                                    dnd_from = Some(*dragged_payload);
                                    dnd_to = Some(insert_row_idx);
                                }
                            }
                            i += 1;
                        }
                    });

                    if let Some(dragged_payload) = dropped_payload {
                        dnd_from = Some(*dragged_payload);
                        dnd_to = Some(usize::MAX);
                    }

                    // Apply reorder
                    if let (Some(from_idx), Some(to_idx)) = (dnd_from, dnd_to) {
                        let old_cp = state.current_page;
                        std::mem::swap(&mut state.canvas, &mut state.pages[old_cp].canvas);
                        std::mem::swap(&mut state.history, &mut state.pages[old_cp].history);

                        let page = state.pages.remove(from_idx);

                        let insert_at = if to_idx == usize::MAX || to_idx >= state.pages.len() {
                            state.pages.len()
                        } else if to_idx > from_idx {
                            to_idx - 1
                        } else {
                            to_idx
                        };
                        let insert_at = insert_at.min(state.pages.len());

                        state.pages.insert(insert_at, page);

                        state.current_page = if old_cp == from_idx {
                            insert_at
                        } else if old_cp > from_idx && old_cp <= insert_at {
                            old_cp - 1
                        } else if old_cp < from_idx && old_cp >= insert_at {
                            old_cp + 1
                        } else {
                            old_cp
                        };

                        let cur = state.current_page;
                        std::mem::swap(&mut state.canvas, &mut state.pages[cur].canvas);
                        std::mem::swap(&mut state.history, &mut state.pages[cur].history);
                    }
                });

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("+ 新页").clicked() {
                    add_new_page_state(state);
                }
                if ui.button("O 加载").clicked() {
                    load_canvas_from_file(state);
                }
                if ui.button("X 关闭").clicked() {
                    state.show_page_management_window = false;
                }
            });

            // Apply deletions
            if !pages_to_remove.is_empty() {
                pages_to_remove.sort();
                pages_to_remove.dedup();
                let old = state.current_page;
                std::mem::swap(&mut state.canvas, &mut state.pages[old].canvas);
                std::mem::swap(&mut state.history, &mut state.pages[old].history);
                for &i in pages_to_remove.iter().rev() {
                    state.pages.remove(i);
                    if state.current_page >= i && state.current_page > 0 {
                        state.current_page -= 1;
                    }
                }
                if state.current_page >= state.pages.len() {
                    state.current_page = state.pages.len() - 1;
                }
                let cur = state.current_page;
                std::mem::swap(&mut state.canvas, &mut state.pages[cur].canvas);
                std::mem::swap(&mut state.history, &mut state.pages[cur].history);
                clear_interaction_state(state);
            }
        });
}

pub fn ui_toolbar(state: &mut AppState, ctx: &Context, window: &Arc<Window>) {
    if state.screenshot_path.is_some() {
        return;
    }

    let content_rect = ctx.content_rect();
    egui::Window::new("工具栏")
        .resizable(false)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .default_pos([content_rect.center().x, content_rect.max.y - 20.0])
        .enabled(!state.show_welcome_window)
        .show(ctx, |ui| {
            // 工具选择
            ui.horizontal(|ui| {
                ui.label("工具:");
                // TODO: egui doesn't support rendering fonts with colors
                let old_tool = state.current_tool;
                if ui
                    .selectable_value(&mut state.current_tool, CanvasTool::Select, "选择")
                    .changed()
                    || ui
                        .selectable_value(&mut state.current_tool, CanvasTool::Brush, "画笔")
                        .changed()
                    || ui
                        .selectable_value(
                            &mut state.current_tool,
                            CanvasTool::ObjectEraser,
                            "对象擦",
                        )
                        .changed()
                    || ui
                        .selectable_value(
                            &mut state.current_tool,
                            CanvasTool::PixelEraser,
                            "像素擦",
                        )
                        .changed()
                    || ui
                        .selectable_value(&mut state.current_tool, CanvasTool::Insert, "插入")
                        .changed()
                    || ui
                        .selectable_value(&mut state.current_tool, CanvasTool::Settings, "设置")
                        .changed()
                {
                    if state.current_tool != old_tool {
                        clear_interaction_state(state);
                    }
                }
            });

            ui.separator();

            // 选择工具相关设置
            if state.current_tool == CanvasTool::Select {
                if let Some(selected_idx) = state.selected_object_index {
                    ui.horizontal(|ui| {
                        ui.label("对象操作:");
                        if ui.button("删除").clicked() {
                            // Save state to history before modification
                            let removed_object = state.canvas.objects.remove(selected_idx);
                            state
                                .history
                                .save_remove_object(selected_idx, removed_object);
                            state.selected_object_index = None;
                            state.toasts.success("对象已删除!");
                        }
                        if ui.button("复制").clicked() {
                            // FIXME: CanvasImage duplication not implemented
                            if !matches!(state.canvas.objects[selected_idx], CanvasObject::Image(_))
                            {
                                let mut clone = state.canvas.objects[selected_idx].clone();
                                CanvasObject::move_object(&mut clone, egui::vec2(20.0, 20.0));
                                let index = state.canvas.objects.len();
                                state.history.save_add_object(index, clone.clone());
                                state.canvas.objects.push(clone);
                                state.selected_object_index = Some(index);
                                state.toasts.success("对象已复制!");
                            }
                        }
                        if ui.button("置顶").clicked() {
                            if selected_idx < state.canvas.objects.len() - 1 {
                                // Save state to history before modification
                                let object = state.canvas.objects.remove(selected_idx);
                                // Actually move the object to the top (end of the array)
                                state.canvas.objects.push(object);
                                state.history.save_add_object(
                                    state.canvas.objects.len() - 1,
                                    state.canvas.objects.last().unwrap().clone(),
                                );
                                state.selected_object_index = Some(state.canvas.objects.len() - 1);
                                state.toasts.success("对象已移至顶部!");
                            }
                        }
                        if ui.button("置底").clicked() {
                            if selected_idx > 0 {
                                // Save state to history before modification
                                let object = state.canvas.objects.remove(selected_idx);
                                // Actually move the object to the bottom (beginning of the array)
                                state.canvas.objects.insert(0, object);
                                state.history.save_add_object(
                                    0,
                                    state.canvas.objects.first().unwrap().clone(),
                                );
                                state.selected_object_index = Some(0);
                                state.toasts.success("对象已移至底部!");
                            }
                        }

                        if let Some(CanvasObject::Text(text)) =
                            state.canvas.objects.get(selected_idx).cloned()
                        {
                            if ui.button("栅格化").clicked() {
                                let strokes =
                                    crate::utils::rasterize_text(&text, utils::font_bytes());

                                state.canvas.objects.remove(selected_idx);

                                for stroke in strokes {
                                    let stroke_obj = CanvasObject::Stroke(stroke);
                                    state.canvas.objects.push(stroke_obj.clone());

                                    state.history.save_add_object(
                                        state.canvas.objects.len() - 1,
                                        stroke_obj,
                                    );
                                }

                                state
                                    .history
                                    .save_remove_object(selected_idx, CanvasObject::Text(text));

                                state.selected_object_index = None;
                                state.toasts.success("已转换为笔画!");
                            }
                        }
                    });
                } else {
                    ui.label(egui::RichText::new("(未选中对象)").italics());
                }
            }

            // 画笔相关设置
            if state.current_tool == CanvasTool::Brush {
                ui.horizontal(|ui| {
                    ui.label("颜色:");
                    let old_color = state.brush_color;
                    if ui.color_edit_button_srgba(&mut state.brush_color).changed() {
                        // 颜色改变时，如果正在绘制，结束所有当前笔画
                        if state.is_drawing {
                            for (_touch_id, active_stroke) in state.active_strokes.drain() {
                                state
                                    .canvas
                                    .objects
                                    .push(CanvasObject::Stroke(CanvasStroke {
                                        points: active_stroke.points,
                                        width: active_stroke.width,
                                        color: old_color,
                                        base_width: state.brush_width,
                                        rot: 0.0,
                                    }));
                            }
                            state.is_drawing = false;
                        }
                    }
                });

                // 颜色快捷按钮
                ui.horizontal(|ui| {
                    ui.label("快捷颜色:");
                    for color in &state.persistent.quick_colors {
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
                            state.brush_color = *color;
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("宽度:");
                    let slider_response =
                        ui.add(egui::Slider::new(&mut state.brush_width, 1.0..=20.0));

                    // 显示大小预览
                    if slider_response.dragged() || slider_response.hovered() {
                        state.show_size_preview = true;
                        // 使用屏幕中心位置
                    } else if !slider_response.dragged() && !slider_response.hovered() {
                        state.show_size_preview = false;
                    }
                });

                // 画笔宽度快捷按钮
                ui.horizontal(|ui| {
                    ui.label("快捷宽度:");
                    if ui.button("小").clicked() {
                        state.brush_width = 1.0;
                    }
                    if ui.button("中").clicked() {
                        state.brush_width = 3.0;
                    }
                    if ui.button("大").clicked() {
                        state.brush_width = 5.0;
                    }
                });
            }

            // 橡皮擦相关设置
            if state.current_tool == CanvasTool::ObjectEraser
                || state.current_tool == CanvasTool::PixelEraser
            {
                ui.horizontal(|ui| {
                    ui.label("大小:");
                    let slider_response =
                        ui.add(egui::Slider::new(&mut state.eraser_size, 5.0..=50.0));

                    // 显示大小预览
                    if slider_response.dragged() || slider_response.hovered() {
                        state.show_size_preview = true;
                    } else if !slider_response.dragged() && !slider_response.hovered() {
                        state.show_size_preview = false;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("清空:");
                    if ui.button("OK").clicked() {
                        // Save state to history before modification
                        let old_objects = std::mem::take(&mut state.canvas.objects);
                        state.history.save_clear_objects(old_objects);
                        state.active_strokes.clear();
                        state.is_drawing = false;
                        state.selected_object_index = None;
                        state.current_tool = CanvasTool::Brush;
                    }
                });
            }

            // 插入工具相关设置
            if state.current_tool == CanvasTool::Insert {
                ui.horizontal(|ui| {
                    if ui.button("图片").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("图片", IMAGE_FILE_EXTS)
                            .pick_file()
                        {
                            if let Ok(img) = image::open(path) {
                                // 最大纹理大小限制（通常为 2048x2048）
                                const MAX_TEXTURE_SIZE: u32 = 2048;

                                // 如果图像太大，调整大小以适应纹理限制
                                let img = if img.width() > MAX_TEXTURE_SIZE
                                    || img.height() > MAX_TEXTURE_SIZE
                                {
                                    crate::utils::resize_image_for_texture(img, MAX_TEXTURE_SIZE)
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
                                let image_data: Arc<[u8]> = img_rgba.into_raw().into();
                                let new_image = CanvasImage {
                                    texture,
                                    pos: Pos2::new(100.0, 100.0),
                                    size: egui::vec2(target_width, target_height),
                                    aspect_ratio,
                                    marked_for_deletion: false,
                                    rot: 0.0,
                                    image_data,
                                    image_size: [width, height],
                                };
                                let index = state.canvas.objects.len();
                                state
                                    .history
                                    .save_add_object(index, CanvasObject::Image(new_image.clone()));
                                state.canvas.objects.push(CanvasObject::Image(new_image));

                                state.current_tool = CanvasTool::Select;
                            }
                        }
                    }
                    if ui.button("文本").clicked() {
                        state.show_insert_text_window = true;
                    }
                    if ui.button("形状").clicked() {
                        state.show_insert_shape_window = true;
                    }
                });

                if state.show_insert_text_window {
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
                                ui.text_edit_singleline(&mut state.new_text_content);
                            });

                            ui.horizontal(|ui| {
                                if ui.button("确认").clicked() {
                                    // Save state to history before modification
                                    let new_text = CanvasText {
                                        text: state.new_text_content.clone(),
                                        pos: Pos2::new(100.0, 100.0),
                                        color: Color32::WHITE,
                                        font_size: 16.0,
                                        rot: 0.0,
                                    };
                                    let index = state.canvas.objects.len();
                                    state.history.save_add_object(
                                        index,
                                        CanvasObject::Text(new_text.clone()),
                                    );
                                    state.canvas.objects.push(CanvasObject::Text(new_text));
                                    state.current_tool = CanvasTool::Select;
                                    state.show_insert_text_window = false;
                                    state.new_text_content.clear();
                                }

                                if ui.button("取消").clicked() {
                                    state.show_insert_text_window = false;
                                    state.new_text_content.clear();
                                }

                                #[cfg(target_os = "windows")]
                                {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let keyboard_btn = ui.button("屏幕键盘");
                                            if keyboard_btn.clicked() {
                                                let _ = crate::utils::windows::show_touch_keyboard(
                                                    None,
                                                );
                                            }
                                        },
                                    );
                                }
                            });
                        });
                }

                if state.show_insert_shape_window {
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
                                    let index = state.canvas.objects.len();
                                    state.history.save_add_object(
                                        index,
                                        CanvasObject::Shape(new_shape.clone()),
                                    );
                                    state.canvas.objects.push(CanvasObject::Shape(new_shape));
                                    state.show_insert_shape_window =
                                        state.persistent.keep_insertion_window_open;
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
                                    let index = state.canvas.objects.len();
                                    state.history.save_add_object(
                                        index,
                                        CanvasObject::Shape(new_shape.clone()),
                                    );
                                    state.canvas.objects.push(CanvasObject::Shape(new_shape));
                                    state.show_insert_shape_window =
                                        state.persistent.keep_insertion_window_open;
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
                                    let index = state.canvas.objects.len();
                                    state.history.save_add_object(
                                        index,
                                        CanvasObject::Shape(new_shape.clone()),
                                    );
                                    state.canvas.objects.push(CanvasObject::Shape(new_shape));
                                    state.show_insert_shape_window =
                                        state.persistent.keep_insertion_window_open;
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
                                    let index = state.canvas.objects.len();
                                    state.history.save_add_object(
                                        index,
                                        CanvasObject::Shape(new_shape.clone()),
                                    );
                                    state.canvas.objects.push(CanvasObject::Shape(new_shape));
                                    state.show_insert_shape_window =
                                        state.persistent.keep_insertion_window_open;
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
                                    let index = state.canvas.objects.len();
                                    state.history.save_add_object(
                                        index,
                                        CanvasObject::Shape(new_shape.clone()),
                                    );
                                    state.canvas.objects.push(CanvasObject::Shape(new_shape));
                                    state.show_insert_shape_window =
                                        state.persistent.keep_insertion_window_open;
                                }
                            });

                            ui.horizontal(|ui| {
                                if ui.button("取消").clicked() {
                                    state.show_insert_shape_window = false;
                                }
                                ui.checkbox(
                                    &mut state.persistent.keep_insertion_window_open,
                                    "保持窗口开启",
                                );
                            });
                        });
                }
            }

            if state.current_tool == CanvasTool::Settings {
                ui_toolbar_settings(state, ctx, ui, window);
            }

            ui.separator();

            ui_history(state, ui);

            ui.separator();

            ui_window_controls(state, ui, window);
        });
}

pub fn ui_canvas(state: &mut AppState, ctx: &Context) {
    #[allow(deprecated)] // seems complicated to migrate; since it works, i'm not going to fix it
    egui::CentralPanel::default().show(ctx, |ui| {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size(),
            if state.persistent.low_latency_mode {
                egui::Sense::drag()
            } else {
                egui::Sense::click_and_drag()
            },
        );

        let painter = ui.painter();

        // 绘制所有对象
        for (i, object) in state.canvas.objects.iter().enumerate() {
            let selected = state.selected_object_index == Some(i);
            object.paint(painter, selected);
        }

        // 绘制当前正在绘制的笔画
        // TODO: unify with CanvasStroke::paint()
        for active_stroke in state.active_strokes.values() {
            if let StrokeWidth::Dynamic(v) = &active_stroke.width {
                if v.len() != active_stroke.points.len() {
                    continue;
                }
            }
            painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                active_stroke.points[0],
                active_stroke.width.first() / 2.0,
                state.brush_color,
            )));
            if active_stroke.points.len() >= 2 {
                painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                    active_stroke.points[active_stroke.points.len() - 1],
                    active_stroke.width.last() / 2.0,
                    state.brush_color,
                )));
                for i in 0..active_stroke.points.len() - 1 {
                    let avg_width =
                        (active_stroke.width.get(i) + active_stroke.width.get(i + 1)) / 2.0;
                    painter.line_segment(
                        [active_stroke.points[i], active_stroke.points[i + 1]],
                        Stroke::new(avg_width, state.brush_color),
                    );
                }
            }
        }

        // 绘制大小预览圆圈
        if state.show_size_preview {
            let content_rect = ui.ctx().content_rect();
            let pos = content_rect.center();
            utils::draw_size_preview(
                painter,
                pos,
                match state.current_tool {
                    CanvasTool::Brush => state.brush_width,
                    CanvasTool::ObjectEraser | CanvasTool::PixelEraser => state.eraser_size,
                    _ => unreachable!(),
                },
            );
        }

        // 绘制触控点
        if state.show_touch_points {
            for (id, pos) in &state.touch_points {
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

        if state.touch_used {
            return;
        }

        // 处理鼠标输入
        let pointer_pos = response.interact_pointer_pos();

        match state.current_tool {
            CanvasTool::Insert | CanvasTool::Settings => {}

            CanvasTool::Select => {
                // Select tool: click to select objects, drag to move/resize selected object

                // Handle click: iterate through objects from last to first, check bounding boxes
                if response.clicked() {
                    if let Some(click_pos) = pointer_pos {
                        // Iterate from last to first (top to bottom in z-order)
                        let mut found_selection = false;
                        for (i, object) in state.canvas.objects.iter().enumerate().rev() {
                            if object.bounding_box().contains(click_pos) {
                                state.selected_object_index = Some(i);
                                found_selection = true;
                                break;
                            }
                        }

                        // If no object was clicked, deselect
                        if !found_selection {
                            state.selected_object_index = None;
                        }
                    }
                }

                // Handle drag start: record the drag start position and check for resize handles
                if response.drag_started() {
                    if let Some(pos) = pointer_pos {
                        state.drag_start_pos = Some(pos);
                        state.dragged_handle = None;
                        state.drag_move_accumulated_delta = egui::Vec2::ZERO;

                        // Check if we're dragging a resize handle
                        if let Some(selected_idx) = state.selected_object_index {
                            if let Some(object) = state.canvas.objects.get(selected_idx) {
                                let bbox = object.bounding_box();
                                if let Some(handle) = utils::get_transform_handle_at_pos(bbox, pos)
                                {
                                    state.dragged_handle = Some(handle);
                                    state.drag_original_transform = Some(object.get_transform());
                                }
                            }
                        }
                    }
                }

                // Handle dragging: move or resize the selected object
                if response.dragged() && state.selected_object_index.is_some() {
                    if let (Some(drag_start), Some(current_pos)) =
                        (state.drag_start_pos, pointer_pos)
                    {
                        let delta = current_pos - drag_start;

                        if let Some(selected_idx) = state.selected_object_index {
                            if let Some(dragged_handle) = state.dragged_handle {
                                // Resize operation — history saved on drag_stopped
                                if let Some(object) = state.canvas.objects.get_mut(selected_idx) {
                                    object.transform(
                                        dragged_handle,
                                        delta,
                                        drag_start,
                                        current_pos,
                                    );
                                }
                            } else {
                                // Move operation
                                if let Some(object) = state.canvas.objects.get_mut(selected_idx) {
                                    CanvasObject::move_object(object, delta);
                                }
                                state.drag_move_accumulated_delta += delta;
                            }
                        }

                        // Update drag start position for continuous dragging
                        state.drag_start_pos = Some(current_pos);
                    }
                }

                // Handle drag stop: save move/resize to history and clear state
                if response.drag_stopped() {
                    if state.drag_move_accumulated_delta != egui::Vec2::ZERO {
                        if let Some(selected_idx) = state.selected_object_index {
                            state.history.save_move_object(
                                selected_idx,
                                -state.drag_move_accumulated_delta,
                                state.drag_move_accumulated_delta,
                            );
                        }
                    } else if let Some(original_transform) = state.drag_original_transform.take() {
                        if let Some(selected_idx) = state.selected_object_index {
                            if let Some(object) = state.canvas.objects.get(selected_idx) {
                                let new_transform = object.get_transform();
                                state.history.save_transform_object(
                                    selected_idx,
                                    original_transform,
                                    new_transform,
                                );
                            }
                        }
                    }
                    state.drag_start_pos = None;
                    state.dragged_handle = None;
                }
            }

            CanvasTool::ObjectEraser => {
                // 对象橡皮擦：点击或拖拽时删除相交的整个对象
                if response.drag_started() || response.clicked() || response.dragged() {
                    if let Some(pos) = pointer_pos {
                        // 绘制指针
                        utils::draw_size_preview(painter, pos, state.eraser_size);

                        // 从后往前删除，避免索引问题
                        let mut to_remove = Vec::new();

                        // 检查所有对象
                        for (i, object) in state.canvas.objects.iter().enumerate().rev() {
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
                                    let text_rect = egui::Rect::from_min_size(text.pos, text_size);
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
                                        state.eraser_size,
                                    ) {
                                        to_remove.push(i);
                                    }
                                }
                            }
                        }

                        // 删除对象，逐个记录到历史
                        for i in to_remove {
                            let object = state.canvas.objects.remove(i);
                            state.history.save_remove_object(i, object);
                        }
                    }
                }
            }

            CanvasTool::PixelEraser => {
                // 像素橡皮擦：从笔画中移除被擦除的段落，并将笔画分割为多个部分
                if response.dragged() || response.clicked() {
                    if let Some(pos) = pointer_pos {
                        // 绘制指针
                        utils::draw_size_preview(painter, pos, state.eraser_size);

                        let eraser_radius = state.eraser_size / 2.0;
                        let eraser_rect = egui::Rect::from_center_size(
                            pos,
                            egui::vec2(state.eraser_size, state.eraser_size),
                        );

                        let mut new_strokes = Vec::new();
                        let mut strokes_modified = false;

                        for object in &state.canvas.objects {
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

                                    let dist = utils::point_to_line_segment_distance(pos, p1, p2);

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
                            let original_stroke_count = state
                                .canvas
                                .objects
                                .iter()
                                .filter(|obj| matches!(obj, CanvasObject::Stroke(_)))
                                .count();
                            let new_stroke_count = new_strokes.len();
                            if original_stroke_count != new_stroke_count {
                                let non_strokes: Vec<_> = state
                                    .canvas
                                    .objects
                                    .iter()
                                    .filter(|obj| !matches!(obj, CanvasObject::Stroke(_)))
                                    .cloned()
                                    .collect();
                                let old_objects = std::mem::take(&mut state.canvas.objects);
                                state.history.save_clear_objects(old_objects);
                                state.canvas.objects = non_strokes;
                            } else {
                                state
                                    .canvas
                                    .objects
                                    .retain(|obj| !matches!(obj, CanvasObject::Stroke(_)));
                            }

                            for stroke in new_strokes {
                                state.canvas.objects.push(CanvasObject::Stroke(stroke));
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
                        state.is_drawing = true;
                        brush_stroke_start(state, 0, pos);
                    }
                } else if response.dragged() {
                    if state.is_drawing
                        && let Some(pos) = pointer_pos
                    {
                        brush_stroke_add_point(state, 0, pos, false);
                    }
                } else if response.drag_stopped() {
                    if state.is_drawing {
                        brush_stroke_end(state, 0);
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
                            width: StrokeWidth::Fixed(state.brush_width),
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
                }

                // 如果鼠标在画布内移动且正在绘制，也添加点（用于平滑绘制）
                if response.hovered()
                    && state.is_drawing
                    && let Some(pos) = pointer_pos
                {
                    brush_stroke_add_point(state, 0, pos, true);
                }
            }
        }
    });
}

const IMAGE_FILE_EXTS: &[&str; 6] = &["png", "jpg", "jpeg", "bmp", "webp", "ico"];
