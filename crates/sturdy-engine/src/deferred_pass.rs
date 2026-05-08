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
    Buffer, BufferDesc, BufferUsage, Engine, Format, GraphImage, ImageDesc, ImageDimension,
    ImageUsage, MeshProgram, MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc,
    ShaderProgram, ShaderSource, ShaderStage, push_constants, scene::Scene,
    shadow_pass::{CsmConfig, CsmPass},
    environment_map::{EnvironmentMap, compute_brdf_lut, compute_e_avg_lut, SPECULAR_LAYER_COUNT},
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

/// Push constants for `deferred_lighting.slang` (128 bytes = Vulkan guaranteed minimum).
#[push_constants]
struct DeferredLightingConstants {
    camera_world_pos: [f32; 4],      // 16
    ambient: [f32; 3],               // 12
    dir_light_count: u32,            //  4  → 32
    ibl_strength: f32,               //  4
    ibl_max_layer: f32,              //  4
    bvh_root: u32,                   //  4
    sky_enabled: u32,                //  4  → 48
    inv_view_proj: [[f32; 4]; 4],    // 64  → 112
    sky_turbidity: f32,              //  4
    sky_exposure: f32,               //  4
    sky_sun_size: f32,               //  4
    _pad: f32,                       //  4  → 128
}

/// Uniform buffer (48 bytes) bound as `"forward_lighting"` for the OIT forward lit pass.
/// Pre-bound by `DeferredPass::draw()` before the OIT collect phase runs.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ForwardLightingUniforms {
    camera_world_pos: [f32; 4],  // 16
    ambient: [f32; 3],           // 12
    dir_light_count: u32,        //  4  → 32
    ibl_strength: f32,           //  4
    ibl_max_layer: f32,          //  4
    bvh_root: u32,               //  4
    _pad: f32,                   //  4  → 48
}

