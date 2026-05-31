// Camera-local motion vector pass.
//
// Generates a per-pixel UV-space velocity image from G-Buffer depth and the
// current SceneCamera (which carries previous-frame matrices and jitter state).
//
// Only camera motion is captured.  Objects that moved between frames will have
// motion vectors reflecting camera reprojection only, not their true velocity.
// G-Buffer MRT motion vectors (output from the vertex shader alongside the
// G-Buffer fill) are needed for correct per-object temporal reprojection and
// will be added in a follow-up pass.

use crate::{
    Engine, Format, GraphImage, ImageDesc, ImageDimension, ImageUsage, RenderFrame, Result,
    SamplerPreset, ShaderProgram, StageMask, shader_program::builtin_shader_path, scene::SceneCamera,
};
use sturdy_engine_core::Extent3d;

/// Generates camera-local motion vectors from G-Buffer depth.
///
/// Obtain one via [`CameraMotionVectorPass::new`] and attach it to
/// [`DeferredPass`](crate::DeferredPass) with
/// `deferred.set_camera_motion_vectors(pass)`.  The pass then runs
/// automatically inside [`DeferredPass::draw_with_camera`].
///
/// The output is an `Rgba16Float` image whose `.xy` channels contain
/// `prev_uv_unjittered - curr_uv_unjittered` in screen UV space `[0, 1]`.
/// This matches the reprojection convention used by `AntiAliasingPass` (TAA)
/// and `SrdDenoiser`.
pub struct CameraMotionVectorPass {
    program: ShaderProgram,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraMotionConstants {
    inv_proj: [[f32; 4]; 4],
    inv_view: [[f32; 4]; 4],
    prev_view_proj: [[f32; 4]; 4],
    jitter_curr: [f32; 2],
    jitter_prev: [f32; 2],
}

impl CameraMotionVectorPass {
    /// Compile the camera motion vector shader.
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            program: ShaderProgram::load_fragment(
                engine,
                builtin_shader_path("camera_motion_vector.slang"),
            )?,
        })
    }

    /// Compute camera-local motion vectors from G-Buffer depth.
    ///
    /// # Arguments
    /// * `frame`  — current render frame
    /// * `depth`  — G-Buffer depth image (typically `"gbuffer_depth"`)
    /// * `camera` — current `SceneCamera` with `previous_view_proj` and jitter set
    ///              via [`SceneCamera::advance_taa_jitter`] or [`SceneCamera::set_jitter_uv`]
    ///
    /// Returns an `Rgba16Float` image registered as `"camera_motion_vectors"`.
    /// The `.xy` channels contain the UV-space velocity; `.zw` are zero.
    pub fn execute(
        &self,
        frame: &RenderFrame,
        depth: &GraphImage,
        camera: &SceneCamera,
    ) -> Result<GraphImage> {
        let src = depth.desc();
        let output_desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: src.extent.width,
                height: src.extent.height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba16Float,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: Some("camera_motion_vectors"),
            compression: Default::default(),
            min_lod_bits: None,
            msaa_resolve_to_single_sampled: false,
            drm_format_modifier: None,
        };
        let output = frame.image("camera_motion_vectors", output_desc)?;

        depth.register_as("gbuffer_depth");
        frame.set_sampler("depth_sampler", SamplerPreset::PixelArt);

        let jitter_curr: [f32; 2] = camera.jitter_uv.into();
        let jitter_prev: [f32; 2] = camera.previous_jitter_uv.into();

        let constants = CameraMotionConstants {
            inv_proj: camera.projection.inverse().to_cols_array_2d(),
            inv_view: camera.view.inverse().to_cols_array_2d(),
            prev_view_proj: camera.previous_view_proj.to_cols_array_2d(),
            jitter_curr,
            jitter_prev,
        };

        output.execute_shader_with_push_constants(
            &self.program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants),
        )?;

        Ok(output)
    }
}
