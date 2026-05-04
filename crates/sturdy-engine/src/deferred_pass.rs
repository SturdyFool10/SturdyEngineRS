// Deferred G-Buffer + GGX PBR lighting pass with directional shadow map.
//
// Drop-in replacement for `Scene::draw()`. Renders all opaque scene objects
// into a four-channel G-Buffer, generates a directional shadow map, then
// evaluates GGX PBR lighting (+ PCF shadows) in a fullscreen deferred pass.
//
// Per-material shader variants:
//   When a mesh has a `UnifiedMaterial` set via `scene.set_unified_material()`,
//   `DeferredPass` compiles a unique G-Buffer fragment shader from that
//   material's expression tree and caches it by content hash. Meshes without a
//   `UnifiedMaterial` fall back to the default G-Buffer program.
//
// Usage:
//   // At init:
//   let deferred = DeferredPass::new(&engine)?;
//
//   // Each frame:
//   scene.prepare(&engine)?;
//   deferred.draw(&mut scene, view, proj, &hdr_output, &frame, &engine, time)?;

use std::collections::HashMap;
use std::path::PathBuf;

use glam::{Mat4, Vec4};

use crate::{
    Engine, Format, GraphImage, ImageDesc, ImageDimension, ImageUsage, MeshProgram,
    MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc, ShaderProgram, ShaderSource,
    ShaderStage, push_constants, scene::Scene,
    shadow_pass::{ShadowPass, ShadowConfig},
};
use crate::scene::CameraConstants;
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

/// Deferred rendering component with GGX PBR, per-material shader variants,
/// and a directional shadow map.
///
/// `DeferredPass::new()` uses sensible defaults for everything. Material
/// expression variants are compiled on first use and cached permanently.
///
/// # Usage
/// ```ignore
/// // At init:
/// let deferred = DeferredPass::new(&engine)?;
///
/// // Set expressive materials on scene meshes:
/// scene.set_unified_material(mesh_id, UnifiedMaterial::pbr_metallic_roughness("rock")
///     .base_color(MaterialExpr::texture_uv("rock_albedo", UvSource::tiled(4.0, 4.0)))
///     .roughness(MaterialExpr::texture("rock_roughness"))
///     .normal(MaterialExpr::texture_uv("rock_normal", UvSource::tiled(4.0, 4.0)))
///     .build());
///
/// // Each frame:
/// scene.prepare(&engine)?;
/// frame.bind_image("rock_albedo",    &rock_albedo_tex);
/// frame.bind_image("rock_roughness", &rock_roughness_tex);
/// frame.bind_image("rock_normal",    &rock_normal_tex);
/// deferred.draw(&mut scene, view, proj, &hdr_output, &frame, &engine, time_secs)?;
/// ```
pub struct DeferredPass {
    /// Default G-Buffer program (used for meshes without a `UnifiedMaterial`).
    default_gbuffer_program: MeshProgram,
    lighting_program: ShaderProgram,
    shadow: ShadowPass,
    /// 1×1 flat tangent-space normal texture ([0.5, 0.5, 1.0, 1.0]).
    /// Bound as the default `"normal_map"` for meshes without a normal map.
    flat_normal_map: crate::Image,
    /// Cache of compiled G-Buffer shader variants, keyed by material content hash.
    variant_cache: HashMap<u64, MeshProgram>,
}

