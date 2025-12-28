use egui::Color32;
use egui::Pos2;
use std::collections::HashMap;
use std::time::Instant;

// 窗口模式
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Windowed,          // 窗口模式
    Fullscreen,        // 全屏模式
    BorderlessFullscreen, // 无边框全屏模式
}

// 动态画笔模式
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DynamicBrushMode {
    Disabled,   // 禁用
    BrushTip,   // 模拟笔锋
    SpeedBased, // 基于速度
}

// 工具类型
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,       // 选择
    Brush,        // 画笔
    ObjectEraser, // 对象橡皮擦
    PixelEraser,  // 像素橡皮擦
    Insert,       // 插入
    Background,   // 背景
    Settings,     // 设置
}

// 插入的图片数据结构
pub struct InsertedImage {
    pub texture: egui::TextureHandle,
    pub pos: Pos2,
    pub size: egui::Vec2,
    pub aspect_ratio: f32,
    pub marked_for_deletion: bool, // deferred deletion to avoid panic
}

// 插入的文本数据结构
pub struct InsertedText {
    pub text: String,
    pub pos: Pos2,
    pub color: Color32,
    pub font_size: f32,
}

// 插入的形状数据结构
#[derive(Clone, Copy, Debug)]
pub enum ShapeType {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

pub struct InsertedShape {
    pub shape_type: ShapeType,
    pub pos: Pos2,
    pub size: f32,
    pub color: Color32,
    pub rotation: f32,
}

// 被选择的对象
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectedObject {
    Stroke(usize),
    Image(usize),
    Text(usize),
    Shape(usize),
}

// 绘图数据结构
#[derive(Clone)]
pub struct DrawingStroke {
    pub points: Vec<Pos2>,
    pub widths: Vec<f32>, // 每个点的宽度（用于动态画笔）
    pub color: Color32,
    pub base_width: f32,
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
    pub widths: Vec<f32>, // 每个点的宽度（用于动态画笔）
    pub times: Vec<f64>,  // 每个点的时间戳（用于速度计算）
    pub start_time: Instant, // 笔画开始时间
}

// 应用程序状态
pub struct AppState {
    pub strokes: Vec<DrawingStroke>,
    pub images: Vec<InsertedImage>,
    pub texts: Vec<InsertedText>,
    pub shapes: Vec<InsertedShape>,
    pub active_strokes: HashMap<u64, ActiveStroke>, // 多点触控笔画，存储触控 ID 到正在绘制的笔画
    pub is_drawing: bool,
    pub brush_color: Color32,
    pub brush_width: f32,
    pub dynamic_brush_mode: DynamicBrushMode,
    pub stroke_smoothing: bool, // 笔画平滑选项
    pub current_tool: Tool,
    pub eraser_size: f32,          // 橡皮擦大小
    pub background_color: Color32, // 背景颜色
    pub selected_object: Option<SelectedObject>,
    pub drag_start_pos: Option<Pos2>,
    pub show_size_preview: bool,
    pub show_text_dialog: bool,
    pub new_text_content: String,
    pub show_shape_dialog: bool,
    pub show_fps: bool,          // 是否显示FPS
    pub fps_counter: FpsCounter, // FPS计数器
    pub should_quit: bool,
    pub touch_points: HashMap<u64, Pos2>, // 多点触控点，存储触控 ID 到位置的映射
    pub window_mode: WindowMode, // 窗口模式
    pub window_mode_changed: bool, // 窗口模式是否已更改
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            strokes: Vec::new(),
            images: Vec::new(),
            texts: Vec::new(),
            shapes: Vec::new(),
            active_strokes: HashMap::new(),
            is_drawing: false,
            brush_color: Color32::WHITE,
            brush_width: 5.0,
            dynamic_brush_mode: DynamicBrushMode::Disabled,
            stroke_smoothing: true,
            current_tool: Tool::Brush,
            eraser_size: 10.0,
            background_color: Color32::from_rgb(16, 80, 60),
            selected_object: None,
            drag_start_pos: None,
            show_size_preview: false,
            show_fps: true,
            fps_counter: FpsCounter::new(),
            should_quit: false,
            show_text_dialog: false,
            new_text_content: String::from(""),
            show_shape_dialog: false,
            touch_points: HashMap::new(),
            window_mode: WindowMode::BorderlessFullscreen,
            window_mode_changed: false,
        }
    }
}
