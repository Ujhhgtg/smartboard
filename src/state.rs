use egui::Color32;
use egui::Pos2;
use egui::Stroke;
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use egui_notify::Toasts;
use rodio::OutputStreamBuilder;
use rodio::{Decoder, OutputStream, Sink};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
use std::time::Instant;
use wgpu::PresentMode;

// 动态画笔模式
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DynamicBrushWidthMode {
    #[default]
    Disabled, // 禁用
    BrushTip,   // 模拟笔锋
    SpeedBased, // 基于速度
}

// 工具类型
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasTool {
    Select, // 选择
    #[default]
    Brush, // 画笔
    ObjectEraser, // 对象橡皮擦
    PixelEraser, // 像素橡皮擦
    Insert, // 插入
    Settings, // 设置
}

// 可绘制对象的 trait
pub trait Draw {
    fn draw(&self, painter: &egui::Painter, selected: bool);
}

// 插入的图片数据结构
#[derive(Clone)]
pub struct CanvasImage {
    pub texture: egui::TextureHandle,
    pub pos: Pos2,
    pub size: egui::Vec2,
    pub aspect_ratio: f32,
    pub marked_for_deletion: bool, // deferred deletion to avoid panic
}

impl Draw for CanvasImage {
    fn draw(&self, painter: &egui::Painter, selected: bool) {
        let img_rect = egui::Rect::from_min_size(self.pos, self.size);
        painter.image(
            self.texture.id(),
            img_rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // 如果被选中，绘制边框
        if selected {
            painter.rect_stroke(
                img_rect,
                0.0,
                Stroke::new(2.0, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
        }
    }
}

impl fmt::Debug for CanvasImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CanvasImage")
            .field("texture", &"<TextureHandle>")
            .field("pos", &self.pos)
            .field("size", &self.size)
            .field("aspect_ratio", &self.aspect_ratio)
            .field("marked_for_deletion", &self.marked_for_deletion)
            .finish()
    }
}

// 插入的文本数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasText {
    pub text: String,
    pub pos: Pos2,
    pub color: Color32,
    pub font_size: f32,
}

impl Draw for CanvasText {
    fn draw(&self, painter: &egui::Painter, selected: bool) {
        // Draw text using egui's text rendering
        let text_galley = painter.layout_no_wrap(
            self.text.clone(),
            egui::FontId::proportional(self.font_size),
            self.color,
        );
        let text_shape = egui::epaint::TextShape {
            pos: self.pos,
            galley: text_galley.clone(),
            underline: egui::Stroke::NONE,
            override_text_color: None,
            angle: 0.0,
            fallback_color: self.color,
            opacity_factor: 1.0,
        };
        painter.add(text_shape);

        if selected {
            let text_size = text_galley.size();
            let text_rect = egui::Rect::from_min_size(self.pos, text_size);
            painter.rect_stroke(
                text_rect,
                0.0,
                Stroke::new(2.0, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
        }
    }
}

// 插入的形状数据结构
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum CanvasShapeType {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasShape {
    pub shape_type: CanvasShapeType,
    pub pos: Pos2,
    pub size: f32,
    pub color: Color32,
    pub rotation: f32,
}

impl Draw for CanvasShape {
    fn draw(&self, painter: &egui::Painter, selected: bool) {
        // 绘制形状本身
        match self.shape_type {
            CanvasShapeType::Line => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                painter.line_segment([self.pos, end_point], Stroke::new(2.0, self.color));
            }
            CanvasShapeType::Arrow => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                painter.line_segment([self.pos, end_point], Stroke::new(2.0, self.color));

                // 绘制箭头头部
                let arrow_size = self.size * 0.1;
                let arrow_angle = std::f32::consts::PI / 6.0; // 30度
                let arrow_point1 = Pos2::new(
                    end_point.x - arrow_size * arrow_angle.cos(),
                    end_point.y - arrow_size * arrow_angle.sin(),
                );
                let arrow_point2 = Pos2::new(
                    end_point.x - arrow_size * arrow_angle.cos(),
                    end_point.y + arrow_size * arrow_angle.sin(),
                );

                painter.line_segment([end_point, arrow_point1], Stroke::new(2.0, self.color));
                painter.line_segment([end_point, arrow_point2], Stroke::new(2.0, self.color));
            }
            CanvasShapeType::Rectangle => {
                let rect = egui::Rect::from_min_size(self.pos, egui::vec2(self.size, self.size));
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0, self.color),
                    egui::StrokeKind::Outside,
                );
            }
            CanvasShapeType::Triangle => {
                let half_size = self.size / 2.0;
                let points = [
                    self.pos,
                    Pos2::new(self.pos.x + self.size, self.pos.y),
                    Pos2::new(self.pos.x + half_size, self.pos.y + half_size),
                ];
                painter.add(egui::Shape::convex_polygon(
                    points.to_vec(),
                    self.color,
                    Stroke::new(2.0, self.color),
                ));
            }
            CanvasShapeType::Circle => {
                painter.circle_stroke(self.pos, self.size / 2.0, Stroke::new(2.0, self.color));
            }
        }

