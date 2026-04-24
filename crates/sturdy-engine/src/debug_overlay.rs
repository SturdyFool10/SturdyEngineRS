use crate::{
    DebugDraw2d, DebugDrawStyle, Engine, GraphImage, MeshProgram, MeshProgramDesc, MeshVertexKind,
    RenderFrame, Result, ShaderDesc, ShaderSource, ShaderStage, TextDrawDesc, TextOverlay,
    TextPlacement, TextTypography,
};

const SOLID_COLOR_FRAGMENT: &str = r#"
struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

float4 main(FSInput input) : SV_TARGET {
    return input.color;
}
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugOverlayAntialiasing {
    Default,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugOverlayTransform {
    pub translation: [f32; 2],
    pub scale: [f32; 2],
}

impl Default for DebugOverlayTransform {
    fn default() -> Self {
        Self {
            translation: [0.0, 0.0],
            scale: [1.0, 1.0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebugOverlayConfig {
    pub style: DebugDrawStyle,
    pub antialiasing: DebugOverlayAntialiasing,
    pub transform: DebugOverlayTransform,
}

impl Default for DebugOverlayConfig {
    fn default() -> Self {
        Self {
            style: DebugDrawStyle::default(),
            antialiasing: DebugOverlayAntialiasing::Default,
            transform: DebugOverlayTransform::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugHitRegion {
    pub tag: String,
    pub origin: [f32; 2],
    pub size: [f32; 2],
}

/// Immediate overlay content that can mix debug primitives and text in one frame.
pub struct DebugOverlay {
    shapes: DebugDraw2d,
    text: Vec<TextDrawDesc>,
    config: DebugOverlayConfig,
    hit_regions: Vec<DebugHitRegion>,
}

impl Default for DebugOverlay {
    fn default() -> Self {
        let config = DebugOverlayConfig::default();
        Self {
            shapes: DebugDraw2d::with_style(config.style),
            text: Vec::new(),
            config,
            hit_regions: Vec::new(),
        }
    }
}

impl DebugOverlay {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty() && self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.shapes.clear();
        self.text.clear();
        self.hit_regions.clear();
    }

    pub fn shapes(&self) -> &DebugDraw2d {
        &self.shapes
    }

    pub fn shapes_mut(&mut self) -> &mut DebugDraw2d {
        &mut self.shapes
    }

    pub fn set_style(&mut self, style: DebugDrawStyle) -> &mut Self {
        self.config.style = style;
        self.shapes.set_style(style);
        self
    }

    pub fn config(&self) -> DebugOverlayConfig {
        self.config
    }

    pub fn set_antialiasing(&mut self, antialiasing: DebugOverlayAntialiasing) -> &mut Self {
        self.config.antialiasing = antialiasing;
        self
    }

    pub fn set_transform(&mut self, transform: DebugOverlayTransform) -> &mut Self {
        self.config.transform = transform;
        self
    }

    pub fn register_hit_region(
        &mut self,
        tag: impl Into<String>,
        origin: [f32; 2],
        size: [f32; 2],
    ) -> &mut Self {
        self.hit_regions.push(DebugHitRegion {
            tag: tag.into(),
            origin,
            size,
        });
        self
    }

    pub fn hit_test_screen(&self, point: [f32; 2]) -> Option<&DebugHitRegion> {
        self.hit_regions.iter().rev().find(|region| {
            point[0] >= region.origin[0]
                && point[0] <= region.origin[0] + region.size[0]
                && point[1] >= region.origin[1]
                && point[1] <= region.origin[1] + region.size[1]
        })
    }

    pub fn add_text(&mut self, desc: TextDrawDesc) -> &mut Self {
        self.text.push(desc);
        self
    }

    pub fn add_screen_text(
        &mut self,
        text: impl Into<String>,
        x: f32,
        y: f32,
    ) -> &mut Self {
        let [x, y] = self.transform_screen_point([x, y]);
        self.text.push(
            TextDrawDesc::new(text.into())
                .placement(TextPlacement::Screen2d { x, y })
                .typography(
                    TextTypography::default()
                        .font_size(18.0)
                        .line_height(24.0)
                        .weight(600),
                )
                .color([0.92, 0.98, 1.0, 1.0])
                .max_width(460.0),
        );
        self
    }

    pub fn line_screen(&mut self, width: u32, height: u32, from: [f32; 2], to: [f32; 2]) -> &mut Self {
        let from = screen_to_ndc(width, height, self.transform_screen_point(from));
        let to = screen_to_ndc(width, height, self.transform_screen_point(to));
        self.shapes.line(from, to);
        self
    }

    pub fn rectangle_screen(
        &mut self,
        width: u32,
        height: u32,
        origin: [f32; 2],
        size: [f32; 2],
    ) -> &mut Self {
        let [origin, size] = self.transform_screen_rect(origin, size);
        let min = screen_to_ndc(width, height, origin);
        let max = screen_to_ndc(width, height, [origin[0] + size[0], origin[1] + size[1]]);
        self.shapes
            .rectangle(min, [max[0] - min[0], max[1] - min[1]]);
        self
    }

    pub fn rounded_rectangle_outline_screen(
        &mut self,
        width: u32,
        height: u32,
        origin: [f32; 2],
        size: [f32; 2],
        radius_pixels: f32,
        thickness_pixels: f32,
        color: [f32; 4],
    ) -> &mut Self {
        let [origin, size] = self.transform_screen_rect(origin, size);
        let radius = radius_pixels.min(size[0] * 0.5).min(size[1] * 0.5).max(0.0);
        let left = origin[0];
        let top = origin[1];

        if self.config.antialiasing == DebugOverlayAntialiasing::Default {
            let outer = [color[0], color[1], color[2], color[3] * 0.22];
            let inner = [color[0], color[1], color[2], color[3] * 0.16];
            self.draw_rounded_rectangle_outline_screen(
                width,
                height,
                [left, top],
                [size[0], size[1]],
                radius + 1.0,
                thickness_pixels + 2.0,
                outer,
            );
            self.draw_rounded_rectangle_outline_screen(
                width,
                height,
                [left, top],
                [size[0], size[1]],
                (radius - 0.5).max(0.0),
                (thickness_pixels - 1.0).max(1.0),
                inner,
            );
        }
        self.draw_rounded_rectangle_outline_screen(
            width,
            height,
            [left, top],
            [size[0], size[1]],
            radius,
            thickness_pixels,
            color,
        );
        self
    }

    pub fn filled_rect_screen(
        &mut self,
        width: u32,
        height: u32,
        origin: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
    ) -> &mut Self {
        let [origin, size] = self.transform_screen_rect(origin, size);
        let min = screen_to_ndc(width, height, origin);
        let max = screen_to_ndc(width, height, [origin[0] + size[0], origin[1] + size[1]]);
        self.shapes
            .filled_rect_with_color(min, [max[0] - min[0], max[1] - min[1]], color);
        self
    }

    pub fn circle_screen(
        &mut self,
        width: u32,
        height: u32,
        center: [f32; 2],
        radius_pixels: f32,
    ) -> &mut Self {
        let center = screen_to_ndc(width, height, self.transform_screen_point(center));
        let scale = (self.config.transform.scale[0].abs() + self.config.transform.scale[1].abs())
            * 0.5;
        let radius = radius_pixels * scale * 2.0 / width.max(1) as f32;
        self.shapes.circle(center, radius);
        self
    }

    pub fn cross_marker_screen(
        &mut self,
        width: u32,
        height: u32,
        center: [f32; 2],
        size_pixels: f32,
    ) -> &mut Self {
        let mut style = self.shapes.style();
        style.point_size =
            size_pixels * self.config.transform.scale[0].abs() * 2.0 / width.max(1) as f32;
        self.shapes
            .cross_marker_with_style(
                screen_to_ndc(width, height, self.transform_screen_point(center)),
                style,
            );
        self
    }

    fn text_descs(&self) -> &[TextDrawDesc] {
        &self.text
    }

    fn line_screen_with_style(
        &mut self,
        width: u32,
        height: u32,
        from: [f32; 2],
        to: [f32; 2],
        style: DebugDrawStyle,
    ) {
        let from = screen_to_ndc(width, height, from);
        let to = screen_to_ndc(width, height, to);
        self.shapes.line_with_style(from, to, style);
    }

    fn draw_rounded_rectangle_outline_screen(
        &mut self,
        width: u32,
        height: u32,
        origin: [f32; 2],
        size: [f32; 2],
        radius: f32,
        thickness_pixels: f32,
        color: [f32; 4],
    ) {
        let left = origin[0];
        let top = origin[1];
        let right = origin[0] + size[0];
        let bottom = origin[1] + size[1];
        let style = self.stroke_style_for_screen_pixels(width, thickness_pixels, color);

        self.line_screen_with_style(
            width,
            height,
            [left + radius, top],
            [right - radius, top],
            style,
        );
        self.line_screen_with_style(
            width,
            height,
            [right, top + radius],
            [right, bottom - radius],
            style,
        );
        self.line_screen_with_style(
            width,
            height,
            [right - radius, bottom],
            [left + radius, bottom],
            style,
        );
        self.line_screen_with_style(
            width,
            height,
            [left, bottom - radius],
            [left, top + radius],
            style,
        );

        self.arc_screen(width, height, [left + radius, top + radius], radius, 180.0, 270.0, style);
        self.arc_screen(width, height, [right - radius, top + radius], radius, 270.0, 360.0, style);
        self.arc_screen(width, height, [right - radius, bottom - radius], radius, 0.0, 90.0, style);
        self.arc_screen(width, height, [left + radius, bottom - radius], radius, 90.0, 180.0, style);
    }

    fn arc_screen(
        &mut self,
        width: u32,
        height: u32,
        center: [f32; 2],
        radius: f32,
        start_deg: f32,
        end_deg: f32,
        style: DebugDrawStyle,
    ) {
        let segments = 6usize;
        let mut points = Vec::with_capacity(segments + 1);
        for index in 0..=segments {
            let t = index as f32 / segments as f32;
            let angle = (start_deg + (end_deg - start_deg) * t).to_radians();
            points.push([
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]);
        }
        let points = points
            .into_iter()
            .map(|point| screen_to_ndc(width, height, point))
            .collect::<Vec<_>>();
        self.shapes.polyline_with_style(&points, style);
    }

    fn stroke_style_for_screen_pixels(
        &self,
        width: u32,
        thickness_pixels: f32,
        color: [f32; 4],
    ) -> DebugDrawStyle {
        let mut style = self.shapes.style();
        style.color = color;
        style.thickness =
            thickness_pixels * self.config.transform.scale[0].abs() * 2.0 / width.max(1) as f32;
        style
    }

    fn transform_screen_point(&self, point: [f32; 2]) -> [f32; 2] {
        [
            point[0] * self.config.transform.scale[0] + self.config.transform.translation[0],
            point[1] * self.config.transform.scale[1] + self.config.transform.translation[1],
        ]
    }

    fn transform_screen_rect(&self, origin: [f32; 2], size: [f32; 2]) -> [[f32; 2]; 2] {
        [
            self.transform_screen_point(origin),
            [
                size[0] * self.config.transform.scale[0],
                size[1] * self.config.transform.scale[1],
            ],
        ]
    }
}

/// Renderer for [`DebugOverlay`].
pub struct DebugOverlayRenderer {
    text_overlay: TextOverlay,
    shape_program: MeshProgram,
}

impl DebugOverlayRenderer {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            text_overlay: TextOverlay::new(engine)?,
            shape_program: MeshProgram::new(
                engine,
                MeshProgramDesc {
                    fragment: ShaderDesc {
                        source: ShaderSource::Inline(SOLID_COLOR_FRAGMENT.to_string()),
                        entry_point: "main".to_string(),
                        stage: ShaderStage::Fragment,
                    },
                    vertex: None,
                    vertex_kind: MeshVertexKind::V2d,
                    alpha_blend: true,
                },
            )?,
        })
    }

    pub fn draw(
        &mut self,
        frame: &RenderFrame,
        target: &GraphImage,
        width: u32,
        height: u32,
        overlay: &DebugOverlay,
    ) -> Result<()> {
        if !overlay.shapes.is_empty() {
            let mesh = overlay.shapes.build(&self.shape_program.engine)?;
            target.draw_mesh(&mesh, &self.shape_program)?;
        }
        if !overlay.text.is_empty() {
            self.text_overlay
                .draw(frame, target, width, height, overlay.text_descs())?;
        }
        Ok(())
    }
}

fn screen_to_ndc(width: u32, height: u32, point: [f32; 2]) -> [f32; 2] {
    [
        point[0] / width.max(1) as f32 * 2.0 - 1.0,
        1.0 - point[1] / height.max(1) as f32 * 2.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_space_primitives_emit_geometry() {
        let mut overlay = DebugOverlay::new();
        overlay.line_screen(800, 600, [0.0, 0.0], [800.0, 600.0]);
        overlay.rectangle_screen(800, 600, [100.0, 100.0], [200.0, 50.0]);
        overlay.circle_screen(800, 600, [400.0, 300.0], 20.0);
        overlay.cross_marker_screen(800, 600, [400.0, 300.0], 12.0);

        assert!(!overlay.shapes().is_empty());
        assert_eq!(overlay.text_descs().len(), 0);
    }

    #[test]
    fn text_and_shapes_can_coexist() {
        let mut overlay = DebugOverlay::new();
        overlay.add_screen_text("hello", 18.0, 18.0);
        overlay.filled_rect_screen(1280, 720, [8.0, 8.0], [220.0, 64.0], [0.0, 0.0, 0.0, 0.35]);

        assert!(!overlay.is_empty());
        assert_eq!(overlay.text_descs().len(), 1);
        assert!(!overlay.shapes().is_empty());
    }

    #[test]
    fn transform_and_hit_regions_are_applied_in_screen_space() {
        let mut overlay = DebugOverlay::new();
        overlay
            .set_antialiasing(DebugOverlayAntialiasing::Disabled)
            .set_transform(DebugOverlayTransform {
                translation: [10.0, 20.0],
                scale: [2.0, 2.0],
            })
            .register_hit_region("panel", [10.0, 20.0], [100.0, 40.0]);
        overlay.rectangle_screen(800, 600, [0.0, 0.0], [50.0, 20.0]);

        assert_eq!(
            overlay.hit_test_screen([32.0, 38.0]).map(|region| region.tag.as_str()),
            Some("panel")
        );
        assert_eq!(overlay.config().antialiasing, DebugOverlayAntialiasing::Disabled);
        assert!(!overlay.shapes().is_empty());
    }

    #[test]
    fn rounded_rectangle_outline_emits_geometry() {
        let mut overlay = DebugOverlay::new();
        overlay.rounded_rectangle_outline_screen(
            1280,
            720,
            [16.0, 16.0],
            [200.0, 80.0],
            10.0,
            3.0,
            [1.0, 1.0, 1.0, 1.0],
        );

        assert!(!overlay.shapes().is_empty());
    }
}
