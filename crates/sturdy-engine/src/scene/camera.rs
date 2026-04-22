use glam::Mat4;
use super::render_target::RenderTarget;

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
    /// Where this camera writes its rendered output.
    pub output: CameraOutput,
}

impl SceneCamera {
    pub fn offscreen(view: Mat4, projection: Mat4, target: RenderTarget) -> Self {
        Self { view, projection, output: CameraOutput::Offscreen(target) }
    }

    /// The combined view-projection matrix.
    pub fn view_proj(&self) -> Mat4 {
        self.projection * self.view
    }
}
