use crate::{PreparedTextDraw, PreparedTextQuad, TextEngineFrame};
use std::sync::Arc;

/// A single tile cut from a text atlas page.
#[derive(Clone, Debug)]
pub struct TiledTextAtlasPage {
    /// Stable tile index for the frame. Use this as the texture slot/page id.
    pub page_index: u32,
    /// Original atlas page index this tile came from.
    pub source_page_index: u32,
    /// Tile origin in the source atlas page, in pixels.
    pub origin_px: [u32; 2],
    /// Tile dimensions in pixels.
    pub size_px: [u32; 2],
    /// Content hash of the tile pixels.
    pub content_hash: u64,
    /// How this tile's texels should be decoded in the shader.
    pub content_mode: crate::TextAtlasContentMode,
    /// Raw RGBA8 pixel data for the tile.
    pub pixels: Arc<[u8]>,
}

/// Text output with atlas pages split to fit a device texture limit.
#[derive(Clone, Debug, Default)]
pub struct TiledTextEngineFrame {
    pub atlas_pages: Vec<TiledTextAtlasPage>,
    pub draws: Vec<PreparedTextDraw>,
}

impl TextEngineFrame {
    /// Split atlas pages into tiles that fit within `max_texture_side_px`.
    ///
    /// Any glyph quads that cross a tile boundary are clipped and duplicated so
    /// each tile can be uploaded as a standalone texture.
    pub fn tile_atlas_pages(&self, max_texture_side_px: u32) -> TiledTextEngineFrame {
        let max_texture_side_px = max_texture_side_px.max(1);
        let mut tiled = TiledTextEngineFrame::default();

        for page in &self.atlas_pages {
            let source_page_index = page.page_index;
            let page_width = page.width.max(1);
            let page_height = page.height.max(1);
            if page_width <= max_texture_side_px && page_height <= max_texture_side_px {
                let page_index = tiled.atlas_pages.len() as u32;
                tiled.atlas_pages.push(TiledTextAtlasPage {
                    page_index,
                    source_page_index,
                    origin_px: [0, 0],
                    size_px: [page_width, page_height],
                    content_hash: page.content_hash,
                    content_mode: page.content_mode,
                    pixels: Arc::clone(&page.pixels),
                });
                for draw in &self.draws {
                    let quads = draw
                        .quads
                        .iter()
                        .filter(|quad| quad.atlas_page == source_page_index)
                        .map(|quad| PreparedTextQuad {
                            atlas_page: page_index,
                            ..*quad
                        })
                        .collect::<Vec<_>>();
                    if !quads.is_empty() {
                        tiled.draws.push(PreparedTextDraw {
                            source_index: draw.source_index,
                            placement: draw.placement.clone(),
                            quads,
                        });
                    }
                }
                continue;
            }
            let tile_width = page_width.min(max_texture_side_px);
            let tile_height = page_height.min(max_texture_side_px);

            let mut tile_page_indices = Vec::new();
            for tile_y in (0..page_height).step_by(tile_height as usize) {
                for tile_x in (0..page_width).step_by(tile_width as usize) {
                    let width = tile_width.min(page_width - tile_x);
                    let height = tile_height.min(page_height - tile_y);
                    let pixels = extract_tile_pixels(
                        &page.pixels,
                        page_width as usize,
                        tile_x as usize,
                        tile_y as usize,
                        width as usize,
                        height as usize,
                    );
                    let page_index = tiled.atlas_pages.len() as u32;
                    tiled.atlas_pages.push(TiledTextAtlasPage {
                        page_index,
                        source_page_index,
                        origin_px: [tile_x, tile_y],
                        size_px: [width, height],
                        content_hash: hash_tile_identity(
                            page.content_hash,
                            tile_x,
                            tile_y,
                            width,
                            height,
                        ),
                        content_mode: page.content_mode,
                        pixels,
                    });
                    tile_page_indices.push((page_index, tile_x, tile_y, width, height));
                }
            }

            for draw in &self.draws {
                let mut tiled_draws = Vec::new();
                for quad in &draw.quads {
                    if quad.atlas_page != source_page_index {
                        continue;
                    }
                    for (page_index, tile_x, tile_y, tile_width, tile_height) in &tile_page_indices
                    {
                        if let Some(quad) = clip_quad_to_tile(
                            quad,
                            [*tile_x, *tile_y],
                            [*tile_width, *tile_height],
                            page_width,
                            page_height,
                        ) {
                            tiled_draws.push(PreparedTextQuad {
                                atlas_page: *page_index,
                                ..quad
                            });
                        }
                    }
                }
                if !tiled_draws.is_empty() {
                    tiled.draws.push(PreparedTextDraw {
                        source_index: draw.source_index,
                        placement: draw.placement.clone(),
                        quads: tiled_draws,
                    });
                }
            }
        }

        tiled
    }
}

