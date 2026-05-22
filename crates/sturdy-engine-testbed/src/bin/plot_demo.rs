use sturdy_engine::{
    AppRuntime, AppRuntimeFrame, DebugOverlay, DebugOverlayRenderer, Plot2d, PlotBar,
    PlotInspection, PlotRange, PlotView, Result, RuntimeApp, WindowConfig, run_with_runtime,
};

struct PlotDemo {
    overlay: DebugOverlayRenderer,
    plot: Plot2d,
}

impl RuntimeApp for PlotDemo {
    type Error = sturdy_engine::Error;

    fn init(runtime: &mut AppRuntime) -> Result<Self> {
        let engine = runtime.engine();
        let mut plot = Plot2d::new(PlotView::new(
            PlotRange::new(0.0, 6.0),
            PlotRange::new(0.0, 10.0),
        ))
        .title("Plot Demo");
        plot.add_line_series(
            "trend",
            vec![
                [0.0, 1.0],
                [1.0, 2.5],
                [2.0, 3.5],
                [3.0, 5.0],
                [4.0, 6.2],
                [5.0, 7.8],
            ],
        );
        plot.add_scatter_series("samples", vec![[0.5, 1.4], [2.4, 4.2], [4.8, 7.1]]);
        plot.add_bar_series(
            "bars",
            vec![
                PlotBar {
                    center: 0.5,
                    value: 1.3,
                    width: 0.35,
                },
                PlotBar {
                    center: 2.0,
                    value: 3.8,
                    width: 0.4,
                },
                PlotBar {
                    center: 4.5,
                    value: 6.6,
                    width: 0.5,
                },
            ],
        );

        Ok(Self {
            overlay: DebugOverlayRenderer::new(engine)?,
            plot,
        })
    }

    fn update(&mut self, appframe: &mut AppRuntimeFrame<'_>) -> Result<()> {
        let shell_frame = appframe.shell_frame();
        let surface_image = appframe.surface_image();
        let ext = surface_image.desc().extent;
        let swapchain = shell_frame.inner().swapchain_image(surface_image)?;
        let frame = shell_frame.inner();
        let mut overlay = DebugOverlay::new();
        let inspection_point = self.plot.nearest_point([2.4, 4.2]).unwrap_or([0.0, 0.0]);
        self.plot.render(
            &mut overlay,
            ext.width,
            ext.height,
            [36.0, 36.0],
            [ext.width as f32 - 72.0, ext.height as f32 - 72.0],
            Some(PlotInspection {
                screen_pos: [420.0, 240.0],
                plot_value: inspection_point,
            }),
        );
        self.overlay
            .draw(frame, &swapchain, ext.width, ext.height, &overlay)?;
        frame.present_image(&swapchain)?;
        Ok(())
    }
}

fn main() {
    run_with_runtime::<PlotDemo>(
        WindowConfig::new("SturdyEngine Plot Demo", 1024, 720).with_resizable(true),
    );
}
