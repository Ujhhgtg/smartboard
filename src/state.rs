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

use crate::utils;

// 动态画笔模式
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DynamicBrushWidthMode {
    #[default]
    Disabled, // 禁用
    BrushTip,   // 模拟笔锋
    SpeedBased, // 基于速度
}

// 调整句柄类型
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransformHandle {
    // 8个调整大小的句柄
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    // 旋转句柄
    Rotate,
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

impl CanvasImage {
    pub fn bounding_box(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, self.size)
    }
}

impl Draw for CanvasImage {
    fn draw(&self, painter: &egui::Painter, selected: bool) {
        let img_rect = self.bounding_box();
        painter.image(
            self.texture.id(),
            img_rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // 如果被选中，绘制边框和调整句柄
        if selected {
            painter.rect_stroke(
                img_rect,
                0.0,
                Stroke::new(2.0, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, img_rect);
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

impl CanvasText {
    pub fn bounding_box(&self) -> egui::Rect {
        // 估算文本尺寸（近似值）
        let approx_char_width = self.font_size * 0.6;
        let approx_width = self.text.len() as f32 * approx_char_width;
        let approx_height = self.font_size * 1.2;
        egui::Rect::from_min_size(self.pos, egui::vec2(approx_width, approx_height))
    }
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
            let text_rect = self.bounding_box();
            painter.rect_stroke(
                text_rect,
                0.0,
                Stroke::new(2.0, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, text_rect);
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

impl CanvasShape {
    pub fn bounding_box(&self) -> egui::Rect {
        match self.shape_type {
            CanvasShapeType::Line => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                let min_x = self.pos.x.min(end_point.x) - 5.0;
                let max_x = self.pos.x.max(end_point.x) + 5.0;
                let min_y = self.pos.y.min(end_point.y) - 5.0;
                let max_y = self.pos.y.max(end_point.y) + 5.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            CanvasShapeType::Arrow => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                let min_x = self.pos.x.min(end_point.x) - 5.0;
                let max_x = self.pos.x.max(end_point.x) + 5.0;
                let min_y = self.pos.y.min(end_point.y) - 15.0;
                let max_y = self.pos.y.max(end_point.y) + 15.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            CanvasShapeType::Rectangle => {
                egui::Rect::from_min_size(self.pos, egui::vec2(self.size, self.size))
            }
            CanvasShapeType::Triangle => {
                let half_size = self.size / 2.0;
                let min_x = self.pos.x - 5.0;
                let max_x = self.pos.x + self.size + 5.0;
                let min_y = self.pos.y - 5.0;
                let max_y = self.pos.y + half_size + 5.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            CanvasShapeType::Circle => {
                let radius = self.size / 2.0;
                egui::Rect::from_min_max(
                    Pos2::new(self.pos.x - radius - 5.0, self.pos.y - radius - 5.0),
                    Pos2::new(self.pos.x + radius + 5.0, self.pos.y + radius + 5.0),
                )
            }
        }
    }
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

        // 如果被选中，绘制边框和调整句柄
        if selected {
            let shape_rect = self.bounding_box();
            painter.rect_stroke(
                shape_rect,
                0.0,
                Stroke::new(2.0, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, shape_rect);
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

    pub fn bounding_box(&self) -> egui::Rect {
        match self {
            CanvasObject::Stroke(stroke) => stroke.bounding_box(),
            CanvasObject::Image(image) => image.bounding_box(),
            CanvasObject::Text(text) => text.bounding_box(),
            CanvasObject::Shape(shape) => shape.bounding_box(),
        }
    }
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
    #[serde(default)]
    pub theme_mode: ThemeMode,
    #[serde(default)]
    pub background_color: Color32,
    #[serde(default)]
    pub window_opacity: f32,

    #[serde(default)]
    pub stroke_smoothing: bool,
    #[serde(default)]
    pub stroke_straightening: bool,
    #[serde(default)]
    pub stroke_straightening_tolerance: f32,
    #[serde(default)]
    pub interpolation_frequency: f32,
    #[serde(default)]
    pub quick_colors: Vec<Color32>,

    #[serde(default)]
    pub show_fps: bool,
    #[serde(default)]
    pub window_mode: WindowMode,
    #[serde(default)]
    pub present_mode: PresentMode,
    #[serde(default)]
    pub optimization_policy: OptimizationPolicy,

    #[serde(default)]
    pub keep_insertion_window_open: bool,

    #[serde(default)]
    pub show_welcome_window_on_start: bool,
    #[serde(default)]
    pub show_startup_animation: bool,

    #[serde(default)]
    pub easter_egg_redo: bool,
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
            quick_colors: utils::get_default_quick_colors(),

            show_fps: true,
            window_mode: WindowMode::default(),
            present_mode: PresentMode::AutoVsync,
            optimization_policy: OptimizationPolicy::default(),

            keep_insertion_window_open: true,

            show_welcome_window_on_start: true,
            show_startup_animation: true,

            easter_egg_redo: false,
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

impl CanvasStroke {
    pub fn bounding_box(&self) -> egui::Rect {
        if self.points.is_empty() {
            return egui::Rect::from_min_max(Pos2::ZERO, Pos2::ZERO);
        }

        // 计算所有点的最小和最大坐标
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;

        for point in &self.points {
            min_x = min_x.min(point.x);
            max_x = max_x.max(point.x);
            min_y = min_y.min(point.y);
            max_y = max_y.max(point.y);
        }

        // 考虑笔画宽度，添加一些边距
        let max_width = self.widths.iter().copied().fold(0.0, f32::max);
        let padding = max_width / 2.0 + 5.0; // 添加额外的5像素边距

        egui::Rect::from_min_max(
            Pos2::new(min_x - padding, min_y - padding),
            Pos2::new(max_x + padding, max_y + padding),
        )
    }
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
            let stroke_rect = self.bounding_box();
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

// 历史记录命令枚举
#[derive(Debug, Clone)]
pub enum HistoryCommand {
    // 添加对象命令
    AddObject {
        index: usize,
        object: CanvasObject,
    },
    // 删除对象命令
    RemoveObject {
        index: usize,
        object: CanvasObject,
    },
    // 修改对象命令（用于对象属性更改）
    ModifyObject {
        index: usize,
        old_object: CanvasObject,
        new_object: CanvasObject,
    },
    // 批量操作（用于清空画布等）
    ClearObjects {
        objects: Vec<CanvasObject>,
    },
}

// 优化的历史记录结构
#[derive(Debug, Clone)]
pub struct History {
    undo_stack: Vec<HistoryCommand>,
    redo_stack: Vec<HistoryCommand>,
    max_history_size: usize,
    memory_usage: usize,
    max_memory_usage: usize, // 最大内存使用量 (字节)
}

impl History {
    pub fn new(max_history_size: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history_size,
            memory_usage: 0,
            max_memory_usage: 50 * 1024 * 1024, // 默认 50MB
        }
    }

    // 估算命令的内存使用量
    fn estimate_command_size(command: &HistoryCommand) -> usize {
        match command {
            HistoryCommand::AddObject { object, .. } => {
                // 估算对象大小
                match object {
                    CanvasObject::Stroke(stroke) => {
                        stroke.points.len() * std::mem::size_of::<Pos2>()
                            + stroke.widths.len() * std::mem::size_of::<f32>()
                            + 128 // 其他字段
                    }
                    CanvasObject::Image(_) => 256, // 图像对象相对较小（不包含纹理数据）
                    CanvasObject::Text(text) => text.text.len() + 128,
                    CanvasObject::Shape(_) => 128,
                }
            }
            HistoryCommand::RemoveObject { object, .. } => {
                Self::estimate_command_size(&HistoryCommand::AddObject {
                    index: 0,
                    object: object.clone(),
                })
            }
            HistoryCommand::ModifyObject {
                old_object,
                new_object,
                ..
            } => {
                Self::estimate_command_size(&HistoryCommand::AddObject {
                    index: 0,
                    object: old_object.clone(),
                }) + Self::estimate_command_size(&HistoryCommand::AddObject {
                    index: 0,
                    object: new_object.clone(),
                })
            }
            HistoryCommand::ClearObjects { objects } => objects
                .iter()
                .map(|obj| {
                    Self::estimate_command_size(&HistoryCommand::AddObject {
                        index: 0,
                        object: obj.clone(),
                    })
                })
                .sum(),
        }
    }

    // 清理历史记录以保持内存限制
    fn enforce_memory_limit(&mut self) {
        while self.memory_usage > self.max_memory_usage && !self.undo_stack.is_empty() {
            let removed_command = self.undo_stack.remove(0);
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&removed_command));
        }
    }

    // 保存添加对象的命令
    pub fn save_add_object(&mut self, index: usize, object: CanvasObject) {
        let command = HistoryCommand::AddObject { index, object };
        self.memory_usage += Self::estimate_command_size(&command);
        self.undo_stack.push(command);

        // 清理超出限制的历史记录
        if self.undo_stack.len() > self.max_history_size {
            let removed_command = self.undo_stack.remove(0);
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&removed_command));
        }
        self.enforce_memory_limit();

        // 清空重做栈
        for cmd in self.redo_stack.drain(..) {
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&cmd));
        }
    }

    // 保存删除对象的命令
    pub fn save_remove_object(&mut self, index: usize, object: CanvasObject) {
        let command = HistoryCommand::RemoveObject { index, object };
        self.memory_usage += Self::estimate_command_size(&command);
        self.undo_stack.push(command);

        if self.undo_stack.len() > self.max_history_size {
            let removed_command = self.undo_stack.remove(0);
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&removed_command));
        }
        self.enforce_memory_limit();

        for cmd in self.redo_stack.drain(..) {
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&cmd));
        }
    }

    // 保存修改对象的命令
    pub fn save_modify_object(
        &mut self,
        index: usize,
        old_object: CanvasObject,
        new_object: CanvasObject,
    ) {
        let command = HistoryCommand::ModifyObject {
            index,
            old_object,
            new_object,
        };
        self.memory_usage += Self::estimate_command_size(&command);
        self.undo_stack.push(command);

        if self.undo_stack.len() > self.max_history_size {
            let removed_command = self.undo_stack.remove(0);
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&removed_command));
        }
        self.enforce_memory_limit();

