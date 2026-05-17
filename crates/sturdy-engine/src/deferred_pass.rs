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

mod config;
mod constants;
mod helpers;
mod hot_reload;
mod oit;
mod shadows;

use std::collections::HashMap;

use glam::{Mat4, Vec4};

pub use config::{RenderPath, SkyConfig};
use constants::{DeferredLightingConstants, ForwardLightingUniforms};
use helpers::{engine_shader, extract_near_far};

use crate::scene::CameraConstants;
use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, Format, GraphImage, ImageDesc, ImageDimension,
    ImageUsage, MeshProgram, MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc,
    ShaderProgram, ShaderSource, ShaderStage,
    ao_pass::AoPass,
    environment_map::{EnvironmentMap, SPECULAR_LAYER_COUNT, compute_brdf_lut, compute_e_avg_lut},
    light_bvh::LightBvhBuilder,
    oit_pass::OitPass,
    scene::Scene,
    shadow_pass::{CsmConfig, CsmPass},
};
use sturdy_engine_core::Extent3d;

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
    /// Forward-lit opaque program used when `render_path == ForwardOnly`.
    forward_opaque_program: MeshProgram,
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
    /// Procedural sky rendered into background pixels.
    pub sky: SkyConfig,
    /// Material variants that failed to compile: hash → error message.
    /// Prevents re-attempting the same broken material every frame.
    failed_variants: HashMap<u64, String>,
    /// BVH over point/spot lights for O(log N) per-pixel light culling.
    light_bvh: LightBvhBuilder,
    /// 1-element zero buffer bound as `light_bvh` when no dynamic lights exist.
    empty_bvh_buf: crate::Buffer,
    /// Uniform buffer (48 bytes) bound as `"forward_lighting"` for the OIT and ForwardOnly passes.
    forward_lighting_buf: Buffer,
    /// E_avg(roughness) LUT — 128 float32 values; hemispherical average single-scatter
    /// reflectance for F0=1. Used by the multi-scatter IBL compensation term.
    e_avg_buf: Buffer,
    /// Selects the rendering path for opaque geometry. Default `DeferredThenForward`.
    pub render_path: RenderPath,
    /// Optional spot light shadow pass. Attach via `set_spot_shadows`.
    spot_shadows: Option<crate::SpotShadowPass>,
    /// Zero-filled spot shadow data buffer bound when no spot shadows are active.
    empty_spot_shadow_buf: Buffer,
    /// 1×1 depth image bound as a placeholder for spot/point shadow maps when disabled.
    black_spot_depth: crate::Image,
    /// Optional point light shadow pass (dual-paraboloid). Attach via `set_point_shadows`.
    point_shadows: Option<crate::PointShadowPass>,
    /// Optional screen-space/ray-traced ambient occlusion pass.
    ao: Option<AoPass>,
    /// 1x1 white AO fallback bound as `gtao_result` when AO is disabled.
    white_ao: crate::Image,
    /// Zero-filled point shadow data buffer bound when no point shadows are active.
    empty_point_shadow_buf: Buffer,
    /// Watches the engine shader files and drives hot-reload + cache invalidation.
    shader_watcher: crate::ShaderWatcher,
    /// Pending environment map being blended toward. `None` when not blending.
    blend_target: Option<EnvironmentMap>,
    /// Current blend alpha [0, 1]. 0 = fully current, 1 = fully target.
    blend_alpha: f32,
    /// Alpha increment per frame (1 / blend_frames). 0 when not blending.
    blend_step: f32,
    /// Scratch GPU buffer for CPU-blended SH9 coefficients during a transition.
    blend_sh9_buf: crate::Buffer,
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
                    requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
                uses_depth: true,
            },
        )?;
        let forward_opaque_program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::File(engine_shader("forward_opaque.slang")),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                    requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
                uses_depth: true,
            },
        )?;
        let lighting_program = engine.load_shader(engine_shader("deferred_lighting.slang"))?;
        let csm = CsmPass::with_config(engine, csm_config)?;

        let flat_normal_map =
            engine.generate_texture_2d("flat_normal_map", 1, 1, |_, _| [128, 128, 255, 255])?;

        let brdf_lut = compute_brdf_lut(engine)?;
        let white_ao = engine.generate_texture_2d("white_ao", 1, 1, |_, _| [255, 255, 255, 255])?;

        let black_env = engine.create_image(crate::ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: sturdy_engine_core::Extent3d {
                width: 1,
                height: 1,
                depth: 1,
            },
            mip_levels: 1,
            layers: SPECULAR_LAYER_COUNT as u16,
            samples: 1,
            format: crate::Format::Rgba16Float,
            usage: crate::ImageUsage::SAMPLED | crate::ImageUsage::COPY_DST,
            transient: false,
            clear_value: None,
            debug_name: Some("black_env"),
                compression: Default::default(), min_lod_bits: None, msaa_resolve_to_single_sampled: false,
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

        let forward_lighting_buf = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<ForwardLightingUniforms>() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;

        // Zero-filled spot shadow buffer for when no spot shadows are active.
        let spot_data_size = std::mem::size_of::<crate::GpuSpotShadowData>() as u64;
        let empty_spot_shadow_buf = engine.create_buffer(BufferDesc {
            size: spot_data_size,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        empty_spot_shadow_buf.write(0, &vec![0u8; spot_data_size as usize])?;

        // Zero-filled point shadow buffer for when no point shadows are active.
        let pt_data_size = std::mem::size_of::<crate::GpuPointShadowData>() as u64;
        let empty_point_shadow_buf = engine.create_buffer(BufferDesc {
            size: pt_data_size,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        empty_point_shadow_buf.write(0, &vec![0u8; pt_data_size as usize])?;

        // 1×1 Depth32Float placeholder for unbound spot shadow slots.
        let black_spot_depth = engine.create_image(crate::ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: sturdy_engine_core::Extent3d {
                width: 1,
                height: 1,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: crate::Format::Depth32Float,
            usage: crate::ImageUsage::DEPTH_STENCIL | crate::ImageUsage::SAMPLED,
            transient: false,
            clear_value: None,
            debug_name: Some("black_spot_depth"),
                compression: Default::default(), min_lod_bits: None, msaa_resolve_to_single_sampled: false,
        })?;

        let e_avg_buf = compute_e_avg_lut(engine)?;

        // Auto-create the default studio environment so PBR materials look
        // reasonable without the user loading an HDR file. Overridable via
        // `set_environment_map`.
        let default_env = EnvironmentMap::studio(engine)?;

        let blend_sh9_buf = engine.create_buffer(crate::BufferDesc {
            size: 9 * 16, // 9 × float4
            usage: crate::BufferUsage::STORAGE | crate::BufferUsage::COPY_DST,
        })?;
        blend_sh9_buf.write(0, &vec![0u8; 9 * 16])?;

        // Pre-register all engine shader files so changes are caught without
        // any additional user setup.
        let mut shader_watcher = crate::ShaderWatcher::new();
        for name in &[
            "deferred_lighting.slang",
            "gbuffer_fragment.slang",
            "forward_opaque.slang",
            "brdf.slang",
            "material_surface.slang",
        ] {
            shader_watcher.watch(engine_shader(name));
        }

        Ok(Self {
            default_gbuffer_program,
            forward_opaque_program,
            lighting_program,
            csm,
            flat_normal_map,
            variant_cache: HashMap::new(),
            failed_variants: HashMap::new(),
            brdf_lut,
            environment_map: Some(default_env),
            black_env,
            zero_sh9,
            oit: None,
            light_bvh: LightBvhBuilder::new(),
            empty_bvh_buf,
            forward_lighting_buf,
            e_avg_buf,
            sky: SkyConfig::default(),
            render_path: RenderPath::DeferredThenForward,
            shader_watcher,
            blend_target: None,
            blend_alpha: 0.0,
            blend_step: 0.0,
            blend_sh9_buf,
            spot_shadows: None,
            empty_spot_shadow_buf,
            black_spot_depth,
            point_shadows: None,
            ao: None,
            white_ao,
            empty_point_shadow_buf,
        })
    }

    /// Attach an ambient occlusion pass. The deferred lighting shader samples
    /// the resulting texture as `gtao_result`.
    pub fn set_ao(&mut self, ao: AoPass) {
        self.ao = Some(ao);
    }

    /// Detach ambient occlusion and return to the white no-occlusion fallback.
    pub fn clear_ao(&mut self) {
        self.ao = None;
    }

    /// Mutable access to the attached AO pass, if any, for runtime tuning.
    pub fn ao_mut(&mut self) -> Option<&mut AoPass> {
        self.ao.as_mut()
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
        self.blend_target = None;
        self.blend_alpha = 0.0;
        self.blend_step = 0.0;
    }

    /// Begin a smooth transition to `target` over `frames` rendered frames.
    ///
    /// While the transition runs, SH9 diffuse irradiance is linearly interpolated
    /// on the CPU each frame. The specular env switches at the midpoint (alpha = 0.5).
    /// Call `set_environment_map` instead for an instant swap.
    ///
    /// # Example
    /// ```ignore
    /// let outdoor = EnvironmentMap::from_hdr(&engine, "outdoor.hdr")?;
    /// deferred.blend_to_environment_map(outdoor, 90);  // ~1.5 s at 60 fps
    /// ```
    pub fn blend_to_environment_map(&mut self, target: EnvironmentMap, frames: u32) {
        let frames = frames.max(1);
        self.blend_target = Some(target);
        self.blend_alpha = 0.0;
        self.blend_step = 1.0 / frames as f32;
    }

    /// Returns `true` if a blend transition is currently in progress.
    pub fn is_blending(&self) -> bool {
        self.blend_target.is_some()
    }

    /// Returns the current blend alpha [0, 1] (0 = source, 1 = target).
    pub fn blend_alpha(&self) -> f32 {
        self.blend_alpha
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
        self.csm.draw(scene, view, proj, cam_near, cam_far, frame)?;

        // ── 2b. Spot light shadow passes ─────────────────────────────────────
        // The spot_light_buf_offset is 1 (directional) + N_point_lights.
        let spot_buf_offset = 1u32 + scene.point_lights.len() as u32;
        if let Some(spot) = &mut self.spot_shadows {
            if !scene.spot_lights.is_empty() {
                spot.draw(scene, spot_buf_offset, frame, engine)?;
                frame.bind_buffer("spot_shadow_data", &spot.shadow_buf);
            }
        }

        // ── 2c. Point light shadow passes (dual-paraboloid) ───────────────────
        // Point lights occupy buffer slots 1..(1 + N_point).
        let point_buf_offset = 1u32;
        if let Some(pt) = &mut self.point_shadows {
            if !scene.point_lights.is_empty() {
                pt.draw(scene, point_buf_offset, frame, engine)?;
            }
        }

        // ── 3. Camera world position ──────────────────────────────────────────
        let cam_world = view.inverse() * Vec4::new(0.0, 0.0, 0.0, 1.0);

        // ── 4. Allocate G-Buffer images (auto-resize) ─────────────────────────
        let ext = output.desc().extent;
        let gbuffer_img = |name: &'static str, format: Format| ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: ext.width,
                height: ext.height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: Some(name),
                compression: Default::default(), min_lod_bits: None, msaa_resolve_to_single_sampled: false,
        };
        // Depth is both a depth-stencil attachment AND sampled by the lighting pass
        // for world position reconstruction.
        let depth_desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: ext.width,
                height: ext.height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL | ImageUsage::SAMPLED,
            transient: false,
            clear_value: None,
            debug_name: Some("gbuffer_depth"),
                compression: Default::default(), min_lod_bits: None, msaa_resolve_to_single_sampled: false,
        };

        let g0 = frame.image(
            "gbuffer_albedo_metallic",
            gbuffer_img("gbuffer_albedo_metallic", Format::Rgba8Unorm),
        )?;
        let g1 = frame.image(
            "gbuffer_normal_rough",
            gbuffer_img("gbuffer_normal_rough", Format::Rgba16Float),
        )?;
        let g2 = frame.image(
            "gbuffer_emissive",
            gbuffer_img("gbuffer_emissive", Format::Rgba16Float),
        )?;
        // G3 (world_pos) removed — reconstructed from depth in deferred_lighting.slang.
        let depth = frame.image("gbuffer_depth", depth_desc)?;

        // ── 5. Opaque draw pass — deferred G-Buffer fill OR forward single-pass ──
        let view_proj = (proj * view).to_cols_array_2d();
        let constants = CameraConstants {
            view_proj,
            previous_view_proj: view_proj,
            time,
            _pad: [0.0; 3],
        };

        if self.render_path == RenderPath::DeferredThenForward {
            let color_targets: &[&GraphImage] = &[&g0, &g1, &g2];
            let primary = color_targets[0];
            let additional = &color_targets[1..];

            // Step A: collect (hash, name, source) for materials not yet compiled or failed.
            let to_compile: Vec<(u64, String, String)> = (0..scene.mesh_count())
                .filter_map(|idx| scene.unified_material_at(idx))
                .filter(|mat| {
                    let h = mat.content_hash();
                    !self.variant_cache.contains_key(&h) && !self.failed_variants.contains_key(&h)
                })
                .map(|mat| {
                    (
                        mat.content_hash(),
                        mat.name.clone(),
                        mat.generate_gbuffer_source(),
                    )
                })
                .collect();

            // Step B: compile missing variants — catch errors and emit readable diagnostics.
            // Failed materials fall back to the default G-Buffer program for this frame;
            // their hash is recorded so we don't retry every frame.
            for (key, mat_name, source) in to_compile {
                match MeshProgram::new(
                    engine,
                    MeshProgramDesc {
                        fragment: ShaderDesc {
                            source: ShaderSource::Inline(source.clone()),
                            entry_point: "main".to_owned(),
                            stage: ShaderStage::Fragment,
                            requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
                        },
                        vertex: None,
                        vertex_kind: MeshVertexKind::V3d,
                        alpha_blend: false,
                        uses_depth: true,
                    },
                ) {
                    Ok(program) => {
                        self.variant_cache.insert(key, program);
                    }
                    Err(e) => {
                        let msg = format!("{e}");
                        eprintln!(
                            "[DeferredPass] G-Buffer variant compile failed for material '{mat_name}'.\n\
                             Error: {msg}\n\
                             Falling back to default G-Buffer shader for this material. \
                             Fix the UnifiedMaterial expression and re-run to recompile."
                        );
                        self.failed_variants.insert(key, msg);
                    }
                }
            }

            // Step C: draw each opaque batch into the G-Buffer.
            for (mesh_idx, instance_buf_opt, instance_count) in scene.drawable_batches() {
                let instance_buf = match instance_buf_opt {
                    Some(b) => b,
                    None => continue,
                };
                if instance_count == 0 {
                    continue;
                }
                let mesh = scene.mesh_at(mesh_idx);

                let mat_hash = scene
                    .unified_material_at(mesh_idx)
                    .map(|m| m.content_hash());
                let program: &MeshProgram = match mat_hash {
                    Some(h) => self
                        .variant_cache
                        .get(&h)
                        .unwrap_or(&self.default_gbuffer_program),
                    None => &self.default_gbuffer_program,
                };

                frame.bind_buffer("instances", instance_buf);
                if let Some(mat_buf) = scene.material_gpu_buffer_at(mesh_idx) {
                    frame.bind_buffer("material_desc", mat_buf);
                }
                let nmap: &crate::Image = scene
                    .normal_map_at(mesh_idx)
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

            // Register G-Buffer + depth for the lighting shader.
            g0.register_as("gbuffer_albedo_metallic");
            g1.register_as("gbuffer_normal_rough");
            g2.register_as("gbuffer_emissive");
            depth.register_as("gbuffer_depth");

            if let Some(ao) = &mut self.ao {
                let inv_proj = proj.inverse();
                let inv_view = view.inverse();
                ao.execute(
                    engine, frame, &depth, &g1, None, inv_proj, inv_view, view, ext.width,
                    ext.height,
                )?;
            } else {
                frame.bind_image("gtao_result", &self.white_ao);
            }
        } else {
            // ForwardOnly: draw all opaque meshes directly into the HDR output.
            // Depth testing uses `depth` to give correct z-order.
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
                if let Some(mat_buf) = scene.material_gpu_buffer_at(mesh_idx) {
                    frame.bind_buffer("material_desc", mat_buf);
                }
                let nmap: &crate::Image = scene
                    .normal_map_at(mesh_idx)
                    .map(|arc| arc.as_ref())
                    .unwrap_or(&self.flat_normal_map);
                frame.bind_image("normal_map", nmap);

                output.draw_mesh_instanced_with_push_constants_and_depth(
                    mesh,
                    &self.forward_opaque_program,
                    instance_buf,
                    instance_count,
                    &constants,
                    Some(&depth),
                )?;
            }
            depth.register_as("gbuffer_depth");
            frame.bind_image("gtao_result", &self.white_ao);
        }

        // shadow_map_0..3 already registered inside CsmPass::draw()

        // ── 7. Rebuild BVH if lights changed, bind IBL + BVH + run deferred ──
        // Rebuild the light BVH when the point/spot light lists change.
        let point_offset = 1u32;
        let spot_offset = point_offset + scene.point_lights.len() as u32;
        let rect_offset = spot_offset + scene.spot_lights.len() as u32;
        let sphere_offset = rect_offset + scene.rect_lights.len() as u32;
        let disk_offset = sphere_offset + scene.sphere_area_lights.len() as u32;
        let any_bvh_lights = scene.point_lights.len()
            + scene.spot_lights.len()
            + scene.rect_lights.len()
            + scene.sphere_area_lights.len()
            + scene.disk_lights.len()
            > 0;
        if self.light_bvh.dirty || any_bvh_lights {
            self.light_bvh.rebuild(
                engine,
                &scene.point_lights,
                &scene.spot_lights,
                &scene.rect_lights,
                &scene.sphere_area_lights,
                &scene.disk_lights,
                point_offset,
                spot_offset,
                rect_offset,
                sphere_offset,
                disk_offset,
            )?;
        }

        let (bvh_buf, bvh_root) = if !self.light_bvh.nodes.is_empty() {
            //panic allowed, reason = "light BVH upload proved gpu_buffer exists before non-empty use"
            (self.light_bvh.gpu_buffer.as_ref().unwrap(), 0u32)
        } else {
            (&self.empty_bvh_buf, crate::light_bvh::BVH_EMPTY)
        };
        frame.bind_buffer("light_bvh", bvh_buf);

        frame.bind_image("brdf_lut", &self.brdf_lut);
        frame.bind_buffer("e_avg_lut", &self.e_avg_buf);
        frame.bind_buffer("csm_data", &self.csm.csm_buffer);

        // Bind spot shadow data (all-invalid when no spot shadows are active).
        if let Some(spot) = &self.spot_shadows {
            frame.bind_buffer("spot_shadow_data", &spot.shadow_buf);
            // spot_shadow_map_0..3 are registered by SpotShadowPass::draw() above.
        } else {
            frame.bind_buffer("spot_shadow_data", &self.empty_spot_shadow_buf);
            // Register placeholder depth images so the binding resolves.
            frame.bind_image("spot_shadow_map_0", &self.black_spot_depth);
            frame.bind_image("spot_shadow_map_1", &self.black_spot_depth);
            frame.bind_image("spot_shadow_map_2", &self.black_spot_depth);
            frame.bind_image("spot_shadow_map_3", &self.black_spot_depth);
        }

        // Bind point shadow data (all-invalid when no point shadows are active).
        if let Some(pt) = &self.point_shadows {
            frame.bind_buffer("point_shadow_data", &pt.shadow_buf);
            // point_shadow_front/back_0..3 registered by PointShadowPass::draw() above.
        } else {
            frame.bind_buffer("point_shadow_data", &self.empty_point_shadow_buf);
            frame.bind_image("point_shadow_front_0", &self.black_spot_depth);
            frame.bind_image("point_shadow_front_1", &self.black_spot_depth);
            frame.bind_image("point_shadow_front_2", &self.black_spot_depth);
            frame.bind_image("point_shadow_front_3", &self.black_spot_depth);
            frame.bind_image("point_shadow_back_0", &self.black_spot_depth);
            frame.bind_image("point_shadow_back_1", &self.black_spot_depth);
            frame.bind_image("point_shadow_back_2", &self.black_spot_depth);
            frame.bind_image("point_shadow_back_3", &self.black_spot_depth);
        }

        // Advance blend transition if one is active.
        if self.blend_target.is_some() {
            self.blend_alpha = (self.blend_alpha + self.blend_step).min(1.0);
            if self.blend_alpha >= 1.0 {
                // Transition complete — commit the target as the current env.
                self.environment_map = self.blend_target.take();
                self.blend_alpha = 0.0;
                self.blend_step = 0.0;
            }
        }

        let (ibl_strength, ibl_max_layer) = if let Some(env) = &self.environment_map {
            if let Some(target) = &self.blend_target {
                // Blending: SH9 is interpolated per-frame on CPU.
                // Specular env switches at midpoint (alpha = 0.5).
                let a = self.blend_alpha;
                if a >= 0.5 {
                    frame.bind_image("env_specular", &target.specular);
                } else {
                    frame.bind_image("env_specular", &env.specular);
                }

                // Lerp SH9 coefficients.
                let mut blended = vec![0u8; 9 * 16];
                for i in 0..9 {
                    for c in 0..3 {
                        let v0 = env.sh9_coefficients[i][c];
                        let v1 = target.sh9_coefficients[i][c];
                        let vb = v0 + (v1 - v0) * a;
                        let offset = i * 16 + c * 4;
                        blended[offset..offset + 4].copy_from_slice(&vb.to_le_bytes());
                    }
                }
                self.blend_sh9_buf.write(0, &blended)?;
                frame.bind_buffer("sh9_irradiance", &self.blend_sh9_buf);
            } else {
                frame.bind_image("env_specular", &env.specular);
                frame.bind_buffer("sh9_irradiance", &env.sh9_buffer);
            }
            (1.0f32, (SPECULAR_LAYER_COUNT - 1) as f32)
        } else {
            frame.bind_image("env_specular", &self.black_env);
            frame.bind_buffer("sh9_irradiance", &self.zero_sh9);
            (0.0f32, (SPECULAR_LAYER_COUNT - 1) as f32)
        };

        // Upload and bind forward lighting uniforms for the OIT collect phase.
        let dl = &scene.directional_light;
        let fwd_uniforms = ForwardLightingUniforms {
            camera_world_pos: [cam_world.x, cam_world.y, cam_world.z, 0.0],
            ambient: [dl.ambient.x, dl.ambient.y, dl.ambient.z],
            dir_light_count: 1,
            ibl_strength,
            ibl_max_layer,
            bvh_root,
            _pad: 0.0,
        };
        self.forward_lighting_buf
            .write(0, bytemuck::bytes_of(&fwd_uniforms))?;
        frame.bind_buffer("forward_lighting", &self.forward_lighting_buf);

        // Deferred lighting fullscreen pass — only in DeferredThenForward mode.
        if self.render_path == RenderPath::DeferredThenForward {
            let inv_vp = (proj * view).inverse().to_cols_array_2d();
            let sky = &self.sky;
            let sun_cos = sky.sun_size_deg.to_radians().cos();
            output.execute_shader_with_constants_auto(
                &self.lighting_program,
                &DeferredLightingConstants {
                    camera_world_pos: [cam_world.x, cam_world.y, cam_world.z, 0.0],
                    ambient: [dl.ambient.x, dl.ambient.y, dl.ambient.z],
                    dir_light_count: 1,
                    ibl_strength,
                    ibl_max_layer,
                    bvh_root,
                    sky_enabled: sky.enabled as u32,
                    inv_view_proj: inv_vp,
                    sky_turbidity: sky.turbidity,
                    sky_exposure: sky.exposure,
                    sky_sun_size: sun_cos,
                    _pad: 0.0,
                },
            )?;
        }

        // ── 8. OIT — Translucent objects (Per-Pixel Linked List) ─────────────
        if let Some(oit) = &mut self.oit {
            oit.draw(scene, view, proj, output, frame, engine, time)?;
        }

        Ok(())
    }
}
