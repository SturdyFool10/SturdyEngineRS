// Directional shadow map pass — single cascade (ShadowPass) and
// cascaded shadow maps (CsmPass).
//
// CsmPass is the recommended path. It divides the camera frustum into N slices,
// fits a tight orthographic projection to each slice, and renders N depth-only
// passes. The resulting N shadow maps are sampled in `deferred_lighting.slang`
// with cascade selection based on view-space depth and PCF blending at borders.
//
// ShadowPass remains for lightweight use-cases where a single fixed-box shadow
// is acceptable (shader playground, minimal scenes, etc.).
//
// Roadmap: Track 6e — Shadow system.

use std::path::PathBuf;

use glam::{Mat4, Vec3, Vec4};

use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, Format, GraphImage, ImageDesc, ImageDimension,
    ImageUsage, MeshProgram, MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc,
    ShaderSource, ShaderStage, push_constants, scene::Scene,
};
use sturdy_engine_core::Extent3d;

const SHADOW_DEPTH_NOOP_FRAGMENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/shadow_depth_noop_fragment.slang"
));

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

// ── Shared depth push constants ───────────────────────────────────────────────

/// Push constants for `shadow_depth.slang`. Same for both ShadowPass and CsmPass.
#[push_constants]
struct ShadowDepthConstants {
    light_view_proj: [[f32; 4]; 4],
}

// ── ShadowPass (single fixed-box cascade) ─────────────────────────────────────

/// Configuration for the single-cascade directional shadow map.
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

/// Output of a single-cascade shadow pass.
pub struct ShadowOutput {
    pub image: GraphImage,
    pub light_view_proj: Mat4,
    pub depth_bias: f32,
}

/// Single fixed-box directional shadow map. Use `CsmPass` for better quality.
pub struct ShadowPass {
    depth_program: MeshProgram,
    masked_depth_program: MeshProgram,
    pub config: ShadowConfig,
}

impl ShadowPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_config(engine, ShadowConfig::default())
    }

    pub fn with_config(engine: &Engine, config: ShadowConfig) -> Result<Self> {
        let depth_program = build_depth_program(engine)?;
        let masked_depth_program = build_masked_depth_program(engine)?;
        Ok(Self {
            depth_program,
            masked_depth_program,
            config,
        })
    }

    pub fn light_matrix(&self, light_direction: Vec3) -> Mat4 {
        let up = if light_direction.y.abs() < 0.99 {
            Vec3::Y
        } else {
            Vec3::Z
        };
        let eye = -(light_direction.normalize()) * (self.config.far * 0.5);
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, up);
        let hs = self.config.half_size;
        let proj = Mat4::orthographic_rh(-hs, hs, -hs, hs, self.config.near, self.config.far);
        proj * view
    }

    pub fn draw(
        &self,
        scene: &Scene,
        frame: &RenderFrame,
        _engine: &Engine,
    ) -> Result<ShadowOutput> {
        let res = self.config.resolution;
        let desc = shadow_image_desc(res, "shadow_map");
        let shadow_image = frame.image("shadow_map", desc)?;
        let light_view_proj = self.light_matrix(scene.directional_light.direction);
        draw_shadow_batches(
            scene,
            frame,
            &shadow_image,
            &self.depth_program,
            &self.masked_depth_program,
            light_view_proj,
        )?;
        shadow_image.register_as("shadow_map");
        Ok(ShadowOutput {
            image: shadow_image,
            light_view_proj,
            depth_bias: self.config.depth_bias,
        })
    }
}

// ── CsmPass (N-cascade tight frustum-fitted shadows) ─────────────────────────

