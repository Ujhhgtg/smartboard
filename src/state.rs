use egui::Color32;
use egui::Pos2;
use egui::Stroke;
use std::collections::HashMap;
use std::time::Instant;
use wgpu::PresentMode;

// 窗口模式
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Windowed,             // 窗口模式
    Fullscreen,           // 全屏模式
    BorderlessFullscreen, // 无边框全屏
}

// 动态画笔模式
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DynamicBrushWidthMode {
    Disabled,   // 禁用
    BrushTip,   // 模拟笔锋
    SpeedBased, // 基于速度
}

// 主题模式
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    System, // 跟随系统
    Light,  // 浅色模式
    Dark,   // 深色模式
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

// 画布对象类型
// 画布对象数据结构
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

// 应用程序状态
pub struct AppState {
    pub canvas_objects: Vec<CanvasObject>,          // 所有画布对象
    pub active_strokes: HashMap<u64, ActiveStroke>, // 多点触控笔画，存储触控 ID 到正在绘制的笔画
    pub is_drawing: bool,                           // 是否正在绘制
    pub brush_color: Color32,                       // 画笔颜色
    pub brush_width: f32,                           // 画笔大小
    pub dynamic_brush_width_mode: DynamicBrushWidthMode, // 动态画笔大小微调
    pub stroke_smoothing: bool,                     // 笔画平滑选项
    pub interpolation_frequency: f32,               // 插值频率
    pub current_tool: CanvasTool,                   // 当前工具
    pub eraser_size: f32,                           // 橡皮擦大小
    pub background_color: Color32,                  // 背景颜色
    pub selected_object: Option<usize>,             // 选中的对象索引
    pub drag_start_pos: Option<Pos2>,               //
    pub show_size_preview: bool,                    //
    pub show_text_dialog: bool,                     //
    pub new_text_content: String,                   //
    pub show_shape_dialog: bool,                    //
    pub show_fps: bool,                             // 是否显示 FPS
    pub fps_counter: FpsCounter,                    // FPS 计数器
    pub should_quit: bool,                          //
    pub touch_points: HashMap<u64, Pos2>,           // 多点触控点，存储触控 ID 到位置的映射
    pub window_mode: WindowMode,                    // 窗口模式
    pub window_mode_changed: bool,                  // 窗口模式是否已更改
    pub keep_insertion_window_open: bool,           // 是否保持插入对象窗口开启
    pub resize_anchor_hovered: Option<ResizeAnchor>, // 当前悬停的调整大小锚点
    pub rotation_anchor_hovered: bool,              // 是否悬停在旋转锚点上
    pub resize_operation: Option<ResizeOperation>,  // 当前正在进行的调整大小操作
    pub rotation_operation: Option<RotationOperation>, // 当前正在进行的旋转操作
    pub available_video_modes: Vec<winit::monitor::VideoModeHandle>, // 可用的视频模式
    pub selected_video_mode_index: Option<usize>,   // 选中的视频模式索引
    pub quick_colors: Vec<Color32>,                 // 快捷颜色列表
    pub show_quick_color_editor: bool,              // 是否显示快捷颜色编辑器
    pub new_quick_color: Color32,                   // 新快捷颜色，用于添加
    pub show_touch_points: bool,                    // 是否显示触控点，用于调试
    pub present_mode: PresentMode,                  // 垂直同步模式
    pub present_mode_changed: bool,                 // 垂直同步模式是否已更改
    pub theme_mode: ThemeMode,                      // 主题模式
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            canvas_objects: Vec::new(),
            active_strokes: HashMap::new(),
            is_drawing: false,
            brush_color: Color32::WHITE,
            brush_width: 3.0,
            dynamic_brush_width_mode: DynamicBrushWidthMode::Disabled,
            stroke_smoothing: true,
            interpolation_frequency: 0.3,
            current_tool: CanvasTool::Brush,
            eraser_size: 10.0,
            background_color: Color32::from_rgb(0, 50, 35),
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
            keep_insertion_window_open: true,
            resize_anchor_hovered: None,
            rotation_anchor_hovered: false,
            resize_operation: None,
            rotation_operation: None,
            available_video_modes: Vec::new(),
            selected_video_mode_index: None,
            quick_colors: vec![
                Color32::from_rgb(255, 0, 0),     // 红色
                Color32::from_rgb(255, 255, 0),   // 黄色
                Color32::from_rgb(0, 255, 0),     // 绿色
                Color32::from_rgb(0, 0, 0),       // 黑色
                Color32::from_rgb(255, 255, 255), // 白色
            ],
            show_quick_color_editor: false,
            new_quick_color: Color32::WHITE,
            show_touch_points: false,
            present_mode: PresentMode::AutoVsync,
            present_mode_changed: false,
            theme_mode: ThemeMode::System,
        }
    }
}
