// Tests extracted from crates/sturdy-engine/src/text_engine.rs
// Runtime code should stay separate from test code.

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

#[test]
fn snaps_screen_space_text_placement_to_pixels() {
    let mut renderer = StaticRenderer::default();
    renderer.output = Some(TextLayoutOutput {
        scene: TextScene {
            quads: vec![TextSceneQuad {
                source_index: 0,
                positions: [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                atlas_page: 0,
                color: [1.0, 1.0, 1.0, 1.0],
            }],
            ..TextScene::default()
        },
        ..TextLayoutOutput::default()
    });
    let desc =
        TextDrawDesc::new("snap").placement(TextPlacement::Screen2d { x: 10.49, y: 20.51 });
    let mut engine = TextEngine::new(renderer);

    let frame = engine.prepare_frame(&[desc], 800, 600);

    assert_eq!(frame.draws[0].quads[0].positions[0], [10.0, 21.0, 0.0]);
    assert_eq!(frame.draws[0].quads[0].positions[2], [20.0, 31.0, 0.0]);
}

#[test]
fn snaps_alpha_mask_screen_glyph_edges_to_pixels() {
    let mut renderer = StaticRenderer::default();
    renderer.output = Some(TextLayoutOutput {
        scene: TextScene {
            quads: vec![TextSceneQuad {
                source_index: 0,
                positions: [[0.25, 0.4], [9.75, 0.4], [9.75, 10.6], [0.25, 10.6]],
                uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                atlas_page: 0,
                color: [1.0, 1.0, 1.0, 1.0],
            }],
            ..TextScene::default()
        },
        ..TextLayoutOutput::default()
    });
    let desc = TextDrawDesc::new("small")
        .font_size(14.0)
        .placement(TextPlacement::Screen2d { x: 10.2, y: 20.2 });
    let mut engine = TextEngine::new(renderer);

    let frame = engine.prepare_frame(&[desc], 800, 600);

    assert_eq!(frame.draws[0].quads[0].positions[0], [10.0, 20.0, 0.0]);
    assert_eq!(frame.draws[0].quads[0].positions[2], [20.0, 31.0, 0.0]);
}

#[test]
fn preserves_sdf_screen_glyph_subpixel_edges() {
    let mut renderer = StaticRenderer::default();
    renderer.output = Some(TextLayoutOutput {
        scene: TextScene {
            quads: vec![TextSceneQuad {
                source_index: 0,
                positions: [[0.25, 0.4], [9.75, 0.4], [9.75, 10.6], [0.25, 10.6]],
                uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                atlas_page: 0,
                color: [1.0, 1.0, 1.0, 1.0],
            }],
            ..TextScene::default()
        },
        ..TextLayoutOutput::default()
    });
    let desc = TextDrawDesc::new("large")
        .font_size(48.0)
        .placement(TextPlacement::Screen2d { x: 10.2, y: 20.2 });
    let mut engine = TextEngine::new(renderer);

    let frame = engine.prepare_frame(&[desc], 800, 600);

    assert_eq!(frame.draws[0].quads[0].positions[0], [10.25, 20.4, 0.0]);
    assert_eq!(frame.draws[0].quads[0].positions[2], [19.75, 30.6, 0.0]);
}

#[test]
fn large_screen_text_does_not_use_msdf_until_ui_path_is_validated() {
    let desc = TextDrawDesc::new("large").font_size(48.0);

    assert!(!should_use_msdf(&desc));
    assert_eq!(text_raster_path(&desc), TextRasterPath::Sdf);
}

#[test]
fn small_screen_text_uses_alpha_masks_for_stable_pixel_alignment() {
    let desc = TextDrawDesc::new("small").font_size(14.0);

    assert_eq!(text_raster_path(&desc), TextRasterPath::AlphaMask);
}

#[test]
fn world_text_uses_msdf_for_scalable_transforms() {
    let desc = TextDrawDesc::new("world").world_3d(
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        16.0,
        false,
    );

    assert_eq!(text_raster_path(&desc), TextRasterPath::Msdf);
}