        // 如果被选中，绘制边框
        if selected {
            let shape_rect = crate::utils::AppUtils::calculate_shape_bounding_box(self);
            painter.rect_stroke(
                shape_rect,
                0.0,
                Stroke::new(2.0, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
        }
    }
}

// 画布对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CanvasObject {
    Stroke(CanvasStroke),
    #[serde(skip)]
    Image(CanvasImage),
    Text(CanvasText),
    Shape(CanvasShape),
}

impl CanvasObject {
    pub fn draw(&self, painter: &egui::Painter, selected: bool) {
        match self {
            CanvasObject::Stroke(stroke) => stroke.draw(painter, selected),
            CanvasObject::Image(image) => image.draw(painter, selected),
            CanvasObject::Text(text) => text.draw(painter, selected),
            CanvasObject::Shape(shape) => shape.draw(painter, selected),
        }
    }
}

// 调整大小锚点类型
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResizeAnchor {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

// 变换操作类型
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransformOperationType {
    Move,
    Resize,
    Rotate,
}

// 变换操作
#[derive(Clone, Copy, Debug)]
pub struct TransformOperation {
    pub operation_type: TransformOperationType,
    pub start_pos: Pos2,
    pub start_object_pos: Pos2,
    pub start_size: egui::Vec2,
    pub start_angle: f32,
    pub anchor: Option<ResizeAnchor>,
    pub center: Pos2,
    pub aspect_ratio: Option<f32>,
}

// 对象变换约束
#[derive(Clone, Copy, Debug)]
pub struct TransformConstraints {
    pub min_width: f32,
    pub min_height: f32,
    pub preserve_aspect_ratio: bool,
    pub allow_rotation: bool,
    pub canvas_bounds: Option<egui::Rect>,
}

// // Toast 通知类型
// #[derive(Clone, Copy, PartialEq, Eq)]
// pub enum ToastType {
//     Success,
//     Error,
// }

// // Toast 通知
// #[derive(Clone)]
// pub struct Toast {
//     pub message: String,
//     pub toast_type: ToastType,
//     pub start_time: Instant,
// }

// impl Toast {
//     pub fn new(message: String, toast_type: ToastType) -> Self {
//         Self {
//             message,
//             toast_type,
//             start_time: Instant::now(),
//         }
//     }

//     pub fn draw(&self, ctx: &Context) {
//         // 检查 Toast 是否过期
//         if self.is_finished() {
//             return;
//         }

//         // 计算 Toast 位置（水平居中，垂直 70% 位置）
//         let content_rect = ctx.available_rect();
//         let toast_width = 300.0; // 固定宽度
//         let toast_height = 80.0; // 固定高度

//         let toast_x = content_rect.center().x - toast_width / 2.0;
//         let toast_y = content_rect.min.y + content_rect.height() * 0.7 - toast_height / 2.0;

//         let toast_rect = egui::Rect::from_min_size(
//             egui::pos2(toast_x, toast_y),
//             egui::vec2(toast_width, toast_height),
//         );

//         // 创建 Toast 窗口
//         let painter = ctx.layer_painter(egui::LayerId::new(
//             egui::Order::Foreground, // 确保 Toast 在最前面
//             egui::Id::new("toast_notification"),
//         ));

//         // 根据 Toast 类型选择颜色
//         let (bg_color, icon, icon_color) = match self.toast_type {
//             ToastType::Success => (
//                 Color32::from_rgba_unmultiplied(46, 125, 50, 230), // 深绿色
//                 "✓",                                               // 成功图标
//                 Color32::WHITE,
//             ),
//             ToastType::Error => (
//                 Color32::from_rgba_unmultiplied(211, 47, 47, 230), // 深红色
//                 "✗",                                               // 错误图标
//                 Color32::WHITE,
//             ),
//         };

//         // 绘制 Toast 背景
//         painter.rect_filled(toast_rect, 10.0, bg_color);

//         // 绘制 Toast 边框
//         painter.rect_stroke(
//             toast_rect,
//             10.0,
//             Stroke::new(2.0, Color32::from_black_alpha(100)),
//             egui::StrokeKind::Outside,
//         );

//         // 绘制图标和文本
//         let icon_font = egui::FontId::proportional(30.0);
//         let text_font = egui::FontId::proportional(16.0);

//         // 计算图标位置
//         let icon_pos = egui::pos2(
//             toast_rect.min.x + 20.0,
//             toast_rect.center().y - 15.0, // 中心对齐图标
//         );

//         // 绘制图标
//         let icon_galley = painter.layout_no_wrap(icon.to_string(), icon_font.clone(), icon_color);
//         let icon_shape = egui::epaint::TextShape {
//             pos: icon_pos,
//             galley: icon_galley,
//             underline: egui::Stroke::NONE,
//             override_text_color: None,
//             angle: 0.0,
//             fallback_color: icon_color,
//             opacity_factor: 1.0,
//         };
//         painter.add(icon_shape);

//         // 计算文本位置（图标右侧）
//         let text_start_x = icon_pos.x + 40.0; // 图标宽度 + 间距
//         let text_pos = egui::pos2(
//             text_start_x,
//             toast_rect.center().y - 10.0, // 中心对齐文本
//         );

//         // 绘制文本
//         let text_galley = painter.layout_no_wrap(self.message.clone(), text_font, Color32::WHITE);
//         let text_shape = egui::epaint::TextShape {
//             pos: text_pos,
//             galley: text_galley,
//             underline: egui::Stroke::NONE,
//             override_text_color: None,
//             angle: 0.0,
//             fallback_color: Color32::WHITE,
//             opacity_factor: 1.0,
//         };
//         painter.add(text_shape);
//     }

//     pub fn is_finished(&self) -> bool {
//         self.start_time.elapsed().as_secs_f32() >= 3.0
//     }
// }

// 窗口模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WindowMode {
    Windowed,
    Fullscreen,
    #[default]
    BorderlessFullscreen,
}

