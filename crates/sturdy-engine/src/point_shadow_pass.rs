// Point light shadow maps using dual-paraboloid projection.
//
// Each point light casts shadows via two depth maps (front and back hemisphere).
// Up to MAX_POINT_SHADOWS (4) point lights cast shadows simultaneously.
//
// Integration: attach to DeferredPass via DeferredPass::set_point_shadows(pass).
//
// Roadmap: Track 6e — Point light shadow maps.

use std::path::PathBuf;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, Format, GraphImage, ImageDesc, ImageDimension,
    ImageUsage, MeshProgram, MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc,
    ShaderSource, ShaderStage, push_constants, scene::Scene,
};
use sturdy_engine_core::Extent3d;

const POINT_SHADOW_DEPTH_FRAGMENT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/point_shadow_depth_fragment.slang"
));

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

// ── Constants ─────────────────────────────────────────────────────────────────

pub const MAX_POINT_SHADOWS: usize = 4;

/// Static names for each shadow map slot — avoids per-frame heap allocation.
static FRONT_NAMES: [&str; MAX_POINT_SHADOWS] = [
    "point_shadow_front_0",
    "point_shadow_front_1",
    "point_shadow_front_2",
    "point_shadow_front_3",
];
static BACK_NAMES: [&str; MAX_POINT_SHADOWS] = [
    "point_shadow_back_0",
    "point_shadow_back_1",
    "point_shadow_back_2",
    "point_shadow_back_3",
];

// ── GPU data ──────────────────────────────────────────────────────────────────

/// Uploaded to `"point_shadow_data"` for the deferred lighting shader.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct GpuPointShadowData {
    /// World-to-light-local transforms for each shadow caster.
    pub world_to_light: [[[f32; 4]; 4]; MAX_POINT_SHADOWS],
    /// Near/far clip planes: [near, far] per caster.
    pub near_far: [[f32; 2]; MAX_POINT_SHADOWS],
    /// Global light-buffer index for each caster. 0xFFFFFFFF = unused.
    pub light_indices: [u32; MAX_POINT_SHADOWS],
    /// Active shadow caster count.
    pub count: u32,
    pub depth_bias: f32,
    pub _pad: [u32; 2],
}

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct PointShadowConfig {
    /// Shadow map resolution (square). Default 512.
    pub resolution: u32,
    /// Depth bias. Default 0.005.
    pub depth_bias: f32,
    /// Near clip plane. Default 0.05.
    pub near: f32,
}

impl Default for PointShadowConfig {
    fn default() -> Self {
        Self {
            resolution: 512,
            depth_bias: 0.005,
            near: 0.05,
        }
    }
}

// ── Push constants ────────────────────────────────────────────────────────────

#[push_constants]
struct DpDepthConstants {
    light_world_to_local: [[f32; 4]; 4],
    near: f32,
    far: f32,
    is_back: f32,
    _pad: f32,
}

// ── PointShadowPass ───────────────────────────────────────────────────────────

pub struct PointShadowPass {
    depth_program: MeshProgram,
    pub shadow_buf: Buffer,
    pub config: PointShadowConfig,
}

impl PointShadowPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_config(engine, PointShadowConfig::default())
    }

    pub fn with_config(engine: &Engine, config: PointShadowConfig) -> Result<Self> {
        let depth_program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                vertex: Some(ShaderDesc {
                    source: ShaderSource::File(engine_shader("shadow_depth_dual_paraboloid.slang")),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Vertex,
                    requires_ray_query: false,
                }),
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(POINT_SHADOW_DEPTH_FRAGMENT.into()),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                    requires_ray_query: false,
                },
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false,
                uses_depth: true,
            },
        )?;

        let shadow_buf = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<GpuPointShadowData>() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        let init = GpuPointShadowData {
            light_indices: [0xFFFFFFFF; MAX_POINT_SHADOWS],
            ..GpuPointShadowData::zeroed()
        };
        shadow_buf.write(0, bytemuck::bytes_of(&init))?;

        Ok(Self {
            depth_program,
            shadow_buf,
            config,
        })
    }

    /// Render dual-paraboloid shadow maps for the highest-intensity point lights.
    pub fn draw(
        &mut self,
        scene: &Scene,
        point_light_buf_offset: u32,
        frame: &RenderFrame,
        _engine: &Engine,
    ) -> Result<GpuPointShadowData> {
        let res = self.config.resolution;

        // Pick up to MAX_POINT_SHADOWS highest-intensity point lights.
        let mut ranked: Vec<(usize, f32)> = scene
            .point_lights
            .iter()
            .enumerate()
            .map(|(i, pl)| (i, pl.intensity))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let selected = &ranked[..ranked.len().min(MAX_POINT_SHADOWS)];

        let mut world_to_light = [[[0.0f32; 4]; 4]; MAX_POINT_SHADOWS];
        let mut near_far = [[0.0f32; 2]; MAX_POINT_SHADOWS];
        let mut light_indices = [0xFFFFFFFFu32; MAX_POINT_SHADOWS];

        for (slot, &(pl_idx, _)) in selected.iter().enumerate() {
            let pl = &scene.point_lights[pl_idx];
            let far = pl.range;
            let near = self.config.near;

            // Light-local frame: point light looks in all directions — no fixed "up".
            // We just need world → light_local (identity at light position).
            let wtl = Mat4::from_translation(-pl.position);
            world_to_light[slot] = wtl.to_cols_array_2d();
            near_far[slot] = [near, far];
            light_indices[slot] = point_light_buf_offset + pl_idx as u32;

            // Render front hemisphere (is_back = 0.0).
            let front_name = FRONT_NAMES[slot];
            let front_img = frame.image(front_name, dp_image_desc(res, front_name))?;
            draw_dp_batches(
                scene,
                frame,
                &front_img,
                &self.depth_program,
                wtl,
                near,
                far,
                false,
            )?;
            front_img.register_as(front_name);

            // Render back hemisphere (is_back = 1.0).
            let back_name = BACK_NAMES[slot];
            let back_img = frame.image(back_name, dp_image_desc(res, back_name))?;
            draw_dp_batches(
                scene,
                frame,
                &back_img,
                &self.depth_program,
                wtl,
                near,
                far,
                true,
            )?;
            back_img.register_as(back_name);
        }

        let gpu_data = GpuPointShadowData {
            world_to_light,
            near_far,
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

fn dp_image_desc(resolution: u32, debug_name: &'static str) -> ImageDesc {
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
    }
}

fn draw_dp_batches(
    scene: &Scene,
    frame: &RenderFrame,
    depth_img: &GraphImage,
    program: &MeshProgram,
    world_to_light: Mat4,
    near: f32,
    far: f32,
    is_back: bool,
) -> Result<()> {
    use crate::scene::material::MaterialDomain;
    let constants = DpDepthConstants {
        light_world_to_local: world_to_light.to_cols_array_2d(),
        near,
        far,
        is_back: if is_back { 1.0 } else { 0.0 },
        _pad: 0.0,
    };
    for (mesh_idx, instance_buf_opt, instance_count) in scene.drawable_batches() {
        let instance_buf = match instance_buf_opt {
            Some(b) => b,
            None => continue,
        };
        if instance_count == 0 {
            continue;
        }
        if scene.domain_at(mesh_idx) == MaterialDomain::Translucent {
            continue;
        }
        let mesh = scene.mesh_at(mesh_idx);
        frame.bind_buffer("instances", instance_buf);
        depth_img.draw_mesh_depth_only_with_push_constants(
            mesh,
            program,
            instance_buf,
            instance_count,
            &constants,
        )?;
    }
    Ok(())
}
