pub mod flat;

use flat::CanvasStateFlat;

use egui::{Color32, Pos2, Stroke};
use egui_notify::Toasts;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tray_icon::TrayIcon;
use wgpu::Backend;
use wgpu::PresentMode;

#[cfg(feature = "startup_animation")]
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
#[cfg(feature = "startup_animation")]
use rodio::Decoder;
#[cfg(feature = "startup_animation")]
use rodio::DeviceSinkBuilder;
#[cfg(feature = "startup_animation")]
use rodio::Player;
#[cfg(feature = "startup_animation")]
use std::io::Cursor;

use crate::utils;

/// Dynamic brush width mode for stroke rendering
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DynamicBrushWidthMode {
    #[default]
    Disabled, // No dynamic width adjustment
    BrushTip,   // Simulates brush tip pressure for calligraphy effect
    SpeedBased, // Adjusts width based on drawing speed
}

/// Stroke width representation
#[derive(Debug, Clone)]
pub enum StrokeWidth {
    Fixed(f32),
    Dynamic(Vec<f32>),
}

impl StrokeWidth {
    pub fn get(&self, index: usize) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => v[index],
        }
    }

    pub fn first(&self) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => v[0],
        }
    }

    pub fn last(&self) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => *v.last().unwrap(),
        }
    }

    pub fn max_width(&self) -> f32 {
        match self {
            StrokeWidth::Fixed(w) => *w,
            StrokeWidth::Dynamic(v) => v.iter().copied().fold(0.0, f32::max),
        }
    }

    pub fn push(&mut self, width: f32) {
        match self {
            StrokeWidth::Fixed(w) => {
                if (*w - width).abs() >= 0.01 {
                    *self = StrokeWidth::Dynamic(vec![*w, width]);
                }
            }
            StrokeWidth::Dynamic(v) => v.push(width),
        }
    }

    pub fn len(&self) -> Option<usize> {
        match self {
            StrokeWidth::Fixed(_) => None,
            StrokeWidth::Dynamic(v) => Some(v.len()),
        }
    }
}

impl From<f32> for StrokeWidth {
    fn from(width: f32) -> Self {
        StrokeWidth::Fixed(width)
    }
}

impl From<Vec<f32>> for StrokeWidth {
    fn from(widths: Vec<f32>) -> Self {
        if widths.is_empty() {
            return StrokeWidth::Fixed(0.0);
        }
        let first = widths[0];
        if widths.iter().all(|w| (w - first).abs() < 0.01) {
            StrokeWidth::Fixed(first)
        } else {
            StrokeWidth::Dynamic(widths)
        }
    }
}

/// Transform handle types for object manipulation (resize and rotate)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransformHandle {
    // 8 resize handles around the bounding box
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    // Rotation handle
    Rotate,
}

/// Available tools for canvas interaction
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasTool {
    Select, // Select and manipulate objects
    #[default]
    Brush, // Draw freehand strokes
    ObjectEraser, // Delete entire objects
    PixelEraser, // Erase pixel by pixel
    Insert, // Insert images, text, or shapes
    Settings, // Open settings panel
}

/// Trait for objects that can be rendered on the canvas
pub trait CanvasObjectOps {
    /// Renders the object using the provided painter
    fn paint(&self, painter: &egui::Painter, selected: bool);
    /// Returns the axis-aligned bounding rectangle of the object
    fn bounding_box(&self) -> egui::Rect;
    /// Transforms the object using the specified handle and drag parameters
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        drag_start: Pos2,
        current_pos: Pos2,
    );
}

/// Image object that can be placed on the canvas
#[derive(Clone)]
pub struct CanvasImage {
    pub texture: egui::TextureHandle,
    pub pos: Pos2,
    pub size: egui::Vec2,
    pub aspect_ratio: f32,
    pub rot: f32,
    pub marked_for_deletion: bool, // Deferred deletion to avoid borrow checker issues
    pub image_data: Arc<[u8]>,     // RGBA pixel data for export
    pub image_size: [u32; 2],      // [width, height] of the original image
}

