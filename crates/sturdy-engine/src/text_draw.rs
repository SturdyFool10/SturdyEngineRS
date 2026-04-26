use std::sync::Arc;

/// Typography controls for a text draw operation.
///
/// The fields mirror the text shaping controls used by `textui`/cosmic-text:
/// ligatures, kerning, OpenType feature tags, font fallback candidates, style,
/// and wrapping all belong to layout, not to the GPU draw pass.
#[derive(Clone, Debug, PartialEq)]
pub struct TextTypography {
    /// Font family fallback list, in preference order.
    pub family_candidates: Vec<String>,
    /// Font size in points.
    pub font_size: f32,
    /// Line height in points.
    pub line_height: f32,
    /// Font weight in CSS/OpenType units, usually 100-900.
    pub weight: u16,
    /// Use italic style when available.
    pub italic: bool,
    /// Prefer a monospace family.
    pub monospace: bool,
    /// Enable kerning.
    pub kerning: bool,
    /// Enable standard ligatures (`liga`).
    pub standard_ligatures: bool,
    /// Enable contextual alternates (`calt`).
    pub contextual_alternates: bool,
    /// Additional OpenType feature tags such as `"ss01"` or `"tnum"`.
    pub open_type_features: Vec<String>,
}

impl Default for TextTypography {
    fn default() -> Self {
        Self {
            family_candidates: Vec::new(),
            font_size: 16.0,
            line_height: 24.0,
            weight: 400,
            italic: false,
            monospace: false,
            kerning: true,
            standard_ligatures: true,
            contextual_alternates: true,
            open_type_features: Vec::new(),
        }
    }
}

impl TextTypography {
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    pub fn italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    pub fn monospace(mut self, monospace: bool) -> Self {
        self.monospace = monospace;
        self
    }

    pub fn family(mut self, family: impl Into<String>) -> Self {
        self.family_candidates.push(family.into());
        self
    }

    pub fn open_type_feature(mut self, tag: impl Into<String>) -> Self {
        self.open_type_features.push(tag.into());
        self
    }
}

/// Where prepared text should be placed by the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum TextPlacement {
    /// 2D screen-space text in target pixels, origin at the top-left.
    Screen2d { x: f32, y: f32 },
    /// 3D world-space text. Glyph quads are generated in a local XY plane and
    /// transformed by this matrix at draw time.
    World3d {
        transform: [[f32; 4]; 4],
        pixels_per_world_unit: f32,
        billboard: bool,
    },
}

impl Default for TextPlacement {
    fn default() -> Self {
        Self::Screen2d { x: 0.0, y: 0.0 }
    }
}

/// Description of a single text draw operation.
#[derive(Clone, Debug)]
pub struct TextDrawDesc {
    /// UTF-8 text to render.
    pub text: String,
    /// X position in pixels from the left edge of the target image.
    pub x: f32,
    /// Y position in pixels from the top edge of the target image.
    pub y: f32,
    /// Font size in points. Kept for compatibility; new code should prefer
    /// `typography.font_size`.
    pub font_size: f32,
    /// RGBA color (0.0–1.0 per channel).
    pub color: [f32; 4],
    /// Optional maximum line width before wrapping, in pixels.
    pub max_width: Option<f32>,
    /// Placement for this text run.
    pub placement: TextPlacement,
    /// Advanced typography options passed through to the layout implementation.
    pub typography: TextTypography,
}

impl Default for TextDrawDesc {
    fn default() -> Self {
        let typography = TextTypography::default();
        Self {
            text: String::new(),
            x: 0.0,
            y: 0.0,
            font_size: typography.font_size,
            color: [1.0, 1.0, 1.0, 1.0],
            max_width: None,
            placement: TextPlacement::default(),
            typography,
        }
    }
}

