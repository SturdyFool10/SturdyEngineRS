// Deferred G-Buffer + GGX PBR lighting pass with directional shadow map.
//
// Drop-in replacement for `Scene::draw()`. Renders all opaque scene objects
// into a four-channel G-Buffer, generates a directional shadow map, then
// evaluates GGX PBR lighting (+ PCF shadows) in a fullscreen deferred pass.
//
// Usage:
//   // At init:
//   let deferred = DeferredPass::new(&engine)?;
//
//   // Each frame:
//   scene.prepare(&engine)?;
//   deferred.draw(&mut scene, view, proj, &hdr_output, &frame, &engine)?;

use std::path::PathBuf;

use glam::{Mat4, Vec4};

use crate::{
    Engine, Format, GraphImage, ImageDesc, ImageDimension, ImageUsage, MeshProgram,
    MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc, ShaderProgram, ShaderSource,
    ShaderStage, push_constants, scene::Scene,
    shadow_pass::{ShadowPass, ShadowConfig},
};
use sturdy_engine_core::Extent3d;

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Push constants for `deferred_lighting.slang`.
/// Must match `DeferredLightingConstants` in the shader exactly.
#[push_constants]
struct DeferredLightingConstants {
    camera_world_pos: [f32; 4],    // xyz = camera pos, w unused
    ambient: [f32; 3],             // constant ambient colour (IBL replaces this in Track 6c)
    light_count: u32,
    light_view_proj: [[f32; 4]; 4], // directional light MVP for shadow coords
    shadow_bias: f32,
    _pad: [f32; 3],
}

/// Deferred rendering component with GGX PBR and a directional shadow map.
///
/// `DeferredPass::new()` uses sensible defaults for everything. To configure
/// the shadow map (resolution, frustum size, bias), use `with_shadow_config`.
///
/// # Usage
/// ```ignore
/// // At init:
/// let deferred = DeferredPass::new(&engine)?;
///
/// // Each frame — replaces scene.draw():
/// scene.prepare(&engine)?;
/// deferred.draw(&mut scene, view, proj, &hdr_output, &frame, &engine)?;
/// ```
pub struct DeferredPass {
    gbuffer_program: MeshProgram,
    lighting_program: ShaderProgram,
    shadow: ShadowPass,
}

impl DeferredPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_shadow_config(engine, ShadowConfig::default())
    }

    pub fn with_shadow_config(engine: &Engine, shadow_config: ShadowConfig) -> Result<Self> {
        let gbuffer_program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::File(engine_shader("gbuffer_fragment.slang")),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
                uses_depth: true,
            },
        )?;
        let lighting_program = engine.load_shader(engine_shader("deferred_lighting.slang"))?;
        let shadow = ShadowPass::with_config(engine, shadow_config)?;
        Ok(Self { gbuffer_program, lighting_program, shadow })
    }

    /// Expose the shadow configuration for live tuning.
    pub fn shadow_config_mut(&mut self) -> &mut ShadowConfig {
        &mut self.shadow.config
    }

    /// Execute the full deferred frame into `output`.
    ///
    /// Pass order:
    ///   1. Shadow map — depth-only pass from the directional light's POV
    ///   2. G-Buffer fill — write albedo/normal/roughness/emissive/world_pos
    ///   3. Deferred lighting — GGX PBR evaluation + PCF shadow sampling
    ///   4. `output` (scene_color) → existing post-process chain unchanged
    pub fn draw(
        &self,
        scene: &mut Scene,
        view: Mat4,
        proj: Mat4,
        output: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> Result<()> {
        // ── 1. Upload lighting uniform + build lights buffer ──────────────────
        scene.prepare_deferred_lighting(view, engine, frame)?;

        // ── 2. Shadow map pass ────────────────────────────────────────────────
        let shadow_out = self.shadow.draw(scene, frame, engine)?;

        // ── 3. Camera world position ──────────────────────────────────────────
        let cam_world = view.inverse() * Vec4::new(0.0, 0.0, 0.0, 1.0);

        // ── 4. Allocate G-Buffer images (auto-resize) ─────────────────────────
        let ext = output.desc().extent;
        let gbuffer_img = |name: &'static str, format: Format| ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d { width: ext.width, height: ext.height, depth: 1 },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: Some(name),
        };
        let depth_desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d { width: ext.width, height: ext.height, depth: 1 },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL,
            transient: false,
            clear_value: None,
            debug_name: Some("gbuffer_depth"),
        };

        let g0    = frame.image("gbuffer_albedo_metallic", gbuffer_img("gbuffer_albedo_metallic", Format::Rgba8Unorm  ))?;
        let g1    = frame.image("gbuffer_normal_rough",    gbuffer_img("gbuffer_normal_rough",    Format::Rgba16Float ))?;
        let g2    = frame.image("gbuffer_emissive",        gbuffer_img("gbuffer_emissive",        Format::Rgba16Float ))?;
        let g3    = frame.image("gbuffer_world_pos",       gbuffer_img("gbuffer_world_pos",       Format::Rgba16Float ))?;
        let depth = frame.image("gbuffer_depth", depth_desc)?;

        // ── 5. G-Buffer fill pass ─────────────────────────────────────────────
        scene.draw_gbuffer(view, proj, &[&g0, &g1, &g2, &g3], &depth, &self.gbuffer_program, frame)?;

        // ── 6. Register G-Buffer images by name for the lighting shader ───────
        g0.register_as("gbuffer_albedo_metallic");
        g1.register_as("gbuffer_normal_rough");
        g2.register_as("gbuffer_emissive");
        g3.register_as("gbuffer_world_pos");
        // shadow_map is already registered by ShadowPass::draw()

        // ── 7. Deferred lighting fullscreen pass → output ─────────────────────
        let dl = &scene.directional_light;
        output.execute_shader_with_constants_auto(
            &self.lighting_program,
            &DeferredLightingConstants {
                camera_world_pos: [cam_world.x, cam_world.y, cam_world.z, 0.0],
                ambient: [dl.ambient.x, dl.ambient.y, dl.ambient.z],
                light_count: scene.deferred_light_count(),
                light_view_proj: shadow_out.light_view_proj.to_cols_array_2d(),
                shadow_bias: shadow_out.depth_bias,
                _pad: [0.0; 3],
            },
        )?;

        Ok(())
    }
}
