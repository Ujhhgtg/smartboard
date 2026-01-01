use egui::{Color32, Painter, Pos2, Stroke};

use crate::state::ResizeAnchor;

pub struct AppUtils;

impl AppUtils {
    // 检查点是否与笔画相交（用于对象橡皮擦）
    pub fn point_intersects_stroke(
        pos: Pos2,
        stroke: &crate::state::CanvasStroke,
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
        mode: crate::state::DynamicBrushWidthMode,
        point_index: usize,
        total_points: usize,
        speed: Option<f32>,
    ) -> f32 {
        match mode {
            crate::state::DynamicBrushWidthMode::Disabled => base_width,

            crate::state::DynamicBrushWidthMode::BrushTip => {
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

            crate::state::DynamicBrushWidthMode::SpeedBased => {
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

    // 插值算法 - 在点之间插入中间点
    pub fn apply_point_interpolation(
        points: &[Pos2],
        widths: &[f32],
        frequency: f32,
    ) -> (Vec<Pos2>, Vec<f32>) {
        if points.len() < 2 || frequency <= 0.0 {
            return (points.to_vec(), widths.to_vec());
        }

        let mut interpolated_points = Vec::new();
        let mut interpolated_widths = Vec::new();

        for i in 0..points.len() - 1 {
            let p1 = points[i];
            let p2 = points[i + 1];
            let width1 = if i < widths.len() {
                widths[i]
            } else {
                widths[0]
            };
            let width2 = if i + 1 < widths.len() {
                widths[i + 1]
            } else {
                widths[widths.len() - 1]
            };

            // 添加第一个点
            interpolated_points.push(p1);
            interpolated_widths.push(width1);

            // 计算插值点数量
            let distance = p1.distance(p2);
            let num_interpolations = (distance * frequency) as usize;

            // 在两点之间插入中间点
            for j in 1..=num_interpolations {
                let t = j as f32 / (num_interpolations + 1) as f32;
                let interpolated_point =
                    Pos2::new(p1.x + t * (p2.x - p1.x), p1.y + t * (p2.y - p1.y));
                let interpolated_width = width1 + t * (width2 - width1);

                interpolated_points.push(interpolated_point);
                interpolated_widths.push(interpolated_width);
            }
        }

        // 添加最后一个点
        if let Some(last_point) = points.last() {
            interpolated_points.push(*last_point);
        }
        if let Some(last_width) = widths.last() {
            interpolated_widths.push(*last_width);
        }

        (interpolated_points, interpolated_widths)
    }

    // 笔画平滑算法 - 使用移动平均和曲线拟合来减少抖动
    // pub fn apply_stroke_smoothing(points: &[Pos2]) -> Vec<Pos2> {
    //     if points.len() < 2 {
    //         return points.to_vec();
    //     }

    //     // 应用移动平均滤波器减少抖动
    //     let mut smoothed_points = Vec::with_capacity(points.len());

    //     // 窗口大小（调整此值以控制平滑强度）
    //     let window_size = 3; // 使用3点移动平均

    //     for i in 0..points.len() {
    //         let start_idx = i.saturating_sub(window_size / 2);
    //         let end_idx = (i + window_size / 2).min(points.len() - 1);

    //         let mut sum_x = 0.0;
    //         let mut sum_y = 0.0;
    //         let mut count = 0;

    //         for j in start_idx..=end_idx {
    //             sum_x += points[j].x;
    //             sum_y += points[j].y;
    //             count += 1;
    //         }

    //         let avg_x = sum_x / count as f32;
    //         let avg_y = sum_y / count as f32;
    //         smoothed_points.push(Pos2::new(avg_x, avg_y));
    //     }

    //     smoothed_points
    // }

    pub fn apply_stroke_smoothing(points: &[Pos2]) -> Vec<Pos2> {
        if points.len() < 3 {
            return points.to_vec();
        }

        // -----------------------------
        // 1. Distance-based resampling
        // -----------------------------
        let target_spacing = 2.0; // pixels; tune for device DPI
        let mut resampled = Vec::new();

        resampled.push(points[0]);
        let mut acc_dist = 0.0;

        for i in 1..points.len() {
            let prev = points[i - 1];
            let curr = points[i];
            let dx = curr.x - prev.x;
            let dy = curr.y - prev.y;
            let dist = (dx * dx + dy * dy).sqrt();

            acc_dist += dist;

            if acc_dist >= target_spacing {
                resampled.push(curr);
                acc_dist = 0.0;
            }
        }

        if resampled.len() < 3 {
            return resampled;
        }

        // --------------------------------
        // 2. Chaikin corner cutting
        // --------------------------------
        let mut smoothed = resampled;

        let iterations = 2; // 2–3 recommended for real-time strokes

        for _ in 0..iterations {
            let mut next = Vec::with_capacity(smoothed.len() * 2);
            next.push(smoothed[0]);

            for i in 0..smoothed.len() - 1 {
                let p0 = smoothed[i];
                let p1 = smoothed[i + 1];

                let q = Pos2 {
                    x: 0.75 * p0.x + 0.25 * p1.x,
                    y: 0.75 * p0.y + 0.25 * p1.y,
                };
                let r = Pos2 {
                    x: 0.25 * p0.x + 0.75 * p1.x,
                    y: 0.25 * p0.y + 0.75 * p1.y,
                };

                next.push(q);
                next.push(r);
            }

            next.push(*smoothed.last().unwrap());
            smoothed = next;
        }

        // --------------------------------
        // 3. Light moving-average cleanup
        // --------------------------------
        let mut final_points = smoothed.clone();

        for i in 1..smoothed.len() - 1 {
            final_points[i] = Pos2 {
                x: (smoothed[i - 1].x + smoothed[i].x + smoothed[i + 1].x) / 3.0,
                y: (smoothed[i - 1].y + smoothed[i].y + smoothed[i + 1].y) / 3.0,
            };
        }

        final_points
    }

    // 计算形状的边界框（用于选择和碰撞检测）
    pub fn calculate_shape_bounding_box(shape: &crate::state::CanvasShape) -> egui::Rect {
        match shape.shape_type {
            crate::state::CanvasShapeType::Line => {
                let end_point = Pos2::new(shape.pos.x + shape.size, shape.pos.y);
                let min_x = shape.pos.x.min(end_point.x) - 5.0;
                let max_x = shape.pos.x.max(end_point.x) + 5.0;
                let min_y = shape.pos.y.min(end_point.y) - 5.0;
                let max_y = shape.pos.y.max(end_point.y) + 5.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            crate::state::CanvasShapeType::Arrow => {
                let end_point = Pos2::new(shape.pos.x + shape.size, shape.pos.y);
                let min_x = shape.pos.x.min(end_point.x) - 5.0;
                let max_x = shape.pos.x.max(end_point.x) + 5.0;
                let min_y = shape.pos.y.min(end_point.y) - 15.0;
                let max_y = shape.pos.y.max(end_point.y) + 15.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            crate::state::CanvasShapeType::Rectangle => {
                egui::Rect::from_min_size(shape.pos, egui::vec2(shape.size, shape.size))
            }
            crate::state::CanvasShapeType::Triangle => {
                let half_size = shape.size / 2.0;
                let min_x = shape.pos.x - 5.0;
                let max_x = shape.pos.x + shape.size + 5.0;
                let min_y = shape.pos.y - 5.0;
                let max_y = shape.pos.y + half_size + 5.0;
                egui::Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
            }
            crate::state::CanvasShapeType::Circle => {
                let radius = shape.size / 2.0;
                egui::Rect::from_min_max(
                    Pos2::new(shape.pos.x - radius - 5.0, shape.pos.y - radius - 5.0),
                    Pos2::new(shape.pos.x + radius + 5.0, shape.pos.y + radius + 5.0),
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

    pub fn draw_resize_and_rotation_anchors(
        painter: &egui::Painter,
        object_rect: egui::Rect,
        resize_anchor_hovered: Option<ResizeAnchor>,
        rotation_anchor_hovered: bool,
    ) {
        const ANCHOR_SIZE: f32 = 10.0;
        const ROTATION_ANCHOR_DISTANCE: f32 = 30.0;

        // 绘制调整大小锚点
        let anchors = [
            (ResizeAnchor::TopLeft, object_rect.left_top()),
            (ResizeAnchor::TopRight, object_rect.right_top()),
            (ResizeAnchor::BottomLeft, object_rect.left_bottom()),
            (ResizeAnchor::BottomRight, object_rect.right_bottom()),
            (
                ResizeAnchor::Top,
                Pos2::new(object_rect.center().x, object_rect.min.y),
            ),
            (
                ResizeAnchor::Bottom,
                Pos2::new(object_rect.center().x, object_rect.max.y),
            ),
            (
                ResizeAnchor::Left,
                Pos2::new(object_rect.min.x, object_rect.center().y),
            ),
            (
                ResizeAnchor::Right,
                Pos2::new(object_rect.max.x, object_rect.center().y),
            ),
        ];

        for (anchor_type, pos) in anchors {
            // 绘制锚点
            let anchor_color = if let Some(hovered_anchor) = resize_anchor_hovered {
                if hovered_anchor == anchor_type {
                    Color32::YELLOW
                } else {
                    Color32::WHITE
                }
            } else {
                Color32::WHITE
            };

            painter.circle_filled(pos, ANCHOR_SIZE, anchor_color);
            painter.circle_stroke(pos, ANCHOR_SIZE, Stroke::new(2.0, Color32::BLACK));
        }

        // 绘制旋转锚点（在顶部中间锚点上方）
        let rotation_anchor_pos = Pos2::new(
            object_rect.center().x,
            object_rect.min.y - ROTATION_ANCHOR_DISTANCE,
        );

        let rotation_color = if rotation_anchor_hovered {
            Color32::YELLOW
        } else {
            Color32::WHITE
        };

        painter.circle_filled(rotation_anchor_pos, ANCHOR_SIZE, rotation_color);
        painter.circle_stroke(
            rotation_anchor_pos,
            ANCHOR_SIZE,
            Stroke::new(2.0, Color32::BLACK),
        );

        // 绘制连接线
        painter.line_segment(
            [object_rect.center_top(), rotation_anchor_pos],
            Stroke::new(2.0, Color32::WHITE),
        );
    }
}
