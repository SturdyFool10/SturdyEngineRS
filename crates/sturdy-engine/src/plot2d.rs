use crate::DebugOverlay;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotTheme {
    pub background: [f32; 4],
    pub border: [f32; 4],
    pub grid: [f32; 4],
    pub text: [f32; 4],
    pub axis: [f32; 4],
    pub palette: [[f32; 4]; 6],
}

impl Default for PlotTheme {
    fn default() -> Self {
        Self {
            background: [0.03, 0.05, 0.09, 0.86],
            border: [0.45, 0.63, 0.82, 0.9],
            grid: [0.22, 0.31, 0.43, 0.55],
            text: [0.93, 0.97, 1.0, 1.0],
            axis: [0.76, 0.88, 1.0, 0.95],
            palette: [
                [0.33, 0.79, 0.95, 1.0],
                [0.99, 0.62, 0.25, 1.0],
                [0.49, 0.85, 0.58, 1.0],
                [0.95, 0.39, 0.46, 1.0],
                [0.76, 0.56, 0.97, 1.0],
                [0.98, 0.86, 0.34, 1.0],
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlotScale {
    Linear,
    Log10,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotRange {
    pub min: f32,
    pub max: f32,
}

impl PlotRange {
    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn span(self) -> f32 {
        (self.max - self.min).max(f32::EPSILON)
    }

    pub fn pan(self, delta: f32) -> Self {
        Self {
            min: self.min + delta,
            max: self.max + delta,
        }
    }

    pub fn zoom(self, factor: f32, pivot: f32) -> Self {
        let factor = factor.max(0.01);
        let min = pivot + (self.min - pivot) / factor;
        let max = pivot + (self.max - pivot) / factor;
        Self { min, max }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotView {
    pub x: PlotRange,
    pub y: PlotRange,
    pub x_scale: PlotScale,
    pub y_scale: PlotScale,
}

impl PlotView {
    pub fn new(x: PlotRange, y: PlotRange) -> Self {
        Self {
            x,
            y,
            x_scale: PlotScale::Linear,
            y_scale: PlotScale::Linear,
        }
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.x = self.x.pan(dx);
        self.y = self.y.pan(dy);
    }

    pub fn zoom(&mut self, x_factor: f32, y_factor: f32, pivot: (f32, f32)) {
        self.x = self.x.zoom(x_factor, pivot.0);
        self.y = self.y.zoom(y_factor, pivot.1);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlotSeries {
    Line {
        name: String,
        points: Vec<[f32; 2]>,
        color: Option<[f32; 4]>,
    },
    Scatter {
        name: String,
        points: Vec<[f32; 2]>,
        color: Option<[f32; 4]>,
    },
    Bars {
        name: String,
        bars: Vec<PlotBar>,
        color: Option<[f32; 4]>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotBar {
    pub center: f32,
    pub value: f32,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotInspection {
    pub screen_pos: [f32; 2],
    pub plot_value: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct Plot2d {
    title: Option<String>,
    pub view: PlotView,
    pub theme: PlotTheme,
    series: Vec<PlotSeries>,
    legend: bool,
}

impl Plot2d {
    pub fn new(view: PlotView) -> Self {
        Self {
            title: None,
            view,
            theme: PlotTheme::default(),
            series: Vec::new(),
            legend: true,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_theme(mut self, theme: PlotTheme) -> Self {
        self.theme = theme;
        self
    }

    pub fn add_line_series(
        &mut self,
        name: impl Into<String>,
        points: impl Into<Vec<[f32; 2]>>,
    ) -> &mut Self {
        self.series.push(PlotSeries::Line {
            name: name.into(),
            points: points.into(),
            color: None,
        });
        self
    }

    pub fn add_scatter_series(
        &mut self,
        name: impl Into<String>,
        points: impl Into<Vec<[f32; 2]>>,
    ) -> &mut Self {
        self.series.push(PlotSeries::Scatter {
            name: name.into(),
            points: points.into(),
            color: None,
        });
        self
    }

    pub fn add_bar_series(
        &mut self,
        name: impl Into<String>,
        bars: impl Into<Vec<PlotBar>>,
    ) -> &mut Self {
        self.series.push(PlotSeries::Bars {
            name: name.into(),
            bars: bars.into(),
            color: None,
        });
        self
    }

    pub fn with_legend(mut self, enabled: bool) -> Self {
        self.legend = enabled;
        self
    }

    pub fn pan(&mut self, dx: f32, dy: f32) -> &mut Self {
        self.view.pan(dx, dy);
        self
    }

    pub fn zoom(&mut self, x_factor: f32, y_factor: f32, pivot: (f32, f32)) -> &mut Self {
        self.view.zoom(x_factor, y_factor, pivot);
        self
    }

    pub fn nearest_point(&self, pos: [f32; 2]) -> Option<[f32; 2]> {
        let mut best = None;
        let mut best_dist_sq = f32::INFINITY;
        for series in &self.series {
            match series {
                PlotSeries::Line { points, .. } | PlotSeries::Scatter { points, .. } => {
                    for point in points {
                        let dx = point[0] - pos[0];
                        let dy = point[1] - pos[1];
                        let dist_sq = dx * dx + dy * dy;
                        if dist_sq < best_dist_sq {
                            best_dist_sq = dist_sq;
                            best = Some(*point);
                        }
                    }
                }
                PlotSeries::Bars { bars, .. } => {
                    for bar in bars {
                        let point = [bar.center, bar.value];
                        let dx = point[0] - pos[0];
                        let dy = point[1] - pos[1];
                        let dist_sq = dx * dx + dy * dy;
                        if dist_sq < best_dist_sq {
                            best_dist_sq = dist_sq;
                            best = Some(point);
                        }
                    }
                }
            }
        }
        best
    }

    pub fn render(
        &self,
        overlay: &mut DebugOverlay,
        width: u32,
        height: u32,
        origin: [f32; 2],
        size: [f32; 2],
        inspection: Option<PlotInspection>,
    ) {
        let rect = PlotRect {
            x: origin[0],
            y: origin[1],
            w: size[0].max(1.0),
            h: size[1].max(1.0),
        };

        overlay.filled_rect_screen(width, height, [rect.x, rect.y], [rect.w, rect.h], self.theme.background);
        overlay.rectangle_screen(width, height, [rect.x, rect.y], [rect.w, rect.h]);

        let plot_area = PlotRect {
            x: rect.x + 52.0,
            y: rect.y + 28.0,
            w: (rect.w - 72.0).max(40.0),
            h: (rect.h - 66.0).max(40.0),
        };

        let mut grid_style = overlay.shapes().style();
        grid_style.color = self.theme.grid;
        grid_style.thickness = 0.0025;
        overlay.set_style(grid_style);
        for step in 1..5 {
            let x = plot_area.x + plot_area.w * step as f32 / 5.0;
            overlay.line_screen(width, height, [x, plot_area.y], [x, plot_area.y + plot_area.h]);
            let y = plot_area.y + plot_area.h * step as f32 / 5.0;
            overlay.line_screen(width, height, [plot_area.x, y], [plot_area.x + plot_area.w, y]);
        }

        let mut axis_style = overlay.shapes().style();
        axis_style.color = self.theme.axis;
        axis_style.thickness = 0.004;
        overlay.set_style(axis_style);
        overlay.line_screen(
            width,
            height,
            [plot_area.x, plot_area.y + plot_area.h],
            [plot_area.x + plot_area.w, plot_area.y + plot_area.h],
        );
        overlay.line_screen(
            width,
            height,
            [plot_area.x, plot_area.y],
            [plot_area.x, plot_area.y + plot_area.h],
        );

        for (index, series) in self.series.iter().enumerate() {
            let color = series_color(series, &self.theme, index);
            let mut style = overlay.shapes().style();
            style.color = color;
            style.thickness = 0.006;
            style.point_size = 10.0 * 2.0 / width.max(1) as f32;
            overlay.set_style(style);

            match series {
                PlotSeries::Line { points, .. } => {
                    let screen_points = points
                        .iter()
                        .filter_map(|point| map_point(self.view, plot_area, *point))
                        .collect::<Vec<_>>();
                    for segment in screen_points.windows(2) {
                        overlay.line_screen(width, height, segment[0], segment[1]);
                    }
                }
                PlotSeries::Scatter { points, .. } => {
                    for point in points {
                        if let Some(screen) = map_point(self.view, plot_area, *point) {
                            overlay.cross_marker_screen(width, height, screen, 8.0);
                        }
                    }
                }
                PlotSeries::Bars { bars, .. } => {
                    for bar in bars {
                        let left = bar.center - bar.width * 0.5;
                        let right = bar.center + bar.width * 0.5;
                        let top = bar.value;
                        let bottom = self.view.y.min.min(0.0);
                        let Some(min_screen) = map_point(self.view, plot_area, [left, bottom]) else {
                            continue;
                        };
                        let Some(max_screen) = map_point(self.view, plot_area, [right, top]) else {
                            continue;
                        };
                        overlay.filled_rect_screen(
                            width,
                            height,
                            [min_screen[0], max_screen[1]],
                            [max_screen[0] - min_screen[0], min_screen[1] - max_screen[1]],
                            [color[0], color[1], color[2], color[3] * 0.75],
                        );
                    }
                }
            }
        }

        if let Some(title) = &self.title {
            overlay.add_screen_text(title.clone(), rect.x + 14.0, rect.y + 8.0);
        }
        overlay.add_screen_text(
            format!("{:.2} .. {:.2}", self.view.x.min, self.view.x.max),
            plot_area.x + plot_area.w - 148.0,
            rect.y + rect.h - 28.0,
        );
        overlay.add_screen_text(
            format!("{:.2} .. {:.2}", self.view.y.min, self.view.y.max),
            rect.x + 10.0,
            plot_area.y + 4.0,
        );

        if self.legend {
            let mut y = rect.y + 10.0;
            for (index, series) in self.series.iter().enumerate() {
                let label = match series {
                    PlotSeries::Line { name, .. }
                    | PlotSeries::Scatter { name, .. }
                    | PlotSeries::Bars { name, .. } => name,
                };
                let color = series_color(series, &self.theme, index);
                overlay.filled_rect_screen(
                    width,
                    height,
                    [rect.x + rect.w - 164.0, y + 4.0],
                    [14.0, 14.0],
                    color,
                );
                overlay.add_screen_text(label.clone(), rect.x + rect.w - 144.0, y);
                y += 20.0;
            }
        }

        if let Some(inspection) = inspection {
            overlay.cross_marker_screen(width, height, inspection.screen_pos, 10.0);
            overlay.add_screen_text(
                format!(
                    "x={:.3} y={:.3}",
                    inspection.plot_value[0], inspection.plot_value[1]
                ),
                inspection.screen_pos[0] + 12.0,
                inspection.screen_pos[1] - 20.0,
            );
        }
    }
}

#[derive(Clone, Copy)]
struct PlotRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn series_color(series: &PlotSeries, theme: &PlotTheme, index: usize) -> [f32; 4] {
    let fallback = theme.palette[index % theme.palette.len()];
    match series {
        PlotSeries::Line { color, .. }
        | PlotSeries::Scatter { color, .. }
        | PlotSeries::Bars { color, .. } => color.unwrap_or(fallback),
    }
}

fn map_point(view: PlotView, rect: PlotRect, point: [f32; 2]) -> Option<[f32; 2]> {
    let x = normalize_value(point[0], view.x, view.x_scale)?;
    let y = normalize_value(point[1], view.y, view.y_scale)?;
    Some([rect.x + x * rect.w, rect.y + (1.0 - y) * rect.h])
}

fn normalize_value(value: f32, range: PlotRange, scale: PlotScale) -> Option<f32> {
    match scale {
        PlotScale::Linear => Some((value - range.min) / range.span()),
        PlotScale::Log10 => {
            if value <= 0.0 || range.min <= 0.0 || range.max <= 0.0 {
                None
            } else {
                let min = range.min.log10();
                let max = range.max.log10();
                Some((value.log10() - min) / (max - min).max(f32::EPSILON))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plot_pan_and_zoom_adjust_view() {
        let mut plot = Plot2d::new(PlotView::new(PlotRange::new(0.0, 10.0), PlotRange::new(-1.0, 1.0)));
        plot.pan(2.0, 0.5).zoom(2.0, 2.0, (4.0, 0.0));

        assert!(plot.view.x.min > 0.0);
        assert!(plot.view.x.max < 12.0);
        assert!(plot.view.y.min > -1.0);
        assert!(plot.view.y.max < 1.5);
    }

    #[test]
    fn nearest_point_finds_closest_series_sample() {
        let mut plot = Plot2d::new(PlotView::new(PlotRange::new(0.0, 10.0), PlotRange::new(0.0, 10.0)));
        plot.add_line_series("line", vec![[1.0, 1.0], [5.0, 5.0], [9.0, 9.0]]);

        assert_eq!(plot.nearest_point([4.7, 4.8]), Some([5.0, 5.0]));
    }

    #[test]
    fn render_populates_overlay_with_shapes_and_text() {
        let mut plot = Plot2d::new(PlotView::new(PlotRange::new(0.0, 4.0), PlotRange::new(0.0, 4.0)))
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
}
