// Tests extracted from crates/clay-ui/src/layout/mosaic.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn mosaic_dense_packs_spanning_tiles_deterministically() {
    let layout = MosaicConfig::new(320.0, 4, 40.0)
        .gap(Vec2::new(8.0, 4.0))
        .tile(MosaicTileSpec::new("hero").spans(2, 2))
        .tile(MosaicTileSpec::new("a"))
        .tile(MosaicTileSpec::new("b"))
        .tile(MosaicTileSpec::new("wide").spans(3, 1))
        .layout()
        .unwrap();

    assert_eq!(layout.column_count, 4);
    assert_eq!(layout.row_count, 3);
    assert_eq!(layout.cell_size, Size::new(74.0, 40.0));
    assert_eq!(layout.tile("hero").unwrap().column, 0);
    assert_eq!(layout.tile("hero").unwrap().row, 0);
    assert_eq!(layout.tile("a").unwrap().column, 2);
    assert_eq!(layout.tile("b").unwrap().column, 3);
    assert_eq!(layout.tile("wide").unwrap().row, 2);
    assert_eq!(layout.content_size, Size::new(320.0, 128.0));
}

#[test]
fn mosaic_respects_explicit_positions_and_reports_collisions() {
    let err = MosaicConfig::new(240.0, 3, 40.0)
        .tile(MosaicTileSpec::new("fixed").spans(2, 1).at(1, 0))
        .tile(MosaicTileSpec::new("collide").at(2, 0))
        .layout()
        .unwrap_err();

    assert_eq!(
        err,
        MosaicError::Collision {
            tile: "collide".into(),
            column: 2,
            row: 0
        }
    );
}

#[test]
fn mosaic_uses_breakpoints_for_column_count() {
    let layout = MosaicConfig::new(500.0, 2, 50.0)
        .breakpoint(MosaicBreakpoint::new(400.0, 5))
        .tile(MosaicTileSpec::new("tile").spans(2, 1))
        .layout()
        .unwrap();

    assert_eq!(layout.column_count, 5);
    assert_eq!(layout.cell_size, Size::new(100.0, 50.0));
}

#[test]
fn mosaic_fit_mode_preserves_intrinsic_aspect_inside_allocated_tile() {
    let layout = MosaicConfig::new(200.0, 1, 200.0)
        .tile(
            MosaicTileSpec::new("image")
                .mode(MosaicTileMode::Fit)
                .aspect_ratio(2.0),
        )
        .layout()
        .unwrap();
    let tile = layout.tile("image").unwrap();

    assert_eq!(tile.allocated_rect, Rect::new(0.0, 0.0, 200.0, 200.0));
    assert_eq!(tile.rect, Rect::new(0.0, 50.0, 200.0, 100.0));
}

#[test]
fn mosaic_hit_testing_and_visible_tiles_are_deterministic() {
    let layout = MosaicConfig::new(200.0, 2, 50.0)
        .tile(MosaicTileSpec::new("a"))
        .tile(MosaicTileSpec::new("b"))
        .tile(MosaicTileSpec::new("c").spans(2, 1))
        .layout()
        .unwrap();

    assert_eq!(
        layout
            .hit_test(Vec2::new(150.0, 25.0))
            .map(|tile| tile.name.as_str()),
        Some("b")
    );
    let visible = layout
        .visible_tiles(Rect::new(0.0, 51.0, 200.0, 10.0), 0.0)
        .map(|tile| tile.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(visible, vec!["c"]);
}
