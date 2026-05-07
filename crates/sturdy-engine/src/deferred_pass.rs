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
    shadow_pass::{CsmConfig, CsmPass},
    environment_map::{EnvironmentMap, compute_brdf_lut, SPECULAR_LAYER_COUNT},
    oit_pass::{OitConfig, OitPass},
    light_bvh::LightBvhBuilder,
};
use crate::scene::CameraConstants;
use sturdy_engine_core::Extent3d;

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Push constants for `deferred_lighting.slang`.
#[push_constants]
struct DeferredLightingConstants {
    camera_world_pos: [f32; 4],
    ambient: [f32; 3],
    dir_light_count: u32,
    ibl_strength: f32,
    ibl_max_layer: f32,
    bvh_root: u32,  // 0 = root present, 0xFFFFFFFF = no BVH
    _pad: f32,
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
    default_gbuffer_program: MeshProgram,
    lighting_program: ShaderProgram,
    csm: CsmPass,
    flat_normal_map: crate::Image,
    variant_cache: HashMap<u64, MeshProgram>,
    brdf_lut: crate::Image,
    environment_map: Option<EnvironmentMap>,
    black_env: crate::Image,
    zero_sh9: crate::Buffer,
    /// OIT pass for Translucent-domain materials (opt-in via `set_oit`).
    oit: Option<OitPass>,
    /// BVH over point/spot lights for O(log N) per-pixel light culling.
    light_bvh: LightBvhBuilder,
    /// 1-element zero buffer bound as `light_bvh` when no dynamic lights exist.
    empty_bvh_buf: crate::Buffer,
}

