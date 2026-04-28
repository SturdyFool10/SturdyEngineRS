use clay_ui::Rect;
use sturdy_engine::{
    DebugOverlay, DebugOverlayRenderer, Engine, EngineApp, RenderTargetPx, Result, ShellFrame,
    Surface, SurfaceImage, UiPx, WindowConfig, WindowLogicalPx, render_target_to_uv, ui_to_surface,
    window_logical_to_surface,
};

struct CoordinateValidationApp {
    overlay: DebugOverlayRenderer,
}

impl EngineApp for CoordinateValidationApp {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, _surface: &Surface) -> Result<Self> {
        Ok(Self {
            overlay: DebugOverlayRenderer::new(engine)?,
        })
    }

    fn render(&mut self, frame: &mut ShellFrame<'_>, surface_image: &SurfaceImage) -> Result<()> {
        let ext = surface_image.desc().extent;
        let swapchain = frame.inner().swapchain_image(surface_image)?;
        let mut overlay = DebugOverlay::new();

        draw_coordinate_validation_scene(&mut overlay, ext.width, ext.height);

        self.overlay
            .draw(frame.inner(), &swapchain, ext.width, ext.height, &overlay)?;
        frame.inner().present_image(&swapchain)?;
        Ok(())
    }

    fn resize(&mut self, _width: u32, _height: u32) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ValidationScene {
    full_target: Rect,
    content: Rect,
    clip_rect: Rect,
    cursor: WindowLogicalPx,
    edge_markers: [WindowLogicalPx; 8],
    uv_samples: [RenderTargetPx; 5],
}

impl ValidationScene {
    fn new(width: u32, height: u32) -> Self {
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        let inset = 24.0_f32.min(w * 0.1).min(h * 0.1);
        let content = Rect::new(
            inset,
            inset,
            (w - inset * 2.0).max(1.0),
            (h - inset * 2.0).max(1.0),
        );
        let clip_rect = Rect::new(
            content.origin.x + content.size.width * 0.18,
            content.origin.y + content.size.height * 0.18,
            content.size.width * 0.36,
            content.size.height * 0.36,
        );

        Self {
            full_target: Rect::new(0.0, 0.0, w, h),
            content,
            clip_rect,
            cursor: WindowLogicalPx::new(w * 0.72, h * 0.64),
            edge_markers: [
                WindowLogicalPx::new(0.0, 0.0),
                WindowLogicalPx::new(w, 0.0),
                WindowLogicalPx::new(0.0, h),
                WindowLogicalPx::new(w, h),
                WindowLogicalPx::new(w * 0.5, 0.0),
                WindowLogicalPx::new(w * 0.5, h),
                WindowLogicalPx::new(0.0, h * 0.5),
                WindowLogicalPx::new(w, h * 0.5),
            ],
            uv_samples: [
                RenderTargetPx::new(0.0, 0.0),
                RenderTargetPx::new(w, 0.0),
                RenderTargetPx::new(0.0, h),
                RenderTargetPx::new(w, h),
                RenderTargetPx::new(w * 0.5, h * 0.5),
            ],
        }
    }

    fn marker_draw_pos(self, marker: WindowLogicalPx) -> [f32; 2] {
        let max = self.full_target.max_exclusive();
        [
            marker.x.clamp(0.0, (max.x - 1.0).max(0.0)),
            marker.y.clamp(0.0, (max.y - 1.0).max(0.0)),
        ]
    }
}

fn draw_coordinate_validation_scene(overlay: &mut DebugOverlay, width: u32, height: u32) {
    let scene = ValidationScene::new(width, height);
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;

    overlay.filled_rect_screen(
        width,
        height,
        [0.0, 0.0],
        [w, h],
        [0.015, 0.017, 0.022, 1.0],
    );

    draw_full_target_edges(overlay, width, height, scene);
    draw_corner_and_edge_markers(overlay, width, height, scene);
    draw_clipped_ui_target(overlay, width, height, scene);
    draw_cursor_marker(overlay, width, height, scene);
    draw_uv_samples(overlay, width, height, scene);
}

fn draw_full_target_edges(
    overlay: &mut DebugOverlay,
    width: u32,
    height: u32,
    scene: ValidationScene,
) {
    let r = scene.content;
    overlay.rectangle_screen(
        width,
        height,
        [r.origin.x, r.origin.y],
        [r.size.width, r.size.height],
    );

    let edge = scene.full_target.max_exclusive();
    overlay.add_screen_text("0,0 top-left", 12.0, 10.0);
    overlay.add_screen_text(
        format!("width,height edge ({:.0},{:.0})", edge.x, edge.y),
        12.0,
        36.0,
    );
}

fn draw_corner_and_edge_markers(
    overlay: &mut DebugOverlay,
    width: u32,
    height: u32,
    scene: ValidationScene,
) {
    for marker in scene.edge_markers {
        overlay.cross_marker_screen(width, height, scene.marker_draw_pos(marker), 11.0);
    }
}

