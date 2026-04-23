use super::render_target::RenderTarget;
use glam::{Mat4, Vec2};

/// Unique identifier for a camera within a [`Scene`](super::Scene).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct CameraId(pub(super) u32);

/// Where a camera writes its output during `Scene::render`.
pub enum CameraOutput {
    /// Renders into a persistent offscreen [`RenderTarget`].
    ///
    /// After this camera's pass executes, the target image is available in the
    /// frame under `target.name()` so any downstream shader can sample it by name.
    /// This is how CRT screens, portal cameras, and reflection probes work.
    Offscreen(RenderTarget),
}

/// A view into the scene from a specific position and projection.
///
/// For cameras whose output is a frame-managed image (e.g. the main HDR buffer),
/// use [`Scene::render_camera_to`] and pass the `GraphImage` directly.
pub struct SceneCamera {
    /// View matrix (world → camera space).
    pub view: Mat4,
    /// Projection matrix (camera → clip space).
    pub projection: Mat4,
    /// Previous jittered view-projection matrix, used for motion-vector passes.
    pub previous_view_proj: Mat4,
    /// Current projection jitter in UV units.
    pub jitter_uv: Vec2,
    /// Previous projection jitter in UV units.
    pub previous_jitter_uv: Vec2,
    /// Where this camera writes its rendered output.
    pub output: CameraOutput,
}

impl SceneCamera {
    pub fn offscreen(view: Mat4, projection: Mat4, target: RenderTarget) -> Self {
        Self {
            view,
            projection,
            previous_view_proj: projection * view,
            jitter_uv: Vec2::ZERO,
            previous_jitter_uv: Vec2::ZERO,
            output: CameraOutput::Offscreen(target),
        }
    }

    pub fn offscreen_msaa(view: Mat4, projection: Mat4, target: RenderTarget) -> Self {
        Self::offscreen(view, projection, target)
    }

    /// The combined view-projection matrix.
    pub fn view_proj(&self) -> Mat4 {
        self.jittered_projection() * self.view
    }

    pub fn unjittered_view_proj(&self) -> Mat4 {
        self.projection * self.view
    }

    pub fn jittered_projection(&self) -> Mat4 {
        let mut projection = self.projection;
        projection.w_axis.x += self.jitter_uv.x * 2.0;
        projection.w_axis.y += self.jitter_uv.y * 2.0;
        projection
    }

    pub fn set_jitter_uv(&mut self, jitter_uv: Vec2) {
        self.previous_view_proj = self.view_proj();
        self.previous_jitter_uv = self.jitter_uv;
        self.jitter_uv = jitter_uv;
    }

    pub fn set_jitter_pixels(&mut self, jitter_pixels: Vec2, width: u32, height: u32) {
        let extent = Vec2::new(width.max(1) as f32, height.max(1) as f32);
        self.set_jitter_uv(jitter_pixels / extent);
    }

    pub fn advance_taa_jitter(&mut self, frame_index: u64, width: u32, height: u32) {
        let jitter = crate::taa_jitter_uv(frame_index, width, height);
        self.set_jitter_uv(Vec2::new(jitter[0], jitter[1]));
    }
}
