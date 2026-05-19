// Tests extracted from crates/clay-ui/src/layout/virtualization.rs
// Runtime code should stay separate from test code.

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
