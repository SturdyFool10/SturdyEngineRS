// Spot light shadow map pass.
//
// Renders perspective depth maps from the viewpoint of up to MAX_SPOT_SHADOWS
// spot lights. The `deferred_lighting.slang` shader samples these to apply
// PCF shadows to spot-lit surfaces.
//
// Integration: attach to `DeferredPass` via `DeferredPass::set_spot_shadows`.
// `DeferredPass::draw` calls `SpotShadowPass::draw` before the G-Buffer fill,
// then binds `spot_shadow_data` and `spot_shadow_map_0..3` for the lighting shader.
//
// Roadmap: Track 6e — Spot light shadow maps.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};

use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, Format, GraphImage, ImageDesc, ImageDimension,
    ImageUsage, MeshProgram, RenderFrame, Result, push_constants, scene::Scene,
};
use sturdy_engine_core::Extent3d;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum number of simultaneously-shadowing spot lights.
pub const MAX_SPOT_SHADOWS: usize = 4;

// ── GPU data ──────────────────────────────────────────────────────────────────

/// Per-spot-shadow data uploaded to a GPU StructuredBuffer.
/// Matched by `deferred_lighting.slang`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct GpuSpotShadowData {
    /// Light-space view-projection matrices (up to MAX_SPOT_SHADOWS).
    pub matrices: [[[f32; 4]; 4]; MAX_SPOT_SHADOWS],
    /// Global light-buffer index for each shadow caster. 0xFFFFFFFF = unused.
    pub light_indices: [u32; MAX_SPOT_SHADOWS],
    /// How many of the slots are active ([0, MAX_SPOT_SHADOWS]).
    pub count: u32,
    /// PCF depth bias.
    pub depth_bias: f32,
    pub _pad: [u32; 2],
}

// ── SpotShadowConfig ──────────────────────────────────────────────────────────

/// Configuration for spot light shadow maps.
///
/// `SpotShadowConfig::default()` gives 1024×1024 shadow maps with standard PCF.
#[derive(Clone, Debug)]
pub struct SpotShadowConfig {
    /// Shadow map resolution (square). \[128, 4096\]. Default 1024.
    pub resolution: u32,
    /// Constant depth bias to prevent self-shadowing. Default 0.002.
    pub depth_bias: f32,
    /// Near clip plane for the spot perspective projection. Default 0.05.
    pub near: f32,
}

impl Default for SpotShadowConfig {
    fn default() -> Self {
        Self {
            resolution: 1024,
            depth_bias: 0.002,
            near: 0.05,
        }
    }
}

// ── SpotShadowPass ────────────────────────────────────────────────────────────

/// Renders perspective depth maps for up to `MAX_SPOT_SHADOWS` spot lights.
///
/// # Usage
/// ```ignore
/// let spot_shadows = SpotShadowPass::new(&engine)?;
/// deferred.set_spot_shadows(spot_shadows);
/// // The pass runs automatically inside DeferredPass::draw().
/// ```
pub struct SpotShadowPass {
    depth_program: MeshProgram,
    masked_depth_program: MeshProgram,
    /// GPU buffer for `GpuSpotShadowData`.
    pub shadow_buf: Buffer,
    pub config: SpotShadowConfig,
}

// ── Push constants ────────────────────────────────────────────────────────────

#[push_constants]
struct SpotDepthConstants {
    light_view_proj: [[f32; 4]; 4],
}