impl CanvasObjectOps for CanvasImage {
    /// Transforms the image based on the dragged handle
    fn transform(
        &mut self,
        handle: TransformHandle,
        _delta: egui::Vec2,
        _drag_start: Pos2,
        current_pos: Pos2,
    ) {
        let bbox = self.bounding_box();

        match handle {
            TransformHandle::TopLeft => {
                let new_min = current_pos;
                let new_max = bbox.max;
                let new_size = egui::vec2(
                    (new_max.x - new_min.x).max(10.0),
                    (new_max.y - new_min.y).max(10.0),
                );
                self.size = new_size;
                self.pos = new_min;
            }
            TransformHandle::Top => {
                let new_height = (bbox.max.y - current_pos.y).max(10.0);
                self.size.y = new_height;
                self.pos.y = current_pos.y;
            }
            TransformHandle::TopRight => {
                let new_max = Pos2::new(current_pos.x, bbox.max.y);
                let new_min = Pos2::new(bbox.min.x, current_pos.y);
                let new_size = egui::vec2(
                    (new_max.x - new_min.x).max(10.0),
                    (new_max.y - new_min.y).max(10.0),
                );
                self.size = new_size;
                self.pos.y = new_min.y;
            }
            TransformHandle::Left => {
                let new_width = (bbox.max.x - current_pos.x).max(10.0);
                self.size.x = new_width;
                self.pos.x = current_pos.x;
            }
            TransformHandle::Right => {
                let new_width = (current_pos.x - bbox.min.x).max(10.0);
                self.size.x = new_width;
            }
            TransformHandle::BottomLeft => {
                let new_min = Pos2::new(current_pos.x, bbox.min.y);
                let new_max = Pos2::new(bbox.max.x, current_pos.y);
                let new_size = egui::vec2(
                    (new_max.x - new_min.x).max(10.0),
                    (new_max.y - new_min.y).max(10.0),
                );
                self.size = new_size;
                self.pos.x = new_min.x;
            }
            TransformHandle::Bottom => {
                let new_height = (current_pos.y - bbox.min.y).max(10.0);
                self.size.y = new_height;
            }
            TransformHandle::BottomRight => {
                let new_size = egui::vec2(
                    (current_pos.x - bbox.min.x).max(10.0),
                    (current_pos.y - bbox.min.y).max(10.0),
                );
                self.size = new_size;
            }
            TransformHandle::Rotate => {
                // For now, ignore rotation for images
            }
        }
    }

    /// Returns the bounding rectangle of the image
    fn bounding_box(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, self.size)
    }

    /// Renders the image on the canvas, drawing selection UI if selected
    fn paint(&self, painter: &egui::Painter, selected: bool) {
        let img_rect = self.bounding_box();
        painter.image(
            self.texture.id(),
            img_rect,
            egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // Draw selection border and resize handles when selected
        if selected {
            painter.rect_stroke(
                img_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
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
            .field("image_size", &self.image_size)
            .finish()
    }
}

/// Text object that can be placed on the canvas
#[derive(Debug, Clone)]
pub struct CanvasText {
    pub text: String,
    pub pos: Pos2,
    pub color: Color32,
    pub font_size: f32,
    pub rot: f32,
}

impl CanvasObjectOps for CanvasText {
    /// Transforms the text object, scaling font size for resize handles
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        _drag_start: Pos2,
        _current_pos: Pos2,
    ) {
        match handle {
            TransformHandle::TopLeft
            | TransformHandle::Top
            | TransformHandle::TopRight
            | TransformHandle::Left
            | TransformHandle::Right
            | TransformHandle::BottomLeft
            | TransformHandle::Bottom
            | TransformHandle::BottomRight => {
                // Scale font size based on drag delta
                let scale_factor = 1.0 + (delta.x + delta.y) / 200.0;
                self.font_size = (self.font_size * scale_factor).max(6.0);
            }
            TransformHandle::Rotate => {
                // Rotation not yet implemented for text
            }
        }
    }

    /// Returns an approximate bounding rectangle for the text
    fn bounding_box(&self) -> egui::Rect {
        // Approximate text dimensions
        let approx_char_width = self.font_size * 0.6;
        let approx_width = self.text.len() as f32 * approx_char_width;
        let approx_height = self.font_size * 1.2;
        egui::Rect::from_min_size(self.pos, egui::vec2(approx_width, approx_height))
    }

    /// Renders the text on the canvas with optional selection UI
    fn paint(&self, painter: &egui::Painter, selected: bool) {
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
            angle: self.rot,
            fallback_color: self.color,
            opacity_factor: 1.0,
        };
        painter.add(text_shape);

        if selected {
            let text_rect = self.bounding_box();
            painter.rect_stroke(
                text_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, text_rect);
        }
    }
}

/// Available shape types for the canvas
#[derive(Clone, Copy, Debug)]
pub enum CanvasShapeType {
    Line,
    Arrow,
    Rectangle,
    Triangle,
    Circle,
}