impl DeferredPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_shadow_config(engine, ShadowConfig::default())
    }

    pub fn with_shadow_config(engine: &Engine, shadow_config: ShadowConfig) -> Result<Self> {
        let default_gbuffer_program = MeshProgram::new(
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
        // 1×1 flat normal: [128, 128, 255, 255] = tangent-space (0,0,1) = no perturbation.
        let flat_normal_map = engine.generate_texture_2d("flat_normal_map", 1, 1, |_, _| {
            [128, 128, 255, 255]
        })?;
        Ok(Self {
            default_gbuffer_program,
            lighting_program,
            shadow,
            flat_normal_map,
            variant_cache: HashMap::new(),
        })
    }

    /// Expose the shadow configuration for live tuning.
    pub fn shadow_config_mut(&mut self) -> &mut ShadowConfig {
        &mut self.shadow.config
    }

    /// Execute the full deferred frame into `output`.
    ///
    /// `time` is the elapsed time in seconds since application start. Pass
    /// `ctx.frame_time().time` from `GameContext`, or `0.0` for static scenes.
    ///
    /// Pass order:
    ///   1. Shadow map — depth-only pass from the directional light's POV
    ///   2. G-Buffer fill — write albedo/normal/roughness/emissive/world_pos
    ///      (per-mesh shader variant compiled from `UnifiedMaterial` when set)
    ///   3. Deferred lighting — GGX PBR evaluation + PCF shadow sampling
    ///   4. `output` (scene_color) → existing post-process chain unchanged
    pub fn draw(
        &mut self,
        scene: &mut Scene,
        view: Mat4,
        proj: Mat4,
        output: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
        time: f32,
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

        let color_targets: &[&GraphImage] = &[&g0, &g1, &g2, &g3];
        let primary    = color_targets[0];
        let additional = &color_targets[1..];

        // ── 5. G-Buffer fill pass (per-mesh material variants) ────────────────
        let view_proj = (proj * view).to_cols_array_2d();
        let constants = CameraConstants {
            view_proj,
            previous_view_proj: view_proj,
            time,
            _pad: [0.0; 3],
        };

        // Step A: collect (hash, source) for any materials not yet compiled.
        // Done in its own block so the borrow of `scene` ends before compilation.
        let to_compile: Vec<(u64, String)> = (0..scene.mesh_count())
            .filter_map(|idx| scene.unified_material_at(idx))
            .filter(|mat| !self.variant_cache.contains_key(&mat.content_hash()))
            .map(|mat| (mat.content_hash(), mat.generate_gbuffer_source()))
            .collect();

        // Step B: compile missing variants (no scene borrow active).
        for (key, source) in to_compile {
            let program = MeshProgram::new(
                engine,
                MeshProgramDesc {
                    fragment: ShaderDesc {
                        source: ShaderSource::Inline(source),
                        entry_point: "main".to_owned(),
                        stage: ShaderStage::Fragment,
                    },
                    vertex: None,
                    vertex_kind: MeshVertexKind::V3d,
                    alpha_blend: false,
                    uses_depth: true,
                },
            )?;
            self.variant_cache.insert(key, program);
        }

        // Step C: draw each batch.
        // All scene accesses inside the loop are &self (immutable), compatible
        // with the iterator also holding an immutable borrow of scene.batches.
        for (mesh_idx, instance_buf_opt, instance_count) in scene.drawable_batches() {
            let instance_buf = match instance_buf_opt {
                Some(b) => b,
                None => continue,
            };
            if instance_count == 0 {
                continue;
            }

            let mesh = scene.mesh_at(mesh_idx);

            // Select: per-material variant if set, otherwise default.
            let mat_hash = scene.unified_material_at(mesh_idx).map(|m| m.content_hash());
            let program: &MeshProgram = match mat_hash {
                Some(h) => self.variant_cache
                    .get(&h)
                    .unwrap_or(&self.default_gbuffer_program),
                None => &self.default_gbuffer_program,
            };

            frame.bind_buffer("instances", instance_buf);

            // Material constants buffer (for the default program path).
            if let Some(mat_buf) = scene.material_gpu_buffer_at(mesh_idx) {
                frame.bind_buffer("material_desc", mat_buf);
            }

            // Normal map: per-mesh override, or flat fallback.
            let nmap: &crate::Image = scene.normal_map_at(mesh_idx)
                .map(|arc| arc.as_ref())
                .unwrap_or(&self.flat_normal_map);
            frame.bind_image("normal_map", nmap);

            primary.draw_mesh_instanced_mrt_with_push_constants_and_depth(
                additional,
                mesh,
                program,
                instance_buf,
                instance_count,
                &constants,
                Some(&depth),
            )?;
        }

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