/// Configuration for cascaded shadow maps.
///
/// `CsmConfig::default()` gives 4 cascades at 2048×2048 — suitable for
/// medium to large outdoor scenes. All fields are `pub` and documented with
/// valid ranges so IDE users can discover every dial.
#[derive(Clone, Debug)]
pub struct CsmConfig {
    /// Number of shadow cascades. \[1, 4\]. Default 4.
    ///
    /// More cascades → smoother depth transitions, more GPU cost.
    pub cascade_count: u32,
    /// Shadow map resolution (square, same for all cascades). Default 2048.
    pub resolution: u32,
    /// Constant depth bias against self-shadowing. \[0.0, 0.05\]. Default 0.003.
    pub depth_bias: f32,
    /// Mix between uniform (0.0) and logarithmic (1.0) cascade partitioning.
    /// Higher values push more cascades close to the camera for sharper near
    /// shadows. \[0.0, 1.0\]. Default 0.75.
    pub lambda: f32,
    /// Fraction of each cascade's depth range used for blending with the next
    /// cascade. Eliminates the hard visual seam at cascade boundaries.
    /// \[0.0, 0.5\]. Default 0.15.
    pub blend_range: f32,
    /// Snap the shadow projection center to texel-aligned increments each frame,
    /// preventing shadow shimmer as the camera moves. Default true.
    pub stabilise: bool,
    /// Light-space Z range extension beyond the fitted frustum corners.
    /// Catches tall geometry (trees, buildings) behind the camera that still
    /// casts shadows into the scene. Default 50.0.
    pub z_extension: f32,
    /// Enable Percentage-Closer Soft Shadows (PCSS). When true, shadow edges are
    /// physically blurred based on blocker distance — contact shadows stay sharp,
    /// far-from-caster shadows soften. Default false (regular 3×3 PCF).
    pub pcss: bool,
    /// World-space angular diameter of the light source used for PCSS penumbra
    /// estimation. Larger values produce softer shadows. \[0.1, 20.0\]. Default 2.5.
    pub pcss_light_size: f32,
}

impl Default for CsmConfig {
    fn default() -> Self {
        Self {
            cascade_count: 4,
            resolution: 2048,
            depth_bias: 0.003,
            lambda: 0.75,
            blend_range: 0.15,
            stabilise: true,
            z_extension: 50.0,
            pcss: false,
            pcss_light_size: 2.5,
        }
    }
}

/// Maximum number of cascades the shader supports.
pub const MAX_CASCADES: usize = 4;

/// Maximum number of cascades the atlas can hold.
pub const ATLAS_COLS: u32 = 2;
pub const ATLAS_ROWS: u32 = 2;

/// GPU-layout CSM frame data written to a `StructuredBuffer<GpuCsmData>`.
/// Matches the Slang `GpuCsmData` struct in `deferred_lighting.slang` exactly.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuCsmData {
    /// Per-cascade light view-projection matrices (up to 4). Unused slots are
    /// copies of the last valid cascade.
    pub matrices: [[[f32; 4]; 4]; MAX_CASCADES], // 256 bytes
    /// View-space (linear) distances where each cascade begins.
    pub split_distances: [f32; MAX_CASCADES], //  16 bytes
    /// Number of active cascades (≤ MAX_CASCADES).
    pub cascade_count: u32, //   4 bytes
    /// Constant depth bias applied in the PCF comparison.
    pub depth_bias: f32, //   4 bytes
    /// Fraction of each cascade range used for blending with the next.
    pub blend_range: f32, //   4 bytes
    /// 1 = PCSS enabled, 0 = regular 3×3 PCF.
    pub pcss_enabled: u32, //   4 bytes → 288 bytes
    /// Per-cascade atlas UV tile bounds: [x_min, y_min, x_max, y_max] in [0,1].
    pub atlas_tiles: [[f32; 4]; MAX_CASCADES], //  64 bytes → 352 bytes
    /// Per-cascade light source size in atlas UV space for PCSS blocker search.
    /// = world_light_size / cascade_world_width * tile_scale.
    pub pcss_light_size_uv: [f32; MAX_CASCADES], //  16 bytes → 368 bytes
}

/// Per-frame output of `CsmPass::draw`.
pub struct CsmOutput {
    /// The shadow atlas image containing all cascades. All cascades are packed into a 2×2 grid.
    pub shadow_atlas: GraphImage,
    /// Packed GPU data ready to write into the `csm_data` StructuredBuffer.
    pub gpu_data: GpuCsmData,
}

/// Directional cascaded shadow map pass.
///
/// Replaces `ShadowPass` inside `DeferredPass`. Tightly fits each cascade's
/// orthographic projection to its camera sub-frustum, eliminating the wasted
/// resolution of the fixed-box approach.
///
/// # Usage
/// ```ignore
/// let csm = CsmPass::new(&engine)?;
/// // Each frame — called internally by DeferredPass:
/// let out = csm.draw(&scene, view, proj, near, far, &frame)?;
/// ```
pub struct CsmPass {
    depth_program: MeshProgram,
    masked_depth_program: MeshProgram,
    pub config: CsmConfig,
    /// Persistent GPU buffer for `GpuCsmData` (reused every frame).
    pub csm_buffer: Buffer,
}