/// Selects how opaque geometry is rendered.
///
/// - `DeferredThenForward` (default): opaque objects via G-Buffer deferred; translucent
///   objects via `OitPass` if enabled. Best quality and scalability.
/// - `ForwardOnly`: all opaque objects rendered with the forward-lit shader in one pass;
///   no G-Buffer, no deferred lighting. Useful as a compatibility fallback on hardware
///   that does not support multiple render targets.
///
/// Set via `DeferredPass::render_path`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum RenderPath {
    #[default]
    DeferredThenForward,
    ForwardOnly,
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
/// Procedural sky configuration.
///
/// The sky is rendered as part of the deferred lighting pass — background pixels
/// (those with no geometry, i.e. depth = 1.0) receive a physically-inspired sky
/// colour derived from the scene's directional light direction. No extra pass needed.
///
/// `SkyConfig::default()` gives a clear midday sky. Set on `DeferredPass` via
/// `deferred.sky = SkyConfig { turbidity: 4.0, ..Default::default() }`.
#[derive(Clone, Debug)]
pub struct SkyConfig {
    /// Atmospheric turbidity \[1, 10\]. 1 = crystal clear, 10 = heavy haze.
    /// Default 2.5.
    pub turbidity: f32,
    /// Overall sky exposure multiplier. Default 1.0.
    pub exposure: f32,
    /// Sun disc angular radius in degrees. Default 0.53 (physically correct).
    pub sun_size_deg: f32,
    /// Enable sky rendering for background pixels. Default true.
    pub enabled: bool,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self { turbidity: 2.5, exposure: 1.0, sun_size_deg: 0.53, enabled: true }
    }
}

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

        let forward_lighting_buf = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<ForwardLightingUniforms>() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;

        let e_avg_buf = compute_e_avg_lut(engine)?;

        // Auto-create the default studio environment so PBR materials look
        // reasonable without the user loading an HDR file. Overridable via
        // `set_environment_map`.
        let default_env = EnvironmentMap::studio(engine)?;

        let blend_sh9_buf = engine.create_buffer(crate::BufferDesc {
            size: 9 * 16,  // 9 × float4
            usage: crate::BufferUsage::STORAGE | crate::BufferUsage::COPY_DST,
        })?;
        blend_sh9_buf.write(0, &vec![0u8; 9 * 16])?;

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
            blend_target: None,
            blend_alpha: 0.0,
            blend_step: 0.0,
            blend_sh9_buf,
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
        self.blend_target = None;
        self.blend_alpha  = 0.0;
        self.blend_step   = 0.0;
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
        self.blend_alpha  = 0.0;
        self.blend_step   = 1.0 / frames as f32;
    }

    /// Returns `true` if a blend transition is currently in progress.
    pub fn is_blending(&self) -> bool {
        self.blend_target.is_some()
    }

    /// Returns the current blend alpha [0, 1] (0 = source, 1 = target).
    pub fn blend_alpha(&self) -> f32 {
        self.blend_alpha
    }

    /// Returns all material variant compile failures since init.
    ///
    /// Each entry is `(content_hash, error_message)`. The failed material falls
    /// back to the default G-Buffer program and is not retried until the hash changes
    /// (i.e. the `UnifiedMaterial` expression is modified).
    pub fn variant_compile_failures(&self) -> impl Iterator<Item = (u64, &str)> {
        self.failed_variants.iter().map(|(k, v)| (*k, v.as_str()))
    }

    /// Clear all recorded variant compile failures, allowing retry on next frame.
    pub fn clear_variant_failures(&mut self) {
        self.failed_variants.clear();
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
        self.csm.draw(scene, view, proj, cam_near, cam_far, frame)?;

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
        // Depth is both a depth-stencil attachment AND sampled by the lighting pass
        // for world position reconstruction.
        let depth_desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d { width: ext.width, height: ext.height, depth: 1 },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL | ImageUsage::SAMPLED,
            transient: false,
            clear_value: None,
            debug_name: Some("gbuffer_depth"),
        };

        let g0    = frame.image("gbuffer_albedo_metallic", gbuffer_img("gbuffer_albedo_metallic", Format::Rgba8Unorm  ))?;
        let g1    = frame.image("gbuffer_normal_rough",    gbuffer_img("gbuffer_normal_rough",    Format::Rgba16Float ))?;
        let g2    = frame.image("gbuffer_emissive",        gbuffer_img("gbuffer_emissive",        Format::Rgba16Float ))?;
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
            let primary    = color_targets[0];
            let additional = &color_targets[1..];

            // Step A: collect (hash, name, source) for materials not yet compiled or failed.
            let to_compile: Vec<(u64, String, String)> = (0..scene.mesh_count())
                .filter_map(|idx| scene.unified_material_at(idx))
                .filter(|mat| {
                    let h = mat.content_hash();
                    !self.variant_cache.contains_key(&h) && !self.failed_variants.contains_key(&h)
                })
                .map(|mat| (mat.content_hash(), mat.name.clone(), mat.generate_gbuffer_source()))
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
                        },
                        vertex: None,
                        vertex_kind: MeshVertexKind::V3d,
                        alpha_blend: false,
                        uses_depth: true,
                    },
                ) {
                    Ok(program) => { self.variant_cache.insert(key, program); }
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
                let instance_buf = match instance_buf_opt { Some(b) => b, None => continue };
                if instance_count == 0 { continue; }
                let mesh = scene.mesh_at(mesh_idx);

                let mat_hash = scene.unified_material_at(mesh_idx).map(|m| m.content_hash());
                let program: &MeshProgram = match mat_hash {
                    Some(h) => self.variant_cache
                        .get(&h)
                        .unwrap_or(&self.default_gbuffer_program),
                    None => &self.default_gbuffer_program,
                };

                frame.bind_buffer("instances", instance_buf);
                if let Some(mat_buf) = scene.material_gpu_buffer_at(mesh_idx) {
                    frame.bind_buffer("material_desc", mat_buf);
                }
                let nmap: &crate::Image = scene.normal_map_at(mesh_idx)
                    .map(|arc| arc.as_ref())
                    .unwrap_or(&self.flat_normal_map);
                frame.bind_image("normal_map", nmap);

                primary.draw_mesh_instanced_mrt_with_push_constants_and_depth(
                    additional, mesh, program, instance_buf, instance_count, &constants, Some(&depth),
                )?;
            }

            // Register G-Buffer + depth for the lighting shader.
            g0.register_as("gbuffer_albedo_metallic");
            g1.register_as("gbuffer_normal_rough");
            g2.register_as("gbuffer_emissive");
            depth.register_as("gbuffer_depth");
        } else {
            // ForwardOnly: draw all opaque meshes directly into the HDR output.
            // Depth testing uses `depth` to give correct z-order.
            for (mesh_idx, instance_buf_opt, instance_count) in scene.drawable_batches() {
                let instance_buf = match instance_buf_opt { Some(b) => b, None => continue };
                if instance_count == 0 { continue; }
                let mesh = scene.mesh_at(mesh_idx);

                frame.bind_buffer("instances", instance_buf);
                if let Some(mat_buf) = scene.material_gpu_buffer_at(mesh_idx) {
                    frame.bind_buffer("material_desc", mat_buf);
                }
                let nmap: &crate::Image = scene.normal_map_at(mesh_idx)
                    .map(|arc| arc.as_ref())
                    .unwrap_or(&self.flat_normal_map);
                frame.bind_image("normal_map", nmap);

                output.draw_mesh_instanced_with_push_constants_and_depth(
                    mesh, &self.forward_opaque_program,
                    instance_buf, instance_count, &constants, Some(&depth),
                )?;
            }
            depth.register_as("gbuffer_depth");
        }

        // shadow_map_0..3 already registered inside CsmPass::draw()

        // ── 7. Rebuild BVH if lights changed, bind IBL + BVH + run deferred ──
        // Rebuild the light BVH when the point/spot light lists change.
        let point_offset  = 1u32;
        let spot_offset   = point_offset  + scene.point_lights.len() as u32;
        let rect_offset   = spot_offset   + scene.spot_lights.len() as u32;
        let sphere_offset = rect_offset   + scene.rect_lights.len() as u32;
        let disk_offset   = sphere_offset + scene.sphere_area_lights.len() as u32;
        let any_bvh_lights = scene.point_lights.len() + scene.spot_lights.len()
            + scene.rect_lights.len() + scene.sphere_area_lights.len() + scene.disk_lights.len() > 0;
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
            (self.light_bvh.gpu_buffer.as_ref().unwrap(), 0u32)
        } else {
            (&self.empty_bvh_buf, crate::light_bvh::BVH_EMPTY)
        };
        frame.bind_buffer("light_bvh", bvh_buf);

        frame.bind_image("brdf_lut", &self.brdf_lut);
        frame.bind_buffer("e_avg_lut", &self.e_avg_buf);
        frame.bind_buffer("csm_data", &self.csm.csm_buffer);

        // Advance blend transition if one is active.
        if self.blend_target.is_some() {
            self.blend_alpha = (self.blend_alpha + self.blend_step).min(1.0);
            if self.blend_alpha >= 1.0 {
                // Transition complete — commit the target as the current env.
                self.environment_map = self.blend_target.take();
                self.blend_alpha = 0.0;
                self.blend_step  = 0.0;
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
                frame.bind_image("env_specular",    &env.specular);
                frame.bind_buffer("sh9_irradiance", &env.sh9_buffer);
            }
            (1.0f32, (SPECULAR_LAYER_COUNT - 1) as f32)
        } else {
            frame.bind_image("env_specular",    &self.black_env);
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
        self.forward_lighting_buf.write(0, bytemuck::bytes_of(&fwd_uniforms))?;
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
                    sky_enabled:   sky.enabled as u32,
                    inv_view_proj: inv_vp,
                    sky_turbidity: sky.turbidity,
                    sky_exposure:  sky.exposure,
                    sky_sun_size:  sun_cos,
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
