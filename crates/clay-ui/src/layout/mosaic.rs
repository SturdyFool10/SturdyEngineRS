use glam::Vec2;

use crate::{Rect, Size};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MosaicTileMode {
    Span,
    Fit,
    Fill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MosaicPlacement {
    Auto,
    At { column: usize, row: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MosaicTileSpec {
    pub name: String,
    pub column_span: usize,
    pub row_span: usize,
    pub mode: MosaicTileMode,
    pub placement: MosaicPlacement,
    pub aspect_ratio: Option<f32>,
}

impl MosaicTileSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            column_span: 1,
            row_span: 1,
            mode: MosaicTileMode::Span,
            placement: MosaicPlacement::Auto,
            aspect_ratio: None,
        }
    }

    pub fn spans(mut self, column_span: usize, row_span: usize) -> Self {
        self.column_span = column_span.max(1);
        self.row_span = row_span.max(1);
        self
    }

    pub fn mode(mut self, mode: MosaicTileMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn at(mut self, column: usize, row: usize) -> Self {
        self.placement = MosaicPlacement::At { column, row };
        self
    }

    pub fn aspect_ratio(mut self, aspect_ratio: f32) -> Self {
        self.aspect_ratio = (aspect_ratio > f32::EPSILON).then_some(aspect_ratio);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MosaicBreakpoint {
    pub min_width: f32,
    pub column_count: usize,
}

impl MosaicBreakpoint {
    pub fn new(min_width: f32, column_count: usize) -> Self {
        Self {
            min_width: min_width.max(0.0),
            column_count: column_count.max(1),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MosaicConfig {
    pub width: f32,
    pub column_count: usize,
    pub cell_height: f32,
    pub gap: Vec2,
    pub breakpoints: Vec<MosaicBreakpoint>,
    pub tiles: Vec<MosaicTileSpec>,
}

impl MosaicConfig {
    pub fn new(width: f32, column_count: usize, cell_height: f32) -> Self {
        Self {
            width,
            column_count,
            cell_height,
            gap: Vec2::ZERO,
            breakpoints: Vec::new(),
            tiles: Vec::new(),
        }
    }

    pub fn gap(mut self, gap: Vec2) -> Self {
        self.gap = gap.max(Vec2::ZERO);
        self
    }

    pub fn breakpoint(mut self, breakpoint: MosaicBreakpoint) -> Self {
        self.breakpoints.push(breakpoint);
        self.breakpoints
            .sort_by(|left, right| left.min_width.total_cmp(&right.min_width));
        self
    }

    pub fn tile(mut self, tile: MosaicTileSpec) -> Self {
        self.tiles.push(tile);
        self
    }

    pub fn resolved_column_count(&self) -> usize {
        self.breakpoints
            .iter()
            .filter(|breakpoint| self.width >= breakpoint.min_width)
            .map(|breakpoint| breakpoint.column_count)
            .last()
            .unwrap_or(self.column_count)
            .max(1)
    }

    pub fn layout(&self) -> Result<MosaicLayout, MosaicError> {
        let column_count = self.resolved_column_count();
        if self.width <= f32::EPSILON {
            return Err(MosaicError::InvalidWidth);
        }
        if self.cell_height <= f32::EPSILON {
            return Err(MosaicError::InvalidCellHeight);
        }

        let gap = self.gap.max(Vec2::ZERO);
        let total_gap = gap.x * column_count.saturating_sub(1) as f32;
        let cell_width = ((self.width - total_gap) / column_count as f32).max(0.0);
        let cell_size = Size::new(cell_width, self.cell_height);
        let mut occupancy = MosaicOccupancy::new(column_count);
        let mut tiles = Vec::with_capacity(self.tiles.len());

        for (source_index, tile) in self.tiles.iter().enumerate() {
            let column_span = tile.column_span.max(1);
            let row_span = tile.row_span.max(1);
            if column_span > column_count {
                return Err(MosaicError::ColumnSpanTooWide {
                    tile: tile.name.clone(),
                    column_span,
                    column_count,
                });
            }

            let (column, row) = match tile.placement {
                MosaicPlacement::Auto => occupancy.find_open(column_span, row_span),
                MosaicPlacement::At { column, row } => {
                    if column + column_span > column_count {
                        return Err(MosaicError::ColumnSpanTooWide {
                            tile: tile.name.clone(),
                            column_span: column + column_span,
                            column_count,
                        });
                    }
                    if !occupancy.is_free(column, row, column_span, row_span) {
                        return Err(MosaicError::Collision {
                            tile: tile.name.clone(),
                            column,
                            row,
                        });
                    }
                    (column, row)
                }
            };

            occupancy.reserve(column, row, column_span, row_span);
            let allocated_rect = grid_rect(column, row, column_span, row_span, cell_size, gap);
            let rect = tile_rect(allocated_rect, tile.mode, tile.aspect_ratio);
            tiles.push(MosaicTileLayout {
                name: tile.name.clone(),
                source_index,
                column,
                row,
                column_span,
                row_span,
                mode: tile.mode,
                allocated_rect,
                rect,
            });
        }

        let row_count = occupancy.row_count();
        let height = if row_count == 0 {
            0.0
        } else {
            row_count as f32 * cell_size.height + row_count.saturating_sub(1) as f32 * gap.y
        };

        Ok(MosaicLayout {
            column_count,
            row_count,
            cell_size,
            gap,
            content_size: Size::new(self.width, height),
            tiles,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MosaicError {
    InvalidWidth,
    InvalidCellHeight,
    ColumnSpanTooWide {
        tile: String,
        column_span: usize,
        column_count: usize,
    },
    Collision {
        tile: String,
        column: usize,
        row: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MosaicLayout {
    pub column_count: usize,
    pub row_count: usize,
    pub cell_size: Size,
    pub gap: Vec2,
    pub content_size: Size,
    pub tiles: Vec<MosaicTileLayout>,
}

impl MosaicLayout {
    pub fn tile(&self, name: &str) -> Option<&MosaicTileLayout> {
        self.tiles.iter().find(|tile| tile.name == name)
    }

    pub fn hit_test(&self, point: Vec2) -> Option<&MosaicTileLayout> {
        self.tiles
            .iter()
            .rev()
            .find(|tile| tile.rect.contains(point))
    }

    pub fn visible_tiles(
        &self,
        viewport: Rect,
        overscan: f32,
    ) -> impl Iterator<Item = &MosaicTileLayout> {
        let viewport = Rect::new(
            viewport.origin.x - overscan,
            viewport.origin.y - overscan,
            viewport.size.width + overscan * 2.0,
            viewport.size.height + overscan * 2.0,
        );
        self.tiles
            .iter()
            .filter(move |tile| rects_overlap(tile.rect, viewport))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MosaicTileLayout {
    pub name: String,
    pub source_index: usize,
    pub column: usize,
    pub row: usize,
    pub column_span: usize,
    pub row_span: usize,
    pub mode: MosaicTileMode,
    pub allocated_rect: Rect,
    pub rect: Rect,
}

#[derive(Clone, Debug)]
struct MosaicOccupancy {
    column_count: usize,
    rows: Vec<Vec<bool>>,
}

impl MosaicOccupancy {
    fn new(column_count: usize) -> Self {
        Self {
            column_count,
            rows: Vec::new(),
        }
    }

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn find_open(&mut self, column_span: usize, row_span: usize) -> (usize, usize) {
        let mut row = 0usize;
        loop {
            self.ensure_rows(row + row_span);
            for column in 0..=self.column_count - column_span {
                if self.is_free(column, row, column_span, row_span) {
                    return (column, row);
                }
            }
            row += 1;
        }
    }

    fn is_free(&mut self, column: usize, row: usize, column_span: usize, row_span: usize) -> bool {
        self.ensure_rows(row + row_span);
        (row..row + row_span).all(|check_row| {
            (column..column + column_span).all(|check_column| !self.rows[check_row][check_column])
        })
    }

    fn reserve(&mut self, column: usize, row: usize, column_span: usize, row_span: usize) {
        self.ensure_rows(row + row_span);
        for check_row in row..row + row_span {
            for check_column in column..column + column_span {
                self.rows[check_row][check_column] = true;
            }
        }
    }

    fn ensure_rows(&mut self, rows: usize) {
        while self.rows.len() < rows {
            self.rows.push(vec![false; self.column_count]);
        }
    }
}

fn grid_rect(
    column: usize,
    row: usize,
    column_span: usize,
    row_span: usize,
    cell_size: Size,
    gap: Vec2,
) -> Rect {
    let x = column as f32 * (cell_size.width + gap.x);
    let y = row as f32 * (cell_size.height + gap.y);
    let width = column_span as f32 * cell_size.width + column_span.saturating_sub(1) as f32 * gap.x;
    let height = row_span as f32 * cell_size.height + row_span.saturating_sub(1) as f32 * gap.y;
    Rect::new(x, y, width, height)
}

fn tile_rect(allocated: Rect, mode: MosaicTileMode, aspect_ratio: Option<f32>) -> Rect {
    let Some(aspect_ratio) = aspect_ratio else {
        return allocated;
    };
    if mode != MosaicTileMode::Fit || aspect_ratio <= f32::EPSILON {
        return allocated;
    }

    let allocated_size = allocated.size.to_vec2();
    let fitted_width = allocated_size.x.min(allocated_size.y * aspect_ratio);
    let fitted_height = (fitted_width / aspect_ratio).min(allocated_size.y);
    let size = Vec2::new(fitted_width, fitted_height);
    let origin = allocated.origin + (allocated_size - size) * 0.5;
    Rect {
        origin,
        size: Size::new(size.x, size.y),
    }
}

fn rects_overlap(left: Rect, right: Rect) -> bool {
    left.origin.x < right.right()
        && left.right() > right.origin.x
        && left.origin.y < right.bottom()
        && left.bottom() > right.origin.y
}

#[cfg(test)]
mod tests {
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
}