impl CsmPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_config(engine, CsmConfig::default())
    }

    pub fn with_config(engine: &Engine, config: CsmConfig) -> Result<Self> {
        let depth_program = build_depth_program(engine)?;
        let masked_depth_program = build_masked_depth_program(engine)?;
        let csm_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<GpuCsmData>() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        Ok(Self {
            depth_program,
            masked_depth_program,
            config,
            csm_buffer,
        })
    }

    /// Render all cascade shadow maps and return the output.
    ///
    /// `view` and `proj` are the camera matrices for this frame. `near`/`far`
    /// are the camera's clip planes — used for cascade partitioning.
    pub fn draw(
        &mut self,
        scene: &Scene,
        view: Mat4,
        proj: Mat4,
        near: f32,
        far: f32,
        frame: &RenderFrame,
    ) -> Result<CsmOutput> {
        let n = (self.config.cascade_count as usize).clamp(1, MAX_CASCADES);
        let light_dir = scene.directional_light.direction.normalize();
        let splits = compute_cascade_splits(near, far, n, self.config.lambda);
        let res = self.config.resolution;

        // Build cascade matrices from tight frustum fitting.
        let mut matrices = [Mat4::IDENTITY; MAX_CASCADES];
        let mut pcss_light_size_uv = [0.0f32; MAX_CASCADES];
        let tile_scale = 1.0 / ATLAS_COLS as f32; // 0.5 for a 2×2 atlas
        for i in 0..n {
            let (mat, world_width) = cascade_light_matrix(
                view,
                proj,
                near,
                far,
                splits[i],
                splits[i + 1],
                light_dir,
                res,
                self.config.z_extension,
                self.config.stabilise,
            );
            matrices[i] = mat;
            // UV-space size of the light source within this cascade's atlas tile.
            // Clamped to reasonable range to avoid huge kernels on tiny cascades.
            pcss_light_size_uv[i] = (self.config.pcss_light_size / world_width.max(1e-3)
                * tile_scale)
                .clamp(0.001, 0.25);
        }
        // Fill unused cascade slots with the last valid cascade.
        for i in n..MAX_CASCADES {
            matrices[i] = matrices[n - 1];
            pcss_light_size_uv[i] = pcss_light_size_uv[n - 1];
        }

        // Allocate one atlas image containing all cascades in a 2×2 tile grid.
        //
        //   Cascade 0 │ Cascade 1
        //   ──────────┼──────────
        //   Cascade 2 │ Cascade 3
        //
        // Each tile is `res × res`; the atlas is `2*res × 2*res`.
        let atlas_w = res * ATLAS_COLS;
        let atlas_h = res * ATLAS_ROWS;
        let atlas_desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: atlas_w,
                height: atlas_h,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL | ImageUsage::SAMPLED,
            transient: false,
            clear_value: None,
            debug_name: Some("shadow_atlas"),
            compression: Default::default(),
            min_lod_bits: None,
            msaa_resolve_to_single_sampled: false,
        };
        let shadow_atlas = frame.image("shadow_atlas", atlas_desc)?;

        // Compute atlas tile UV bounds and render each cascade into its tile.
        let tile_scale_x = 1.0 / ATLAS_COLS as f32;
        let tile_scale_y = 1.0 / ATLAS_ROWS as f32;
        let mut atlas_tiles = [[0.0f32; 4]; MAX_CASCADES];

        for i in 0..n {
            let col = (i % ATLAS_COLS as usize) as u32;
            let row = (i / ATLAS_COLS as usize) as u32;
            let tile_x = col * res;
            let tile_y = row * res;

            // Atlas UV sub-region for this cascade [x_min, y_min, x_max, y_max].
            atlas_tiles[i] = [
                col as f32 * tile_scale_x,
                row as f32 * tile_scale_y,
                (col + 1) as f32 * tile_scale_x,
                (row + 1) as f32 * tile_scale_y,
            ];

            // Render this cascade into its tile using a viewport override.
            draw_shadow_batches_viewport(
                scene,
                frame,
                &shadow_atlas,
                &self.depth_program,
                &self.masked_depth_program,
                matrices[i],
                [tile_x, tile_y, res, res],
            )?;
        }
        // Fill unused tile bounds with the last valid cascade's bounds.
        for i in n..MAX_CASCADES {
            atlas_tiles[i] = atlas_tiles[n - 1];
        }

        // Register for the deferred lighting shader.
        shadow_atlas.register_as("shadow_atlas");

        // Also register the legacy per-cascade names so existing shaders keep working.
        for i in 0..MAX_CASCADES {
            let name = format!("shadow_map_{i}");
            shadow_atlas.register_as(&name);
        }

        // Pack split distances (view-space linear depth of each cascade start).
        let mut split_dists = [0.0f32; MAX_CASCADES];
        for i in 0..MAX_CASCADES {
            split_dists[i] = splits[i.min(n)];
        }

        let gpu_data = GpuCsmData {
            matrices: matrices.map(|m| m.to_cols_array_2d()),
            split_distances: split_dists,
            cascade_count: n as u32,
            depth_bias: self.config.depth_bias,
            blend_range: self.config.blend_range,
            pcss_enabled: self.config.pcss as u32,
            atlas_tiles,
            pcss_light_size_uv,
        };

        // Upload to the persistent GPU buffer.
        self.csm_buffer.write(0, bytemuck::bytes_of(&gpu_data))?;

        let shadow_atlas = frame
            .find_image_by_name("shadow_atlas")
            //panic allowed, reason = "draw_shadow_batches_viewport creates shadow_atlas before lookup"
            .expect("shadow_atlas must exist after draw_shadow_batches_viewport");

        Ok(CsmOutput {
            shadow_atlas,
            gpu_data,
        })
    }
}