fn extract_tile_pixels(
    src: &[u8],
    src_width: usize,
    tile_x: usize,
    tile_y: usize,
    tile_width: usize,
    tile_height: usize,
) -> Arc<[u8]> {
    let mut pixels = vec![0u8; tile_width.saturating_mul(tile_height).saturating_mul(4)];
    for row in 0..tile_height {
        let src_start = ((tile_y + row) * src_width + tile_x) * 4;
        let dst_start = row * tile_width * 4;
        let byte_count = tile_width * 4;
        pixels[dst_start..dst_start + byte_count]
            .copy_from_slice(&src[src_start..src_start + byte_count]);
    }
    pixels.into()
}

fn hash_tile_identity(
    page_content_hash: u64,
    tile_x: u32,
    tile_y: u32,
    width: u32,
    height: u32,
) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    page_content_hash.hash(&mut hasher);
    tile_x.hash(&mut hasher);
    tile_y.hash(&mut hasher);
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    hasher.finish()
}

fn clip_quad_to_tile(
    quad: &PreparedTextQuad,
    tile_origin: [u32; 2],
    tile_size: [u32; 2],
    page_width: u32,
    page_height: u32,
) -> Option<PreparedTextQuad> {
    let [u0, v0, u1, v1] = quad_uv_bounds(quad.uvs, page_width, page_height);

    let tile_x0 = tile_origin[0] as f32;
    let tile_y0 = tile_origin[1] as f32;
    let tile_x1 = tile_x0 + tile_size[0] as f32;
    let tile_y1 = tile_y0 + tile_size[1] as f32;

    let clipped_u0_px = u0.max(tile_x0);
    let clipped_v0_px = v0.max(tile_y0);
    let clipped_u1_px = u1.min(tile_x1);
    let clipped_v1_px = v1.min(tile_y1);

    if clipped_u0_px >= clipped_u1_px || clipped_v0_px >= clipped_v1_px {
        return None;
    }

    let u_span_x = (u1 - u0).max(f32::EPSILON);
    let v_span_y = (v1 - v0).max(f32::EPSILON);

    let left_t = (clipped_u0_px - u0) / u_span_x;
    let right_t = (clipped_u1_px - u0) / u_span_x;
    let top_t = (clipped_v0_px - v0) / v_span_y;
    let bottom_t = (clipped_v1_px - v0) / v_span_y;

    let clipped_top_left = interpolate_quad_position(quad.positions, left_t, top_t);
    let clipped_top_right = interpolate_quad_position(quad.positions, right_t, top_t);
    let clipped_bottom_right = interpolate_quad_position(quad.positions, right_t, bottom_t);
    let clipped_bottom_left = interpolate_quad_position(quad.positions, left_t, bottom_t);

    let tile_width = tile_size[0].max(1) as f32;
    let tile_height = tile_size[1].max(1) as f32;
    let tile_u0 = (clipped_u0_px - tile_x0).clamp(0.0, tile_width) / tile_width;
    let tile_u1 = (clipped_u1_px - tile_x0).clamp(0.0, tile_width) / tile_width;
    let tile_v0 = (clipped_v0_px - tile_y0).clamp(0.0, tile_height) / tile_height;
    let tile_v1 = (clipped_v1_px - tile_y0).clamp(0.0, tile_height) / tile_height;

    Some(PreparedTextQuad {
        positions: [
            clipped_top_left,
            clipped_top_right,
            clipped_bottom_right,
            clipped_bottom_left,
        ],
        uvs: [
            [tile_u0, tile_v0],
            [tile_u1, tile_v0],
            [tile_u1, tile_v1],
            [tile_u0, tile_v1],
        ],
        atlas_page: quad.atlas_page,
        color: quad.color,
    })
}

fn interpolate_quad_position(positions: [[f32; 3]; 4], x_t: f32, y_t: f32) -> [f32; 3] {
    let top = lerp_position(positions[0], positions[1], x_t);
    let bottom = lerp_position(positions[3], positions[2], x_t);
    lerp_position(top, bottom, y_t)
}

