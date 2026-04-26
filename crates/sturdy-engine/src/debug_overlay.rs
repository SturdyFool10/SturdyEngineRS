use crate::{
    DebugDraw2d, DebugDrawStyle, Engine, GraphImage, Mesh, MeshProgram, MeshProgramDesc,
    MeshVertexKind, QuadBatch, RenderFrame, Result, ShaderDesc, ShaderSource, ShaderStage,
    StageMask, TextDrawDesc, TextOverlay, TextPlacement, TextTypography,
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

const UI_SHAPE_FRAGMENT: &str = r#"
struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

struct UiShapeConstants {
    float4 sizeRadiusBorder;
    float4 fillColor;
    float4 borderColor;
};

float roundedBoxSdf(float2 p, float2 halfSize, float radius) {
    float2 q = abs(p) - max(halfSize - radius, float2(0.0, 0.0));
    return length(max(q, float2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

float4 main(FSInput input, uniform UiShapeConstants push) : SV_TARGET {
    float2 size = max(push.sizeRadiusBorder.xy, float2(1.0, 1.0));
    float radius = clamp(push.sizeRadiusBorder.z, 0.0, min(size.x, size.y) * 0.5);
    float border = max(push.sizeRadiusBorder.w, 0.0);
    float2 p = input.uv - size * 0.5;
    float sd = roundedBoxSdf(p, size * 0.5, radius);
    float aa = max(fwidth(sd), 0.75);
    float outer = 1.0 - smoothstep(-aa, aa, sd);

    if (border > 0.0 && push.borderColor.a > 0.0) {
        float fillCoverage = outer * (1.0 - smoothstep(-border - aa, -border + aa, sd));
        float borderCoverage = max(outer - fillCoverage, 0.0);
        float fillAlpha = push.fillColor.a * fillCoverage;
        float borderAlpha = push.borderColor.a * borderCoverage;
        float alpha = fillAlpha + borderAlpha;
        float3 rgb = alpha > 0.0
            ? (push.fillColor.rgb * fillAlpha + push.borderColor.rgb * borderAlpha) / alpha
            : float3(0.0, 0.0, 0.0);
        return float4(rgb, alpha);
    }

    return float4(push.fillColor.rgb, push.fillColor.a * outer);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct UiShapeConstants {
    size_radius_border: [f32; 4],
    fill_color: [f32; 4],
    border_color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct UiShape {
    origin: [f32; 2],
    size: [f32; 2],
    radius: f32,
    border_width: f32,
    fill_color: [f32; 4],
    border_color: [f32; 4],
}

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
    ui_shapes: Vec<UiShape>,
    text: Vec<TextDrawDesc>,
    config: DebugOverlayConfig,
    hit_regions: Vec<DebugHitRegion>,
}

impl Default for DebugOverlay {
    fn default() -> Self {
        let config = DebugOverlayConfig::default();
        Self {
            shapes: DebugDraw2d::with_style(config.style),
            ui_shapes: Vec::new(),
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
        self.shapes.is_empty() && self.ui_shapes.is_empty() && self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.shapes.clear();
        self.ui_shapes.clear();
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

    pub fn add_screen_text(&mut self, text: impl Into<String>, x: f32, y: f32) -> &mut Self {
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

    pub fn line_screen(
        &mut self,
        width: u32,
        height: u32,
        from: [f32; 2],
        to: [f32; 2],
    ) -> &mut Self {
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
        let _ = (width, height);
        let [origin, size] = self.transform_screen_rect(origin, size);
        let radius = self.transform_screen_scalar(radius_pixels);
        let thickness = self.transform_screen_scalar(thickness_pixels);
        self.push_ui_shape(origin, size, radius, thickness, [0.0, 0.0, 0.0, 0.0], color);
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
        let _ = (width, height);
        let [origin, size] = self.transform_screen_rect(origin, size);
        self.push_ui_shape(origin, size, 0.0, 0.0, color, [0.0, 0.0, 0.0, 0.0]);
        self
    }

    pub fn filled_rounded_rect_screen(
        &mut self,
        width: u32,
        height: u32,
        origin: [f32; 2],
        size: [f32; 2],
        radius_pixels: f32,
        color: [f32; 4],
    ) -> &mut Self {
        let _ = (width, height);
        let [origin, size] = self.transform_screen_rect(origin, size);
        let radius = self.transform_screen_scalar(radius_pixels);
        self.push_ui_shape(origin, size, radius, 0.0, color, [0.0, 0.0, 0.0, 0.0]);
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
        let radius = self.transform_screen_scalar(radius_pixels) * 2.0 / width.max(1) as f32;
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
        style.point_size = self.transform_screen_scalar(size_pixels) * 2.0 / width.max(1) as f32;
        self.shapes.cross_marker_with_style(
            screen_to_ndc(width, height, self.transform_screen_point(center)),
            style,
        );
        self
    }

    fn text_descs(&self) -> &[TextDrawDesc] {
        &self.text
    }

    fn push_ui_shape(
        &mut self,
        origin: [f32; 2],
        size: [f32; 2],
        radius: f32,
        border_width: f32,
        fill_color: [f32; 4],
        border_color: [f32; 4],
    ) {
        if fill_color[3] <= 0.0 && border_color[3] <= 0.0 {
            return;
        }

        let left = origin[0].min(origin[0] + size[0]);
        let right = origin[0].max(origin[0] + size[0]);
        let top = origin[1].min(origin[1] + size[1]);
        let bottom = origin[1].max(origin[1] + size[1]);
        let size = [right - left, bottom - top];
        if size[0] <= f32::EPSILON || size[1] <= f32::EPSILON {
            return;
        }

        let half_min = size[0].min(size[1]) * 0.5;
        self.ui_shapes.push(UiShape {
            origin: [left, top],
            size,
            radius: radius.clamp(0.0, half_min),
            border_width: border_width.max(0.0).min(half_min),
            fill_color,
            border_color,
        });
    }

    fn transform_screen_point(&self, point: [f32; 2]) -> [f32; 2] {
        [
            point[0] * self.config.transform.scale[0] + self.config.transform.translation[0],
            point[1] * self.config.transform.scale[1] + self.config.transform.translation[1],
        ]
    }

    fn transform_screen_scalar(&self, value: f32) -> f32 {
        let scale =
            (self.config.transform.scale[0].abs() + self.config.transform.scale[1].abs()) * 0.5;
        value * scale
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
    ui_shape_program: MeshProgram,
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
            ui_shape_program: MeshProgram::new(
                engine,
                MeshProgramDesc {
                    fragment: ShaderDesc {
                        source: ShaderSource::Inline(UI_SHAPE_FRAGMENT.to_string()),
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
        for shape in &overlay.ui_shapes {
            let mesh = ui_shape_mesh(&self.ui_shape_program.engine, width, height, shape)?;
            let constants = UiShapeConstants {
                size_radius_border: [
                    shape.size[0],
                    shape.size[1],
                    shape.radius,
                    shape.border_width,
                ],
                fill_color: shape.fill_color,
                border_color: shape.border_color,
            };
            target.draw_mesh_with_push_constants(
                &mesh,
                &self.ui_shape_program,
                StageMask::FRAGMENT,
                bytemuck::bytes_of(&constants),
            )?;
        }
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

fn ui_shape_mesh(engine: &Engine, width: u32, height: u32, shape: &UiShape) -> Result<Mesh> {
    let aa_padding = 2.0;
    let origin = [shape.origin[0] - aa_padding, shape.origin[1] - aa_padding];
    let size = [
        shape.size[0] + aa_padding * 2.0,
        shape.size[1] + aa_padding * 2.0,
    ];
    let ndc_origin = screen_to_ndc(width, height, origin);
    let ndc_size = [
        size[0] / width.max(1) as f32 * 2.0,
        -size[1] / height.max(1) as f32 * 2.0,
    ];

    let mut batch = QuadBatch::new();
    batch.push(
        ndc_origin,
        ndc_size,
        [
            -aa_padding,
            -aa_padding,
            shape.size[0] + aa_padding,
            shape.size[1] + aa_padding,
        ],
        [1.0, 1.0, 1.0, 1.0],
    );
    batch.build(engine)
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
        assert_eq!(overlay.ui_shapes.len(), 1);
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
            overlay
                .hit_test_screen([32.0, 38.0])
                .map(|region| region.tag.as_str()),
            Some("panel")
        );
        assert_eq!(
            overlay.config().antialiasing,
            DebugOverlayAntialiasing::Disabled
        );
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

        assert_eq!(overlay.ui_shapes.len(), 1);
        assert_eq!(overlay.ui_shapes[0].border_width, 3.0);
    }
}