fn draw_clipped_ui_target(
    overlay: &mut DebugOverlay,
    width: u32,
    height: u32,
    scene: ValidationScene,
) {
    let clip = scene.clip_rect;
    overlay.filled_rect_screen(
        width,
        height,
        [clip.origin.x, clip.origin.y],
        [clip.size.width, clip.size.height],
        [0.1, 0.28, 0.65, 0.35],
    );
    overlay.rounded_rectangle_outline_screen(
        width,
        height,
        [clip.origin.x, clip.origin.y],
        [clip.size.width, clip.size.height],
        0.0,
        2.0,
        [0.35, 0.65, 1.0, 0.9],
    );

    let clipped_child = Rect::new(
        clip.origin.x + clip.size.width * 0.65,
        clip.origin.y + clip.size.height * 0.65,
        clip.size.width * 0.5,
        clip.size.height * 0.5,
    );
    let visible = intersect_rects(clip, clipped_child);
    overlay.filled_rect_screen(
        width,
        height,
        [visible.origin.x, visible.origin.y],
        [visible.size.width, visible.size.height],
        [0.95, 0.72, 0.18, 0.75],
    );
    overlay.add_screen_text("clipped UI", clip.origin.x + 8.0, clip.origin.y + 8.0);
}

fn draw_cursor_marker(overlay: &mut DebugOverlay, width: u32, height: u32, scene: ValidationScene) {
    let surface = window_logical_to_surface(scene.cursor, 1.0);
    let ui_surface = ui_to_surface(UiPx::new(scene.cursor.x, scene.cursor.y), 1.0);
    debug_assert_eq!(surface, ui_surface);

    overlay.cross_marker_screen(width, height, [surface.x, surface.y], 18.0);
    overlay.add_screen_text(
        format!("cursor ({:.0},{:.0})", scene.cursor.x, scene.cursor.y),
        surface.x + 12.0,
        surface.y + 12.0,
    );
}

fn draw_uv_samples(overlay: &mut DebugOverlay, width: u32, height: u32, scene: ValidationScene) {
    let swatch_size = 22.0;
    let colors = [
        [1.0, 0.18, 0.22, 1.0],
        [0.25, 0.95, 0.35, 1.0],
        [0.25, 0.48, 1.0, 1.0],
        [1.0, 0.88, 0.18, 1.0],
        [1.0, 1.0, 1.0, 1.0],
    ];

    for (sample, color) in scene.uv_samples.into_iter().zip(colors) {
        let uv = render_target_to_uv(sample, width.max(1), height.max(1));
        let origin = [
            (uv.u * width.max(1) as f32 - swatch_size * 0.5)
                .clamp(0.0, (width.max(1) as f32 - swatch_size).max(0.0)),
            (uv.v * height.max(1) as f32 - swatch_size * 0.5)
                .clamp(0.0, (height.max(1) as f32 - swatch_size).max(0.0)),
        ];
        overlay.filled_rect_screen(width, height, origin, [swatch_size, swatch_size], color);
    }
}

fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let left = a.origin.x.max(b.origin.x);
    let top = a.origin.y.max(b.origin.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

fn main() {
    sturdy_engine::run::<CoordinateValidationApp>(
        WindowConfig::new("SturdyEngine Coordinate Validation", 960, 640).with_resizable(true),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_uses_bottom_right_edge_not_last_pixel_as_target_max() {
        let scene = ValidationScene::new(640, 360);

        assert_eq!(
            scene.full_target.max_exclusive(),
            glam::Vec2::new(640.0, 360.0)
        );
        assert!(scene.full_target.contains(glam::Vec2::new(639.0, 359.0)));
        assert!(!scene.full_target.contains(glam::Vec2::new(640.0, 360.0)));
        assert_eq!(
            scene.marker_draw_pos(WindowLogicalPx::new(640.0, 360.0)),
            [639.0, 359.0]
        );
    }

    #[test]
    fn scene_uv_samples_cover_corners_edges_and_center() {
        let scene = ValidationScene::new(640, 360);
        let samples = scene
            .uv_samples
            .map(|sample| render_target_to_uv(sample, 640, 360).to_vec2());

        assert_eq!(samples[0], glam::Vec2::new(0.0, 0.0));
        assert_eq!(samples[1], glam::Vec2::new(1.0, 0.0));
        assert_eq!(samples[2], glam::Vec2::new(0.0, 1.0));
        assert_eq!(samples[3], glam::Vec2::new(1.0, 1.0));
        assert_eq!(samples[4], glam::Vec2::new(0.5, 0.5));
    }

    #[test]
    fn clipped_ui_child_is_limited_to_clip_rect() {
        let scene = ValidationScene::new(640, 360);
        let child = Rect::new(
            scene.clip_rect.origin.x + scene.clip_rect.size.width * 0.65,
            scene.clip_rect.origin.y + scene.clip_rect.size.height * 0.65,
            scene.clip_rect.size.width * 0.5,
            scene.clip_rect.size.height * 0.5,
        );
        let visible = intersect_rects(scene.clip_rect, child);

        assert!(scene.clip_rect.contains(visible.origin));
        assert_eq!(visible.max_exclusive(), scene.clip_rect.max_exclusive());
    }
}