// ── Cascade math ──────────────────────────────────────────────────────────────

/// Compute N+1 view-space linear split depths for N cascades.
/// Uses a mix of uniform and logarithmic partitioning controlled by `lambda`.
fn compute_cascade_splits(near: f32, far: f32, n: usize, lambda: f32) -> Vec<f32> {
    let mut splits = vec![0.0f32; n + 1];
    splits[0] = near;
    splits[n] = far;
    for i in 1..n {
        let t = i as f32 / n as f32;
        let log = near * f32::powf(far / near, t);
        let uniform = near + (far - near) * t;
        splits[i] = lambda * log + (1.0 - lambda) * uniform;
    }
    splits
}

/// Convert view-space depth d (positive, distance from camera) to NDC Z using the projection matrix.
fn view_depth_to_ndc_z(proj: Mat4, d: f32) -> f32 {
    // For RH perspective looking down -Z, view_z = -d.
    // clip_z = proj[2][2] * (-d) + proj[3][2]
    // clip_w = proj[2][3] * (-d) + proj[3][3]  = -(-d)... wait
    // Using glam column-major: proj.z_axis is col 2, proj.w_axis is col 3.
    let a = proj.z_axis.z; // col2 row2
    let b = proj.w_axis.z; // col3 row2
    let c = proj.z_axis.w; // col2 row3
    let e = proj.w_axis.w; // col3 row3  (= 0 for perspective)
    let view_z = -d;
    let clip_z = a * view_z + b;
    let clip_w = c * view_z + e;
    if clip_w.abs() < 1e-7 {
        0.0
    } else {
        clip_z / clip_w
    }
}

/// Compute the 8 world-space corners of the cascade sub-frustum.
/// `z_near_ndc` and `z_far_ndc` are the NDC Z values for the cascade depth range.
fn cascade_world_corners(inv_vp: Mat4, z_near_ndc: f32, z_far_ndc: f32) -> [Vec3; 8] {
    let ndc_pts: [[f32; 4]; 8] = [
        [-1.0, -1.0, z_near_ndc, 1.0],
        [1.0, -1.0, z_near_ndc, 1.0],
        [-1.0, 1.0, z_near_ndc, 1.0],
        [1.0, 1.0, z_near_ndc, 1.0],
        [-1.0, -1.0, z_far_ndc, 1.0],
        [1.0, -1.0, z_far_ndc, 1.0],
        [-1.0, 1.0, z_far_ndc, 1.0],
        [1.0, 1.0, z_far_ndc, 1.0],
    ];
    let mut out = [Vec3::ZERO; 8];
    for (i, p) in ndc_pts.iter().enumerate() {
        let h = inv_vp * Vec4::from_array(*p);
        out[i] = h.truncate() / h.w;
    }
    out
}

