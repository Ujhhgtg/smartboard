use egui::Color32;
use egui::Pos2;
use egui::Stroke;
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use rodio::OutputStreamBuilder;
use rodio::{Decoder, OutputStream, Sink};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use std::time::Instant;
use wgpu::PresentMode;

// 动态画笔模式
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DynamicBrushWidthMode {
    Disabled,   // 禁用
    BrushTip,   // 模拟笔锋
    SpeedBased, // 基于速度
}

// 工具类型
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CanvasTool {
    Select,       // 选择
    Brush,        // 画笔
    ObjectEraser, // 对象橡皮擦
    PixelEraser,  // 像素橡皮擦
    Insert,       // 插入
    Settings,     // 设置
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

// 插入的文本数据结构
#[derive(Clone)]
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
#[derive(Clone, Copy, Debug)]
pub enum CanvasShapeType {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

#[derive(Clone)]
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
#[derive(Clone)]
pub enum CanvasObject {
    Stroke(CanvasStroke),
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
#[derive(Clone, Copy, PartialEq, Eq)]
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

// 调整大小操作
#[derive(Clone, Copy)]
pub struct ResizeOperation {
    pub anchor: ResizeAnchor,
    pub start_pos: Pos2,
    pub start_size: egui::Vec2,
    pub start_object_pos: Pos2,
}

// 旋转操作
#[derive(Clone, Copy)]
pub struct RotationOperation {
    pub start_pos: Pos2,
    pub start_angle: f32,
    pub center: Pos2,
}

// 窗口模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowMode {
    Windowed,
    Fullscreen,
    BorderlessFullscreen,
}

// 主题模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

// 应用程序设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentState {
    pub stroke_smoothing: bool,
    pub interpolation_frequency: f32,
    pub show_fps: bool,
    pub window_mode: WindowMode,
    pub keep_insertion_window_open: bool,
    pub quick_colors: Vec<Color32>,
    pub theme_mode: ThemeMode,
    pub background_color: Color32,
    pub show_welcome_window_on_start: bool,
}

impl Default for PersistentState {
    fn default() -> Self {
        Self {
            stroke_smoothing: true,
            interpolation_frequency: 0.1,
            show_fps: true,
            window_mode: WindowMode::BorderlessFullscreen,
            keep_insertion_window_open: true,
            quick_colors: vec![
                Color32::from_rgb(255, 0, 0),     // 红色
                Color32::from_rgb(255, 255, 0),   // 黄色
                Color32::from_rgb(0, 255, 0),     // 绿色
                Color32::from_rgb(0, 0, 0),       // 黑色
                Color32::from_rgb(255, 255, 255), // 白色
            ],
            theme_mode: ThemeMode::System,
            background_color: Color32::from_rgb(15, 38, 30),
            show_welcome_window_on_start: true,
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
#[derive(Clone)]
pub struct CanvasStroke {
    pub points: Vec<Pos2>,
    pub widths: Vec<f32>, // 每个点的宽度（用于动态画笔）
    pub color: Color32,
    pub base_width: f32,
}

impl Draw for CanvasStroke {
    fn draw(&self, painter: &egui::Painter, selected: bool) {
        if self.points.len() < 2 {
            return;
        }

        let color = if selected { Color32::BLUE } else { self.color };

        // 如果所有宽度相同，使用简单路径
        let all_same_width = self.widths.windows(2).all(|w| (w[0] - w[1]).abs() < 0.01);

        if all_same_width && self.points.len() == 2 {
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
    pub widths: Vec<f32>,    // 每个点的宽度（用于动态画笔）
    pub times: Vec<f64>,     // 每个点的时间戳（用于速度计算）
    pub start_time: Instant, // 笔画开始时间
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

// 应用程序状态
pub struct AppState {
    pub canvas_objects: Vec<CanvasObject>,          // 所有画布对象
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
    pub resize_operation: Option<ResizeOperation>,  // 当前正在进行的调整大小操作
    pub rotation_operation: Option<RotationOperation>, // 当前正在进行的旋转操作
    // pub available_video_modes: Vec<winit::monitor::VideoModeHandle>, // 可用的视频模式
    // pub selected_video_mode_index: Option<usize>,   // 选中的视频模式索引
    pub show_quick_color_editor: bool, // 是否显示快捷颜色编辑器
    pub new_quick_color: Color32,      // 新快捷颜色，用于添加
    pub show_touch_points: bool,       // 是否显示触控点，用于调试
    pub present_mode: PresentMode,     // 垂直同步模式
    pub present_mode_changed: bool,    // 垂直同步模式是否已更改
    pub show_console: bool,            // 是否显示控制台 [Windows]
    pub startup_animation: StartupAnimation, // 启动动画
    pub show_welcome_window: bool,
    pub persistent: PersistentState,
}

// 启动动画
const STARTUP_FRAMES: &[&[u8]] = &[
    include_bytes!("../assets/startup_animation/frames/frame_0001.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0002.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0003.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0004.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0005.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0006.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0007.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0008.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0009.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0010.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0011.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0012.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0013.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0014.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0015.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0016.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0017.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0018.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0019.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0020.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0021.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0022.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0023.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0024.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0025.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0026.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0027.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0028.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0029.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0030.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0031.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0032.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0033.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0034.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0035.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0036.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0037.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0038.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0039.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0040.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0041.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0042.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0043.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0044.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0045.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0046.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0047.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0048.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0049.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0050.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0051.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0052.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0053.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0054.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0055.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0056.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0057.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0058.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0059.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0060.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0061.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0062.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0063.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0064.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0065.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0066.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0067.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0068.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0069.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0070.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0071.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0072.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0073.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0074.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0075.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0076.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0077.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0078.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0079.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0080.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0081.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0082.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0083.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0084.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0085.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0086.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0087.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0088.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0089.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0090.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0091.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0092.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0093.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0094.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0095.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0096.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0097.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0098.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0099.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0100.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0101.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0102.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0103.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0104.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0105.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0106.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0107.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0108.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0109.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0110.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0111.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0112.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0113.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0114.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0115.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0116.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0117.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0118.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0119.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0120.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0121.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0122.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0123.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0124.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0125.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0126.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0127.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0128.png"),
    include_bytes!("../assets/startup_animation/frames/frame_0129.png"),
];
const STARTUP_AUDIO: &[u8] = include_bytes!("../assets/startup_animation/audio.wav");

impl Default for AppState {
    fn default() -> Self {
        Self {
            canvas_objects: Vec::new(),
            active_strokes: HashMap::new(),
            is_drawing: false,
            brush_color: Color32::WHITE,
            brush_width: 3.0,
            dynamic_brush_width_mode: DynamicBrushWidthMode::Disabled,
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
            resize_operation: None,
            rotation_operation: None,
            // available_video_modes: Vec::new(),
            // selected_video_mode_index: None,
            show_quick_color_editor: false,
            new_quick_color: Color32::WHITE,
            show_touch_points: false,
            present_mode: PresentMode::AutoVsync,
            present_mode_changed: false,
            show_console: false,
            startup_animation: StartupAnimation::new(30.0, STARTUP_FRAMES, STARTUP_AUDIO),
            show_welcome_window: true,
            persistent: PersistentState::load_from_file(),
        }
    }
}
