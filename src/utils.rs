use egui::{Color32, Painter, Pos2, Stroke};

pub struct AppUtils;

impl AppUtils {
    // 检查点是否与笔画相交（用于对象橡皮擦）
    pub fn point_intersects_stroke(
        pos: Pos2,
        stroke: &crate::state::DrawingStroke,
        eraser_size: f32,
    ) -> bool {
        let eraser_radius = eraser_size / 2.0;
        for i in 0..stroke.points.len() - 1 {
            let p1 = stroke.points[i];
            let p2 = stroke.points[i + 1];
            let stroke_width = if i < stroke.widths.len() {
                stroke.widths[i].max(
                    stroke
                        .widths
                        .get(i + 1)
                        .copied()
                        .unwrap_or(stroke.widths[i]),
                )
            } else {
                stroke.widths[0]
            };

            // 计算点到线段的距离
            let dist = Self::point_to_line_segment_distance(pos, p1, p2);
            if dist <= eraser_radius + stroke_width / 2.0 {
                return true;
            }
        }
        false
    }

    // 计算点到线段的最短距离
    pub fn point_to_line_segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
        let ab = Pos2::new(b.x - a.x, b.y - a.y);
        let ap = Pos2::new(p.x - a.x, p.y - a.y);
        let ab_sq = ab.x * ab.x + ab.y * ab.y;

        if ab_sq < 0.0001 {
            // a 和 b 几乎重合
            return (p.x - a.x).hypot(p.y - a.y);
        }

        let t = ((ap.x * ab.x + ap.y * ab.y) / ab_sq).max(0.0).min(1.0);
        let closest = Pos2::new(a.x + t * ab.x, a.y + t * ab.y);
        (p.x - closest.x).hypot(p.y - closest.y)
    }

    // 计算动态画笔宽度
    pub fn calculate_dynamic_width(
        base_width: f32,
        mode: crate::state::DynamicBrushMode,
        point_index: usize,
        total_points: usize,
        speed: Option<f32>,
    ) -> f32 {
        match mode {
            crate::state::DynamicBrushMode::Disabled => base_width,

            crate::state::DynamicBrushMode::BrushTip => {
                // 模拟笔锋：在笔画末尾逐渐缩小
                let progress = point_index as f32 / total_points.max(1) as f32;
                // 在最后 30% 的笔画中逐渐缩小到 40% 的宽度
                if progress > 0.7 {
                    let shrink_progress = (progress - 0.7) / 0.3; // 0.0 到 1.0
                    base_width * (1.0 - shrink_progress * 0.6) // 从 100% 缩小到 40%
                } else {
                    base_width
                }
            }

            crate::state::DynamicBrushMode::SpeedBased => {
                // 基于速度：速度快时变细，速度慢时变粗
                if let Some(speed_val) = speed {
                    // 速度范围假设：0-500 像素/秒
                    // 速度越快，宽度越小（最小到 50%）
                    // 速度越慢，宽度越大（最大到 150%）
                    let normalized_speed = (speed_val / 500.0).min(1.0);
                    base_width * (1.5 - normalized_speed) // 从 150% 到 50%
                } else {
                    base_width
                }
            }
        }
    }

    // 笔画平滑算法 - 使用移动平均和曲线拟合来减少抖动
    pub fn apply_stroke_smoothing(points: &[Pos2]) -> Vec<Pos2> {
        if points.len() < 2 {
            return points.to_vec();
        }

        // 应用移动平均滤波器减少抖动
        let mut smoothed_points = Vec::with_capacity(points.len());

        // 窗口大小（调整此值以控制平滑强度）
        let window_size = 3; // 使用3点移动平均

        for i in 0..points.len() {
            let start_idx = i.saturating_sub(window_size / 2);
            let end_idx = (i + window_size / 2).min(points.len() - 1);

            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut count = 0;

            for j in start_idx..=end_idx {
                sum_x += points[j].x;
                sum_y += points[j].y;
                count += 1;
            }

            let avg_x = sum_x / count as f32;
            let avg_y = sum_y / count as f32;
            smoothed_points.push(Pos2::new(avg_x, avg_y));
        }

        smoothed_points
    }

    // 计算形状的边界框（用于选择和碰撞检测）
    pub fn calculate_shape_bounding_box(shape: &crate::state::InsertedShape) -> egui::Rect {
        match shape.shape_type {
            crate::state::ShapeType::Line => {
                let end_point = Pos2::new(shape.pos.x + shape.size, shape.pos.y);
                let min_x = shape.pos.x.min(end_point.x) - 5.0;
                let max_x = shape.pos.x.max(end_point.x) + 5.0;
                let min_y = shape.pos.y.min(end_point.y) - 5.0;
                let max_y = shape.pos.y.max(end_point.y) + 5.0;
                egui::Rect::from_min_max(
                    Pos2::new(min_x, min_y),
                    Pos2::new(max_x, max_y),
                )
            }
            crate::state::ShapeType::Arrow => {
                let end_point = Pos2::new(shape.pos.x + shape.size, shape.pos.y);
                let min_x = shape.pos.x.min(end_point.x) - 5.0;
                let max_x = shape.pos.x.max(end_point.x) + 5.0;
                let min_y = shape.pos.y.min(end_point.y) - 15.0;
                let max_y = shape.pos.y.max(end_point.y) + 15.0;
                egui::Rect::from_min_max(
                    Pos2::new(min_x, min_y),
                    Pos2::new(max_x, max_y),
                )
            }
            crate::state::ShapeType::Rectangle => egui::Rect::from_min_size(
                shape.pos,
                egui::vec2(shape.size, shape.size),
            ),
            crate::state::ShapeType::Triangle => {
                let half_size = shape.size / 2.0;
                let min_x = shape.pos.x - 5.0;
                let max_x = shape.pos.x + shape.size + 5.0;
                let min_y = shape.pos.y - 5.0;
                let max_y = shape.pos.y + half_size + 5.0;
                egui::Rect::from_min_max(
                    Pos2::new(min_x, min_y),
                    Pos2::new(max_x, max_y),
                )
            }
            crate::state::ShapeType::Circle => {
                let radius = shape.size / 2.0;
                egui::Rect::from_min_max(
                    Pos2::new(
                        shape.pos.x - radius - 5.0,
                        shape.pos.y - radius - 5.0,
                    ),
                    Pos2::new(
                        shape.pos.x + radius + 5.0,
                        shape.pos.y + radius + 5.0,
                    ),
                )
            }
        }
    }

    pub fn draw_size_preview(painter: &Painter, pos: Pos2, size: f32) -> () {
        const SIZE_PREVIEW_BORDER_WIDTH: f32 = 2.0;
        let radius = size / SIZE_PREVIEW_BORDER_WIDTH;
        painter.circle_filled(pos, radius, Color32::WHITE);
        painter.circle_stroke(
            pos,
            radius,
            Stroke::new(SIZE_PREVIEW_BORDER_WIDTH, Color32::BLACK),
        );
    }
}