        for cmd in self.redo_stack.drain(..) {
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&cmd));
        }
    }

    // 保存清空对象的命令
    pub fn save_clear_objects(&mut self, objects: Vec<CanvasObject>) {
        let command = HistoryCommand::ClearObjects { objects };
        self.memory_usage += Self::estimate_command_size(&command);
        self.undo_stack.push(command);

        if self.undo_stack.len() > self.max_history_size {
            let removed_command = self.undo_stack.remove(0);
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&removed_command));
        }
        self.enforce_memory_limit();

        for cmd in self.redo_stack.drain(..) {
            self.memory_usage = self
                .memory_usage
                .saturating_sub(Self::estimate_command_size(&cmd));
        }
    }

    // 执行撤销操作
    pub fn undo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(command) = self.undo_stack.pop() {
            match command {
                HistoryCommand::AddObject { index, object } => {
                    // 撤销添加操作 = 删除对象
                    if index < current_state.objects.len() {
                        current_state.objects.remove(index);
                        // 将撤销操作的逆操作推送到重做栈
                        let inverse_cmd = HistoryCommand::RemoveObject { index, object };
                        self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                        self.redo_stack.push(inverse_cmd);
                    }
                }
                HistoryCommand::RemoveObject { index, object } => {
                    // 撤销删除操作 = 添加对象回来
                    if index <= current_state.objects.len() {
                        current_state.objects.insert(index, object.clone());
                        // 将撤销操作的逆操作推送到重做栈
                        let inverse_cmd = HistoryCommand::AddObject { index, object };
                        self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                        self.redo_stack.push(inverse_cmd);
                    }
                }
                HistoryCommand::ModifyObject {
                    index,
                    old_object,
                    new_object,
                } => {
                    // 撤销修改操作 = 恢复旧对象
                    if index < current_state.objects.len() {
                        let current_object =
                            std::mem::replace(&mut current_state.objects[index], old_object);
                        // 将撤销操作的逆操作推送到重做栈
                        let inverse_cmd = HistoryCommand::ModifyObject {
                            index,
                            old_object: current_object,
                            new_object,
                        };
                        self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                        self.redo_stack.push(inverse_cmd);
                    }
                }
                HistoryCommand::ClearObjects { objects } => {
                    // 撤销清空操作 = 恢复所有对象
                    let old_objects = std::mem::replace(&mut current_state.objects, objects);
                    // 将撤销操作的逆操作推送到重做栈
                    let inverse_cmd = HistoryCommand::ClearObjects {
                        objects: old_objects,
                    };
                    self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                    self.redo_stack.push(inverse_cmd);
                }
            }
            true
        } else {
            false
        }
    }

    // 执行重做操作
    pub fn redo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(command) = self.redo_stack.pop() {
            match command {
                HistoryCommand::AddObject { index, object } => {
                    // 重做添加操作 = 添加对象
                    if index <= current_state.objects.len() {
                        current_state.objects.insert(index, object.clone());
                        // 将逆操作推送到撤销栈
                        let inverse_cmd = HistoryCommand::RemoveObject { index, object };
                        self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                        self.undo_stack.push(inverse_cmd);
                    }
                }
                HistoryCommand::RemoveObject { index, object } => {
                    // 重做删除操作 = 删除对象
                    if index < current_state.objects.len() {
                        current_state.objects.remove(index);
                        // 将逆操作推送到撤销栈
                        let inverse_cmd = HistoryCommand::AddObject { index, object };
                        self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                        self.undo_stack.push(inverse_cmd);
                    }
                }
                HistoryCommand::ModifyObject {
                    index,
                    old_object: _,
                    new_object,
                } => {
                    // 重做修改操作 = 应用新对象
                    if index < current_state.objects.len() {
                        let current_object = std::mem::replace(
                            &mut current_state.objects[index],
                            new_object.clone(),
                        );
                        // 将逆操作推送到撤销栈
                        let inverse_cmd = HistoryCommand::ModifyObject {
                            index,
                            old_object: current_object,
                            new_object,
                        };
                        self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                        self.undo_stack.push(inverse_cmd);
                    }
                }
                HistoryCommand::ClearObjects { objects } => {
                    // 重做清空操作 = 恢复保存的对象
                    let old_objects = std::mem::replace(&mut current_state.objects, objects);
                    // 将逆操作推送到撤销栈
                    let inverse_cmd = HistoryCommand::ClearObjects {
                        objects: old_objects,
                    };
                    self.memory_usage += Self::estimate_command_size(&inverse_cmd);
                    self.undo_stack.push(inverse_cmd);
                }
            }
            true
        } else {
            false
        }
    }

    // // 获取内存使用量（用于调试）
    // pub fn memory_usage(&self) -> usize {
    //     self.memory_usage
    // }

    // // 清空历史记录
    // pub fn clear(&mut self) {
    //     self.undo_stack.clear();
    //     self.redo_stack.clear();
    //     self.memory_usage = 0;
    // }

    // // 检查是否可以撤销
    // pub fn can_undo(&self) -> bool {
    //     !self.undo_stack.is_empty()
    // }

    // // 检查是否可以重做
    // pub fn can_redo(&self) -> bool {
    //     !self.redo_stack.is_empty()
    // }

    // 兼容性方法：保存完整状态（用于向后兼容）
    pub fn save_state(&mut self, state: &CanvasState) {
        // 对于复杂操作，回退到保存完整状态
        // 这种情况很少发生，主要用于批量操作
        self.save_clear_objects(state.objects.clone());
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
    pub drag_start_pos: Option<Pos2>,               // 拖拽开始位置
    pub dragged_handle: Option<TransformHandle>,    // 正在拖拽的调整句柄
    pub show_size_preview: bool,                    //
    pub show_insert_text_dialog: bool,              //
    pub new_text_content: String,                   //
    pub show_insert_shape_dialog: bool,             //
    pub fps_counter: FpsCounter,                    // FPS 计数器
    pub should_quit: bool,                          //
    pub touch_points: HashMap<u64, Pos2>,           // 多点触控点，存储触控 ID 到位置的映射
    pub window_mode_changed: bool,                  // 窗口模式是否已更改
    pub available_video_modes: Vec<winit::monitor::VideoModeHandle>,
    pub selected_video_mode_index: Option<usize>, // 选中的视频模式索引
    pub show_quick_color_editor: bool,            // 是否显示快捷颜色编辑器
    pub new_quick_color: Color32,                 // 新快捷颜色，用于添加
    pub show_touch_points: bool,                  // 是否显示触控点，用于调试
    pub present_mode_changed: bool,               // 垂直同步模式是否已更改
    #[cfg(target_os = "windows")]
    pub show_console: bool, // 是否显示控制台 [Windows]
    pub startup_animation: Option<StartupAnimation>, // 启动动画
    pub show_welcome_window: bool,
    pub persistent: PersistentState,
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
            dragged_handle: None,
            show_size_preview: false,
            fps_counter: FpsCounter::new(),
            should_quit: false,
            show_insert_text_dialog: false,
            new_text_content: String::from(""),
            show_insert_shape_dialog: false,
            touch_points: HashMap::new(),
            window_mode_changed: false,
            available_video_modes: Vec::new(),
            selected_video_mode_index: None,
            show_quick_color_editor: false,
            new_quick_color: Color32::WHITE,
            show_touch_points: false,
            present_mode_changed: false,
            #[cfg(target_os = "windows")]
            show_console: false,
            startup_animation: None,
            show_welcome_window: true,
            persistent: PersistentState::load_from_file(),
            toasts: Toasts::default()
                .with_anchor(egui_notify::Anchor::BottomRight)
                .with_margin(egui::vec2(20.0, 20.0)),
            history: History::new(50), // 历史记录，最大保存50个状态
        }
    }
}

pub const FONT: &[u8] = include_bytes!("../assets/fonts/NotoSansCJKsc-Regular.otf");
pub const ICON: &[u8] = include_bytes!("../assets/images/icon.ico");