/// Shape object that can be placed on the canvas
#[derive(Debug, Clone)]
pub struct CanvasShape {
    pub shape_type: CanvasShapeType,
    pub pos: Pos2,
    pub size: f32,
    pub color: Color32,
    pub rotation: f32,
}

impl CanvasObjectOps for CanvasShape {
    /// Transforms the shape, scaling uniformly for resize handles
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        _drag_start: Pos2,
        _current_pos: Pos2,
    ) {
        match handle {
            TransformHandle::TopLeft
            | TransformHandle::Top
            | TransformHandle::TopRight
            | TransformHandle::Left
            | TransformHandle::Right
            | TransformHandle::BottomLeft
            | TransformHandle::Bottom
            | TransformHandle::BottomRight => {
                // Scale the shape size uniformly
                let scale_factor = 1.0 + (delta.x + delta.y) / 200.0;
                self.size = (self.size * scale_factor).max(10.0);
            }
            TransformHandle::Rotate => {
                // Rotation not yet implemented for shapes
            }
        }
    }

    /// Returns the bounding rectangle of the shape with padding for handles
    fn bounding_box(&self) -> egui::Rect {
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

    /// Renders the shape and optional selection UI
    fn paint(&self, painter: &egui::Painter, selected: bool) {
        // Draw the shape itself
        match self.shape_type {
            CanvasShapeType::Line => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                painter.line_segment([self.pos, end_point], Stroke::new(2.0_f32, self.color));
            }
            CanvasShapeType::Arrow => {
                let end_point = Pos2::new(self.pos.x + self.size, self.pos.y);
                painter.line_segment([self.pos, end_point], Stroke::new(2.0_f32, self.color));

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

                painter.line_segment([end_point, arrow_point1], Stroke::new(2.0_f32, self.color));
                painter.line_segment([end_point, arrow_point2], Stroke::new(2.0_f32, self.color));
            }
            CanvasShapeType::Rectangle => {
                let rect = egui::Rect::from_min_size(self.pos, egui::vec2(self.size, self.size));
                painter.rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0_f32, self.color),
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
                    Stroke::new(2.0_f32, self.color),
                ));
            }
            CanvasShapeType::Circle => {
                painter.circle_stroke(self.pos, self.size / 2.0, Stroke::new(2.0_f32, self.color));
            }
        }

        // Draw selection border and resize handles when selected
        if selected {
            let shape_rect = self.bounding_box();
            painter.rect_stroke(
                shape_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, shape_rect);
        }
    }
}

/// Enum representing all possible canvas object types
#[derive(Debug, Clone)]
pub enum CanvasObject {
    Stroke(CanvasStroke),
    Image(CanvasImage),
    Text(CanvasText),
    Shape(CanvasShape),
}

impl CanvasObject {
    /// Moves an object by the specified delta vector
    pub fn move_object(object: &mut CanvasObject, delta: egui::Vec2) {
        match object {
            CanvasObject::Image(img) => {
                img.pos += delta;
            }
            CanvasObject::Text(text) => {
                text.pos += delta;
            }
            CanvasObject::Shape(shape) => {
                shape.pos += delta;
            }
            CanvasObject::Stroke(stroke) => {
                // For strokes, move all points
                for point in &mut stroke.points {
                    *point += delta;
                }
            }
        }
    }

    /// Extracts transform information (position, size, rotation) from an object
    pub fn get_transform(&self) -> ObjectTransform {
        match self {
            CanvasObject::Image(img) => ObjectTransform {
                pos: img.pos,
                size: img.size,
                rotation: img.rot,
            },
            CanvasObject::Text(text) => ObjectTransform {
                pos: text.pos,
                size: egui::vec2(text.font_size, text.font_size), // Using font_size for both dimensions
                rotation: text.rot,
            },
            CanvasObject::Shape(shape) => ObjectTransform {
                pos: shape.pos,
                size: egui::vec2(shape.size, shape.size), // Using shape.size for both dimensions
                rotation: shape.rotation,
            },
            CanvasObject::Stroke(_stroke) => ObjectTransform {
                pos: egui::Pos2::new(0.0, 0.0), // Strokes don't have a single position
                size: egui::Vec2::new(0.0, 0.0), // Strokes don't have a single size
                rotation: 0.0,                  // Strokes handle rotation differently
            },
        }
    }
}