impl DeferredPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_csm_config(engine, CsmConfig::default())
    }

    pub fn with_csm_config(engine: &Engine, csm_config: CsmConfig) -> Result<Self> {
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
        let csm = CsmPass::with_config(engine, csm_config)?;

        let flat_normal_map = engine.generate_texture_2d("flat_normal_map", 1, 1, |_, _| {
            [128, 128, 255, 255]
        })?;

        let brdf_lut = compute_brdf_lut(engine)?;

        let black_env = engine.create_image(crate::ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: sturdy_engine_core::Extent3d { width: 1, height: 1, depth: 1 },
            mip_levels: 1,
            layers: SPECULAR_LAYER_COUNT as u16,
            samples: 1,
            format: crate::Format::Rgba16Float,
            usage: crate::ImageUsage::SAMPLED | crate::ImageUsage::COPY_DST,
            transient: false,
            clear_value: None,
            debug_name: Some("black_env"),
        })?;

        let zero_sh9 = engine.create_buffer(crate::BufferDesc {
            size: 9 * 16,
            usage: crate::BufferUsage::STORAGE | crate::BufferUsage::COPY_DST,
        })?;
        zero_sh9.write(0, &vec![0u8; 9 * 16])?;

        // 1-node zero BVH placeholder (32 bytes) for when no point/spot lights exist.
        let empty_bvh_buf = engine.create_buffer(crate::BufferDesc {
            size: 32,
            usage: crate::BufferUsage::STORAGE | crate::BufferUsage::COPY_DST,
        })?;
        empty_bvh_buf.write(0, &[0u8; 32])?;

        Ok(Self {
            default_gbuffer_program,
            lighting_program,
            csm,
            flat_normal_map,
            variant_cache: HashMap::new(),
            brdf_lut,
            environment_map: None,
            black_env,
            zero_sh9,
            oit: None,
            light_bvh: LightBvhBuilder::new(),
            empty_bvh_buf,
        })
    }

    /// Expose the CSM configuration for live tuning.
    pub fn csm_config_mut(&mut self) -> &mut CsmConfig {
        &mut self.csm.config
    }

    /// Attach an environment map for image-based lighting.
    ///
    /// Enables split-sum specular reflections and SH9 diffuse irradiance.
    /// Call once at init after loading the HDR:
    /// ```ignore
    /// let env = EnvironmentMap::from_hdr(&engine, "assets/outdoor.hdr")?;
    /// deferred.set_environment_map(env);
    /// ```
    pub fn set_environment_map(&mut self, env: EnvironmentMap) {
        self.environment_map = Some(env);
    }

    /// Remove the current environment map (reverts to flat ambient lighting).
    pub fn clear_environment_map(&mut self) {
        self.environment_map = None;
    }

    /// Enable order-independent transparency for `Translucent`-domain materials.
    ///
    /// Without this, translucent objects are silently skipped.
    /// ```ignore
    /// deferred.set_oit(OitPass::new(&engine)?);
    /// ```
    pub fn set_oit(&mut self, oit: OitPass) {
        self.oit = Some(oit);
    }

    /// Remove the OIT pass (translucent objects will no longer be rendered).
    pub fn clear_oit(&mut self) {
        self.oit = None;
    }

    /// Expose OIT configuration for live tuning.
    pub fn oit_config_mut(&mut self) -> Option<&mut OitConfig> {
        self.oit.as_mut().map(|o| &mut o.config)
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

        // ── 2. CSM shadow passes ──────────────────────────────────────────────
        // Extract camera near/far from the projection matrix (RH perspective).
        // proj.w_axis.z = near*far/(near-far),  proj.z_axis.z = far/(near-far)
        let (cam_near, cam_far) = extract_near_far(proj);
        let csm_out = self.csm.draw(scene, view, proj, cam_near, cam_far, frame)?;

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

        // ── 6. Register G-Buffer + shadow images for the lighting shader ──────
        g0.register_as("gbuffer_albedo_metallic");
        g1.register_as("gbuffer_normal_rough");
        g2.register_as("gbuffer_emissive");
        g3.register_as("gbuffer_world_pos");
        // shadow_map_0..3 already registered inside CsmPass::draw()

        // ── 7. Rebuild BVH if lights changed, bind IBL + BVH + run deferred ──
        // Rebuild the light BVH when the point/spot light lists change.
        let point_offset = 1u32; // directional at index 0
        let spot_offset  = point_offset + scene.point_lights.len() as u32;
        if self.light_bvh.dirty
            || scene.point_lights.len() as u32 + scene.spot_lights.len() as u32 > 0
        {
            self.light_bvh.rebuild(
                engine,
                &scene.point_lights,
                &scene.spot_lights,
                point_offset,
                spot_offset,
            )?;
        }

        let (bvh_buf, bvh_root) = if !self.light_bvh.nodes.is_empty() {
            (self.light_bvh.gpu_buffer.as_ref().unwrap(), 0u32)
        } else {
            (&self.empty_bvh_buf, crate::light_bvh::BVH_EMPTY)
        };
        frame.bind_buffer("light_bvh", bvh_buf);

        frame.bind_image("brdf_lut", &self.brdf_lut);
        frame.bind_buffer("csm_data", &self.csm.csm_buffer);

        let (ibl_strength, ibl_max_layer) = if let Some(env) = &self.environment_map {
            frame.bind_image("env_specular",    &env.specular);
            frame.bind_buffer("sh9_irradiance", &env.sh9_buffer);
            (1.0f32, (SPECULAR_LAYER_COUNT - 1) as f32)
        } else {
            frame.bind_image("env_specular",    &self.black_env);
            frame.bind_buffer("sh9_irradiance", &self.zero_sh9);
            (0.0f32, (SPECULAR_LAYER_COUNT - 1) as f32)
        };

        let dl = &scene.directional_light;
        output.execute_shader_with_constants_auto(
            &self.lighting_program,
            &DeferredLightingConstants {
                camera_world_pos: [cam_world.x, cam_world.y, cam_world.z, 0.0],
                ambient: [dl.ambient.x, dl.ambient.y, dl.ambient.z],
                dir_light_count: 1,
                ibl_strength,
                ibl_max_layer,
                bvh_root,
                _pad: 0.0,
            },
        )?;

        // ── 8. OIT — Translucent objects (Per-Pixel Linked List) ─────────────
        if let Some(oit) = &mut self.oit {
            oit.draw(scene, view, proj, output, frame, engine, time)?;
        }

        Ok(())
    }
}

/// Extract camera near and far clip planes from a RH perspective projection matrix.
fn extract_near_far(proj: Mat4) -> (f32, f32) {
    // For glam perspective_rh:  col3.z = near*far/(near-far),  col2.z = far/(near-far)
    let a = proj.z_axis.z;   // far/(near-far)
    let b = proj.w_axis.z;   // near*far/(near-far)
    // a = far/(near-far) → near-far = far/a → near = far/a + far
    // b = near*far/(near-far) = near * a_inv_... easier:
    // b/a = near → near = b/a  (note: both negative for RH looking down -Z)
    if a.abs() < 1e-7 { return (0.1, 1000.0); } // orthographic fallback
    let near = b / a;
    let far  = near * a / (a - 1.0 + 1e-7);
    (near.abs().max(0.01), far.abs().max(near.abs() + 1.0))
}