// 主题模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    System,
    Light,
    #[default]
    Dark,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OptimizationPolicy {
    #[default]
    Performance,
    ResourceUsage,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CanvasState {
    pub objects: Vec<CanvasObject>,
}

impl CanvasState {
    // 加载画布从文件
    pub fn load_from_file(path: &std::path::PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let canvas = serde_json::from_str(&content)?;
        Ok(canvas)
    }

    // 保存画布到文件
    pub fn save_to_file(
        &self,
        path: &std::path::PathBuf,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let content = serde_json::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn load_from_file_with_dialog() -> Result<Self, Box<dyn std::error::Error>> {
        let path = rfd::FileDialog::new()
            .add_filter("画布文件", &["json"])
            .pick_file()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidFilename,
                "Cancelled",
            ))?;
        let canvas = CanvasState::load_from_file(&path)?;
        Ok(canvas)
    }

    pub fn save_to_file_with_dialog(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = rfd::FileDialog::new()
            .add_filter("画布文件", &["json"])
            .set_file_name("canvas.json")
            .save_file()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidFilename,
                "Cancelled",
            ))?;

        self.save_to_file(&path)?;
        Ok(())
    }
}

// 应用程序设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentState {
    pub theme_mode: ThemeMode,
    pub background_color: Color32,
    pub window_opacity: f32,

    pub stroke_smoothing: bool,
    pub stroke_straightening: bool,
    pub stroke_straightening_tolerance: f32,
    pub interpolation_frequency: f32,
    pub quick_colors: Vec<Color32>,

    pub show_fps: bool,
    pub window_mode: WindowMode,
    pub present_mode: PresentMode,
    pub optimization_policy: OptimizationPolicy,

    pub keep_insertion_window_open: bool,

    pub show_welcome_window_on_start: bool,
    pub show_startup_animation: bool,
}

