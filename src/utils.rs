use egui::Pos2;

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

    // 笔画平滑算法 - 使用移动平均和曲线拟合来减少抖动并添加圆角
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
}
