use crate::{Engine, Mesh, Result, Vertex2d};

/// Default drawing style for immediate 2D debug primitives.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugDrawStyle {
    pub color: [f32; 4],
    pub thickness: f32,
    pub point_size: f32,
    pub circle_segments: u16,
}

impl Default for DebugDrawStyle {
    fn default() -> Self {
        Self {
            color: [1.0, 1.0, 1.0, 1.0],
            thickness: 0.01,
            point_size: 0.03,
            circle_segments: 32,
        }
    }
}

/// Immediate-mode 2D debug drawing batch.
///
/// Positions are expressed in NDC space to match the existing `QuadBatch` and
/// text overlay helpers. The batch collects colored triangles that can be
/// uploaded as one indexed mesh.
#[derive(Default)]
pub struct DebugDraw2d {
    vertices: Vec<Vertex2d>,
    indices: Vec<u32>,
    style: DebugDrawStyle,
}

impl DebugDraw2d {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_style(style: DebugDrawStyle) -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            style,
        }
    }

    pub fn style(&self) -> DebugDrawStyle {
        self.style
    }

    pub fn set_style(&mut self, style: DebugDrawStyle) -> &mut Self {
        self.style = style;
        self
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    pub fn index_count(&self) -> usize {
        self.indices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    pub fn line(&mut self, from: [f32; 2], to: [f32; 2]) -> &mut Self {
        self.line_with_style(from, to, self.style)
    }

    pub fn line_with_style(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        style: DebugDrawStyle,
    ) -> &mut Self {
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let length_sq = dx * dx + dy * dy;
        if length_sq <= f32::EPSILON {
            return self.point_with_style(from, style);
        }

        let inv_length = length_sq.sqrt().recip();
        let half_thickness = style.thickness.max(0.0) * 0.5;
        if half_thickness <= f32::EPSILON {
            return self;
        }

        let fringe = half_thickness.min(style.thickness.max(0.0) * 0.35);
        let core_half_thickness = (half_thickness - fringe).max(0.0);
        if core_half_thickness > f32::EPSILON {
            let nx = -dy * inv_length * core_half_thickness;
            let ny = dx * inv_length * core_half_thickness;
            self.push_quad(
                [from[0] + nx, from[1] + ny],
                [to[0] + nx, to[1] + ny],
                [to[0] - nx, to[1] - ny],
                [from[0] - nx, from[1] - ny],
                style.color,
            );
        }

        let core_nx = -dy * inv_length * core_half_thickness;
        let core_ny = dx * inv_length * core_half_thickness;
        self.push_quad(
            [
                from[0] - dy * inv_length * half_thickness,
                from[1] + dx * inv_length * half_thickness,
            ],
            [
                to[0] - dy * inv_length * half_thickness,
                to[1] + dx * inv_length * half_thickness,
            ],
            [to[0] + core_nx, to[1] + core_ny],
            [from[0] + core_nx, from[1] + core_ny],
            faded_color(style.color, 0.42),
        );
        self.push_quad(
            [from[0] - core_nx, from[1] - core_ny],
            [to[0] - core_nx, to[1] - core_ny],
            [
                to[0] + dy * inv_length * half_thickness,
                to[1] - dx * inv_length * half_thickness,
            ],
            [
                from[0] + dy * inv_length * half_thickness,
                from[1] - dx * inv_length * half_thickness,
            ],
            faded_color(style.color, 0.42),
        );
        self
    }

    pub fn polyline(&mut self, points: &[[f32; 2]]) -> &mut Self {
        self.polyline_with_style(points, self.style)
    }

    pub fn polyline_with_style(&mut self, points: &[[f32; 2]], style: DebugDrawStyle) -> &mut Self {
        for segment in points.windows(2) {
            self.line_with_style(segment[0], segment[1], style);
        }
        self
    }

    pub fn rectangle(&mut self, origin: [f32; 2], size: [f32; 2]) -> &mut Self {
        self.rectangle_with_style(origin, size, self.style)
    }

    pub fn rectangle_with_style(
        &mut self,
        origin: [f32; 2],
        size: [f32; 2],
        style: DebugDrawStyle,
    ) -> &mut Self {
        let [x, y] = origin;
        let [w, h] = size;
        let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h], [x, y]];
        self.polyline_with_style(&corners, style)
    }

    pub fn filled_rect(&mut self, origin: [f32; 2], size: [f32; 2]) -> &mut Self {
        self.filled_rect_with_color(origin, size, self.style.color)
    }

    pub fn filled_rect_with_color(
        &mut self,
        origin: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
    ) -> &mut Self {
        let [x, y] = origin;
        let [w, h] = size;
        self.push_quad([x, y], [x + w, y], [x + w, y + h], [x, y + h], color);
        self
    }

    pub fn point(&mut self, center: [f32; 2]) -> &mut Self {
        self.point_with_style(center, self.style)
    }

    pub fn point_with_style(&mut self, center: [f32; 2], style: DebugDrawStyle) -> &mut Self {
        let half = style.point_size.max(0.0) * 0.5;
        self.filled_rect_with_color(
            [center[0] - half, center[1] - half],
            [half * 2.0, half * 2.0],
            style.color,
        )
    }

    pub fn cross_marker(&mut self, center: [f32; 2]) -> &mut Self {
        self.cross_marker_with_style(center, self.style)
    }

    pub fn cross_marker_with_style(
        &mut self,
        center: [f32; 2],
        style: DebugDrawStyle,
    ) -> &mut Self {
        let half = style.point_size.max(0.0) * 0.5;
        self.line_with_style(
            [center[0] - half, center[1]],
            [center[0] + half, center[1]],
            style,
        );
        self.line_with_style(
            [center[0], center[1] - half],
            [center[0], center[1] + half],
            style,
        );
        self
    }

    pub fn circle(&mut self, center: [f32; 2], radius: f32) -> &mut Self {
        self.circle_with_style(center, radius, self.style)
    }

    pub fn circle_with_style(
        &mut self,
        center: [f32; 2],
        radius: f32,
        style: DebugDrawStyle,
    ) -> &mut Self {
        let segments = style.circle_segments.max(3) as usize;
        let mut points = Vec::with_capacity(segments + 1);
        for index in 0..=segments {
            let angle = (index as f32 / segments as f32) * std::f32::consts::TAU;
            points.push([
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]);
        }
        self.polyline_with_style(&points, style)
    }

    pub fn filled_circle(&mut self, center: [f32; 2], radius: f32) -> &mut Self {
        let segments = self.style.circle_segments.max(3) as usize;
        let mut points = Vec::with_capacity(segments);
        for index in 0..segments {
            let angle = (index as f32 / segments as f32) * std::f32::consts::TAU;
            points.push([
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]);
        }
        self.filled_polygon(&points, self.style.color)
    }

    /// Fill a convex polygon using a triangle fan.
    pub fn filled_polygon(&mut self, points: &[[f32; 2]], color: [f32; 4]) -> &mut Self {
        if points.len() < 3 {
            return self;
        }
        let base = self.vertices.len() as u32;
        self.vertices
            .extend(points.iter().map(|&position| Vertex2d {
                position,
                uv: [0.0, 0.0],
                color,
            }));
        for index in 1..points.len() - 1 {
            self.indices
                .extend_from_slice(&[base, base + index as u32, base + index as u32 + 1]);
        }
        self
    }

    pub fn build(&self, engine: &Engine) -> Result<Mesh> {
        Mesh::indexed_2d(engine, &self.vertices, &self.indices)
    }

    fn push_quad(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2], color: [f32; 4]) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&[
            Vertex2d {
                position: a,
                uv: [0.0, 0.0],
                color,
            },
            Vertex2d {
                position: b,
                uv: [0.0, 0.0],
                color,
            },
            Vertex2d {
                position: c,
                uv: [0.0, 0.0],
                color,
            },
            Vertex2d {
                position: d,
                uv: [0.0, 0.0],
                color,
            },
        ]);
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn faded_color(mut color: [f32; 4], alpha_scale: f32) -> [f32; 4] {
    color[3] *= alpha_scale;
    color
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_emits_core_and_fringe_quads() {
        let mut draw = DebugDraw2d::new();
        draw.line([-0.5, 0.0], [0.5, 0.0]);

        assert_eq!(draw.vertex_count(), 12);
        assert_eq!(draw.index_count(), 18);
    }

    #[test]
    fn polyline_emits_one_segment_per_pair() {
        let mut draw = DebugDraw2d::new();
        draw.polyline(&[[-0.5, -0.5], [0.0, 0.0], [0.5, -0.5]]);

        assert_eq!(draw.vertex_count(), 24);
        assert_eq!(draw.index_count(), 36);
    }

    #[test]
    fn rectangle_and_point_marker_accumulate_geometry() {
        let mut draw = DebugDraw2d::new();
        draw.rectangle([-0.5, -0.5], [1.0, 1.0]);
        draw.cross_marker([0.0, 0.0]);
        draw.point([0.0, 0.0]);

        assert_eq!(draw.vertex_count(), 76);
        assert_eq!(draw.index_count(), 114);
    }

    #[test]
    fn filled_polygon_uses_triangle_fan() {
        let mut draw = DebugDraw2d::new();
        draw.filled_polygon(
            &[[-0.5, -0.5], [0.5, -0.5], [0.0, 0.5]],
            [1.0, 0.0, 0.0, 1.0],
        );

        assert_eq!(draw.vertex_count(), 3);
        assert_eq!(draw.index_count(), 3);
    }

    #[test]
    fn circle_respects_minimum_segment_count() {
        let mut draw = DebugDraw2d::with_style(DebugDrawStyle {
            circle_segments: 2,
            ..DebugDrawStyle::default()
        });
        draw.circle([0.0, 0.0], 0.5);
        draw.filled_circle([0.0, 0.0], 0.25);

        assert_eq!(draw.vertex_count(), 39);
        assert_eq!(draw.index_count(), 57);
    }
}
