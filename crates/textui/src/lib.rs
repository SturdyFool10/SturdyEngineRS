use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cosmic_text::CacheKeyFlags;
use cosmic_text::{
    Angle as SwashAngle, Attrs, Buffer, CacheKey, Color, Family, FeatureTag, FontFeatures,
    FontSystem, Metrics, Shaping, Style as FontStyle, SwashCache, SwashContent, SwashImage,
    Transform as SwashTransform, Weight, Wrap,
};
use swash::scale::ScaleContext;
use swash::zeno::PathData as _;

const DEFAULT_ATLAS_PAGE_TARGET_PX: usize = 1024;
const DEFAULT_ATLAS_PADDING_PX: usize = 1;
const DEFAULT_ATLAS_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextFrameInfo {
    pub frame_number: u64,
    pub max_texture_side_px: usize,
}

impl TextFrameInfo {
    pub const fn new(frame_number: u64, max_texture_side_px: usize) -> Self {
        Self {
            frame_number,
            max_texture_side_px,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TextColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl TextColor {
    pub const WHITE: Self = Self::from_rgba8(255, 255, 255, 255);

    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn to_normalized_f32(self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }
}

impl Default for TextColor {
    fn default() -> Self {
        Self::WHITE
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextKerning {
    Auto,
    Normal,
    None,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TextFeatureSetting {
    pub tag: [u8; 4],
    pub value: u16,
}

impl TextFeatureSetting {
    pub const fn new(tag: [u8; 4], value: u16) -> Self {
        Self { tag, value }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextFundamentals {
    pub kerning: TextKerning,
    pub standard_ligatures: bool,
    pub contextual_alternates: bool,
    pub discretionary_ligatures: bool,
    pub historical_ligatures: bool,
    pub feature_settings: Vec<TextFeatureSetting>,
}

impl Default for TextFundamentals {
    fn default() -> Self {
        Self {
            kerning: TextKerning::Auto,
            standard_ligatures: true,
            contextual_alternates: true,
            discretionary_ligatures: false,
            historical_ligatures: false,
            feature_settings: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextGlyphRasterMode {
    Auto,
    AlphaMask,
    Sdf,
    Msdf,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextAtlasContentMode {
    AlphaMask,
    Sdf,
    Msdf,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextRasterizationConfig {
    pub glyph_raster_mode: TextGlyphRasterMode,
    pub field_range_px: f32,
}

impl Default for TextRasterizationConfig {
    fn default() -> Self {
        Self {
            glyph_raster_mode: TextGlyphRasterMode::Auto,
            field_range_px: 8.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextGraphicsConfig {
    pub atlas_page_target_px: usize,
    pub atlas_padding_px: usize,
    pub atlas_max_bytes: usize,
    pub rasterization: TextRasterizationConfig,
}

impl Default for TextGraphicsConfig {
    fn default() -> Self {
        Self {
            atlas_page_target_px: DEFAULT_ATLAS_PAGE_TARGET_PX,
            atlas_padding_px: DEFAULT_ATLAS_PADDING_PX,
            atlas_max_bytes: DEFAULT_ATLAS_MAX_BYTES,
            rasterization: TextRasterizationConfig::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLabelOptions {
    pub font_size: f32,
    pub line_height: f32,
    pub color: TextColor,
    pub wrap: bool,
    pub monospace: bool,
    pub weight: u16,
    pub italic: bool,
    pub family_candidates: Vec<String>,
    pub fundamentals: TextFundamentals,
}

impl Default for TextLabelOptions {
    fn default() -> Self {
        Self {
            font_size: 18.0,
            line_height: 27.0,
            color: TextColor::WHITE,
            wrap: true,
            monospace: false,
            weight: 400,
            italic: false,
            family_candidates: Vec::new(),
            fundamentals: TextFundamentals::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TextAtlasPageData {
    pub page_index: usize,
    pub size_px: [usize; 2],
    pub content_hash: u64,
    pub content_mode: TextAtlasContentMode,
    pub rgba8: Arc<[u8]>,
}

#[derive(Clone, Debug)]
pub struct TextGpuQuad {
    pub atlas_page_index: usize,
    pub positions: [[f32; 2]; 4],
    pub uvs: [[f32; 2]; 4],
    pub tint_rgba: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct TextGpuScene {
    pub atlas_pages: Vec<TextAtlasPageData>,
    pub quads: Vec<TextGpuQuad>,
    pub bounds_min: [f32; 2],
    pub bounds_max: [f32; 2],
    pub size_points: [f32; 2],
    pub fingerprint: u64,
}

#[derive(Clone, Debug)]
struct AtlasPage {
    size: usize,
    rgba8: Vec<u8>,
    cached_data: Option<TextAtlasPageData>,
    cursor_x: usize,
    cursor_y: usize,
    row_h: usize,
    content_hash: u64,
    content_mode: TextAtlasContentMode,
}

impl AtlasPage {
    fn new(size: usize, content_mode: TextAtlasContentMode) -> Self {
        Self {
            size,
            rgba8: vec![0; size.saturating_mul(size).saturating_mul(4)],
            cached_data: None,
            cursor_x: 0,
            cursor_y: 0,
            row_h: 0,
            content_hash: 0,
            content_mode,
        }
    }

    fn allocate(&mut self, width: usize, height: usize) -> Option<[usize; 2]> {
        if width > self.size || height > self.size {
            return None;
        }
        if self.cursor_x + width > self.size {
            self.cursor_x = 0;
            self.cursor_y = self.cursor_y.saturating_add(self.row_h);
            self.row_h = 0;
        }
        if self.cursor_y + height > self.size {
            return None;
        }
        let pos = [self.cursor_x, self.cursor_y];
        self.cursor_x = self.cursor_x.saturating_add(width);
        self.row_h = self.row_h.max(height);
        Some(pos)
    }

    fn reset(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.row_h = 0;
        self.rgba8.fill(0);
        self.content_hash = hash_page_bytes(self.size, &self.rgba8);
        self.cached_data = None;
    }

    fn blit(&mut self, src: &[u8], src_size: [usize; 2], pos: [usize; 2]) {
        let copy_w = src_size[0].min(self.size.saturating_sub(pos[0]));
        let copy_h = src_size[1].min(self.size.saturating_sub(pos[1]));
        for y in 0..copy_h {
            let src_start = y * src_size[0] * 4;
            let dst_start = ((pos[1] + y) * self.size + pos[0]) * 4;
            let byte_count = copy_w * 4;
            self.rgba8[dst_start..dst_start + byte_count]
                .copy_from_slice(&src[src_start..src_start + byte_count]);
        }
        self.content_hash = hash_page_bytes(self.size, &self.rgba8);
        self.cached_data = None;
    }

    fn data(&mut self, page_index: usize) -> TextAtlasPageData {
        if let Some(data) = &self.cached_data {
            return data.clone();
        }

        let data = TextAtlasPageData {
            page_index,
            size_px: [self.size, self.size],
            content_hash: self.content_hash,
            content_mode: self.content_mode,
            rgba8: Arc::from(self.rgba8.as_slice()),
        };
        self.cached_data = Some(data.clone());
        data
    }
}

#[derive(Clone, Debug)]
struct AtlasEntry {
    page_index: usize,
    min_px: [usize; 2],
    size_px: [usize; 2],
    placement_left_px: i32,
    placement_top_px: i32,
    last_used_frame: u64,
    approx_bytes: usize,
}

pub struct TextUi {
    font_system: FontSystem,
    scale_context: ScaleContext,
    swash_cache: SwashCache,
    graphics_config: TextGraphicsConfig,
    current_frame: u64,
    max_texture_side_px: usize,
    pages: Vec<AtlasPage>,
    entries: HashMap<CacheKey, AtlasEntry>,
    cached_bytes: usize,
}

impl Default for TextUi {
    fn default() -> Self {
        Self::new()
    }
}

impl TextUi {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            scale_context: ScaleContext::new(),
            swash_cache: SwashCache::new(),
            graphics_config: TextGraphicsConfig::default(),
            current_frame: 0,
            max_texture_side_px: usize::MAX,
            pages: Vec::new(),
            entries: HashMap::new(),
            cached_bytes: 0,
        }
    }

    pub fn begin_frame_info(&mut self, frame_info: TextFrameInfo) {
        self.current_frame = frame_info.frame_number;
        self.max_texture_side_px = frame_info.max_texture_side_px.max(1);
        self.evict_to_budget();
    }

    pub fn register_font_data(&mut self, bytes: Vec<u8>) {
        self.font_system.db_mut().load_font_data(bytes);
        self.clear_atlas();
    }

    pub fn graphics_config(&self) -> TextGraphicsConfig {
        self.graphics_config
    }

    pub fn set_graphics_config(&mut self, graphics_config: TextGraphicsConfig) {
        if self.graphics_config != graphics_config {
            self.graphics_config = graphics_config;
            self.clear_atlas();
        }
    }

    pub fn max_texture_side_px(&self) -> usize {
        self.max_texture_side_px
    }

    pub fn prepare_label_gpu_scene_at_scale(
        &mut self,
        id_source: impl Hash,
        text: &str,
        options: &TextLabelOptions,
        width_points_opt: Option<f32>,
        scale: f32,
    ) -> Arc<TextGpuScene> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        id_source.hash(&mut hasher);
        text.hash(&mut hasher);
        options_hash(options, &mut hasher);
        width_points_opt
            .map(f32::to_bits)
            .unwrap_or(0)
            .hash(&mut hasher);
        scale.to_bits().hash(&mut hasher);
        let fingerprint = hasher.finish();

        let metrics = Metrics::new(
            (options.font_size * scale).max(1.0),
            (options.line_height * scale).max(1.0),
        );
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(
            &mut self.font_system,
            width_points_opt.map(|w| (w * scale).max(1.0)),
            None,
        );
        buffer.set_wrap(
            &mut self.font_system,
            if options.wrap {
                Wrap::WordOrGlyph
            } else {
                Wrap::None
            },
        );
        let attrs = attrs_for_options(options);
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, true);

        let mut quads = Vec::new();
        let mut bounds_min = [f32::INFINITY, f32::INFINITY];
        let mut bounds_max = [f32::NEG_INFINITY, f32::NEG_INFINITY];

        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, run.line_y), 1.0);
                let Some(entry) = self.resolve_glyph(physical.cache_key) else {
                    continue;
                };

                let x = physical.x + entry.placement_left_px;
                let y = physical.y - entry.placement_top_px;
                let w = entry.size_px[0] as i32;
                let h = entry.size_px[1] as i32;
                if w == 0 || h == 0 {
                    continue;
                }

                let min_x = x as f32 / scale;
                let min_y = y as f32 / scale;
                let max_x = (x + w) as f32 / scale;
                let max_y = (y + h) as f32 / scale;
                bounds_min[0] = bounds_min[0].min(min_x);
                bounds_min[1] = bounds_min[1].min(min_y);
                bounds_max[0] = bounds_max[0].max(max_x);
                bounds_max[1] = bounds_max[1].max(max_y);

                let page_size = self.pages[entry.page_index].size as f32;
                let u0 = entry.min_px[0] as f32 / page_size;
                let v0 = entry.min_px[1] as f32 / page_size;
                let u1 = (entry.min_px[0] + entry.size_px[0]) as f32 / page_size;
                let v1 = (entry.min_px[1] + entry.size_px[1]) as f32 / page_size;

                quads.push(TextGpuQuad {
                    atlas_page_index: entry.page_index,
                    positions: [
                        [min_x, min_y],
                        [max_x, min_y],
                        [max_x, max_y],
                        [min_x, max_y],
                    ],
                    uvs: [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
                    tint_rgba: options.color.to_array(),
                });
            }
        }

        if quads.is_empty() {
            bounds_min = [0.0, 0.0];
            bounds_max = [0.0, 0.0];
        }

        let atlas_pages = self
            .pages
            .iter_mut()
            .enumerate()
            .map(|(page_index, page)| page.data(page_index))
            .collect();
        Arc::new(TextGpuScene {
            atlas_pages,
            quads,
            bounds_min,
            bounds_max,
            size_points: [
                (bounds_max[0] - bounds_min[0]).max(0.0),
                (bounds_max[1] - bounds_min[1]).max(0.0),
            ],
            fingerprint,
        })
    }

    pub fn measure_label_size_at_scale(
        &mut self,
        text: &str,
        options: &TextLabelOptions,
        width_points_opt: Option<f32>,
        scale: f32,
    ) -> [f32; 2] {
        let metrics = Metrics::new(
            (options.font_size * scale).max(1.0),
            (options.line_height * scale).max(1.0),
        );
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(
            &mut self.font_system,
            width_points_opt.map(|w| (w * scale).max(1.0)),
            None,
        );
        buffer.set_wrap(
            &mut self.font_system,
            if options.wrap {
                Wrap::WordOrGlyph
            } else {
                Wrap::None
            },
        );
        let attrs = attrs_for_options(options);
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, true);

        let [width_px, height_px] = measure_buffer_layout_pixels(&buffer);
        [
            width_px as f32 / scale.max(0.001),
            height_px as f32 / scale.max(0.001),
        ]
    }

    fn resolve_glyph(&mut self, cache_key: CacheKey) -> Option<AtlasEntry> {
        let raster_mode = self.resolved_raster_mode(cache_key);
        let cache_key = normalize_cache_key_for_raster_mode(cache_key, raster_mode);

        if let Some(entry) = self.entries.get_mut(&cache_key) {
            entry.last_used_frame = self.current_frame;
            return Some(entry.clone());
        }

        let field_range_px = self.graphics_config.rasterization.field_range_px.max(1.0);
        let rasterized = if matches!(
            raster_mode,
            TextGlyphRasterMode::Sdf | TextGlyphRasterMode::Msdf
        ) {
            render_outline_field_glyph(
                &mut self.font_system,
                &mut self.scale_context,
                cache_key,
                raster_mode,
                field_range_px,
            )
            .or_else(|| render_alpha_glyph(&mut self.font_system, &mut self.swash_cache, cache_key))
        } else {
            render_alpha_glyph(&mut self.font_system, &mut self.swash_cache, cache_key)
        }?;
        let size = rasterized.size_px;
        if size[0] == 0 || size[1] == 0 {
            return None;
        }

        let padding = self.graphics_config.atlas_padding_px;
        let alloc_size = [
            size[0].saturating_add(padding * 2),
            size[1].saturating_add(padding * 2),
        ];
        let page_side = self.resolved_page_side();
        if alloc_size[0] > page_side || alloc_size[1] > page_side {
            return None;
        }
        let (page_index, min) =
            self.allocate_slot(page_side, alloc_size, rasterized.content_mode)?;
        self.pages[page_index].blit(&rasterized.rgba, size, [min[0] + padding, min[1] + padding]);

        let entry = AtlasEntry {
            page_index,
            min_px: [min[0] + padding, min[1] + padding],
            size_px: size,
            placement_left_px: rasterized.placement_left_px,
            placement_top_px: rasterized.placement_top_px,
            last_used_frame: self.current_frame,
            approx_bytes: rasterized.rgba.len(),
        };
        self.cached_bytes = self.cached_bytes.saturating_add(entry.approx_bytes);
        self.entries.insert(cache_key, entry.clone());
        self.evict_to_budget();
        Some(entry)
    }

    fn allocate_slot(
        &mut self,
        page_side: usize,
        size: [usize; 2],
        content_mode: TextAtlasContentMode,
    ) -> Option<(usize, [usize; 2])> {
        for (page_index, page) in self.pages.iter_mut().enumerate() {
            if page.size == page_side
                && page.content_mode == content_mode
                && let Some(pos) = page.allocate(size[0], size[1])
            {
                return Some((page_index, pos));
            }
        }
        let mut page = AtlasPage::new(page_side, content_mode);
        let pos = page.allocate(size[0], size[1])?;
        self.pages.push(page);
        Some((self.pages.len() - 1, pos))
    }

    fn resolved_page_side(&self) -> usize {
        self.graphics_config
            .atlas_page_target_px
            .max(1)
            .min(self.max_texture_side_px.max(1))
    }

    fn resolved_raster_mode(&self, cache_key: CacheKey) -> TextGlyphRasterMode {
        match self.graphics_config.rasterization.glyph_raster_mode {
            TextGlyphRasterMode::Auto => {
                let font_size = f32::from_bits(cache_key.font_size_bits);
                if font_size > 28.0 {
                    TextGlyphRasterMode::Msdf
                } else {
                    TextGlyphRasterMode::Sdf
                }
            }
            mode => mode,
        }
    }

    fn clear_atlas(&mut self) {
        self.pages.clear();
        self.entries.clear();
        self.cached_bytes = 0;
    }

    fn evict_to_budget(&mut self) {
        if self.cached_bytes <= self.graphics_config.atlas_max_bytes {
            return;
        }

        // Compute the most-recent use frame for every page.
        let mut page_last_used = vec![0u64; self.pages.len()];
        for entry in self.entries.values() {
            if entry.page_index < page_last_used.len() {
                page_last_used[entry.page_index] =
                    page_last_used[entry.page_index].max(entry.last_used_frame);
            }
        }

        // Evict pages from oldest to newest until under budget.
        let mut pages_by_age: Vec<usize> = (0..self.pages.len()).collect();
        pages_by_age.sort_by_key(|&i| page_last_used[i]);

        for page_index in pages_by_age {
            if self.cached_bytes <= self.graphics_config.atlas_max_bytes {
                break;
            }
            let mut bytes_freed = 0usize;
            self.entries.retain(|_, entry| {
                if entry.page_index == page_index {
                    bytes_freed += entry.approx_bytes;
                    false
                } else {
                    true
                }
            });
            self.cached_bytes = self.cached_bytes.saturating_sub(bytes_freed);
            if let Some(page) = self.pages.get_mut(page_index) {
                page.reset();
            }
        }

        // Safety-net fallback if somehow still over budget.
        if self.cached_bytes > self.graphics_config.atlas_max_bytes {
            self.clear_atlas();
        }
    }
}

fn font_features(fundamentals: &TextFundamentals) -> FontFeatures {
    let mut features = FontFeatures::new();
    features.set(
        FeatureTag::KERNING,
        if fundamentals.kerning == TextKerning::None {
            0
        } else {
            1
        },
    );
    features.set(
        FeatureTag::STANDARD_LIGATURES,
        u32::from(fundamentals.standard_ligatures),
    );
    features.set(
        FeatureTag::CONTEXTUAL_ALTERNATES,
        u32::from(fundamentals.contextual_alternates),
    );
    features.set(
        FeatureTag::DISCRETIONARY_LIGATURES,
        u32::from(fundamentals.discretionary_ligatures),
    );
    features.set(
        FeatureTag::new(b"hlig"),
        u32::from(fundamentals.historical_ligatures),
    );
    for feature in &fundamentals.feature_settings {
        features.set(FeatureTag::new(&feature.tag), u32::from(feature.value));
    }
    features
}

fn attrs_for_options<'a>(options: &'a TextLabelOptions) -> Attrs<'a> {
    let family = if options.monospace {
        Family::Monospace
    } else if let Some(family) = options.family_candidates.first() {
        Family::Name(family.as_str())
    } else {
        Family::SansSerif
    };
    let color = options.color.to_array();
    Attrs::new()
        .family(family)
        .weight(Weight(options.weight))
        .style(if options.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        })
        .color(Color::rgba(color[0], color[1], color[2], color[3]))
        .font_features(font_features(&options.fundamentals))
}

fn swash_image_to_rgba8(
    image: &SwashImage,
    mode: TextGlyphRasterMode,
    field_range_px: f32,
) -> Option<(Vec<u8>, TextAtlasContentMode)> {
    let width = image.placement.width as usize;
    let height = image.placement.height as usize;
    if width == 0 || height == 0 {
        return None;
    }
    match image.content {
        SwashContent::Mask => {
            if matches!(mode, TextGlyphRasterMode::Sdf | TextGlyphRasterMode::Msdf) {
                return Some((
                    alpha_mask_to_distance_field(
                        &image.data,
                        [width, height],
                        field_range_px,
                        matches!(mode, TextGlyphRasterMode::Msdf),
                    ),
                    match mode {
                        TextGlyphRasterMode::Msdf => TextAtlasContentMode::Msdf,
                        _ => TextAtlasContentMode::Sdf,
                    },
                ));
            }
            let mut out = Vec::with_capacity(width * height * 4);
            for alpha in &image.data {
                out.extend_from_slice(&[255, 255, 255, *alpha]);
            }
            Some((out, TextAtlasContentMode::AlphaMask))
        }
        SwashContent::SubpixelMask | SwashContent::Color => {
            Some((image.data.clone(), TextAtlasContentMode::AlphaMask))
        }
    }
}

#[derive(Clone)]
struct RasterizedGlyph {
    rgba: Vec<u8>,
    size_px: [usize; 2],
    placement_left_px: i32,
    placement_top_px: i32,
    content_mode: TextAtlasContentMode,
}

fn render_alpha_glyph(
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    cache_key: CacheKey,
) -> Option<RasterizedGlyph> {
    let image = swash_cache.get_image(font_system, cache_key).clone()?;
    let (rgba, content_mode) = swash_image_to_rgba8(&image, TextGlyphRasterMode::AlphaMask, 1.0)?;
    Some(RasterizedGlyph {
        rgba,
        size_px: [
            image.placement.width as usize,
            image.placement.height as usize,
        ],
        placement_left_px: image.placement.left,
        placement_top_px: image.placement.top,
        content_mode,
    })
}

fn render_outline_field_glyph(
    font_system: &mut FontSystem,
    scale_context: &mut ScaleContext,
    cache_key: CacheKey,
    raster_mode: TextGlyphRasterMode,
    field_range_px: f32,
) -> Option<RasterizedGlyph> {
    let commands = render_swash_outline_commands(font_system, scale_context, cache_key)?;
    let content_mode = match raster_mode {
        TextGlyphRasterMode::Sdf => TextAtlasContentMode::Sdf,
        TextGlyphRasterMode::Msdf => TextAtlasContentMode::Msdf,
        _ => TextAtlasContentMode::AlphaMask,
    };
    let outline = flatten_outline_commands_for_field(&commands, content_mode);
    if outline.segments.is_empty()
        || !outline.min[0].is_finite()
        || !outline.min[1].is_finite()
        || !outline.max[0].is_finite()
        || !outline.max[1].is_finite()
        || outline_has_same_winding_overlap(&outline.contours)
    {
        return None;
    }

    let left = (outline.min[0] - field_range_px).floor() as i32;
    let bottom = (outline.min[1] - field_range_px).floor() as i32;
    let right = (outline.max[0] + field_range_px).ceil() as i32;
    let top = (outline.max[1] + field_range_px).ceil() as i32;
    let glyph_width = (right - left).max(1) as usize;
    let glyph_height = (top - bottom).max(1) as usize;
    let mut rgba = vec![0u8; glyph_width.saturating_mul(glyph_height).saturating_mul(4)];

    for y in 0..glyph_height {
        for x in 0..glyph_width {
            let sample = [left as f32 + x as f32 + 0.5, top as f32 - y as f32 - 0.5];
            let inside = point_inside_outline(sample, &outline.contours);
            let encoded = match content_mode {
                TextAtlasContentMode::Sdf => {
                    encode_sdf_sample(sample, inside, &outline.segments, field_range_px)
                }
                TextAtlasContentMode::Msdf => {
                    encode_msdf_sample(sample, inside, &outline.segments, field_range_px)
                }
                TextAtlasContentMode::AlphaMask => unreachable!(),
            };
            let base = (y * glyph_width + x) * 4;
            rgba[base..base + 4].copy_from_slice(&encoded);
        }
    }

    Some(RasterizedGlyph {
        rgba,
        size_px: [glyph_width, glyph_height],
        placement_left_px: left,
        placement_top_px: top,
        content_mode,
    })
}

fn normalize_cache_key_for_raster_mode(
    mut cache_key: CacheKey,
    raster_mode: TextGlyphRasterMode,
) -> CacheKey {
    if matches!(
        raster_mode,
        TextGlyphRasterMode::Sdf | TextGlyphRasterMode::Msdf
    ) {
        cache_key.x_bin = cosmic_text::SubpixelBin::Zero;
        cache_key.y_bin = cosmic_text::SubpixelBin::Zero;
    }
    cache_key
}

fn measure_buffer_layout_pixels(buffer: &Buffer) -> [usize; 2] {
    let mut max_right = 0.0_f32;
    let mut max_bottom = 0.0_f32;

    for run in buffer.layout_runs() {
        max_bottom = max_bottom.max(run.line_top + run.line_height);
        for glyph in run.glyphs {
            max_right = max_right.max(glyph.x + glyph.w);
        }
    }

    if max_bottom <= 0.0 {
        max_bottom = buffer.metrics().line_height.max(1.0);
    }

    [
        max_right.ceil().max(1.0) as usize,
        max_bottom.ceil().max(1.0) as usize,
    ]
}

fn alpha_mask_to_distance_field(
    alpha: &[u8],
    size: [usize; 2],
    max_distance: f32,
    rgb_distance: bool,
) -> Vec<u8> {
    let width = size[0];
    let height = size[1];
    let mut out = vec![0; width.saturating_mul(height).saturating_mul(4)];
    let edge_pixels = collect_edge_pixels(alpha, size);
    if edge_pixels.is_empty() {
        return out;
    }

    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let inside = alpha[index] >= 128;
            let mut nearest_sq = f32::INFINITY;
            for [edge_x, edge_y] in &edge_pixels {
                let dx = x as f32 - *edge_x as f32;
                let dy = y as f32 - *edge_y as f32;
                nearest_sq = nearest_sq.min(dx * dx + dy * dy);
            }
            let signed_distance =
                nearest_sq.sqrt().min(max_distance) * if inside { 1.0 } else { -1.0 };
            let encoded = (((signed_distance / max_distance) * 0.5 + 0.5) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            let base = index * 4;
            if rgb_distance {
                out[base] = encoded;
                out[base + 1] = encoded;
                out[base + 2] = encoded;
                out[base + 3] = 255;
            } else {
                out[base] = 255;
                out[base + 1] = 255;
                out[base + 2] = 255;
                out[base + 3] = encoded;
            }
        }
    }
    out
}

#[derive(Clone, Copy)]
struct FieldLineSegment {
    a: [f32; 2],
    b: [f32; 2],
    color_mask: u8,
}

#[derive(Default)]
struct FlattenedOutline {
    contours: Vec<Vec<[f32; 2]>>,
    segments: Vec<FieldLineSegment>,
    min: [f32; 2],
    max: [f32; 2],
}

impl FlattenedOutline {
    fn new() -> Self {
        Self {
            contours: Vec::new(),
            segments: Vec::new(),
            min: [f32::INFINITY, f32::INFINITY],
            max: [f32::NEG_INFINITY, f32::NEG_INFINITY],
        }
    }

    fn include_point(&mut self, point: [f32; 2]) {
        self.min[0] = self.min[0].min(point[0]);
        self.min[1] = self.min[1].min(point[1]);
        self.max[0] = self.max[0].max(point[0]);
        self.max[1] = self.max[1].max(point[1]);
    }
}

fn render_swash_outline_commands(
    font_system: &mut FontSystem,
    scale_context: &mut ScaleContext,
    cache_key: CacheKey,
) -> Option<Box<[swash::zeno::Command]>> {
    let ppem = f32::from_bits(cache_key.font_size_bits);
    let font = font_system.get_font(cache_key.font_id, cache_key.font_weight)?;
    let swash_font = font.as_swash();
    let mut scaler = scale_context
        .builder(swash_font)
        .size(ppem)
        .hint(true)
        .build();
    let mut outline = scaler
        .scale_outline(cache_key.glyph_id)
        .or_else(|| scaler.scale_color_outline(cache_key.glyph_id))?;
    if cache_key.flags.contains(CacheKeyFlags::FAKE_ITALIC) {
        outline.transform(&SwashTransform::skew(
            SwashAngle::from_degrees(14.0),
            SwashAngle::from_degrees(0.0),
        ));
    }
    Some(outline.path().commands().collect())
}

fn flatten_outline_commands_for_field(
    commands: &[swash::zeno::Command],
    content_mode: TextAtlasContentMode,
) -> FlattenedOutline {
    let mut outline = FlattenedOutline::new();
    let mut current = [0.0, 0.0];
    let mut contour_start = [0.0, 0.0];
    let mut contour_points = Vec::<[f32; 2]>::new();
    let mut contour_segments = Vec::<([f32; 2], [f32; 2])>::new();

    let flush_contour = |outline: &mut FlattenedOutline,
                         contour_points: &mut Vec<[f32; 2]>,
                         contour_segments: &mut Vec<([f32; 2], [f32; 2])>| {
        if contour_segments.is_empty() {
            contour_points.clear();
            return;
        }
        if contour_points.len() >= 3 {
            outline.contours.push(contour_points.clone());
        }
        let colors = [1_u8, 2_u8, 4_u8];
        if matches!(content_mode, TextAtlasContentMode::Msdf) {
            // Assign colors to segments based on corner detection so that each
            // channel traces a geometrically continuous arc of the outline.
            // Color cycles at corners (direction change > 90°); this lets the
            // MSDF median function reconstruct sharp corners accurately.
            let mut color_index = 0_usize;
            for (i, &(a, b)) in contour_segments.iter().enumerate() {
                outline.include_point(a);
                outline.include_point(b);
                if i > 0 {
                    let (pa, pb) = contour_segments[i - 1];
                    let d1 = [pb[0] - pa[0], pb[1] - pa[1]];
                    let d2 = [b[0] - a[0], b[1] - a[1]];
                    let len1_sq = d1[0] * d1[0] + d1[1] * d1[1];
                    let len2_sq = d2[0] * d2[0] + d2[1] * d2[1];
                    if len1_sq > 1e-10 && len2_sq > 1e-10 {
                        let dot = d1[0] * d2[0] + d1[1] * d2[1];
                        let cos_a = dot / (len1_sq.sqrt() * len2_sq.sqrt());
                        if cos_a <= 0.0 {
                            color_index = (color_index + 1) % 3;
                        }
                    }
                }
                outline.segments.push(FieldLineSegment {
                    a,
                    b,
                    color_mask: colors[color_index],
                });
            }
        } else {
            for (_, (a, b)) in contour_segments.iter().copied().enumerate() {
                outline.include_point(a);
                outline.include_point(b);
                outline.segments.push(FieldLineSegment {
                    a,
                    b,
                    color_mask: 0b111,
                });
            }
        }
        contour_points.clear();
        contour_segments.clear();
    };

    for command in commands {
        match *command {
            swash::zeno::Command::MoveTo(point) => {
                flush_contour(&mut outline, &mut contour_points, &mut contour_segments);
                current = [point.x, point.y];
                contour_start = current;
                contour_points.push(current);
            }
            swash::zeno::Command::LineTo(point) => {
                let next = [point.x, point.y];
                contour_segments.push((current, next));
                contour_points.push(next);
                current = next;
            }
            swash::zeno::Command::QuadTo(control, point) => {
                let next = [point.x, point.y];
                let control = [control.x, control.y];
                let steps = curve_steps(current, control, control, next);
                let mut prev = current;
                for step in 1..=steps {
                    let t = step as f32 / steps as f32;
                    let p = eval_quad(current, control, next, t);
                    contour_segments.push((prev, p));
                    contour_points.push(p);
                    prev = p;
                }
                current = next;
            }
            swash::zeno::Command::CurveTo(control_a, control_b, point) => {
                let next = [point.x, point.y];
                let control_a = [control_a.x, control_a.y];
                let control_b = [control_b.x, control_b.y];
                let steps = curve_steps(current, control_a, control_b, next);
                let mut prev = current;
                for step in 1..=steps {
                    let t = step as f32 / steps as f32;
                    let p = eval_cubic(current, control_a, control_b, next, t);
                    contour_segments.push((prev, p));
                    contour_points.push(p);
                    prev = p;
                }
                current = next;
            }
            swash::zeno::Command::Close => {
                if current != contour_start {
                    contour_segments.push((current, contour_start));
                    contour_points.push(contour_start);
                    current = contour_start;
                }
                flush_contour(&mut outline, &mut contour_points, &mut contour_segments);
            }
        }
    }
    flush_contour(&mut outline, &mut contour_points, &mut contour_segments);
    outline
}

fn outline_has_same_winding_overlap(contours: &[Vec<[f32; 2]>]) -> bool {
    if contours.len() < 2 {
        return false;
    }
    let mut winding = Vec::with_capacity(contours.len());
    let mut bboxes: Vec<([f32; 2], [f32; 2])> = Vec::with_capacity(contours.len());
    for contour in contours {
        let mut min = [f32::INFINITY, f32::INFINITY];
        let mut max = [f32::NEG_INFINITY, f32::NEG_INFINITY];
        let mut area = 0.0f32;
        let n = contour.len();
        for i in 0..n {
            let a = contour[i];
            let b = contour[(i + 1) % n];
            area += (b[0] - a[0]) * (b[1] + a[1]);
            min[0] = min[0].min(a[0]);
            min[1] = min[1].min(a[1]);
            max[0] = max[0].max(a[0]);
            max[1] = max[1].max(a[1]);
        }
        winding.push(area.signum());
        bboxes.push((min, max));
    }
    for i in 0..contours.len() {
        for j in (i + 1)..contours.len() {
            if winding[i] != winding[j] {
                continue;
            }
            let (a_min, a_max) = bboxes[i];
            let (b_min, b_max) = bboxes[j];
            if a_min[0] < b_max[0]
                && a_max[0] > b_min[0]
                && a_min[1] < b_max[1]
                && a_max[1] > b_min[1]
            {
                return true;
            }
        }
    }
    false
}

fn curve_steps(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) -> usize {
    let len = point_distance(p0, p1) + point_distance(p1, p2) + point_distance(p2, p3);
    ((len / 6.0).ceil() as usize).clamp(4, 24)
}

fn eval_quad(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], t: f32) -> [f32; 2] {
    let mt = 1.0 - t;
    [
        mt * mt * p0[0] + 2.0 * mt * t * p1[0] + t * t * p2[0],
        mt * mt * p0[1] + 2.0 * mt * t * p1[1] + t * t * p2[1],
    ]
}

fn eval_cubic(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;
    [
        mt2 * mt * p0[0] + 3.0 * mt2 * t * p1[0] + 3.0 * mt * t2 * p2[0] + t2 * t * p3[0],
        mt2 * mt * p0[1] + 3.0 * mt2 * t * p1[1] + 3.0 * mt * t2 * p2[1] + t2 * t * p3[1],
    ]
}

fn point_distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn point_inside_outline(point: [f32; 2], contours: &[Vec<[f32; 2]>]) -> bool {
    let mut inside = false;
    for contour in contours {
        if contour.len() < 3 {
            continue;
        }
        let mut j = contour.len() - 1;
        for i in 0..contour.len() {
            let a = contour[i];
            let b = contour[j];
            let intersects = ((a[1] > point[1]) != (b[1] > point[1]))
                && (point[0] < (b[0] - a[0]) * (point[1] - a[1]) / ((b[1] - a[1]) + 1e-6) + a[0]);
            if intersects {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

fn encode_sdf_sample(
    point: [f32; 2],
    inside: bool,
    segments: &[FieldLineSegment],
    field_range_px: f32,
) -> [u8; 4] {
    let signed = signed_distance_to_segments(point, inside, segments.iter().copied());
    let encoded = encode_signed_distance(signed, field_range_px);
    [encoded, encoded, encoded, 255]
}

fn encode_msdf_sample(
    point: [f32; 2],
    global_inside: bool,
    segments: &[FieldLineSegment],
    field_range_px: f32,
) -> [u8; 4] {
    // Each channel gets its own signed distance determined by its own inside test.
    // This is the key property of MSDF: channels can disagree at corners, letting
    // the median function correctly reconstruct sharp edges.
    let channel_bits = [1u8, 2u8, 4u8];
    let mut result = [0u8; 4];
    result[3] = 255;

    for (ch, &bit) in channel_bits.iter().enumerate() {
        let mut min_dist = f32::INFINITY;
        let mut has_segs = false;
        for seg in segments {
            if seg.color_mask & bit != 0 {
                let d = distance_to_segment(point, seg.a, seg.b);
                if d < min_dist {
                    min_dist = d;
                }
                has_segs = true;
            }
        }
        let signed = if !has_segs {
            if global_inside {
                field_range_px
            } else {
                -field_range_px
            }
        } else {
            let ch_inside = point_inside_channel(point, segments, bit);
            if ch_inside { min_dist } else { -min_dist }
        };
        result[ch] = encode_signed_distance(signed, field_range_px);
    }

    result
}

fn point_inside_channel(point: [f32; 2], segments: &[FieldLineSegment], ch_bit: u8) -> bool {
    let mut inside = false;
    for seg in segments {
        if seg.color_mask & ch_bit == 0 {
            continue;
        }
        let a = seg.a;
        let b = seg.b;
        let intersects = ((a[1] > point[1]) != (b[1] > point[1]))
            && (point[0] < (b[0] - a[0]) * (point[1] - a[1]) / ((b[1] - a[1]) + 1e-6) + a[0]);
        if intersects {
            inside = !inside;
        }
    }
    inside
}

fn signed_distance_to_segments(
    point: [f32; 2],
    inside: bool,
    segments: impl Iterator<Item = FieldLineSegment>,
) -> f32 {
    let mut min_distance = f32::INFINITY;
    for segment in segments {
        min_distance = min_distance.min(distance_to_segment(point, segment.a, segment.b));
    }
    if inside { min_distance } else { -min_distance }
}

fn encode_signed_distance(distance: f32, field_range_px: f32) -> u8 {
    let normalized = (0.5 + 0.5 * (distance / field_range_px).clamp(-1.0, 1.0)).clamp(0.0, 1.0);
    (normalized * 255.0).round() as u8
}

fn distance_to_segment(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [point[0] - a[0], point[1] - a[1]];
    let denom = ab[0] * ab[0] + ab[1] * ab[1];
    if denom <= 1e-6 {
        return point_distance(point, a);
    }
    let t = ((ap[0] * ab[0] + ap[1] * ab[1]) / denom).clamp(0.0, 1.0);
    let closest = [a[0] + ab[0] * t, a[1] + ab[1] * t];
    point_distance(point, closest)
}

fn collect_edge_pixels(alpha: &[u8], size: [usize; 2]) -> Vec<[usize; 2]> {
    let width = size[0];
    let height = size[1];
    let mut edges = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let inside = alpha[index] >= 128;
            let mut is_edge = false;
            for [nx, ny] in neighbor_pixels(x, y, width, height) {
                if (alpha[ny * width + nx] >= 128) != inside {
                    is_edge = true;
                    break;
                }
            }
            if is_edge {
                edges.push([x, y]);
            }
        }
    }
    edges
}

fn neighbor_pixels(x: usize, y: usize, width: usize, height: usize) -> Vec<[usize; 2]> {
    let mut neighbors = Vec::with_capacity(4);
    if x > 0 {
        neighbors.push([x - 1, y]);
    }
    if x + 1 < width {
        neighbors.push([x + 1, y]);
    }
    if y > 0 {
        neighbors.push([x, y - 1]);
    }
    if y + 1 < height {
        neighbors.push([x, y + 1]);
    }
    neighbors
}

fn hash_page_bytes(size: usize, bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    size.hash(&mut hasher);
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn options_hash<H: Hasher>(options: &TextLabelOptions, state: &mut H) {
    options.font_size.to_bits().hash(state);
    options.line_height.to_bits().hash(state);
    options.color.hash(state);
    options.wrap.hash(state);
    options.monospace.hash(state);
    options.weight.hash(state);
    options.italic.hash(state);
    options.family_candidates.hash(state);
    options.fundamentals.kerning.hash(state);
    options.fundamentals.standard_ligatures.hash(state);
    options.fundamentals.contextual_alternates.hash(state);
    options.fundamentals.discretionary_ligatures.hash(state);
    options.fundamentals.historical_ligatures.hash(state);
    options.fundamentals.feature_settings.hash(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_page_side_is_clamped_to_hardware_limit() {
        let mut text = TextUi::new();
        text.begin_frame_info(TextFrameInfo::new(1, 128));
        assert_eq!(text.resolved_page_side(), 128);
    }

    #[test]
    fn atlas_page_data_reuses_cached_snapshot_until_pixels_change() {
        let mut page = AtlasPage::new(4, TextAtlasContentMode::AlphaMask);
        page.blit(&[255, 255, 255, 255], [1, 1], [0, 0]);

        let first = page.data(0);
        let second = page.data(0);
        assert!(Arc::ptr_eq(&first.rgba8, &second.rgba8));

        page.blit(&[127, 127, 127, 127], [1, 1], [1, 0]);
        let changed = page.data(0);
        assert!(!Arc::ptr_eq(&first.rgba8, &changed.rgba8));
    }
}
