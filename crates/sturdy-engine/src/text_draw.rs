/// Description of a single text draw operation.
#[derive(Clone, Debug)]
pub struct TextDrawDesc {
    /// UTF-8 text to render.
    pub text: String,
    /// X position in pixels from the left edge of the target image.
    pub x: f32,
    /// Y position in pixels from the top edge of the target image.
    pub y: f32,
    /// Font size in points.
    pub font_size: f32,
    /// RGBA color (0.0–1.0 per channel).
    pub color: [f32; 4],
    /// Optional maximum line width before wrapping, in pixels.
    pub max_width: Option<f32>,
}

impl Default for TextDrawDesc {
    fn default() -> Self {
        Self {
            text: String::new(),
            x: 0.0,
            y: 0.0,
            font_size: 16.0,
            color: [1.0, 1.0, 1.0, 1.0],
            max_width: None,
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
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
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
}

/// An opaque CPU-side atlas page ready to be uploaded to the GPU.
///
/// Populated by a `TextRenderer` implementation; consumed by
/// `TextEngineAdapter::upload_atlas_page`.
#[derive(Clone, Debug)]
pub struct TextAtlasPage {
    /// Width of the atlas texture in pixels.
    pub width: u32,
    /// Height of the atlas texture in pixels.
    pub height: u32,
    /// Raw RGBA8 pixel data, row-major.
    pub pixels: Vec<u8>,
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

/// Output of a CPU text layout pass — a list of glyph quads and atlas pages to
/// upload before drawing.
#[derive(Default, Debug)]
pub struct TextLayoutOutput {
    pub quads: Vec<TextGlyphQuad>,
    pub new_atlas_pages: Vec<TextAtlasPage>,
}

/// Adapter trait for plugging a text renderer into `GraphFrame`.
///
/// Implementations may use any shaping library (cosmic-text, textui, etc.).
/// The engine calls `layout` to produce glyph quads, then uploads atlas
/// pages via the standard `TextureUpload` path and draws quads using a
/// caller-supplied graphics pipeline.
pub trait TextRenderer: Send + Sync {
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
        assert_eq!(desc.color, [1.0, 0.5, 0.0, 1.0]);
        assert_eq!(desc.max_width, Some(400.0));
    }

    #[test]
    fn text_draw_desc_default_is_opaque_white() {
        let desc = TextDrawDesc::default();
        assert_eq!(desc.color, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(desc.font_size, 16.0);
        assert_eq!(desc.max_width, None);
    }
}