impl CanvasObjectOps for CanvasObject {
    /// Delegates transform to the inner object type
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        drag_start: Pos2,
        current_pos: Pos2,
    ) {
        match self {
            CanvasObject::Image(img) => img.transform(handle, delta, drag_start, current_pos),
            CanvasObject::Text(text) => text.transform(handle, delta, drag_start, current_pos),
            CanvasObject::Shape(shape) => shape.transform(handle, delta, drag_start, current_pos),
            CanvasObject::Stroke(stroke) => {
                stroke.transform(handle, delta, drag_start, current_pos)
            }
        }
    }

    /// Delegates painting to the inner object type
    fn paint(&self, painter: &egui::Painter, selected: bool) {
        match self {
            CanvasObject::Stroke(stroke) => stroke.paint(painter, selected),
            CanvasObject::Image(image) => image.paint(painter, selected),
            CanvasObject::Text(text) => text.paint(painter, selected),
            CanvasObject::Shape(shape) => shape.paint(painter, selected),
        }
    }

    /// Delegates bounding box calculation to the inner object type
    fn bounding_box(&self) -> egui::Rect {
        match self {
            CanvasObject::Stroke(stroke) => stroke.bounding_box(),
            CanvasObject::Image(image) => image.bounding_box(),
            CanvasObject::Text(text) => text.bounding_box(),
            CanvasObject::Shape(shape) => shape.bounding_box(),
        }
    }
}

/// Window display mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WindowMode {
    Windowed,
    Fullscreen,
    #[default]
    BorderlessFullscreen,
}

/// UI theme mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    System,
    Light,
    #[default]
    Dark,
}

/// GPU optimization policy for performance vs resource usage tradeoff
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OptimizationPolicy {
    #[default]
    Performance,
    ResourceUsage,
}

/// Graphics API backend selection
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GraphicsApi {
    #[default]
    Auto,
    Vulkan,
    Dx12,
    Metal,
    WebGpu,
    Gl,
}

impl GraphicsApi {
    pub fn to_backends(self) -> wgpu::Backends {
        match self {
            GraphicsApi::Auto => wgpu::Backends::all(),
            GraphicsApi::Vulkan => wgpu::Backends::VULKAN,
            GraphicsApi::Dx12 => wgpu::Backends::DX12,
            GraphicsApi::Metal => wgpu::Backends::METAL,
            GraphicsApi::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
            GraphicsApi::Gl => wgpu::Backends::GL,
        }
    }
}

/// Represents the current state of the canvas including all objects
#[derive(Debug, Clone, Default)]
pub struct CanvasState {
    pub objects: Vec<CanvasObject>,
}

/// State for a single page including canvas and undo/redo history
#[derive(Debug, Clone, Default)]
pub struct PageState {
    pub canvas: CanvasState,
    pub history: History,
}

impl CanvasState {
    /// Loads canvas state from a file using rkyv binary format
    pub fn load_from_file(path: &std::path::PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = std::fs::read(path)?;
        let archived = rkyv::access::<flat::ArchivedCanvasStateFlat, rkyv::rancor::Error>(&bytes)
            .map_err(|e| format!("rkyv error: {e}"))?;
        Ok(Self::from(archived))
    }

    /// Saves canvas state to a file using rkyv binary format
    pub fn save_to_file(
        &self,
        path: &std::path::PathBuf,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let flat = CanvasStateFlat::from(self);
        let bytes =
            rkyv::to_bytes::<rkyv::rancor::Error>(&flat).map_err(|e| format!("rkyv error: {e}"))?;
        std::fs::write(path, bytes.as_slice())?;
        Ok(())
    }

