use crate::{
    Engine, TextAtlasPage, TextDrawDesc, TextGlyphQuad, TextLayoutOutput, TextPlacement,
    TextRenderer, TextScene, TextSceneQuad, TiledTextEngineFrame,
};

/// Prepared text for one frame.
///
/// The quads are already transformed into their requested 2D or 3D placement.
/// Atlas pages contain RGBA8 glyph data that callers can upload with the
/// regular texture upload path before drawing the quads.
#[derive(Clone, Debug, Default)]
pub struct TextEngineFrame {
    pub atlas_pages: Vec<TextAtlasPage>,
    pub draws: Vec<PreparedTextDraw>,
}

/// A transformed text draw ready for a renderer-specific batching path.
#[derive(Clone, Debug)]
pub struct PreparedTextDraw {
    pub source_index: usize,
    pub placement: TextPlacement,
    pub quads: Vec<PreparedTextQuad>,
}

/// A text quad after applying screen-space or world-space placement.
#[derive(Copy, Clone, Debug)]
pub struct PreparedTextQuad {
    pub positions: [[f32; 3]; 4],
    pub uvs: [[f32; 2]; 4],
    pub atlas_page: u32,
    pub color: [f32; 4],
}

/// Engine-side adapter for shaped text.
///
/// `TextEngine` intentionally owns only placement and frame packaging. Shaping,
/// ligatures, fallback, SDF/MSDF glyph generation, and atlas eviction stay in
/// the supplied [`TextRenderer`] implementation. A textui-backed implementation
/// can therefore preserve textui's typography behavior while emitting sturdy
/// engine draw data.
pub struct TextEngine<R> {
    renderer: R,
}

impl<R> TextEngine<R> {
    pub fn new(renderer: R) -> Self {
        Self { renderer }
    }

    pub fn renderer(&self) -> &R {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut R {
        &mut self.renderer
    }
}

impl<R: TextRenderer> TextEngine<R> {
    /// Shape and package text for the current target.
    pub fn prepare_frame(
        &mut self,
        descs: &[TextDrawDesc],
        target_width: u32,
        target_height: u32,
    ) -> TextEngineFrame {
        let TextLayoutOutput {
            quads,
            new_atlas_pages,
            scene,
        } = self.renderer.layout(descs, target_width, target_height);

        let mut draws = Vec::with_capacity(descs.len());

        if !scene.quads.is_empty() {
            for (source_index, desc) in descs.iter().enumerate() {
                let quads = scene
                    .quads
                    .iter()
                    .filter(|quad| quad.source_index == source_index)
                    .map(|quad| prepare_scene_quad(quad, &desc.placement))
                    .collect::<Vec<_>>();
                if quads.is_empty() {
                    continue;
                }
                draws.push(PreparedTextDraw {
                    source_index,
                    placement: desc.placement.clone(),
                    quads,
                });
            }
        } else if !quads.is_empty() {
            draws.push(PreparedTextDraw {
                source_index: 0,
                placement: TextPlacement::Screen2d { x: 0.0, y: 0.0 },
                quads: quads.iter().map(prepare_legacy_quad).collect(),
            });
        }

        TextEngineFrame {
            atlas_pages: if scene.atlas_pages.is_empty() {
                new_atlas_pages
            } else {
                scene.atlas_pages
            },
            draws,
        }
    }

    /// Shape text and split atlas pages so no text image exceeds `max_texture_side_px`.
    pub fn prepare_tiled_frame(
        &mut self,
        descs: &[TextDrawDesc],
        target_width: u32,
        target_height: u32,
        max_texture_side_px: u32,
    ) -> TiledTextEngineFrame {
        self.prepare_frame(descs, target_width, target_height)
            .tile_atlas_pages(max_texture_side_px)
    }

    /// Shape text and tile atlas pages using the engine's 2D image limit.
    pub fn prepare_tiled_frame_with_engine_limits(
        &mut self,
        engine: &Engine,
        descs: &[TextDrawDesc],
        target_width: u32,
        target_height: u32,
    ) -> TiledTextEngineFrame {
        let max_side = engine
            .caps()
            .limits
            .max_image_dimension_2d
            .min(engine.caps().limits.max_texture_2d_size)
            .max(1);
        self.prepare_tiled_frame(descs, target_width, target_height, max_side)
    }
}

/// `textui` implementation of [`TextRenderer`].
///
/// This is a headless bridge: `textui` shapes with cosmic-text, rasterizes glyphs
/// into CPU atlas pages, and SturdyEngine owns all eventual GPU upload/draw work.
pub struct TextUiRenderer {
    inner: textui::TextUi,
    frame_number: u64,
    max_texture_side_px: usize,
}

impl TextUiRenderer {
    pub fn new(max_texture_side_px: usize) -> Self {
        Self {
            inner: textui::TextUi::new(),
            frame_number: 0,
            max_texture_side_px: max_texture_side_px.max(1),
        }
    }