impl TextDrawDesc {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.x = x;
        self.y = y;
        self.placement = TextPlacement::Screen2d { x, y };
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self.typography.font_size = size;
        self
    }

    pub fn color(mut self, rgba: [f32; 4]) -> Self {
        self.color = rgba;
        self
    }

    pub fn max_width(mut self, width: f32) -> Self {
        self.max_width = Some(width);
        self
    }

    pub fn typography(mut self, typography: TextTypography) -> Self {
        self.font_size = typography.font_size;
        self.typography = typography;
        self
    }

    pub fn placement(mut self, placement: TextPlacement) -> Self {
        if let TextPlacement::Screen2d { x, y } = placement {
            self.x = x;
            self.y = y;
        }
        self.placement = placement;
        self
    }

    pub fn world_3d(
        mut self,
        transform: [[f32; 4]; 4],
        pixels_per_world_unit: f32,
        billboard: bool,
    ) -> Self {
        self.placement = TextPlacement::World3d {
            transform,
            pixels_per_world_unit,
            billboard,
        };
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextAtlasContentMode {
    AlphaMask,
    Sdf,
    Msdf,
}

/// An opaque CPU-side atlas page ready to be uploaded to the GPU.
///
/// Populated by a `TextRenderer` implementation; consumed by
/// `TextEngineAdapter::upload_atlas_page`.
#[derive(Clone, Debug)]
pub struct TextAtlasPage {
    /// Stable atlas page identifier from the text backend, when available.
    pub page_index: u32,
    /// Width of the atlas texture in pixels.
    pub width: u32,
    /// Height of the atlas texture in pixels.
    pub height: u32,
    /// Content hash from the text backend, when available.
    pub content_hash: u64,
    /// How the atlas texels should be decoded in the shader.
    pub content_mode: TextAtlasContentMode,
    /// Raw RGBA8 pixel data, row-major.
    pub pixels: Arc<[u8]>,
}

/// A text quad in local text-space before 2D/3D placement is applied.
#[derive(Clone, Debug)]
pub struct TextSceneQuad {
    /// Index of the source `TextDrawDesc` that produced this quad.
    pub source_index: usize,
    /// Four local-space positions in points or pixels, depending on backend scale.
    pub positions: [[f32; 2]; 4],
    /// Atlas UV coordinates for the four corners.
    pub uvs: [[f32; 2]; 4],
    /// Atlas page index.
    pub atlas_page: u32,
    /// RGBA color (0.0-1.0 per channel).
    pub color: [f32; 4],
}

/// A single quad that should be drawn to render one glyph.
#[derive(Copy, Clone, Debug)]
pub struct TextGlyphQuad {
    /// Target position on screen (x, y, width, height) in pixels.
    pub dst_rect: [f32; 4],
    /// Atlas UV coordinates (u0, v0, u1, v1) in 0.0–1.0 range.
    pub uv_rect: [f32; 4],
    /// Atlas page index.
    pub atlas_page: u32,
    /// RGBA color (0.0–1.0 per channel).
    pub color: [f32; 4],
}

/// Backend-neutral output of shaped text that can be rendered as 2D screen
/// quads, world-space 3D billboards, or into an off-screen target.
#[derive(Default, Debug)]
pub struct TextScene {
    pub quads: Vec<TextSceneQuad>,
    pub atlas_pages: Vec<TextAtlasPage>,
    pub bounds_min: [f32; 2],
    pub bounds_max: [f32; 2],
    pub size: [f32; 2],
    pub fingerprint: u64,
}

/// Output of a CPU text layout pass — a list of glyph quads and atlas pages to
/// upload before drawing.
#[derive(Default, Debug)]
pub struct TextLayoutOutput {
    pub quads: Vec<TextGlyphQuad>,
    pub new_atlas_pages: Vec<TextAtlasPage>,
    pub scene: TextScene,
}

/// Adapter trait for plugging a text renderer into `GraphFrame`.
///
/// Implementations may use any shaping library (cosmic-text, textui, etc.).
/// The engine calls `layout` to produce glyph quads, then uploads atlas
/// pages via the standard `TextureUpload` path and draws quads using a
/// caller-supplied graphics pipeline.
pub trait TextRenderer {
    /// Lay out and rasterize `descs` into glyph quads for the given target
    /// dimensions.
    fn layout(
        &mut self,
        descs: &[TextDrawDesc],
        target_width: u32,
        target_height: u32,
    ) -> TextLayoutOutput;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_draw_desc_builder_chain_works() {
        let desc = TextDrawDesc::new("Hello, world!")
            .at(10.0, 20.0)
            .font_size(24.0)
            .color([1.0, 0.5, 0.0, 1.0])
            .max_width(400.0);

        assert_eq!(desc.text, "Hello, world!");
        assert_eq!(desc.x, 10.0);
        assert_eq!(desc.y, 20.0);
        assert_eq!(desc.font_size, 24.0);
        assert_eq!(desc.typography.font_size, 24.0);
        assert_eq!(desc.color, [1.0, 0.5, 0.0, 1.0]);
        assert_eq!(desc.max_width, Some(400.0));
    }

    #[test]
    fn text_draw_desc_default_is_opaque_white() {
        let desc = TextDrawDesc::default();
        assert_eq!(desc.color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(desc.font_size, 16.0);
        assert!(desc.typography.standard_ligatures);
        assert!(desc.typography.kerning);
        assert_eq!(desc.max_width, None);
    }
}
