// Directional shadow map pass.
//
// Renders all opaque scene meshes into a depth-only texture from the
// directional light's point of view. The resulting shadow map is sampled
// in `deferred_lighting.slang` with a 3×3 PCF kernel.
//
// Shadow map resolution: 2048×2048 by default (overridable via `ShadowConfig`).
//
// Light frustum: orthographic box. The default bounds (±`half_size` world units,
// depth range [near, far]) cover a typical game scene centred at the world
// origin. For tight fitting to the camera frustum use CSM (Track 6e follow-up).
//
// Roadmap: Track 6e — Shadow system.

use std::path::PathBuf;

use glam::{Mat4, Vec3};

use crate::{
    Engine, Format, GraphImage, ImageDesc, ImageDimension, ImageUsage, MeshProgram, MeshProgramDesc,
    MeshVertexKind, RenderFrame, Result, ShaderDesc, ShaderSource, ShaderStage, push_constants,
    scene::Scene,
};
use sturdy_engine_core::Extent3d;

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Configuration for the directional shadow map.
///
/// `ShadowConfig::default()` gives a 2048×2048 map covering a 100×100 unit
/// world-space area — correct for medium-scale outdoor scenes.
#[derive(Clone, Debug)]
pub struct ShadowConfig {
    /// Shadow map resolution (square). Default 2048.
    pub resolution: u32,
    /// Half-size of the orthographic projection in world units. Default 50.0.
    pub half_size: f32,
    /// Near plane of the light frustum. Default 1.0.
    pub near: f32,
    /// Far plane of the light frustum. Default 200.0.
    pub far: f32,
    /// Constant depth bias to prevent self-shadowing. Default 0.005.
    pub depth_bias: f32,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            resolution: 2048,
            half_size: 50.0,
            near: 1.0,
            far: 200.0,
            depth_bias: 0.005,
        }
    }
}

/// Push constants for `shadow_depth.slang`.
#[push_constants]
struct ShadowDepthConstants {
    light_view_proj: [[f32; 4]; 4],
}

/// Output of a shadow pass — the shadow map image and the light matrix.
pub struct ShadowOutput {
    /// The shadow depth image registered as `"shadow_map"` in the frame.
    pub image: GraphImage,
    /// Combined light view-projection matrix. Pass to the deferred lighting push constants.
    pub light_view_proj: Mat4,
    /// Depth bias configured for this pass.
    pub depth_bias: f32,
}

/// Directional shadow map component.
///
/// Creates the shadow MeshProgram once at init. Call `draw()` each frame
/// before `DeferredPass::draw()` to get a `ShadowOutput`.
///
/// # Usage
/// ```ignore
/// // At init:
/// let shadow = ShadowPass::new(&engine)?;
///
/// // Each frame (call before DeferredPass::draw):
/// let shadow_out = shadow.draw(&mut scene, &frame, &engine)?;
/// // shadow_out is used by DeferredPass to bind the shadow map.
/// ```
pub struct ShadowPass {
    depth_program: MeshProgram,
    pub config: ShadowConfig,
}

impl ShadowPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_config(engine, ShadowConfig::default())
    }

    pub fn with_config(engine: &Engine, config: ShadowConfig) -> Result<Self> {
        let depth_program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    // Depth-only passes don't need a fragment shader — the vertex
                    // shader alone writes depth. Use an empty inline fragment.
                    source: ShaderSource::Inline("float4 main() : SV_TARGET { return float4(0,0,0,0); }".into()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: Some(ShaderDesc {
                    source: ShaderSource::File(engine_shader("shadow_depth.slang")),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Vertex,
                }),
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
                uses_depth: true,
            },
        )?;
        Ok(Self { depth_program, config })
    }

    /// Compute the light view-projection matrix from the scene's directional light.
    ///
    /// The light "looks at" the world origin from a point along the reverse
    /// of the light direction, using a fixed orthographic frustum. CSM with
    /// tight frustum fitting is a Track 6e follow-up.
    pub fn light_matrix(&self, light_direction: Vec3) -> Mat4 {
        // Stable up vector: avoid degenerate case when light points straight down.
        let up = if light_direction.y.abs() < 0.99 { Vec3::Y } else { Vec3::Z };
        let eye = -(light_direction.normalize()) * (self.config.far * 0.5);
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, up);
        let hs = self.config.half_size;
        let proj = Mat4::orthographic_rh(-hs, hs, -hs, hs, self.config.near, self.config.far);
        proj * view
    }

    /// Render all opaque scene objects into the shadow depth map.
    ///
    /// Returns a `ShadowOutput` containing the shadow image (registered as
    /// `"shadow_map"` in the frame) and the light matrix to pass to the
    /// deferred lighting pass.
    pub fn draw(
        &self,
        scene: &Scene,
        frame: &RenderFrame,
        _engine: &Engine,
    ) -> Result<ShadowOutput> {
        let res = self.config.resolution;
        let shadow_desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d { width: res, height: res, depth: 1 },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL | ImageUsage::SAMPLED,
            transient: false,
            clear_value: None,
            debug_name: Some("shadow_map"),
        };
        let shadow_image = frame.image("shadow_map", shadow_desc)?;

        let light_view_proj = self.light_matrix(scene.directional_light.direction);
        let constants = ShadowDepthConstants {
            light_view_proj: light_view_proj.to_cols_array_2d(),
        };

        // Draw all opaque batches into the shadow depth map.
        for (mesh_idx, instance_buf_opt, instance_count) in scene.drawable_batches() {
            let instance_buf = match instance_buf_opt {
                Some(b) => b,
                None => continue,
            };
            if instance_count == 0 {
                continue;
            }
            let mesh = scene.mesh_at(mesh_idx);
            frame.bind_buffer("instances", instance_buf);
            shadow_image.draw_mesh_depth_only_with_push_constants(
                mesh,
                &self.depth_program,
                instance_buf,
                instance_count,
                &constants,
            )?;
        }

        shadow_image.register_as("shadow_map");

        Ok(ShadowOutput {
            image: shadow_image,
            light_view_proj,
            depth_bias: self.config.depth_bias,
        })
    }
}
