use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Color, Family, FeatureTag, FontFeatures, FontSystem, Metrics, Shaping,
    Style as FontStyle, SwashCache, SwashContent, SwashImage, Weight, Wrap,
};

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
    cursor_x: usize,
    cursor_y: usize,
    row_h: usize,
    content_hash: u64,
}

impl AtlasPage {
    fn new(size: usize) -> Self {
        Self {
            size,
            rgba8: vec![0; size.saturating_mul(size).saturating_mul(4)],
            cursor_x: 0,
            cursor_y: 0,
            row_h: 0,
            content_hash: 0,
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
    }

    fn data(&self, page_index: usize) -> TextAtlasPageData {
        TextAtlasPageData {
            page_index,
            size_px: [self.size, self.size],
            content_hash: self.content_hash,
            rgba8: Arc::from(self.rgba8.as_slice()),
        }
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
            .iter()
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

    fn resolve_glyph(&mut self, cache_key: CacheKey) -> Option<AtlasEntry> {
        if let Some(entry) = self.entries.get_mut(&cache_key) {
            entry.last_used_frame = self.current_frame;
            return Some(entry.clone());
        }

        let image = self
            .swash_cache
            .get_image(&mut self.font_system, cache_key)
            .clone()?;
        let rgba = swash_image_to_rgba8(
            &image,
            self.resolved_raster_mode(cache_key),
            self.graphics_config.rasterization.field_range_px.max(1.0),
        )?;
        let size = [
            image.placement.width as usize,
            image.placement.height as usize,
        ];
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
        let (page_index, min) = self.allocate_slot(page_side, alloc_size)?;
        self.pages[page_index].blit(&rgba, size, [min[0] + padding, min[1] + padding]);

        let entry = AtlasEntry {
            page_index,
            min_px: [min[0] + padding, min[1] + padding],
            size_px: size,
            placement_left_px: image.placement.left,
            placement_top_px: image.placement.top,
            last_used_frame: self.current_frame,
            approx_bytes: rgba.len(),
        };
        self.cached_bytes = self.cached_bytes.saturating_add(entry.approx_bytes);
        self.entries.insert(cache_key, entry.clone());
        self.evict_to_budget();
        Some(entry)
    }

    fn allocate_slot(&mut self, page_side: usize, size: [usize; 2]) -> Option<(usize, [usize; 2])> {
        for (page_index, page) in self.pages.iter_mut().enumerate() {
            if page.size == page_side
                && let Some(pos) = page.allocate(size[0], size[1])
            {
                return Some((page_index, pos));
            }
        }
        let mut page = AtlasPage::new(page_side);
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
                if font_size >= 28.0 {
                    TextGlyphRasterMode::Sdf
                } else {
                    TextGlyphRasterMode::AlphaMask
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
        self.clear_atlas();
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
) -> Option<Vec<u8>> {
    let width = image.placement.width as usize;
    let height = image.placement.height as usize;
    if width == 0 || height == 0 {
        return None;
    }
    match image.content {
        SwashContent::Mask => {
            if matches!(mode, TextGlyphRasterMode::Sdf | TextGlyphRasterMode::Msdf) {
                return Some(alpha_mask_to_distance_field(
                    &image.data,
                    [width, height],
                    field_range_px,
                    matches!(mode, TextGlyphRasterMode::Msdf),
                ));
            }
            let mut out = Vec::with_capacity(width * height * 4);
            for alpha in &image.data {
                out.extend_from_slice(&[255, 255, 255, *alpha]);
            }
            Some(out)
        }
        SwashContent::SubpixelMask | SwashContent::Color => Some(image.data.clone()),
    }
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
}