impl Default for PersistentState {
    fn default() -> Self {
        Self {
            theme_mode: ThemeMode::default(),
            background_color: Color32::from_rgb(15, 38, 30),
            window_opacity: 1.0,

            stroke_smoothing: true,
            stroke_straightening: true,
            stroke_straightening_tolerance: 20.0,
            interpolation_frequency: 0.1,
            quick_colors: vec![
                Color32::from_rgb(255, 0, 0),     // 红色
                Color32::from_rgb(255, 255, 0),   // 黄色
                Color32::from_rgb(0, 255, 0),     // 绿色
                Color32::from_rgb(0, 0, 0),       // 黑色
                Color32::from_rgb(255, 255, 255), // 白色
            ],

            show_fps: true,
            window_mode: WindowMode::default(),
            present_mode: PresentMode::AutoVsync,
            optimization_policy: OptimizationPolicy::default(),

            keep_insertion_window_open: true,

            show_welcome_window_on_start: true,
            show_startup_animation: true,
        }
    }
}

impl PersistentState {
    // 获取设置文件路径
    fn get_settings_path() -> std::path::PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        path.push("smartboard");
        std::fs::create_dir_all(&path).ok();
        path.push("settings.json");
        path
    }

    // 加载设置从文件
    pub fn load_from_file() -> Self {
        let settings_path = Self::get_settings_path();
        if let Ok(content) = std::fs::read_to_string(settings_path) {
            if let Ok(settings) = serde_json::from_str(&content) {
                return settings;
            }
        }
        Self::default()
    }

    // 保存设置到文件
    pub fn save_to_file(&self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let settings_path = Self::get_settings_path();
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(settings_path, content)?;
        Ok(())
    }
}

// 绘图数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasStroke {
    pub points: Vec<Pos2>,
    pub widths: Vec<f32>, // 每个点的宽度（用于动态画笔）
    pub color: Color32,
    pub base_width: f32,
}

impl Draw for CanvasStroke {
    fn draw(&self, painter: &egui::Painter, selected: bool) {
        let color = if selected { Color32::BLUE } else { self.color };

        // 如果所有宽度相同，使用简单路径
        let all_same_width = self.widths.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01);

        if all_same_width && self.points.len() == 1 {
            painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                self.points[0],
                self.widths[0] / 2.0,
                color,
            )));
        } else if all_same_width && self.points.len() == 2 {
            // 只有两个点且宽度相同，直接画线段
            painter.line_segment(
                [self.points[0], self.points[1]],
                Stroke::new(self.widths[0], color),
            );
        } else if all_same_width {
            // 多个点但宽度相同，使用路径
            let path = egui::epaint::PathShape::line(
                self.points.clone(),
                Stroke::new(self.widths[0], color),
            );
            painter.add(egui::Shape::Path(path));
        } else {
            // 宽度不同，分段绘制
            for i in 0..self.points.len() - 1 {
                let avg_width = (self.widths[i] + self.widths[i + 1]) / 2.0;
                painter.line_segment(
                    [self.points[i], self.points[i + 1]],
                    Stroke::new(avg_width, color),
                );
            }
        }

        // 如果被选中，绘制边框（类似于形状的实现）
        if selected {
            let stroke_rect = crate::utils::AppUtils::calculate_stroke_bounding_box(self);
            painter.rect_stroke(
                stroke_rect,
                0.0,
                Stroke::new(2.0, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
        }
    }
}

// FPS 计数器
pub struct FpsCounter {
    pub frame_count: u32,
    pub last_time: Instant,
    pub current_fps: f32,
}