fn lerp_position(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn quad_uv_bounds(uvs: [[f32; 2]; 4], page_width: u32, page_height: u32) -> [f32; 4] {
    let mut min_u = f32::INFINITY;
    let mut min_v = f32::INFINITY;
    let mut max_u = f32::NEG_INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    for uv in uvs {
        min_u = min_u.min(uv[0]);
        min_v = min_v.min(uv[1]);
        max_u = max_u.max(uv[0]);
        max_v = max_v.max(uv[1]);
    }
    [
        min_u * page_width.max(1) as f32,
        min_v * page_height.max(1) as f32,
        max_u * page_width.max(1) as f32,
        max_v * page_height.max(1) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_and_clips_quads_across_boundaries() {
        let mut pixels = vec![0u8; 4 * 4 * 4];
        for (i, byte) in pixels.iter_mut().enumerate() {
            *byte = i as u8;
        }
        let frame = TextEngineFrame {
            atlas_pages: vec![crate::TextAtlasPage {
                page_index: 0,
                width: 4,
                height: 4,
                content_hash: 123,
                content_mode: crate::TextAtlasContentMode::AlphaMask,
                pixels: pixels.into(),
            }],
            draws: vec![PreparedTextDraw {
                source_index: 0,
                placement: crate::TextPlacement::default(),
                quads: vec![PreparedTextQuad {
                    positions: [
                        [1.0, 1.0, 0.0],
                        [3.0, 1.0, 0.0],
                        [3.0, 3.0, 0.0],
                        [1.0, 3.0, 0.0],
                    ],
                    uvs: [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]],
                    atlas_page: 0,
                    color: [1.0, 1.0, 1.0, 1.0],
                }],
            }],
        };

        let tiled = frame.tile_atlas_pages(2);

        assert_eq!(tiled.atlas_pages.len(), 4);
        assert_eq!(tiled.draws.len(), 1);
        assert_eq!(tiled.draws[0].quads.len(), 4);
        for quad in &tiled.draws[0].quads {
            assert!(quad.positions[0][0] >= 1.0);
            assert!(quad.positions[0][1] >= 1.0);
            assert!(quad.atlas_page < 4);
        }
    }

    #[test]
    fn tiling_clips_by_atlas_uvs_not_screen_position() {
        let pixels = vec![255u8; 4 * 4 * 4];
        let frame = TextEngineFrame {
            atlas_pages: vec![crate::TextAtlasPage {
                page_index: 0,
                width: 4,
                height: 4,
                content_hash: 123,
                content_mode: crate::TextAtlasContentMode::AlphaMask,
                pixels: pixels.into(),
            }],
            draws: vec![PreparedTextDraw {
                source_index: 0,
                placement: crate::TextPlacement::default(),
                quads: vec![PreparedTextQuad {
                    positions: [
                        [300.0, 40.0, 0.0],
                        [340.0, 40.0, 0.0],
                        [340.0, 80.0, 0.0],
                        [300.0, 80.0, 0.0],
                    ],
                    uvs: [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]],
                    atlas_page: 0,
                    color: [1.0, 1.0, 1.0, 1.0],
                }],
            }],
        };

        let tiled = frame.tile_atlas_pages(2);

        assert_eq!(tiled.draws.len(), 1);
        assert_eq!(tiled.draws[0].quads.len(), 4);
        assert!(
            tiled.draws[0]
                .quads
                .iter()
                .any(|quad| quad.positions[0][0] >= 300.0)
        );
    }

    #[test]
    fn pages_within_texture_limit_reuse_source_content_hash() {
        let pixels = vec![255u8; 4 * 4 * 4];
        let frame = TextEngineFrame {
            atlas_pages: vec![crate::TextAtlasPage {
                page_index: 4,
                width: 4,
                height: 4,
                content_hash: 9876,
                content_mode: crate::TextAtlasContentMode::AlphaMask,
                pixels: pixels.into(),
            }],
            draws: vec![PreparedTextDraw {
                source_index: 0,
                placement: crate::TextPlacement::default(),
                quads: vec![PreparedTextQuad {
                    positions: [
                        [300.0, 40.0, 0.0],
                        [340.0, 40.0, 0.0],
                        [340.0, 80.0, 0.0],
                        [300.0, 80.0, 0.0],
                    ],
                    uvs: [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]],
                    atlas_page: 4,
                    color: [1.0, 1.0, 1.0, 1.0],
                }],
            }],
        };

        let tiled = frame.tile_atlas_pages(16);

        assert_eq!(tiled.atlas_pages.len(), 1);
        assert_eq!(tiled.atlas_pages[0].content_hash, 9876);
        assert_eq!(tiled.atlas_pages[0].source_page_index, 4);
        assert_eq!(tiled.draws[0].quads[0].atlas_page, 0);
        assert_eq!(tiled.draws[0].quads[0].positions[0], [300.0, 40.0, 0.0]);
    }
}