/// Build the tight orthographic light-view-proj matrix for one cascade.
/// Returns `(matrix, cascade_world_width)` where the world width is the
/// X-extent of the fitted frustum in light space — used for PCSS light-size scaling.
fn cascade_light_matrix(
    view: Mat4,
    proj: Mat4,
    cam_near: f32,
    cam_far: f32,
    cascade_near: f32,
    cascade_far: f32,
    light_dir: Vec3,
    resolution: u32,
    z_extension: f32,
    stabilise: bool,
) -> (Mat4, f32) {
    let inv_vp = (proj * view).inverse();
    let z_near_ndc = view_depth_to_ndc_z(proj, cascade_near.max(cam_near));
    let z_far_ndc = view_depth_to_ndc_z(proj, cascade_far.min(cam_far));
    let corners = cascade_world_corners(inv_vp, z_near_ndc, z_far_ndc);

    // Build a light-view matrix looking from light direction.
    let up = if light_dir.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::Z
    };
    let centroid: Vec3 = corners.iter().sum::<Vec3>() / 8.0;
    let light_view = Mat4::look_at_rh(centroid - light_dir, centroid, up);

    // Compute AABB of frustum corners in light space.
    let mut min_ls = Vec3::splat(f32::INFINITY);
    let mut max_ls = Vec3::splat(f32::NEG_INFINITY);
    for &corner in &corners {
        let ls = light_view.transform_point3(corner);
        min_ls = min_ls.min(ls);
        max_ls = max_ls.max(ls);
    }

    // Extend Z to catch casters behind the camera frustum (trees, buildings).
    min_ls.z -= z_extension;

    // Optional: snap center to texel-aligned increments (reduces shimmer).
    if stabilise {
        let size_x = max_ls.x - min_ls.x;
        let size_y = max_ls.y - min_ls.y;
        let texel_x = size_x / resolution as f32;
        let texel_y = size_y / resolution as f32;
        let center_x = ((min_ls.x + max_ls.x) * 0.5 / texel_x).floor() * texel_x;
        let center_y = ((min_ls.y + max_ls.y) * 0.5 / texel_y).floor() * texel_y;
        let half_x = size_x * 0.5;
        let half_y = size_y * 0.5;
        min_ls.x = center_x - half_x;
        max_ls.x = center_x + half_x;
        min_ls.y = center_y - half_y;
        max_ls.y = center_y + half_y;
    }

    // Light-space orthographic projection (Z range: min_ls.z → max_ls.z, depth 0..1).
    let world_width = max_ls.x - min_ls.x;
    let light_proj = Mat4::orthographic_rh(
        min_ls.x, max_ls.x, min_ls.y, max_ls.y, -max_ls.z,
        -min_ls.z, // light looks toward -Z; negate to get near/far
    );

    (light_proj * light_view, world_width)
}

// ── Shared helpers ────────────────────────────────────────────────────────────

pub(crate) fn build_depth_program_pub(engine: &Engine) -> Result<MeshProgram> {
    build_depth_program(engine)
}

fn build_depth_program(engine: &Engine) -> Result<MeshProgram> {
    MeshProgram::new(
        engine,
        MeshProgramDesc {
            fragment: ShaderDesc {
                source: ShaderSource::Inline(SHADOW_DEPTH_NOOP_FRAGMENT.into()),
                entry_point: "main".to_owned(),
                stage: ShaderStage::Fragment,
                requires_ray_query: false,
                requires_cooperative_matrix: false,
                uses_ser: false,
            },
            vertex: Some(ShaderDesc {
                source: ShaderSource::File(engine_shader("shadow_depth.slang")),
                entry_point: "main".to_owned(),
                stage: ShaderStage::Vertex,
                requires_ray_query: false,
                requires_cooperative_matrix: false,
                uses_ser: false,
            }),
            vertex_kind: MeshVertexKind::V3d,
            alpha_blend: false,
            uses_depth: true,
        },
    )
}

fn shadow_image_desc(resolution: u32, debug_name: &'static str) -> ImageDesc {
    ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width: resolution,
            height: resolution,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Depth32Float,
        usage: ImageUsage::DEPTH_STENCIL | ImageUsage::SAMPLED,
        transient: false,
        clear_value: None,
        debug_name: Some(debug_name),
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
    }
}