    pub fn with_engine(engine: &Engine) -> Self {
        Self::new(engine.caps().limits.max_image_dimension_2d as usize)
    }

    pub fn inner(&self) -> &textui::TextUi {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut textui::TextUi {
        &mut self.inner
    }

    pub fn set_max_texture_side_px(&mut self, max_texture_side_px: usize) {
        self.max_texture_side_px = max_texture_side_px.max(1);
    }

    pub fn register_font_data(&mut self, bytes: Vec<u8>) {
        self.inner.register_font_data(bytes);
    }
}

impl Default for TextUiRenderer {
    fn default() -> Self {
        Self::new(usize::MAX)
    }
}

impl TextRenderer for TextUiRenderer {
    fn layout(
        &mut self,
        descs: &[TextDrawDesc],
        target_width: u32,
        target_height: u32,
    ) -> TextLayoutOutput {
        self.frame_number = self.frame_number.saturating_add(1);
        self.inner.begin_frame_info(textui::TextFrameInfo::new(
            self.frame_number,
            self.max_texture_side_px,
        ));

        let mut scene = TextScene::default();
        scene.bounds_min = [f32::INFINITY, f32::INFINITY];
        scene.bounds_max = [f32::NEG_INFINITY, f32::NEG_INFINITY];

        for (source_index, desc) in descs.iter().enumerate() {
            let options = textui_options_from_desc(desc);
            let width = desc.max_width.or(Some(target_width as f32));
            let text_scene = self.inner.prepare_label_gpu_scene_at_scale(
                ("sturdy_text", source_index, self.frame_number),
                &desc.text,
                &options,
                width,
                1.0,
            );

            merge_textui_scene(source_index, &text_scene, &mut scene);
        }

        if scene.quads.is_empty() {
            scene.bounds_min = [0.0, 0.0];
            scene.bounds_max = [0.0, 0.0];
        }
        scene.size = [
            (scene.bounds_max[0] - scene.bounds_min[0]).max(0.0),
            (scene.bounds_max[1] - scene.bounds_min[1]).max(0.0),
        ];
        let _ = target_height;

        TextLayoutOutput {
            scene,
            ..TextLayoutOutput::default()
        }
    }
}

fn textui_options_from_desc(desc: &TextDrawDesc) -> textui::TextLabelOptions {
    let mut fundamentals = textui::TextFundamentals::default();
    fundamentals.kerning = if desc.typography.kerning {
        textui::TextKerning::Normal
    } else {
        textui::TextKerning::None
    };
    fundamentals.standard_ligatures = desc.typography.standard_ligatures;
    fundamentals.contextual_alternates = desc.typography.contextual_alternates;
    fundamentals.feature_settings = desc
        .typography
        .open_type_features
        .iter()
        .filter_map(|tag| feature_setting_from_tag(tag))
        .collect();

    textui::TextLabelOptions {
        font_size: desc.typography.font_size,
        line_height: desc.typography.line_height,
        color: color_to_textui(desc.color),
        wrap: desc.max_width.is_some(),
        monospace: desc.typography.monospace,
        weight: desc.typography.weight,
        italic: desc.typography.italic,
        family_candidates: desc.typography.family_candidates.clone(),
        fundamentals,
    }
}

fn feature_setting_from_tag(tag: &str) -> Option<textui::TextFeatureSetting> {
    let bytes = tag.as_bytes();
    if bytes.len() != 4 {
        return None;
    }
    Some(textui::TextFeatureSetting::new(
        [bytes[0], bytes[1], bytes[2], bytes[3]],
        1,
    ))
}

fn color_to_textui(color: [f32; 4]) -> textui::TextColor {
    textui::TextColor::from_rgba8(
        float_channel_to_u8(color[0]),
        float_channel_to_u8(color[1]),
        float_channel_to_u8(color[2]),
        float_channel_to_u8(color[3]),
    )
}

fn float_channel_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn merge_textui_scene(
    source_index: usize,
    text_scene: &textui::TextGpuScene,
    scene: &mut TextScene,
) {
    for page in &text_scene.atlas_pages {
        if !scene
            .atlas_pages
            .iter()
            .any(|existing| existing.page_index == page.page_index as u32)
        {
            scene.atlas_pages.push(TextAtlasPage {
                page_index: page.page_index as u32,
                width: page.size_px[0] as u32,
                height: page.size_px[1] as u32,
                content_hash: page.content_hash,
                pixels: page.rgba8.to_vec(),
            });
        }
    }

    for quad in &text_scene.quads {
        let color = [
            quad.tint_rgba[0] as f32 / 255.0,
            quad.tint_rgba[1] as f32 / 255.0,
            quad.tint_rgba[2] as f32 / 255.0,
            quad.tint_rgba[3] as f32 / 255.0,
        ];
        let positions = flip_textui_quad_y(quad.positions, text_scene.bounds_max[1]);
        for position in positions {
            scene.bounds_min[0] = scene.bounds_min[0].min(position[0]);
            scene.bounds_min[1] = scene.bounds_min[1].min(position[1]);
            scene.bounds_max[0] = scene.bounds_max[0].max(position[0]);
            scene.bounds_max[1] = scene.bounds_max[1].max(position[1]);
        }
        scene.quads.push(TextSceneQuad {
            source_index,
            positions,
            uvs: quad.uvs,
            atlas_page: quad.atlas_page_index as u32,
            color,
        });
    }
    scene.fingerprint ^= text_scene.fingerprint;
}

fn flip_textui_quad_y(positions: [[f32; 2]; 4], source_bounds_max_y: f32) -> [[f32; 2]; 4] {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for position in positions {
        min_x = min_x.min(position[0]);
        max_x = max_x.max(position[0]);
        min_y = min_y.min(position[1]);
        max_y = max_y.max(position[1]);
    }

    let flipped_min_y = source_bounds_max_y - max_y;
    let flipped_max_y = source_bounds_max_y - min_y;
    [
        [min_x, flipped_min_y],
        [max_x, flipped_min_y],
        [max_x, flipped_max_y],
        [min_x, flipped_max_y],
    ]
}

fn prepare_legacy_quad(quad: &TextGlyphQuad) -> PreparedTextQuad {
    let [x, y, w, h] = quad.dst_rect;
    let [u0, v0, u1, v1] = quad.uv_rect;
    PreparedTextQuad {
        positions: [
            [x, y, 0.0],
            [x + w, y, 0.0],
            [x + w, y + h, 0.0],
            [x, y + h, 0.0],
        ],
        uvs: [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
        atlas_page: quad.atlas_page,
        color: quad.color,
    }
}

fn prepare_scene_quad(quad: &TextSceneQuad, placement: &TextPlacement) -> PreparedTextQuad {
    let positions = match placement {
        TextPlacement::Screen2d { x, y } => quad.positions.map(|p| [p[0] + *x, p[1] + *y, 0.0]),
        TextPlacement::World3d {
            transform,
            pixels_per_world_unit,
            billboard: _,
        } => quad.positions.map(|p| {
            let scale = pixels_per_world_unit.max(f32::EPSILON);
            transform_point(transform, [p[0] / scale, p[1] / scale, 0.0])
        }),
    };

    PreparedTextQuad {
        positions,
        uvs: quad.uvs,
        atlas_page: quad.atlas_page,
        color: quad.color,
    }
}

fn transform_point(transform: &[[f32; 4]; 4], point: [f32; 3]) -> [f32; 3] {
    let x = point[0];
    let y = point[1];
    let z = point[2];
    [
        transform[0][0] * x + transform[1][0] * y + transform[2][0] * z + transform[3][0],
        transform[0][1] * x + transform[1][1] * y + transform[2][1] * z + transform[3][1],
        transform[0][2] * x + transform[1][2] * y + transform[2][2] * z + transform[3][2],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TextLayoutOutput, TextScene};

    #[derive(Default)]
    struct StaticRenderer {
        output: Option<TextLayoutOutput>,
    }

    impl TextRenderer for StaticRenderer {
        fn layout(
            &mut self,
            _descs: &[TextDrawDesc],
            _target_width: u32,
            _target_height: u32,
        ) -> TextLayoutOutput {
            self.output.take().unwrap_or_default()
        }
    }

    #[test]
    fn prepares_world_space_text_quads() {
        let mut renderer = StaticRenderer::default();
        renderer.output = Some(TextLayoutOutput {
            scene: TextScene {
                quads: vec![TextSceneQuad {
                    source_index: 0,
                    positions: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                    uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                    atlas_page: 7,
                    color: [1.0, 1.0, 1.0, 1.0],
                }],
                ..TextScene::default()
            },
            ..TextLayoutOutput::default()
        });

        let transform = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [2.0, 3.0, 4.0, 1.0],
        ];
        let desc = TextDrawDesc::new("ffi")
            .font_size(48.0)
            .world_3d(transform, 10.0, false);
        let mut engine = TextEngine::new(renderer);

        let frame = engine.prepare_frame(&[desc], 800, 600);

        assert_eq!(frame.draws.len(), 1);
        assert_eq!(frame.draws[0].quads[0].positions[0], [2.0, 3.0, 4.0]);
        assert_eq!(frame.draws[0].quads[0].positions[2], [3.0, 4.0, 4.0]);
    }
}