    /// Opens a file dialog to load canvas from user-selected file
    pub fn load_from_file_with_dialog() -> Result<Self, Box<dyn std::error::Error>> {
        let path = rfd::FileDialog::new()
            .add_filter("画布文件", &["sb"])
            .pick_file()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidFilename,
                "已取消",
            ))?;
        let canvas = CanvasState::load_from_file(&path)?;
        Ok(canvas)
    }

    /// Opens a file dialog to save canvas to user-selected file
    pub fn save_to_file_with_dialog(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = rfd::FileDialog::new()
            .add_filter("画布文件", &["sb"])
            .set_file_name("canvas.sb")
            .save_file()
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::InvalidFilename,
                "已取消",
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
    pub canvas_color: Color32,
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
    pub graphics_api: GraphicsApi,
    #[serde(default)]
    pub low_latency_mode: bool,
    #[serde(default)]
    pub force_redraw_every_frame: bool,

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
            canvas_color: utils::get_default_canvas_color(),
            window_opacity: 1.0,

            stroke_smoothing: true,
            stroke_straightening: true,
            stroke_straightening_tolerance: 20.0,
            interpolation_frequency: 0.1,
            quick_colors: utils::get_default_quick_colors(),

            show_fps: false,
            window_mode: WindowMode::default(),
            present_mode: PresentMode::AutoVsync,
            optimization_policy: OptimizationPolicy::default(),
            graphics_api: GraphicsApi::default(),
            low_latency_mode: false,
            force_redraw_every_frame: false,

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
        path.push("uwu");
        std::fs::create_dir_all(&path).ok();
        path.push("settings.json");
        path
    }

    // 加载设置从文件
    pub fn load_from_file() -> Self {
        let settings_path = Self::get_settings_path();
        if let Ok(content) = std::fs::read_to_string(settings_path)
            && let Ok(settings) = serde_json::from_str(&content)
        {
            return settings;
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
#[derive(Debug, Clone)]
pub struct CanvasStroke {
    pub points: Vec<Pos2>,
    pub width: StrokeWidth,
    pub color: Color32,
    pub base_width: f32,
    pub rot: f32,
}

impl CanvasStroke {
    fn scale_stroke_points(stroke: &mut CanvasStroke, center: Pos2, scale_x: f32, scale_y: f32) {
        for point in &mut stroke.points {
            let relative = *point - center;
            point.x = center.x + relative.x * scale_x;
            point.y = center.y + relative.y * scale_y;
        }
        // Scale widths proportionally
        let avg_scale = (scale_x + scale_y) / 2.0;
        match &mut stroke.width {
            StrokeWidth::Fixed(w) => *w *= avg_scale,
            StrokeWidth::Dynamic(v) => {
                for width in v.iter_mut() {
                    *width *= avg_scale;
                }
            }
        }
    }

    fn move_stroke_to_center(stroke: &mut CanvasStroke, new_center: Pos2) {
        let current_center = stroke.bounding_box().center();
        let offset = new_center - current_center;
        for point in &mut stroke.points {
            *point += offset;
        }
    }
}

impl CanvasObjectOps for CanvasStroke {
    fn transform(
        &mut self,
        handle: TransformHandle,
        delta: egui::Vec2,
        _drag_start: Pos2,
        _current_pos: Pos2,
    ) {
        let bbox = self.bounding_box();
        let center = bbox.center();

        // Calculate scale factors
        let scale_x = if bbox.width() > 0.0 {
            (bbox.width() + delta.x) / bbox.width()
        } else {
            1.0
        };
        let scale_y = if bbox.height() > 0.0 {
            (bbox.height() + delta.y) / bbox.height()
        } else {
            1.0
        };

        match handle {
            TransformHandle::TopLeft => {
                let scale = scale_x.min(scale_y);
                Self::scale_stroke_points(self, center, scale, scale);
                // Adjust position
                let new_center = center + delta / 2.0;
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::Top => {
                Self::scale_stroke_points(self, center, 1.0, scale_y);
                let new_center = Pos2::new(center.x, center.y + delta.y / 2.0);
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::TopRight => {
                let scale = scale_x.min(scale_y);
                Self::scale_stroke_points(self, center, scale, scale);
                let new_center = center + delta / 2.0;
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::Left => {
                Self::scale_stroke_points(self, center, scale_x, 1.0);
                let new_center = Pos2::new(center.x + delta.x / 2.0, center.y);
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::Right => {
                Self::scale_stroke_points(self, center, scale_x, 1.0);
                let new_center = Pos2::new(center.x + delta.x / 2.0, center.y);
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::BottomLeft => {
                let scale = scale_x.min(scale_y);
                Self::scale_stroke_points(self, center, scale, scale);
                let new_center = center + delta / 2.0;
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::Bottom => {
                Self::scale_stroke_points(self, center, 1.0, scale_y);
                let new_center = Pos2::new(center.x, center.y + delta.y / 2.0);
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::BottomRight => {
                let scale = scale_x.min(scale_y);
                Self::scale_stroke_points(self, center, scale, scale);
                let new_center = center + delta / 2.0;
                Self::move_stroke_to_center(self, new_center);
            }
            TransformHandle::Rotate => {
                // Calculate rotation angle based on drag
                let center = bbox.center();
                let current_angle = (_current_pos - center).angle();
                let start_angle = (_drag_start - center).angle();
                let delta_angle = current_angle - start_angle;
                self.rot += delta_angle;
            }
        }
    }

    fn bounding_box(&self) -> egui::Rect {
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
        let max_width = self.width.max_width();
        let padding = max_width / 2.0 + 5.0; // 添加额外的5像素边距

        egui::Rect::from_min_max(
            Pos2::new(min_x - padding, min_y - padding),
            Pos2::new(max_x + padding, max_y + padding),
        )
    }

    fn paint(&self, painter: &egui::Painter, selected: bool) {
        let color = if selected { Color32::BLUE } else { self.color };

        // Apply rotation if needed
        let rotated_points: Vec<Pos2> = if self.rot.abs() > 0.001 {
            let center = self.bounding_box().center();
            self.points
                .iter()
                .map(|p| {
                    let dx = p.x - center.x;
                    let dy = p.y - center.y;
                    let cos_rot = self.rot.cos();
                    let sin_rot = self.rot.sin();
                    Pos2::new(
                        center.x + dx * cos_rot - dy * sin_rot,
                        center.y + dx * sin_rot + dy * cos_rot,
                    )
                })
                .collect()
        } else {
            self.points.clone()
        };

        painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
            rotated_points[0],
            self.width.first() / 2.0,
            color,
        )));
        if rotated_points.len() >= 2 {
            painter.add(egui::Shape::Circle(egui::epaint::CircleShape::filled(
                rotated_points[rotated_points.len() - 1],
                self.width.last() / 2.0,
                color,
            )));
            match &self.width {
                StrokeWidth::Fixed(w) => {
                    if rotated_points.len() == 2 {
                        painter.line_segment(
                            [rotated_points[0], rotated_points[1]],
                            Stroke::new(*w, color),
                        );
                    } else {
                        let path =
                            egui::epaint::PathShape::line(rotated_points, Stroke::new(*w, color));
                        painter.add(egui::Shape::Path(path));
                    }
                }
                StrokeWidth::Dynamic(widths) => {
                    for i in 0..rotated_points.len() - 1 {
                        let avg_width = (widths[i] + widths[i + 1]) / 2.0;
                        painter.line_segment(
                            [rotated_points[i], rotated_points[i + 1]],
                            Stroke::new(avg_width, color),
                        );
                    }
                }
            }
        }

        if selected {
            let stroke_rect = self.bounding_box();
            painter.rect_stroke(
                stroke_rect,
                0.0,
                Stroke::new(2.0_f32, Color32::BLUE),
                egui::StrokeKind::Outside,
            );
            utils::draw_resize_handles(painter, stroke_rect);
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
    pub width: StrokeWidth,
    pub times: Vec<f64>,             // 每个点的时间戳（用于速度计算）
    pub start_time: Instant,         // 笔画开始时间
    pub last_movement_time: Instant, // 最后一次移动的时间（用于检测停留）
}

/// Unified per-pointer interaction state for all tools
pub enum PointerInteraction {
    Drawing {
        active_stroke: ActiveStroke,
    },
    Selecting {
        drag_start: Pos2,
        dragged_handle: Option<TransformHandle>,
        drag_original_transform: Option<ObjectTransform>,
        drag_accumulated_delta: egui::Vec2,
    },
    Erasing,
}

/// Represents a single pointer (touch or mouse) on the canvas
pub struct PointerState {
    pub id: u64,
    pub pos: Pos2,
    pub interaction: PointerInteraction,
}

#[cfg(feature = "startup_animation")]
pub struct StartupAnimation {
    fps: f32,
    start_time: Option<Instant>,

    // Video
    frames: &'static [&'static [u8]],
    texture: Option<TextureHandle>,
    last_frame_index: usize,

    // Audio
    _audio_sink: Option<Player>,

    finished: bool,
}

#[cfg(feature = "startup_animation")]
impl StartupAnimation {
    pub fn new(fps: f32, frames: &'static [&'static [u8]], audio: &'static [u8]) -> Self {
        Self {
            fps,
            start_time: None,
            frames,
            texture: None,
            last_frame_index: usize::MAX,
            _audio_sink: Some(Self::play_audio(audio)),
            finished: false,
        }
    }

    fn play_audio(audio: &'static [u8]) -> Player {
        let handle = DeviceSinkBuilder::open_default_sink().expect("failed to open stream");

        let player = Player::connect_new(handle.mixer());

        let cursor = Cursor::new(audio);
        let source = Decoder::new(cursor).unwrap();

        handle.mixer().add(source);

        // keep stream alive
        std::mem::forget(handle);

        player
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
    // 批量操作（用于清空画布等）
    ClearObjects {
        objects: Vec<CanvasObject>,
    },
    // 移动对象命令
    MoveObject {
        index: usize,
        old_position: egui::Vec2,
        new_position: egui::Vec2,
    },
    // 变换对象命令
    TransformObject {
        index: usize,
        old_transform: ObjectTransform,
        new_transform: ObjectTransform,
    },
}

// 对象变换信息
#[derive(Debug, Clone)]
pub struct ObjectTransform {
    pub pos: egui::Pos2,
    pub size: egui::Vec2,
    pub rotation: f32,
}

// 历史记录结构
#[derive(Debug, Clone)]
pub struct History {
    undo_stack: Vec<HistoryCommand>,
    redo_stack: Vec<HistoryCommand>,
    max_history_size: usize,
}

impl History {
    pub fn new(max_history_size: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_history_size,
        }
    }

    // 保存添加对象的命令
    pub fn save_add_object(&mut self, index: usize, object: CanvasObject) {
        let command = HistoryCommand::AddObject { index, object };
        self.push_command(command);
    }

    // 保存删除对象的命令
    pub fn save_remove_object(&mut self, index: usize, object: CanvasObject) {
        let command = HistoryCommand::RemoveObject { index, object };
        self.push_command(command);
    }

    // 保存清空对象的命令
    pub fn save_clear_objects(&mut self, objects: Vec<CanvasObject>) {
        let command = HistoryCommand::ClearObjects { objects };
        self.push_command(command);
    }

    // 保存移动对象的命令
    pub fn save_move_object(
        &mut self,
        index: usize,
        old_position: egui::Vec2,
        new_position: egui::Vec2,
    ) {
        let command = HistoryCommand::MoveObject {
            index,
            old_position,
            new_position,
        };
        self.push_command(command);
    }

    // 保存变换对象的命令
    pub fn save_transform_object(
        &mut self,
        index: usize,
        old_transform: ObjectTransform,
        new_transform: ObjectTransform,
    ) {
        let command = HistoryCommand::TransformObject {
            index,
            old_transform,
            new_transform,
        };
        self.push_command(command);
    }

    // 推送命令并维护历史记录大小
    fn push_command(&mut self, command: HistoryCommand) {
        self.undo_stack.push(command);
        self.redo_stack.clear();

        // 清理超出限制的历史记录
        if self.undo_stack.len() > self.max_history_size {
            self.undo_stack.remove(0);
        }
    }

    // 执行撤销操作
    pub fn undo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(command) = self.undo_stack.pop() {
            self.apply_reverse(&command, current_state);
            self.redo_stack.push(command);
            true
        } else {
            false
        }
    }

    // 执行重做操作
    pub fn redo(&mut self, current_state: &mut CanvasState) -> bool {
        if let Some(command) = self.redo_stack.pop() {
            self.apply_forward(&command, current_state);
            self.undo_stack.push(command);
            true
        } else {
            false
        }
    }

    fn apply_reverse(&self, command: &HistoryCommand, current_state: &mut CanvasState) {
        match command {
            HistoryCommand::AddObject { index, object: _ } => {
                if *index < current_state.objects.len() {
                    current_state.objects.remove(*index);
                }
            }
            HistoryCommand::RemoveObject { index, object } => {
                if *index <= current_state.objects.len() {
                    current_state.objects.insert(*index, object.clone());
                }
            }
            HistoryCommand::ClearObjects { objects } => {
                current_state.objects = objects.clone();
            }
            HistoryCommand::MoveObject {
                index,
                old_position,
                new_position: _,
            } => {
                if *index < current_state.objects.len() {
                    CanvasObject::move_object(&mut current_state.objects[*index], *old_position);
                }
            }
            HistoryCommand::TransformObject {
                index,
                old_transform,
                new_transform: _,
            } => {
                if *index < current_state.objects.len() {
                    History::apply_transform(&mut current_state.objects[*index], old_transform);
                }
            }
        }
    }

    fn apply_forward(&self, command: &HistoryCommand, current_state: &mut CanvasState) {
        match command {
            HistoryCommand::AddObject { index, object } => {
                if *index <= current_state.objects.len() {
                    current_state.objects.insert(*index, object.clone());
                }
            }
            HistoryCommand::RemoveObject { index, object: _ } => {
                if *index < current_state.objects.len() {
                    current_state.objects.remove(*index);
                }
            }
            HistoryCommand::ClearObjects { objects: _ } => {
                current_state.objects.clear();
            }
            HistoryCommand::MoveObject {
                index,
                old_position: _,
                new_position,
            } => {
                if *index < current_state.objects.len() {
                    CanvasObject::move_object(&mut current_state.objects[*index], *new_position);
                }
            }
            HistoryCommand::TransformObject {
                index,
                old_transform: _,
                new_transform,
            } => {
                if *index < current_state.objects.len() {
                    History::apply_transform(&mut current_state.objects[*index], new_transform);
                }
            }
        }
    }

    fn apply_transform(object: &mut CanvasObject, transform: &ObjectTransform) {
        match object {
            CanvasObject::Image(img) => {
                img.pos = transform.pos;
                img.size = transform.size;
            }
            CanvasObject::Text(text) => {
                text.pos = transform.pos;
                text.font_size = transform.size.x;
            }
            CanvasObject::Shape(shape) => {
                shape.pos = transform.pos;
                shape.size = transform.size.x;
                shape.rotation = transform.rotation;
            }
            CanvasObject::Stroke(_) => {}
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(50)
    }
}

// 应用程序状态
pub struct AppState {
    // canvas states
    pub canvas: CanvasState,                             // 当前页面的画布
    pub history: History,                                // 当前页面的历史记录
    pub pages: Vec<PageState>,                           // 分页
    pub current_page: usize,                             // 当前页码
    pub pointers: HashMap<u64, PointerState>, // 统一指针状态表（鼠标 id=0，触控使用 winit touch id）
    pub brush_color: Color32,                 // 画笔颜色
    pub brush_width: f32,                     // 画笔大小
    pub dynamic_brush_width_mode: DynamicBrushWidthMode, // 动态画笔大小微调
    pub current_tool: CanvasTool,             // 当前工具
    pub eraser_size: f32,                     // 橡皮擦大小
    pub selected_object_index: Option<usize>, // 选中的对象索引（全局共享）

    // persistent states
    pub persistent: PersistentState,

    // ui states
    pub show_quick_color_edit_window: bool, // 是否显示快捷颜色编辑器
    pub show_insert_text_window: bool,
    pub show_insert_shape_window: bool,
    pub show_welcome_window: bool,
    pub show_page_management_window: bool,

    pub show_size_preview: bool,
    pub new_text_content: String,
    pub should_quit: bool,
    pub fullscreen_video_modes: Vec<winit::monitor::VideoModeHandle>,
    pub selected_video_mode_index: Option<usize>, // 选中的视频模式索引
    pub fps_counter: FpsCounter,                  // FPS 计数器
    pub new_quick_color: Color32,                 // 新快捷颜色，用于添加
    pub show_touch_points: bool,                  // 是否显示触控点，用于调试

    // screenshot states
    pub screenshot_path: Option<PathBuf>,

    // cached states
    pub active_backend: Option<Backend>,

    // reactive states
    pub present_mode_changed: bool,

    #[cfg(feature = "startup_animation")]
    pub startup_animation: Option<StartupAnimation>, // 启动动画

    // utils
    pub toasts: Toasts,
    pub tray: Option<TrayIcon>,
}

impl Default for AppState {
    fn default() -> Self {
        let default_page = PageState::default();
        Self {
            canvas: default_page.canvas.clone(),
            pages: vec![default_page],
            current_page: 0,
            pointers: HashMap::new(),
            brush_color: Color32::WHITE,
            brush_width: 3.0,
            dynamic_brush_width_mode: DynamicBrushWidthMode::default(),
            current_tool: CanvasTool::Brush,
            eraser_size: 10.0,
            selected_object_index: None,
            show_size_preview: false,
            fps_counter: FpsCounter::new(),
            should_quit: false,
            show_insert_text_window: false,
            new_text_content: "".to_string(),
            show_insert_shape_window: false,
            fullscreen_video_modes: Vec::new(),
            selected_video_mode_index: None,
            show_quick_color_edit_window: false,
            new_quick_color: Color32::WHITE,
            show_touch_points: false,
            show_welcome_window: true,
            show_page_management_window: false,
            persistent: PersistentState::load_from_file(),
            screenshot_path: None,
            toasts: Toasts::default()
                .with_anchor(egui_notify::Anchor::BottomRight)
                .with_margin(egui::vec2(20.0, 20.0)),
            history: History::default(),
            tray: None,
            active_backend: None,
            present_mode_changed: false,
            #[cfg(feature = "startup_animation")]
            startup_animation: None,
        }
    }
}