/// Draw all shadow-casting batches into a viewport tile of `shadow_image`.
///
/// Used by the atlas path: `viewport` is `[x, y, width, height]` in pixels.
fn draw_shadow_batches_viewport(
    scene: &Scene,
    frame: &RenderFrame,
    shadow_image: &GraphImage,
    program: &MeshProgram,
    masked_program: &MeshProgram,
    light_view_proj: Mat4,
    viewport: [u32; 4],
) -> Result<()> {
    use crate::scene::material::MaterialDomain;
    let constants = ShadowDepthConstants {
        light_view_proj: light_view_proj.to_cols_array_2d(),
    };
    for (mesh_idx, instance_buf_opt, instance_count) in scene.drawable_batches() {
        let instance_buf = match instance_buf_opt {
            Some(b) => b,
            None => continue,
        };
        if instance_count == 0 {
            continue;
        }
        let domain = scene.domain_at(mesh_idx);
        if domain == MaterialDomain::Translucent {
            continue;
        }
        let mesh = scene.mesh_at(mesh_idx);
        frame.bind_buffer("instances", instance_buf);
        let prog = if domain == MaterialDomain::Masked {
            if let Some(mb) = scene.material_gpu_buffer_at(mesh_idx) {
                frame.bind_buffer("material_desc", mb);
            }
            masked_program
        } else {
            program
        };
        shadow_image.draw_mesh_depth_only_with_push_constants_and_viewport(
            mesh,
            prog,
            instance_buf,
            instance_count,
            &constants,
            viewport,
        )?;
    }
    Ok(())
}

/// Draw all shadow-casting batches into `shadow_image`.
///
/// - `Opaque` / `Unlit` / `Decal` meshes use the plain depth program.
/// - `Masked` meshes use the alpha-tested depth program and bind `material_desc`.
/// - `Translucent` meshes are skipped (they don't cast hard shadows).
fn draw_shadow_batches(
    scene: &Scene,
    frame: &RenderFrame,
    shadow_image: &GraphImage,
    program: &MeshProgram,
    masked_program: &MeshProgram,
    light_view_proj: Mat4,
) -> Result<()> {
    use crate::scene::material::MaterialDomain;

    let constants = ShadowDepthConstants {
        light_view_proj: light_view_proj.to_cols_array_2d(),
    };
    for (mesh_idx, instance_buf_opt, instance_count) in scene.drawable_batches() {
        let instance_buf = match instance_buf_opt {
            Some(b) => b,
            None => continue,
        };
        if instance_count == 0 {
            continue;
        }

        let domain = scene.domain_at(mesh_idx);
        if domain == MaterialDomain::Translucent {
            continue; // translucent objects don't cast hard shadows
        }

        let mesh = scene.mesh_at(mesh_idx);
        frame.bind_buffer("instances", instance_buf);

        if domain == MaterialDomain::Masked {
            // Alpha-tested shadow: bind material constants so the fragment shader
            // can read `albedo.a` and discard fragments below the cutoff.
            if let Some(mat_buf) = scene.material_gpu_buffer_at(mesh_idx) {
                frame.bind_buffer("material_desc", mat_buf);
            }
            shadow_image.draw_mesh_depth_only_with_push_constants(
                mesh,
                masked_program,
                instance_buf,
                instance_count,
                &constants,
            )?;
        } else {
            shadow_image.draw_mesh_depth_only_with_push_constants(
                mesh,
                program,
                instance_buf,
                instance_count,
                &constants,
            )?;
        }
    }
    Ok(())
}

pub(crate) fn build_masked_depth_program_pub(engine: &Engine) -> Result<MeshProgram> {
    build_masked_depth_program(engine)
}

fn build_masked_depth_program(engine: &Engine) -> Result<MeshProgram> {
    MeshProgram::new(
        engine,
        MeshProgramDesc {
            fragment: ShaderDesc {
                source: ShaderSource::File(engine_shader("shadow_depth_masked.slang")),
                entry_point: "main".to_owned(),
                stage: ShaderStage::Fragment,
                requires_ray_query: false,
                requires_cooperative_matrix: false,
                uses_ser: false,
            },
            vertex: Some(ShaderDesc {
                source: ShaderSource::File(engine_shader("shadow_depth.slang")),
                entry_point: "main".to_owned(),
                stage: ShaderStage::Vertex,
                requires_ray_query: false,
                requires_cooperative_matrix: false,
                uses_ser: false,
            }),
            vertex_kind: MeshVertexKind::V3d,
            alpha_blend: false,
            uses_depth: true,
        },
    )
}
