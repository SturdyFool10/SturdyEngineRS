// Deferred G-Buffer + GGX PBR lighting pass.
//
// Drop-in replacement for `Scene::draw()`. Renders all opaque scene objects
// into a four-channel G-Buffer, then evaluates physically based lighting in a
// fullscreen deferred pass. Transparent and forward-only objects are NOT yet
// handled — forward tail will be added in Track 6d.
//
// Usage:
//
//   // At init:
//   let deferred = DeferredPass::new(&engine)?;
//
//   // Each frame (replaces scene.draw(view, proj, &hdr_output, &frame, &engine)):
//   scene.prepare(&engine)?;
//   deferred.draw(&mut scene, view, proj, &hdr_output, &frame, &engine)?;
//   // hdr_output feeds directly into the existing bloom → AA → tonemap chain.

use std::path::PathBuf;

use glam::{Mat4, Vec3, Vec4};

use crate::{
    Engine, Format, GraphImage, ImageDesc, ImageDimension, ImageUsage, MeshProgram,
    MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc, ShaderProgram,
    ShaderSource, ShaderStage, push_constants, scene::Scene,
};
use sturdy_engine_core::Extent3d;

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Push constants for the deferred lighting fullscreen pass.
#[push_constants]
struct DeferredLightingConstants {
    /// Camera world-space position (w unused). Used for Fresnel / specular V vector.
    camera_world_pos: [f32; 4],
}

/// A self-contained deferred rendering component.
///
/// Creates and manages the G-Buffer program and the deferred lighting shader.
/// The G-Buffer images are allocated per-frame from the render graph (matching
/// the output's current extent) so resize is handled automatically.
pub struct DeferredPass {
    /// MeshProgram using `gbuffer_fragment.slang` — fills the four G-Buffer targets.
    gbuffer_program: MeshProgram,
    /// Fullscreen `ShaderProgram` using `deferred_lighting.slang` — evaluates GGX PBR.
    lighting_program: ShaderProgram,
}

impl DeferredPass {
    /// Create the deferred pass and compile both shader programs.
    pub fn new(engine: &Engine) -> Result<Self> {
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
        Ok(Self { gbuffer_program, lighting_program })
    }

    /// Execute the full deferred frame into `output`.
    ///
    /// Equivalent to `scene.draw(view, proj, output, frame, engine)` but with
    /// a physically correct GGX BRDF instead of Lambert + Blinn-Phong.
    ///
    /// `output` receives the linear-HDR lit result and feeds directly into the
    /// existing post-processing pipeline (bloom → AA → tone mapping).
    ///
    /// `scene.prepare(engine)` must have been called this frame before `draw`.
    pub fn draw(
        &self,
        scene: &mut Scene,
        view: Mat4,
        proj: Mat4,
        output: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> Result<()> {
        // ── 1. Upload and register the lighting uniform ───────────────────────
        // This writes LightingUniforms to the GPU buffer and registers it as
        // "lighting" in the frame so the deferred lighting shader can read it.
        scene.prepare_deferred_lighting(view, engine, frame)?;

        // ── 2. Camera world position ──────────────────────────────────────────
        let cam_world = view.inverse() * Vec4::new(0.0, 0.0, 0.0, 1.0);

        // ── 3. Allocate G-Buffer images (frame-managed, auto-resize) ──────────
        let ext = output.desc().extent;
        let gbuffer_desc = |name: &'static str, format: Format| ImageDesc {
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

        let g0    = frame.image("gbuffer_albedo_metallic", gbuffer_desc("gbuffer_albedo_metallic", Format::Rgba8Unorm  ))?;
        let g1    = frame.image("gbuffer_normal_rough",    gbuffer_desc("gbuffer_normal_rough",    Format::Rgba16Float ))?;
        let g2    = frame.image("gbuffer_emissive",        gbuffer_desc("gbuffer_emissive",        Format::Rgba16Float ))?;
        let g3    = frame.image("gbuffer_world_pos",       gbuffer_desc("gbuffer_world_pos",       Format::Rgba16Float ))?;
        let depth = frame.image("gbuffer_depth", depth_desc)?;

        // ── 4. G-Buffer fill pass ─────────────────────────────────────────────
        scene.draw_gbuffer(
            view,
            proj,
            &[&g0, &g1, &g2, &g3],
            &depth,
            &self.gbuffer_program,
            frame,
        )?;

        // ── 5. Register G-Buffer images by name for the lighting shader ───────
        // deferred_lighting.slang declares these as named Texture2D bindings;
        // the engine resolves them from images_by_name at flush time.
        g0.register_as("gbuffer_albedo_metallic");
        g1.register_as("gbuffer_normal_rough");
        g2.register_as("gbuffer_emissive");
        g3.register_as("gbuffer_world_pos");

        // ── 6. Deferred lighting fullscreen pass → output (scene_color) ───────
        output.execute_shader_with_constants_auto(
            &self.lighting_program,
            &DeferredLightingConstants {
                camera_world_pos: [cam_world.x, cam_world.y, cam_world.z, 0.0],
            },
        )?;

        Ok(())
    }
}
