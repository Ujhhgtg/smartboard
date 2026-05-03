use std::sync::Arc;

use egui::{Color32, Context, FontDefinitions, Visuals};
use egui_notify::Toasts;
use winit::window::{Fullscreen, Window};

use crate::{
    assets,
    state::{AppState, CanvasState, PageState, ThemeMode, WindowMode},
};

pub fn apply_theme_mode_and_canvas_color(
    ctx: &Context,
    theme_mode: ThemeMode,
    canvas_color: Color32,
) {
    let is_dark = if theme_mode == ThemeMode::System {
        super::dark_mode::is_dark_mode().unwrap_or(true)
    } else {
        theme_mode == ThemeMode::Dark
    };

    if is_dark {
        // let bg_color = Visuals::dark().window_fill;
        ctx.set_visuals(Visuals {
            panel_fill: canvas_color, // for canvas
            // extreme_bg_color: bg_color, // for scroll area; this also affects text input field's bg color, which is unwanted
            dark_mode: true,
            ..Visuals::dark()
        });
    } else {
        // let bg_color = Visuals::light().window_fill;
        ctx.set_visuals(Visuals {
            panel_fill: canvas_color, // for canvas
            // extreme_bg_color: bg_color, // for scroll area; this also affects text input field's bg color, which is unwanted
            dark_mode: false,
            ..Visuals::light()
        });
    }
}

pub fn apply_window_mode(state: &mut AppState, window: &Arc<Window>) {
    match state.persistent.window_mode {
        WindowMode::Windowed => {
            // 窗口化
            window.set_fullscreen(None);
        }
        WindowMode::ExclusiveFullscreen => {
            // 全屏
            // 使用选中的视频模式
            if let Some(selected_index) = state.selected_video_mode_index {
                if selected_index < state.fullscreen_video_modes.len() {
                    if let Some(mode) = state.fullscreen_video_modes.get(selected_index) {
                        window.set_fullscreen(Some(Fullscreen::Exclusive(mode.clone())));
                        return;
                    }
                }
            }

            // 回退到第一个可用的视频模式
            window.set_fullscreen(Some(Fullscreen::Exclusive(
                state
                    .fullscreen_video_modes
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

pub enum PageAction {
    None,
    Previous,
    Next,
    New,
}

pub fn clear_interaction_state(state: &mut AppState) {
    state.selected_object_index = None;
    state.pointers.clear();
}

pub fn switch_to_page_state(state: &mut AppState, page_index: usize) {
    let old = state.current_page;
    if old != page_index {
        std::mem::swap(&mut state.canvas, &mut state.pages[old].canvas);
        std::mem::swap(&mut state.history, &mut state.pages[old].history);
        state.current_page = page_index;
        std::mem::swap(&mut state.canvas, &mut state.pages[page_index].canvas);
        std::mem::swap(&mut state.history, &mut state.pages[page_index].history);
    }
    clear_interaction_state(state);
}

pub fn add_new_page_state(state: &mut AppState) {
    let old = state.current_page;
    state.pages[old].canvas = std::mem::take(&mut state.canvas);
    state.pages[old].history = std::mem::take(&mut state.history);
    state.pages.push(PageState::default());
    let new_idx = state.pages.len() - 1;
    state.current_page = new_idx;
    clear_interaction_state(state);
}

pub fn load_canvas_from_file(state: &mut AppState) {
    match CanvasState::load_from_file_with_dialog() {
        Ok(canvas) => {
            add_new_page_state(state);
            state.canvas = canvas;
            state.show_welcome_window = false;
            state.toasts.success("成功加载画布!");
        }
        Err(err) => {
            state.toasts.error(format!("画布加载失败: {}!", err));
        }
    };
}

pub fn save_canvas_to_file(toasts: &mut Toasts, canvas: &CanvasState) {
    match canvas.save_to_file_with_dialog() {
        Ok(_) => {
            toasts.success("成功保存画布!");
        }
        Err(err) => {
            toasts.error(format!("画布保存失败: {}!", err));
        }
    }
}

pub fn setup_fonts(ctx: &mut Context) {
    let mut fonts = FontDefinitions::default();

    let font_bytes = assets::font_bytes();
    let font_name = "cjk_font";
    fonts.font_data.insert(
        font_name.to_owned(),
        Arc::new(egui::FontData::from_owned(font_bytes.to_vec())),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, font_name.to_owned());

    ctx.set_fonts(fonts);
}
