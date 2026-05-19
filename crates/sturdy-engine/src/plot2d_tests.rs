// Tests extracted from crates/sturdy-engine/src/plot2d.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn plot_pan_and_zoom_adjust_view() {
    let mut plot = Plot2d::new(PlotView::new(
        PlotRange::new(0.0, 10.0),
        PlotRange::new(-1.0, 1.0),
    ));
    plot.pan(2.0, 0.5).zoom(2.0, 2.0, (4.0, 0.0));

    assert!(plot.view.x.min > 0.0);
    assert!(plot.view.x.max < 12.0);
    assert!(plot.view.y.min > -1.0);
    assert!(plot.view.y.max < 1.5);
}

#[test]
fn nearest_point_finds_closest_series_sample() {
    let mut plot = Plot2d::new(PlotView::new(
        PlotRange::new(0.0, 10.0),
        PlotRange::new(0.0, 10.0),
    ));
    plot.add_line_series("line", vec![[1.0, 1.0], [5.0, 5.0], [9.0, 9.0]]);

    assert_eq!(plot.nearest_point([4.7, 4.8]), Some([5.0, 5.0]));
}

#[test]
fn render_populates_overlay_with_shapes_and_text() {
    let mut plot = Plot2d::new(PlotView::new(
        PlotRange::new(0.0, 4.0),
        PlotRange::new(0.0, 4.0),
    ))
    .title("demo");
    plot.add_line_series("trend", vec![[0.0, 0.0], [2.0, 3.0], [4.0, 2.0]]);
    plot.add_scatter_series("points", vec![[1.0, 1.5], [3.0, 2.5]]);
    plot.add_bar_series(
        "bars",
        vec![
            PlotBar {
                center: 0.5,
                value: 1.0,
                width: 0.5,
            },
            PlotBar {
                center: 3.0,
                value: 2.5,
                width: 0.75,
            },
        ],
    );

    let mut overlay = DebugOverlay::new();
    plot.render(&mut overlay, 1280, 720, [32.0, 32.0], [480.0, 320.0], None);

    assert!(!overlay.is_empty());
}