impl FpsCounter {
    pub fn new() -> Self {
        Self {
            frame_count: 0,
            last_time: Instant::now(),
            current_fps: 0.0,
        }
    }

    pub fn update(&mut self) -> f32 {
        self.frame_count += 1;

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f32();

        if elapsed >= 0.05 {
            self.current_fps = self.frame_count as f32 / elapsed;
            self.frame_count = 0;
            self.last_time = now;
        }

        self.current_fps
    }
}

// 单个正在绘制的笔画数据
pub struct ActiveStroke {
    pub points: Vec<Pos2>,
    pub widths: Vec<f32>,            // 每个点的宽度（用于动态画笔）
    pub times: Vec<f64>,             // 每个点的时间戳（用于速度计算）
    pub start_time: Instant,         // 笔画开始时间
    pub last_movement_time: Instant, // 最后一次移动的时间（用于检测停留）
}

pub struct StartupAnimation {
    fps: f32,
    start_time: Option<Instant>,

    // Video
    frames: &'static [&'static [u8]],
    texture: Option<TextureHandle>,
    last_frame_index: usize,

    // Audio
    _audio_stream: Option<OutputStream>,
    _audio_sink: Option<Sink>,

    finished: bool,
}

impl StartupAnimation {
    pub fn new(fps: f32, frames: &'static [&'static [u8]], audio: &'static [u8]) -> Self {
        Self {
            fps,
            start_time: None,
            frames,
            texture: None,
            last_frame_index: usize::MAX,
            _audio_stream: None,
            _audio_sink: Some(Self::play_audio(audio)),
            finished: false,
        }
    }

    fn play_audio(audio: &'static [u8]) -> Sink {
        let stream = OutputStreamBuilder::open_default_stream().expect("Failed to open stream");

        let sink = Sink::connect_new(&stream.mixer());

        let cursor = Cursor::new(audio);
        let source = Decoder::new(cursor).unwrap();

        sink.append(source);
        sink.play();

        // keep stream alive
        std::mem::forget(stream);

        sink
    }

    pub fn update(&mut self, ctx: &Context) {
        if self.finished {
            return;
        }

        let start = self.start_time.get_or_insert_with(Instant::now);
        let elapsed = start.elapsed().as_secs_f32();
        let frame_index = (elapsed * self.fps) as usize;

        if frame_index >= self.frames.len() {
            self.finished = true;
            return;
        }

        if frame_index == self.last_frame_index {
            return;
        }

        self.last_frame_index = frame_index;

        let image = image::load_from_memory(self.frames[frame_index])
            .expect("Invalid startup frame")
            .to_rgba8();

        let color_image = ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );

        match &mut self.texture {
            Some(tex) => tex.set(color_image, TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture(
                    "startup_animation",
                    color_image,
                    TextureOptions::LINEAR,
                ));
            }
        }
    }

    pub fn draw_fullscreen(&self, ctx: &Context) {
        if self.finished {
            return;
        }

        let Some(tex) = &self.texture else { return };

        let rect = ctx.content_rect();

        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("startup_animation"),
        ));

        painter.image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

// 历史记录结构
#[derive(Debug, Clone)]
pub struct History {
    pub undo_stack: Vec<CanvasState>,
    pub redo_stack: Vec<CanvasState>,
    pub max_history_size: usize,
}

