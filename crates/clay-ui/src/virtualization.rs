use std::ops::Range;

use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualListConfig {
    pub item_count: usize,
    pub item_extent: f32,
    pub viewport_extent: f32,
    pub scroll_offset: f32,
    pub overscan_items: usize,
}

impl VirtualListConfig {
    pub fn new(
        item_count: usize,
        item_extent: f32,
        viewport_extent: f32,
        scroll_offset: f32,
    ) -> Self {
        Self {
            item_count,
            item_extent,
            viewport_extent,
            scroll_offset,
            overscan_items: 1,
        }
    }

    pub fn overscan_items(mut self, overscan_items: usize) -> Self {
        self.overscan_items = overscan_items;
        self
    }

    pub fn layout(self) -> VirtualListLayout {
        if self.item_count == 0
            || self.item_extent <= f32::EPSILON
            || self.viewport_extent <= f32::EPSILON
        {
            return VirtualListLayout {
                item_count: self.item_count,
                item_extent: self.item_extent.max(0.0),
                viewport_extent: self.viewport_extent.max(0.0),
                total_extent: 0.0,
                max_scroll_offset: 0.0,
                scroll_offset: 0.0,
                visible_range: 0..0,
                render_range: 0..0,
                before_extent: 0.0,
                after_extent: 0.0,
            };
        }

        let total_extent = self.item_count as f32 * self.item_extent;
        let max_scroll_offset = (total_extent - self.viewport_extent).max(0.0);
        let scroll_offset = self.scroll_offset.clamp(0.0, max_scroll_offset);
        let visible_start = (scroll_offset / self.item_extent).floor() as usize;
        let visible_end =
            ((scroll_offset + self.viewport_extent) / self.item_extent).ceil() as usize;
        let visible_range = visible_start.min(self.item_count)..visible_end.min(self.item_count);
        let render_range = visible_range.start.saturating_sub(self.overscan_items)
            ..visible_range
                .end
                .saturating_add(self.overscan_items)
                .min(self.item_count);
        let before_extent = render_range.start as f32 * self.item_extent;
        let after_extent = (self.item_count - render_range.end) as f32 * self.item_extent;

        VirtualListLayout {
            item_count: self.item_count,
            item_extent: self.item_extent,
            viewport_extent: self.viewport_extent,
            total_extent,
            max_scroll_offset,
            scroll_offset,
            visible_range,
            render_range,
            before_extent,
            after_extent,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualListLayout {
    pub item_count: usize,
    pub item_extent: f32,
    pub viewport_extent: f32,
    pub total_extent: f32,
    pub max_scroll_offset: f32,
    pub scroll_offset: f32,
    pub visible_range: Range<usize>,
    pub render_range: Range<usize>,
    pub before_extent: f32,
    pub after_extent: f32,
}

impl VirtualListLayout {
    pub fn is_empty(&self) -> bool {
        self.render_range.is_empty()
    }

    pub fn render_count(&self) -> usize {
        self.render_range.len()
    }

    pub fn item_offset(&self, index: usize) -> Option<f32> {
        (index < self.item_count).then_some(index as f32 * self.item_extent)
    }

    pub fn render_items(&self) -> impl Iterator<Item = VirtualItem> + '_ {
        self.render_range.clone().map(|index| VirtualItem {
            index,
            offset: index as f32 * self.item_extent,
            extent: self.item_extent,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualItem {
    pub index: usize,
    pub offset: f32,
    pub extent: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualGridConfig {
    pub item_count: usize,
    pub item_size: Vec2,
    pub viewport_size: Vec2,
    pub scroll_offset: Vec2,
    pub column_count: usize,
    pub overscan_rows: usize,
    pub overscan_columns: usize,
}

impl VirtualGridConfig {
    pub fn new(
        item_count: usize,
        item_size: Vec2,
        viewport_size: Vec2,
        scroll_offset: Vec2,
        column_count: usize,
    ) -> Self {
        Self {
            item_count,
            item_size,
            viewport_size,
            scroll_offset,
            column_count,
            overscan_rows: 1,
            overscan_columns: 1,
        }
    }

    pub fn overscan_rows(mut self, overscan_rows: usize) -> Self {
        self.overscan_rows = overscan_rows;
        self
    }

    pub fn overscan_columns(mut self, overscan_columns: usize) -> Self {
        self.overscan_columns = overscan_columns;
        self
    }

    pub fn layout(self) -> VirtualGridLayout {
        if self.item_count == 0
            || self.item_size.x <= f32::EPSILON
            || self.item_size.y <= f32::EPSILON
            || self.viewport_size.x <= f32::EPSILON
            || self.viewport_size.y <= f32::EPSILON
            || self.column_count == 0
        {
            return VirtualGridLayout {
                item_count: self.item_count,
                item_size: self.item_size.max(Vec2::ZERO),
                viewport_size: self.viewport_size.max(Vec2::ZERO),
                column_count: self.column_count,
                row_count: 0,
                content_size: Vec2::ZERO,
                max_scroll_offset: Vec2::ZERO,
                scroll_offset: Vec2::ZERO,
                visible_rows: 0..0,
                render_rows: 0..0,
                visible_columns: 0..0,
                render_columns: 0..0,
                before_rows_extent: 0.0,
                after_rows_extent: 0.0,
                before_columns_extent: 0.0,
                after_columns_extent: 0.0,
            };
        }

        let row_count = self.item_count.div_ceil(self.column_count);
        let content_size = Vec2::new(
            self.column_count as f32 * self.item_size.x,
            row_count as f32 * self.item_size.y,
        );
        let max_scroll_offset = (content_size - self.viewport_size).max(Vec2::ZERO);
        let scroll_offset = self.scroll_offset.clamp(Vec2::ZERO, max_scroll_offset);
        let visible_row_start = (scroll_offset.y / self.item_size.y).floor() as usize;
        let visible_row_end =
            ((scroll_offset.y + self.viewport_size.y) / self.item_size.y).ceil() as usize;
        let visible_column_start = (scroll_offset.x / self.item_size.x).floor() as usize;
        let visible_column_end =
            ((scroll_offset.x + self.viewport_size.x) / self.item_size.x).ceil() as usize;
        let visible_rows = visible_row_start.min(row_count)..visible_row_end.min(row_count);
        let visible_columns =
            visible_column_start.min(self.column_count)..visible_column_end.min(self.column_count);
        let render_rows = visible_rows.start.saturating_sub(self.overscan_rows)
            ..visible_rows
                .end
                .saturating_add(self.overscan_rows)
                .min(row_count);
        let render_columns = visible_columns.start.saturating_sub(self.overscan_columns)
            ..visible_columns
                .end
                .saturating_add(self.overscan_columns)
                .min(self.column_count);

        VirtualGridLayout {
            item_count: self.item_count,
            item_size: self.item_size,
            viewport_size: self.viewport_size,
            column_count: self.column_count,
            row_count,
            content_size,
            max_scroll_offset,
            scroll_offset,
            visible_rows,
            render_rows: render_rows.clone(),
            visible_columns,
            render_columns: render_columns.clone(),
            before_rows_extent: render_rows.start as f32 * self.item_size.y,
            after_rows_extent: (row_count - render_rows.end) as f32 * self.item_size.y,
            before_columns_extent: render_columns.start as f32 * self.item_size.x,
            after_columns_extent: (self.column_count - render_columns.end) as f32
                * self.item_size.x,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualGridLayout {
    pub item_count: usize,
    pub item_size: Vec2,
    pub viewport_size: Vec2,
    pub column_count: usize,
    pub row_count: usize,
    pub content_size: Vec2,
    pub max_scroll_offset: Vec2,
    pub scroll_offset: Vec2,
    pub visible_rows: Range<usize>,
    pub render_rows: Range<usize>,
    pub visible_columns: Range<usize>,
    pub render_columns: Range<usize>,
    pub before_rows_extent: f32,
    pub after_rows_extent: f32,
    pub before_columns_extent: f32,
    pub after_columns_extent: f32,
}

impl VirtualGridLayout {
    pub fn is_empty(&self) -> bool {
        self.render_rows.is_empty() || self.render_columns.is_empty()
    }

    pub fn render_count(&self) -> usize {
        self.render_items().count()
    }

    pub fn item_row_column(&self, index: usize) -> Option<(usize, usize)> {
        (index < self.item_count).then_some((index / self.column_count, index % self.column_count))
    }

    pub fn item_offset(&self, index: usize) -> Option<Vec2> {
        self.item_row_column(index).map(|(row, column)| {
            Vec2::new(
                column as f32 * self.item_size.x,
                row as f32 * self.item_size.y,
            )
        })
    }

    pub fn render_items(&self) -> impl Iterator<Item = VirtualGridItem> + '_ {
        self.render_rows.clone().flat_map(move |row| {
            self.render_columns.clone().filter_map(move |column| {
                let index = row * self.column_count + column;
                (index < self.item_count).then_some(VirtualGridItem {
                    index,
                    row,
                    column,
                    offset: Vec2::new(
                        column as f32 * self.item_size.x,
                        row as f32 * self.item_size.y,
                    ),
                    size: self.item_size,
                })
            })
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualGridItem {
    pub index: usize,
    pub row: usize,
    pub column: usize,
    pub offset: Vec2,
    pub size: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualTableConfig {
    pub row_count: usize,
    pub column_count: usize,
    pub cell_size: Vec2,
    pub viewport_size: Vec2,
    pub scroll_offset: Vec2,
    pub overscan_rows: usize,
    pub overscan_columns: usize,
}

impl VirtualTableConfig {
    pub fn new(
        row_count: usize,
        column_count: usize,
        cell_size: Vec2,
        viewport_size: Vec2,
        scroll_offset: Vec2,
    ) -> Self {
        Self {
            row_count,
            column_count,
            cell_size,
            viewport_size,
            scroll_offset,
            overscan_rows: 1,
            overscan_columns: 1,
        }
    }

    pub fn overscan_rows(mut self, overscan_rows: usize) -> Self {
        self.overscan_rows = overscan_rows;
        self
    }

    pub fn overscan_columns(mut self, overscan_columns: usize) -> Self {
        self.overscan_columns = overscan_columns;
        self
    }

    pub fn layout(self) -> VirtualTableLayout {
        let item_count = self.row_count.saturating_mul(self.column_count);
        let grid = VirtualGridConfig::new(
            item_count,
            self.cell_size,
            self.viewport_size,
            self.scroll_offset,
            self.column_count,
        )
        .overscan_rows(self.overscan_rows)
        .overscan_columns(self.overscan_columns)
        .layout();

        VirtualTableLayout {
            row_count: self.row_count.min(grid.row_count),
            column_count: self.column_count,
            cell_size: grid.item_size,
            viewport_size: grid.viewport_size,
            content_size: grid.content_size,
            max_scroll_offset: grid.max_scroll_offset,
            scroll_offset: grid.scroll_offset,
            visible_rows: grid.visible_rows,
            render_rows: grid.render_rows,
            visible_columns: grid.visible_columns,
            render_columns: grid.render_columns,
            before_rows_extent: grid.before_rows_extent,
            after_rows_extent: grid.after_rows_extent,
            before_columns_extent: grid.before_columns_extent,
            after_columns_extent: grid.after_columns_extent,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualTableLayout {
    pub row_count: usize,
    pub column_count: usize,
    pub cell_size: Vec2,
    pub viewport_size: Vec2,
    pub content_size: Vec2,
    pub max_scroll_offset: Vec2,
    pub scroll_offset: Vec2,
    pub visible_rows: Range<usize>,
    pub render_rows: Range<usize>,
    pub visible_columns: Range<usize>,
    pub render_columns: Range<usize>,
    pub before_rows_extent: f32,
    pub after_rows_extent: f32,
    pub before_columns_extent: f32,
    pub after_columns_extent: f32,
}

impl VirtualTableLayout {
    pub fn is_empty(&self) -> bool {
        self.render_rows.is_empty() || self.render_columns.is_empty()
    }

    pub fn render_count(&self) -> usize {
        self.render_cells().count()
    }

    pub fn cell_index(&self, row: usize, column: usize) -> Option<usize> {
        (row < self.row_count && column < self.column_count)
            .then_some(row * self.column_count + column)
    }

    pub fn cell_offset(&self, row: usize, column: usize) -> Option<Vec2> {
        self.cell_index(row, column).map(|_| {
            Vec2::new(
                column as f32 * self.cell_size.x,
                row as f32 * self.cell_size.y,
            )
        })
    }

    pub fn render_cells(&self) -> impl Iterator<Item = VirtualTableCell> + '_ {
        self.render_rows.clone().flat_map(move |row| {
            self.render_columns.clone().filter_map(move |column| {
                self.cell_index(row, column).map(|index| VirtualTableCell {
                    index,
                    row,
                    column,
                    offset: Vec2::new(
                        column as f32 * self.cell_size.x,
                        row as f32 * self.cell_size.y,
                    ),
                    size: self.cell_size,
                })
            })
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualTableCell {
    pub index: usize,
    pub row: usize,
    pub column: usize,
    pub offset: Vec2,
    pub size: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualTreeConfig {
    pub row_count: usize,
    pub row_extent: f32,
    pub viewport_extent: f32,
    pub scroll_offset: f32,
    pub overscan_rows: usize,
    pub indent_width: f32,
}

impl VirtualTreeConfig {
    pub fn new(
        row_count: usize,
        row_extent: f32,
        viewport_extent: f32,
        scroll_offset: f32,
    ) -> Self {
        Self {
            row_count,
            row_extent,
            viewport_extent,
            scroll_offset,
            overscan_rows: 1,
            indent_width: 16.0,
        }
    }

    pub fn overscan_rows(mut self, overscan_rows: usize) -> Self {
        self.overscan_rows = overscan_rows;
        self
    }

    pub fn indent_width(mut self, indent_width: f32) -> Self {
        self.indent_width = indent_width.max(0.0);
        self
    }

    pub fn layout(self) -> VirtualTreeLayout {
        let list = VirtualListConfig::new(
            self.row_count,
            self.row_extent,
            self.viewport_extent,
            self.scroll_offset,
        )
        .overscan_items(self.overscan_rows)
        .layout();

        VirtualTreeLayout {
            row_count: list.item_count,
            row_extent: list.item_extent,
            viewport_extent: list.viewport_extent,
            total_extent: list.total_extent,
            max_scroll_offset: list.max_scroll_offset,
            scroll_offset: list.scroll_offset,
            visible_rows: list.visible_range,
            render_rows: list.render_range,
            before_extent: list.before_extent,
            after_extent: list.after_extent,
            indent_width: self.indent_width.max(0.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualTreeLayout {
    pub row_count: usize,
    pub row_extent: f32,
    pub viewport_extent: f32,
    pub total_extent: f32,
    pub max_scroll_offset: f32,
    pub scroll_offset: f32,
    pub visible_rows: Range<usize>,
    pub render_rows: Range<usize>,
    pub before_extent: f32,
    pub after_extent: f32,
    pub indent_width: f32,
}

impl VirtualTreeLayout {
    pub fn is_empty(&self) -> bool {
        self.render_rows.is_empty()
    }

    pub fn render_count(&self) -> usize {
        self.render_rows.len()
    }

    pub fn row_offset(&self, row_index: usize) -> Option<f32> {
        (row_index < self.row_count).then_some(row_index as f32 * self.row_extent)
    }

    pub fn indent_for_depth(&self, depth: usize) -> f32 {
        depth as f32 * self.indent_width
    }

    pub fn render_rows(&self) -> impl Iterator<Item = VirtualTreeRow> + '_ {
        self.render_rows.clone().map(|row_index| VirtualTreeRow {
            row_index,
            offset: row_index as f32 * self.row_extent,
            extent: self.row_extent,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VirtualTreeRow {
    pub row_index: usize,
    pub offset: f32,
    pub extent: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_list_returns_visible_and_overscanned_ranges() {
        let layout = VirtualListConfig::new(100, 20.0, 100.0, 45.0)
            .overscan_items(2)
            .layout();

        assert_eq!(layout.visible_range, 2..8);
        assert_eq!(layout.render_range, 0..10);
        assert_eq!(layout.before_extent, 0.0);
        assert_eq!(layout.after_extent, 1800.0);
        assert_eq!(layout.render_count(), 10);
    }

    #[test]
    fn virtual_list_clamps_scroll_to_content_bounds() {
        let layout = VirtualListConfig::new(10, 12.0, 48.0, 500.0)
            .overscan_items(1)
            .layout();

        assert_eq!(layout.max_scroll_offset, 72.0);
        assert_eq!(layout.scroll_offset, 72.0);
        assert_eq!(layout.visible_range, 6..10);
        assert_eq!(layout.render_range, 5..10);
        assert_eq!(layout.before_extent, 60.0);
        assert_eq!(layout.after_extent, 0.0);
    }

    #[test]
    fn virtual_list_handles_empty_or_invalid_inputs() {
        let empty = VirtualListConfig::new(0, 20.0, 100.0, 0.0).layout();
        let invalid_extent = VirtualListConfig::new(10, 0.0, 100.0, 0.0).layout();
        let invalid_viewport = VirtualListConfig::new(10, 20.0, 0.0, 0.0).layout();

        assert!(empty.is_empty());
        assert!(invalid_extent.is_empty());
        assert!(invalid_viewport.is_empty());
    }

    #[test]
    fn virtual_list_items_report_absolute_offsets() {
        let layout = VirtualListConfig::new(20, 16.0, 64.0, 48.0)
            .overscan_items(1)
            .layout();

        let items = layout.render_items().collect::<Vec<_>>();

        assert_eq!(layout.visible_range, 3..7);
        assert_eq!(
            items.first().copied(),
            Some(VirtualItem {
                index: 2,
                offset: 32.0,
                extent: 16.0,
            })
        );
        assert_eq!(layout.item_offset(6), Some(96.0));
        assert_eq!(layout.item_offset(20), None);
    }

    #[test]
    fn virtual_grid_returns_visible_and_overscanned_ranges() {
        let layout = VirtualGridConfig::new(
            100,
            Vec2::new(20.0, 10.0),
            Vec2::new(60.0, 30.0),
            Vec2::new(25.0, 15.0),
            5,
        )
        .overscan_rows(1)
        .overscan_columns(1)
        .layout();

        assert_eq!(layout.visible_rows, 1..5);
        assert_eq!(layout.render_rows, 0..6);
        assert_eq!(layout.visible_columns, 1..5);
        assert_eq!(layout.render_columns, 0..5);
        assert_eq!(layout.before_rows_extent, 0.0);
        assert_eq!(layout.after_rows_extent, 140.0);
        assert_eq!(layout.render_count(), 30);
    }

    #[test]
    fn virtual_grid_clamps_scroll_and_reports_offsets() {
        let layout = VirtualGridConfig::new(
            10,
            Vec2::new(12.0, 8.0),
            Vec2::new(24.0, 16.0),
            Vec2::new(500.0, 500.0),
            3,
        )
        .overscan_rows(0)
        .overscan_columns(0)
        .layout();

        assert_eq!(layout.row_count, 4);
        assert_eq!(layout.max_scroll_offset, Vec2::new(12.0, 16.0));
        assert_eq!(layout.scroll_offset, Vec2::new(12.0, 16.0));
        assert_eq!(layout.visible_rows, 2..4);
        assert_eq!(layout.visible_columns, 1..3);
        assert_eq!(layout.item_offset(7), Some(Vec2::new(12.0, 16.0)));
        assert_eq!(layout.item_row_column(10), None);
    }

    #[test]
    fn virtual_grid_handles_empty_or_invalid_inputs() {
        let empty = VirtualGridConfig::new(
            0,
            Vec2::new(20.0, 10.0),
            Vec2::new(60.0, 30.0),
            Vec2::ZERO,
            4,
        )
        .layout();
        let invalid_columns = VirtualGridConfig::new(
            10,
            Vec2::new(20.0, 10.0),
            Vec2::new(60.0, 30.0),
            Vec2::ZERO,
            0,
        )
        .layout();

        assert!(empty.is_empty());
        assert!(invalid_columns.is_empty());
    }

    #[test]
    fn virtual_table_returns_visible_and_overscanned_cells() {
        let layout = VirtualTableConfig::new(
            100,
            8,
            Vec2::new(20.0, 12.0),
            Vec2::new(60.0, 36.0),
            Vec2::new(25.0, 18.0),
        )
        .overscan_rows(1)
        .overscan_columns(1)
        .layout();

        assert_eq!(layout.visible_rows, 1..5);
        assert_eq!(layout.render_rows, 0..6);
        assert_eq!(layout.visible_columns, 1..5);
        assert_eq!(layout.render_columns, 0..6);
        assert_eq!(layout.render_count(), 36);
        assert_eq!(layout.cell_index(2, 3), Some(19));
        assert_eq!(layout.cell_offset(2, 3), Some(Vec2::new(60.0, 24.0)));
    }

    #[test]
    fn virtual_table_handles_empty_or_invalid_inputs() {
        let empty_rows = VirtualTableConfig::new(
            0,
            4,
            Vec2::new(20.0, 12.0),
            Vec2::new(60.0, 36.0),
            Vec2::ZERO,
        )
        .layout();
        let empty_columns = VirtualTableConfig::new(
            4,
            0,
            Vec2::new(20.0, 12.0),
            Vec2::new(60.0, 36.0),
            Vec2::ZERO,
        )
        .layout();

        assert!(empty_rows.is_empty());
        assert!(empty_columns.is_empty());
    }

    #[test]
    fn virtual_tree_wraps_list_virtualization_with_indent_metrics() {
        let layout = VirtualTreeConfig::new(50, 18.0, 72.0, 45.0)
            .overscan_rows(2)
            .indent_width(14.0)
            .layout();

        assert_eq!(layout.visible_rows, 2..7);
        assert_eq!(layout.render_rows, 0..9);
        assert_eq!(layout.render_count(), 9);
        assert_eq!(layout.row_offset(4), Some(72.0));
        assert_eq!(layout.row_offset(50), None);
        assert_eq!(layout.indent_for_depth(3), 42.0);
        assert_eq!(
            layout.render_rows().next(),
            Some(VirtualTreeRow {
                row_index: 0,
                offset: 0.0,
                extent: 18.0
            })
        );
    }
}
