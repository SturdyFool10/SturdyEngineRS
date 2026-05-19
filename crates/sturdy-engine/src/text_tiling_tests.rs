// Tests extracted from crates/sturdy-engine/src/text_tiling.rs
// Runtime code should stay separate from test code.

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