impl History {
    pub fn new(max_history_size: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history_size,
        }
    }

    pub fn save_state(&mut self, state: &CanvasState) {
        // 保存当前状态到撤销栈
        self.undo_stack.push(state.clone());

        // 如果超过最大历史大小，移除最旧的状态
        if self.undo_stack.len() > self.max_history_size {
            self.undo_stack.remove(0);
        }

        // 清空重做栈，因为新的操作使得重做历史无效
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(previous_state) = self.undo_stack.pop() {
            // 保存当前状态到重做栈
            self.redo_stack.push(current_state.clone());

            // 恢复到之前的状态
            *current_state = previous_state;
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(next_state) = self.redo_stack.pop() {
            // 保存当前状态到撤销栈
            self.undo_stack.push(current_state.clone());

            // 恢复到下一个状态
            *current_state = next_state;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

// 应用程序状态
pub struct AppState {
    pub canvas: CanvasState,
    pub active_strokes: HashMap<u64, ActiveStroke>, // 多点触控笔画，存储触控 ID 到正在绘制的笔画
    pub is_drawing: bool,                           // 是否正在绘制
    pub brush_color: Color32,                       // 画笔颜色
    pub brush_width: f32,                           // 画笔大小
    pub dynamic_brush_width_mode: DynamicBrushWidthMode, // 动态画笔大小微调
    pub current_tool: CanvasTool,                   // 当前工具
    pub eraser_size: f32,                           // 橡皮擦大小
    pub selected_object: Option<usize>,             // 选中的对象索引
    pub drag_start_pos: Option<Pos2>,               //
    pub show_size_preview: bool,                    //
    pub show_text_dialog: bool,                     //
    pub new_text_content: String,                   //
    pub show_shape_dialog: bool,                    //
    pub fps_counter: FpsCounter,                    // FPS 计数器
    pub should_quit: bool,                          //
    pub touch_points: HashMap<u64, Pos2>,           // 多点触控点，存储触控 ID 到位置的映射
    pub window_mode_changed: bool,                  // 窗口模式是否已更改
    pub resize_anchor_hovered: Option<ResizeAnchor>, // 当前悬停的调整大小锚点
    pub rotation_anchor_hovered: bool,              // 是否悬停在旋转锚点上
    pub transform_operation: Option<TransformOperation>, // 当前正在进行的变换操作
    // pub available_video_modes: Vec<winit::monitor::VideoModeHandle>, // 可用的视频模式
    // pub selected_video_mode_index: Option<usize>,   // 选中的视频模式索引
    pub show_quick_color_editor: bool, // 是否显示快捷颜色编辑器
    pub new_quick_color: Color32,      // 新快捷颜色，用于添加
    pub show_touch_points: bool,       // 是否显示触控点，用于调试
    pub present_mode_changed: bool,    // 垂直同步模式是否已更改
    #[cfg(target_os = "windows")]
    pub show_console: bool, // 是否显示控制台 [Windows]
    pub startup_animation: Option<StartupAnimation>, // 启动动画
    pub show_welcome_window: bool,
    pub persistent: PersistentState,
    // pub toast: Option<Toast>, // 当前显示的 Toast 通知
    pub toasts: Toasts,
    pub history: History, // 历史记录
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            canvas: CanvasState::default(),
            active_strokes: HashMap::new(),
            is_drawing: false,
            brush_color: Color32::WHITE,
            brush_width: 3.0,
            dynamic_brush_width_mode: DynamicBrushWidthMode::default(),
            current_tool: CanvasTool::Brush,
            eraser_size: 10.0,
            selected_object: None,
            drag_start_pos: None,
            show_size_preview: false,
            fps_counter: FpsCounter::new(),
            should_quit: false,
            show_text_dialog: false,
            new_text_content: String::from(""),
            show_shape_dialog: false,
            touch_points: HashMap::new(),
            window_mode_changed: false,
            resize_anchor_hovered: None,
            rotation_anchor_hovered: false,
            transform_operation: None,
            // available_video_modes: Vec::new(),
            // selected_video_mode_index: None,
            show_quick_color_editor: false,
            new_quick_color: Color32::WHITE,
            show_touch_points: false,
            present_mode_changed: false,
            #[cfg(target_os = "windows")]
            show_console: false,
            startup_animation: None,
            show_welcome_window: true,
            persistent: PersistentState::load_from_file(),
            // toast: None,
            // toasts: Toasts::default()
            //     .anchor(egui::Align2::CENTER_BOTTOM, egui::pos2(0.0, -300.0))
            //     .direction(egui::Direction::BottomUp),
            toasts: Toasts::default()
                .with_anchor(egui_notify::Anchor::BottomRight)
                .with_margin(egui::vec2(20.0, 20.0)),
            history: History::new(50), // 历史记录，最大保存50个状态
        }
    }
}