impl SpotShadowPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_config(engine, SpotShadowConfig::default())
    }

    pub fn with_config(engine: &Engine, config: SpotShadowConfig) -> Result<Self> {
        let depth_program = crate::shadow_pass::build_depth_program_pub(engine)?;
        let masked_depth_program = crate::shadow_pass::build_masked_depth_program_pub(engine)?;
        let shadow_buf = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<GpuSpotShadowData>() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        // Initialize with no active shadows.
        let empty = GpuSpotShadowData::zeroed();
        let init = GpuSpotShadowData {
            light_indices: [0xFFFFFFFF; MAX_SPOT_SHADOWS],
            ..empty
        };
        shadow_buf.write(0, bytemuck::bytes_of(&init))?;
        Ok(Self {
            depth_program,
            masked_depth_program,
            shadow_buf,
            config,
        })
    }

    /// Render shadow maps for up to `MAX_SPOT_SHADOWS` spot lights.
    ///
    /// Selects the highest-intensity lights first. Returns the number of active
    /// shadow casters.
    ///
    /// `spot_light_buf_offset` is the base index within the GPU `lights` buffer
    /// where spot lights start (after the directional light + point lights).
    pub fn draw(
        &mut self,
        scene: &Scene,
        spot_light_buf_offset: u32,
        frame: &RenderFrame,
        _engine: &Engine,
    ) -> Result<GpuSpotShadowData> {
        let res = self.config.resolution;

        // Pick the highest-intensity spot lights (up to MAX_SPOT_SHADOWS).
        let mut ranked: Vec<(usize, f32)> = scene
            .spot_lights
            .iter()
            .enumerate()
            .map(|(i, sl)| (i, sl.intensity))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let selected = &ranked[..ranked.len().min(MAX_SPOT_SHADOWS)];

        let mut matrices = [[[0.0f32; 4]; 4]; MAX_SPOT_SHADOWS];
        let mut light_indices = [0xFFFFFFFFu32; MAX_SPOT_SHADOWS];

        for (slot, &(sl_idx, _)) in selected.iter().enumerate() {
            let sl = &scene.spot_lights[sl_idx];

            // Perspective projection from the spot light's viewpoint.
            let lv_mat = spot_light_view(sl.position, sl.direction);
            let lp_mat = spot_light_proj(sl.outer_angle, self.config.near, sl.range);
            let lvp = lp_mat * lv_mat;

            matrices[slot] = lvp.to_cols_array_2d();
            light_indices[slot] = spot_light_buf_offset + sl_idx as u32;

            // Render depth into the slot's shadow map.
            let map_name = format!("spot_shadow_map_{slot}");
            let map_name_static = Box::leak(map_name.clone().into_boxed_str());
            let desc = spot_shadow_image_desc(res, map_name_static);
            let img = frame.image(&map_name, desc)?;
            draw_spot_shadow_batches(
                scene,
                frame,
                &img,
                &self.depth_program,
                &self.masked_depth_program,
                lvp,
            )?;
            img.register_as(&map_name);
        }

        let gpu_data = GpuSpotShadowData {
            matrices,
            light_indices,
            count: selected.len() as u32,
            depth_bias: self.config.depth_bias,
            _pad: [0; 2],
        };
        self.shadow_buf.write(0, bytemuck::bytes_of(&gpu_data))?;
        Ok(gpu_data)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn spot_light_view(pos: Vec3, dir: Vec3) -> Mat4 {
    let dir = dir.normalize();
    let up = if dir.y.abs() < 0.99 { Vec3::Y } else { Vec3::Z };
    Mat4::look_at_rh(pos, pos + dir, up)
}

fn spot_light_proj(outer_angle: f32, near: f32, far: f32) -> Mat4 {
    // fov = 2 * outer_angle (full cone angle)
    let fov = (outer_angle * 2.0).clamp(0.01, std::f32::consts::PI - 0.01);
    Mat4::perspective_rh(fov, 1.0, near, far.max(near + 0.1))
}

fn spot_shadow_image_desc(resolution: u32, debug_name: &'static str) -> ImageDesc {
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
        drm_format_modifier: None,
    }
}

fn draw_spot_shadow_batches(
    scene: &Scene,
    frame: &RenderFrame,
    shadow_image: &GraphImage,
    program: &MeshProgram,
    masked_program: &MeshProgram,
    light_view_proj: Mat4,
) -> Result<()> {
    use crate::scene::material::MaterialDomain;

    let constants = SpotDepthConstants {
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

        shadow_image.draw_mesh_depth_only_with_push_constants(
            mesh,
            prog,
            instance_buf,
            instance_count,
            &constants,
        )?;
    }
    Ok(())
}
