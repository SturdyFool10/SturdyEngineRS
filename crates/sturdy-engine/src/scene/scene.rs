use glam::{Mat4, Vec3, Vec4};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{
    batch::InstanceBatch,
    camera::{CameraId, CameraOutput, SceneCamera},
    commands::{SceneCommands, SceneView},
    object::{InstanceData, MeshId, ObjectId, ObjectKind, SceneObject},
};
use crate::{
    Buffer, BufferDesc, BufferUsage, ComputeProgram, DrawIndexedIndirectCommand, Engine, Error,
    Format, Frustum, GeometryBackend, GraphImage, ImageDesc, ImageDimension, ImageUsage, Mesh,
    MeshProgram, RenderFrame, Result, push_constants,
};
use sturdy_engine_core::Extent3d;

/// A directional light illuminating the entire scene.
///
/// Set via [`Scene::directional_light`] before calling [`Scene::draw`].
/// The default is a warm sunlight from above-right with a dark-grey ambient.
#[derive(Clone, Debug)]
pub struct DirectionalLight {
    /// World-space direction the light shines **toward** (normalized before upload).
    pub direction: Vec3,
    /// Diffuse + specular colour of the light.
    pub color: Vec3,
    /// Multiplier applied to both diffuse and specular contributions.
    pub intensity: f32,
    /// Constant ambient term added to every fragment regardless of normal.
    pub ambient: Vec3,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: Vec3::new(0.45, -0.75, -0.55).normalize(),
            color: Vec3::new(1.0, 0.95, 0.88),
            intensity: 1.0,
            ambient: Vec3::new(0.08, 0.08, 0.10),
        }
    }
}

/// An omnidirectional point light with physically correct inverse-square falloff.
///
/// Add to a scene via `scene.point_lights.push(PointLight { ... })`.
/// Changes take effect from the next `DeferredPass::draw()` call.
#[derive(Clone, Debug)]
pub struct PointLight {
    /// World-space position.
    pub position: Vec3,
    /// Linear RGB colour (linear, not sRGB).
    pub color: Vec3,
    /// Luminous intensity in candela (or a relative scale when `intensity` < 1000).
    pub intensity: f32,
    /// Maximum influence radius in world units. The light is clamped to zero at this distance.
    pub range: f32,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            color: Vec3::ONE,
            intensity: 100.0,
            range: 10.0,
        }
    }
}

/// A cone-shaped spot light with inner (full intensity) and outer (zero intensity) angles.
///
/// Add to a scene via `scene.spot_lights.push(SpotLight { ... })`.
#[derive(Clone, Debug)]
pub struct SpotLight {
    /// World-space position of the light source.
    pub position: Vec3,
    /// World-space direction the spot **shines toward** (normalized before upload).
    pub direction: Vec3,
    /// Linear RGB colour.
    pub color: Vec3,
    /// Luminous intensity in candela (or a relative scale).
    pub intensity: f32,
    /// Maximum influence radius in world units.
    pub range: f32,
    /// Inner cone half-angle in radians — full intensity inside this cone.
    pub inner_angle: f32,
    /// Outer cone half-angle in radians — zero intensity outside this cone.
    pub outer_angle: f32,
}

impl Default for SpotLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            direction: Vec3::new(0.0, -1.0, 0.0),
            color: Vec3::ONE,
            intensity: 500.0,
            range: 20.0,
            inner_angle: 0.35, // ~20°
            outer_angle: 0.52, // ~30°
        }
    }
}

// ── Area lights ───────────────────────────────────────────────────────────────

/// A rectangular area light. Illuminates via polygon solid-angle diffuse +
/// representative-point specular (same BVH as point/spot lights).
///
/// `right` and `up` define the local axes; the light surface spans
/// `[-half_extents.x, +half_extents.x] × [-half_extents.y, +half_extents.y]`.
#[derive(Clone, Debug)]
pub struct RectLight {
    /// World-space center.
    pub position: Vec3,
    /// Local X axis (right direction), unit vector.
    pub right: Vec3,
    /// Local Y axis (up direction), unit vector.
    pub up: Vec3,
    /// \[half_width, half_height\] — half-dimensions along `right` and `up`.
    pub half_extents: [f32; 2],
    /// Emitted radiance (linear RGB).
    pub color: Vec3,
    /// Intensity multiplier. Default 1.0.
    pub intensity: f32,
    /// Maximum influence distance in world units.
    pub range: f32,
    /// If true, illuminates from both sides of the rectangle.
    pub two_sided: bool,
}

impl Default for RectLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            right: Vec3::X,
            up: Vec3::Y,
            half_extents: [0.5, 0.5],
            color: Vec3::ONE,
            intensity: 1.0,
            range: 10.0,
            two_sided: false,
        }
    }
}

/// A spherical area light (emissive sphere, e.g. bare bulb, sun disc).
///
/// `radius` is the source radius (controls apparent size and specular highlight
/// softness). `range` is the maximum influence distance.
#[derive(Clone, Debug)]
pub struct SphereLight {
    pub position: Vec3,
    /// Emissive radius of the source (not the influence radius). Default 0.1.
    pub radius: f32,
    pub color: Vec3,
    pub intensity: f32,
    /// Maximum influence distance.
    pub range: f32,
}

impl Default for SphereLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            radius: 0.1,
            color: Vec3::ONE,
            intensity: 100.0,
            range: 10.0,
        }
    }
}

/// A disc (circular) area light — e.g. a recessed ceiling downlight.
#[derive(Clone, Debug)]
pub struct DiskLight {
    pub position: Vec3,
    /// Outward normal of the disc surface.
    pub normal: Vec3,
    /// Disc radius.
    pub radius: f32,
    pub color: Vec3,
    pub intensity: f32,
    pub range: f32,
    pub two_sided: bool,
}

impl Default for DiskLight {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            normal: Vec3::NEG_Y,
            radius: 0.3,
            color: Vec3::ONE,
            intensity: 500.0,
            range: 10.0,
            two_sided: false,
        }
    }
}

// ── GPU-packed light data ─────────────────────────────────────────────────────
// Matches GpuLightData in deferred_lighting.slang exactly (96 bytes = 6 × float4).
//
// Layout (backward-compatible: bytes 0-63 unchanged from the old 64-byte struct):
//   0-15:  color_intensity   rgb × intensity
//   16-31: position_range    world pos + influence range
//   32-47: direction_kind    dir/normal xyz + kind (float: 0=dir,1=pt,2=spot,3=rect,4=sphere,5=disk)
//   48-63: misc              [cos_inner, cos_outer, area_radius, two_sided 0/1]
//   64-79: right_half_w      rect: right.xyz + half_width;  others: zero
//   80-95: up_half_h         rect: up.xyz + half_height;    others: zero

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuLightData {
    color_intensity: [f32; 4],
    position_range: [f32; 4],
    direction_kind: [f32; 4],
    misc: [f32; 4],         // [cos_inner, cos_outer, area_radius, two_sided]
    right_half_w: [f32; 4], // rect: right.xyz + half_width
    up_half_h: [f32; 4],    // rect: up.xyz + half_height
}

impl GpuLightData {
    pub(crate) fn from_directional(light: &DirectionalLight) -> Self {
        let toward = (-light.direction).normalize();
        let lum = light.intensity;
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [toward.x, toward.y, toward.z, 0.0],
            direction_kind: [0.0, 0.0, 0.0, 0.0], // kind=0
            misc: [1.0, 1.0, 0.0, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_point(light: &PointLight) -> Self {
        let lum = light.intensity;
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [0.0, 0.0, 0.0, 1.0], // kind=1
            misc: [1.0, 1.0, 0.0, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_spot(light: &SpotLight) -> Self {
        let lum = light.intensity;
        let dir = light.direction.normalize();
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [dir.x, dir.y, dir.z, 2.0], // kind=2
            misc: [light.inner_angle.cos(), light.outer_angle.cos(), 0.0, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_rect(light: &RectLight) -> Self {
        let lum = light.intensity;
        let n = light.right.cross(light.up).normalize();
        let r = light.right.normalize();
        let u = light.up.normalize();
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [n.x, n.y, n.z, 3.0], // kind=3, normal = cross(right, up)
            misc: [0.0, 0.0, 0.0, if light.two_sided { 1.0 } else { 0.0 }],
            right_half_w: [r.x, r.y, r.z, light.half_extents[0]],
            up_half_h: [u.x, u.y, u.z, light.half_extents[1]],
        }
    }

    pub(crate) fn from_sphere_area(light: &SphereLight) -> Self {
        let lum = light.intensity;
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [0.0, 0.0, 0.0, 4.0], // kind=4
            misc: [1.0, 1.0, light.radius, 0.0],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }

    pub(crate) fn from_disk(light: &DiskLight) -> Self {
        let lum = light.intensity;
        let n = light.normal.normalize();
        Self {
            color_intensity: [
                light.color.x * lum,
                light.color.y * lum,
                light.color.z * lum,
                1.0,
            ],
            position_range: [
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
            ],
            direction_kind: [n.x, n.y, n.z, 5.0], // kind=5
            misc: [
                0.0,
                0.0,
                light.radius,
                if light.two_sided { 1.0 } else { 0.0 },
            ],
            right_half_w: [0.0; 4],
            up_half_h: [0.0; 4],
        }
    }
}

/// GPU-layout mirror of the data read by `lit_fragment.slang` (forward path only).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LightingUniforms {
    dir_direction: [f32; 4],    // xyz = toward-light dir, w = intensity
    dir_color: [f32; 4],        // xyz = colour, w = 0
    ambient: [f32; 4],          // xyz = ambient colour, w = 0
    camera_world_pos: [f32; 4], // xyz = camera pos, w = 0
}

impl LightingUniforms {
    fn from_light_and_camera(light: &DirectionalLight, camera_world_pos: Vec3) -> Self {
        let dir = (-light.direction).normalize();
        Self {
            dir_direction: [dir.x, dir.y, dir.z, light.intensity],
            dir_color: [light.color.x, light.color.y, light.color.z, 0.0],
            ambient: [light.ambient.x, light.ambient.y, light.ambient.z, 0.0],
            camera_world_pos: [
                camera_world_pos.x,
                camera_world_pos.y,
                camera_world_pos.z,
                0.0,
            ],
        }
    }
}

/// Per-mesh material parameters for the built-in lit shader.
///
/// Set via [`Scene::set_material`] after calling [`Scene::add_mesh`]. The default
/// is a white, fully opaque, mid-roughness dielectric (no metallic, no emission).
///
/// # In the shader
///
/// `lit_fragment.slang` reads a `StructuredBuffer<MaterialConstants> material_desc`
/// buffer that [`Scene::draw`] binds automatically. Custom fragment shaders can
/// declare the same binding to receive these values.
#[derive(Clone, Debug)]
pub struct MaterialDescriptor {
    /// Base colour (linear RGB). Multiplied with the lighting result.
    pub albedo: Vec3,
    /// Opacity in \[0, 1\]. Values below 1.0 require an alpha-blend program.
    pub opacity: f32,
    /// Self-emission added after lighting (HDR values are valid).
    pub emissive: Vec3,
    /// Metallic factor in \[0, 1\]. 0 = dielectric, 1 = conductor.
    pub metallic: f32,
    /// Surface roughness in \[0, 1\]. 0 = mirror, 1 = fully diffuse.
    pub roughness: f32,
    /// Whether a tangent-space normal map is bound for this mesh.
    ///
    /// Set automatically by [`Scene::set_normal_map`]. When `true`,
    /// `gbuffer_fragment.slang` applies a TBN transform using the bound
    /// `"normal_map"` texture; when `false`, the geometric normal is used.
    pub has_normal_map: bool,
}

impl Default for MaterialDescriptor {
    fn default() -> Self {
        Self {
            albedo: Vec3::ONE,
            opacity: 1.0,
            emissive: Vec3::ZERO,
            metallic: 0.0,
            roughness: 0.5,
            has_normal_map: false,
        }
    }
}

/// GPU-layout mirror of `MaterialConstants` in the lit and G-Buffer shaders.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialConstants {
    /// xyz = albedo, w = opacity
    albedo: [f32; 4],
    /// xyz = emissive, w = metallic
    emissive_metallic: [f32; 4],
    /// x = roughness, y = has_normal_map (1.0 = yes), zw = unused
    roughness_flags: [f32; 4],
}

impl MaterialConstants {
    fn from_descriptor(desc: &MaterialDescriptor) -> Self {
        Self {
            albedo: [desc.albedo.x, desc.albedo.y, desc.albedo.z, desc.opacity],
            emissive_metallic: [
                desc.emissive.x,
                desc.emissive.y,
                desc.emissive.z,
                desc.metallic,
            ],
            roughness_flags: [
                desc.roughness,
                if desc.has_normal_map { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ],
        }
    }
}

/// Per-mesh material state owned by the scene.
struct MeshMaterial {
    descriptor: MaterialDescriptor,
    gpu_buffer: Option<Buffer>,
    dirty: bool,
    /// Optional tangent-space normal map for the G-Buffer fill pass.
    ///
    /// Set via [`Scene::set_normal_map`]. When present, it is bound as
    /// `"normal_map"` before each G-Buffer draw call for this mesh.
    normal_map: Option<std::sync::Arc<crate::Image>>,
    /// Full expression-based material for the deferred G-Buffer path.
    ///
    /// When set, `DeferredPass` compiles a unique G-Buffer fragment shader
    /// variant from this material's expression tree and uses it instead of the
    /// default G-Buffer program. The `descriptor` above is still used by the
    /// forward lit path and for `set_material` compatibility.
    unified: Option<super::material::UnifiedMaterial>,
}

/// Push constants sent to the vertex and fragment shaders for each camera draw call.
///
/// Shaders must declare `uniform CameraConstants cam` to receive these values.
/// `time` is the elapsed time in seconds since application start — available
/// in fragment shaders for procedural animation and UV scrolling.
#[push_constants]
pub struct CameraConstants {
    pub view_proj: [[f32; 4]; 4],
    pub previous_view_proj: [[f32; 4]; 4],
    /// Elapsed time in seconds (application start = 0). Use in material expressions.
    pub time: f32,
    pub _pad: [f32; 3],
}

impl CameraConstants {
    pub fn identity() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            previous_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            time: 0.0,
            _pad: [0.0; 3],
        }
    }

    pub fn from_camera(camera: &SceneCamera) -> Self {
        Self {
            view_proj: camera.view_proj().to_cols_array_2d(),
            previous_view_proj: camera.previous_view_proj.to_cols_array_2d(),
            time: 0.0,
            _pad: [0.0; 3],
        }
    }

    pub fn with_time(mut self, time: f32) -> Self {
        self.time = time;
        self
    }
}

/// A managed collection of meshes, object instances, and cameras.
///
/// # Workflow
///
/// ```ignore
/// // At init:
/// let mesh_id = scene.add_mesh(Mesh::cube(&engine, 1.0)?, MeshProgram::unlit(&engine)?);
/// let _obj    = scene.add_object(mesh_id, ObjectKind::Static);
///
/// // Each frame:
/// scene.prepare(&engine)?;
/// scene.draw(cam.view_matrix(), cam.projection_matrix(aspect), &hdr_out, &frame)?;
/// ```
///
/// # Instance buffer convention
///
/// Every draw pass binds the instance storage buffer under the name `"instances"`.
/// Vertex shaders must declare:
/// ```slang
/// struct InstanceData { float4x4 model; };
/// StructuredBuffer<InstanceData> instances;
/// ```
/// and read `instances[SV_InstanceID].model` for the per-instance model matrix.
pub struct Scene {
    pub(super) meshes: Vec<(Mesh, MeshProgram)>,
    /// Parallel to `meshes` — one material entry per registered mesh.
    materials: Vec<MeshMaterial>,
    pub(super) objects: Vec<SceneObject>,
    pub(super) cameras: Vec<SceneCamera>,
    batches: HashMap<u32, InstanceBatch>,
    /// Atomic ID counter so `reserve_object_id()` works from any thread.
    next_object_id: AtomicU32,
    /// Atomic ID counter so `reserve_mesh_id()` works from any thread.
    next_mesh_id: AtomicU32,
    /// Thread-safe queue for structural mutations posted from worker threads.
    commands: SceneCommands,
    /// Directional light applied to all rendering passes.
    pub directional_light: DirectionalLight,
    /// Point lights included in the deferred lighting pass.
    pub point_lights: Vec<PointLight>,
    /// Spot lights included in the deferred lighting pass.
    pub spot_lights: Vec<SpotLight>,
    /// Rectangular area lights (ceiling panels, windows, screens).
    pub rect_lights: Vec<RectLight>,
    /// Spherical area lights (bare bulbs, neon tubes approximated as spheres).
    pub sphere_area_lights: Vec<SphereLight>,
    /// Disc area lights (recessed downlights, portals).
    pub disk_lights: Vec<DiskLight>,
    /// Persistent GPU buffer for the forward-path `LightingUniforms`.
    light_buffer: Option<Buffer>,
    /// GPU-resident array of [`GpuLightData`] for the deferred lighting pass.
    pub(crate) deferred_lights_buffer: Option<Buffer>,
    geometry_backend: GeometryBackend,
    /// Lazily created GPU frustum culling compute program.
    culling_program: Option<ComputeProgram>,
    /// Set by `cull_gpu()` each frame; tells the draw path to use `total_count`
    /// rather than `indirect_commands.len()` as the indirect draw count.
    gpu_cull_active: bool,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            meshes: Vec::new(),
            materials: Vec::new(),
            objects: Vec::new(),
            cameras: Vec::new(),
            batches: HashMap::new(),
            next_object_id: AtomicU32::new(0),
            next_mesh_id: AtomicU32::new(0),
            commands: SceneCommands::new(),
            directional_light: DirectionalLight::default(),
            point_lights: Vec::new(),
            spot_lights: Vec::new(),
            rect_lights: Vec::new(),
            sphere_area_lights: Vec::new(),
            disk_lights: Vec::new(),
            light_buffer: None,
            deferred_lights_buffer: None,
            geometry_backend: GeometryBackend::ClassicVertex,
            culling_program: None,
            gpu_cull_active: false,
        }
    }

    /// Select the geometry front-end for draw submission.
    ///
    /// Call this once at init (after checking [`GeometryRendererCaps`]) to opt
    /// into frustum culling and indirect draw. Safe to change between frames.
    ///
    /// ```rust
    /// let caps = GeometryRendererCaps::from_caps(&engine.caps());
    /// scene.set_geometry_backend(caps.best_opaque_backend());
    /// ```
    pub fn set_geometry_backend(&mut self, backend: GeometryBackend) {
        self.geometry_backend = backend;
    }

    /// Returns the currently active geometry backend.
    pub fn geometry_backend(&self) -> GeometryBackend {
        self.geometry_backend
    }

    /// Iterate batches whose `UnifiedMaterial` has `MaterialDomain::Translucent`.
    ///
    /// Used by `OitPass` to draw only transparent geometry in the collect pass.
    pub fn translucent_batches(&self) -> impl Iterator<Item = (usize, Option<&Buffer>, u32)> {
        self.batches
            .values()
            .filter(|b| {
                let idx = b.mesh_idx as usize;
                self.materials
                    .get(idx)
                    .and_then(|m| m.unified.as_ref())
                    .map(|u| u.domain == super::material::MaterialDomain::Translucent)
                    .unwrap_or(false)
            })
            .map(|b| (b.mesh_idx as usize, b.gpu_buffer.as_ref(), b.total_count()))
    }

    /// Dispatch the GPU frustum culling compute shader for all batches.
    ///
    /// Call this once per frame **after** `scene.prepare()` and **before** the
    /// draw pass. Only active when `geometry_backend == ComputeIndirect`.
    ///
    /// The compute shader writes one `DrawIndexedIndirectCommand` per instance
    /// slot: visible instances get `instance_count = 1`, invisible ones get
    /// `instance_count = 0` (silently skipped by the GPU). The CPU draws all N
    /// slots via `DrawIndexedIndirect` — no readback or atomic counter needed.
    ///
    /// # Example
    /// ```ignore
    /// scene.set_geometry_backend(GeometryBackend::ComputeIndirect);
    /// // Each frame:
    /// scene.prepare(&engine)?;
    /// scene.cull_gpu(view_proj, &frame, &engine)?;
    /// deferred.draw(&mut scene, view, proj, &output, &frame, &engine, time)?;
    /// ```
    pub fn cull_gpu(
        &mut self,
        view_proj: Mat4,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> Result<()> {
        if self.geometry_backend != GeometryBackend::ComputeIndirect {
            return Ok(());
        }

        // Lazily compile the culling compute program.
        if self.culling_program.is_none() {
            let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("shaders")
                .join("cull_compute.slang");
            self.culling_program = Some(ComputeProgram::load(engine, path)?);
        }
        //panic allowed, reason = "culling_program was initialized when absent immediately above"
        let program = self.culling_program.as_ref().unwrap();

        // Extract frustum planes from the VP matrix.
        let frustum = Frustum::from_view_proj(view_proj);
        let planes: [[f32; 4]; 6] = {
            let raw = frustum.planes_raw();
            [
                raw[0].to_array(),
                raw[1].to_array(),
                raw[2].to_array(),
                raw[3].to_array(),
                raw[4].to_array(),
                raw[5].to_array(),
            ]
        };

        for batch in self.batches.values() {
            let total = batch.total_count();
            if total == 0 {
                continue;
            }

            let bounds_buf = match &batch.bounds_gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            let indirect_buf = match &batch.indirect_gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            let mesh_idx = batch.mesh_idx as usize;
            let mesh = &self.meshes[mesh_idx].0;

            let index_count = if mesh.is_indexed() {
                mesh.index_count
            } else {
                mesh.vertex_count
            };
            let first_index = 0u32;
            let vertex_offset = 0i32;

            #[repr(C)]
            #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
            struct CullConstants {
                frustum_planes: [[f32; 4]; 6],
                instance_count: u32,
                index_count: u32,
                first_index: u32,
                vertex_offset: i32,
            }
            let constants = CullConstants {
                frustum_planes: planes,
                instance_count: total,
                index_count,
                first_index,
                vertex_offset,
            };

            // Bind source + dest buffers, then dispatch the culling compute.
            frame.bind_buffer("instance_bounds", bounds_buf);
            frame.bind_buffer("indirect_commands", indirect_buf);

            let groups = [(total + 63) / 64, 1, 1];
            frame.dispatch_compute_auto(
                format!("cull-batch-{mesh_idx}"),
                program,
                &constants,
                groups,
            )?;
        }

        self.gpu_cull_active = true;
        Ok(())
    }

    /// Iterate over all drawable batches as `(mesh_index, instance_gpu_buffer, instance_count)`.
    ///
    /// Used by external passes (e.g. `ShadowPass`) that need to draw scene
    /// geometry with a custom shader. The GPU instance buffer is `None` when
    /// the batch has not yet been prepared for this frame.
    pub fn drawable_batches(&self) -> impl Iterator<Item = (usize, Option<&Buffer>, u32)> {
        self.batches
            .values()
            .map(|b| (b.mesh_idx as usize, b.gpu_buffer.as_ref(), b.total_count()))
    }

    /// Number of registered meshes.
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Return the `Mesh` at the given index (as registered by `add_mesh`).
    pub fn mesh_at(&self, index: usize) -> &Mesh {
        &self.meshes[index].0
    }

    /// Register a mesh+program pair. Returns a `MeshId` used to spawn objects.
    ///
    /// A default [`MaterialDescriptor`] (white, opaque, roughness=0.5) is assigned.
    /// Override it with [`set_material`](Self::set_material).
    pub fn add_mesh(&mut self, mesh: Mesh, program: MeshProgram) -> MeshId {
        let id = MeshId(self.next_mesh_id.fetch_add(1, Ordering::Relaxed));
        self.meshes.push((mesh, program));
        self.materials.push(MeshMaterial {
            descriptor: MaterialDescriptor::default(),
            gpu_buffer: None,
            dirty: true,
            normal_map: None,
            unified: None,
        });
        id
    }

    /// Set the material parameters for a registered mesh.
    ///
    /// Changes take effect on the next [`prepare`](Self::prepare) call.
    pub fn set_material(&mut self, id: MeshId, descriptor: MaterialDescriptor) {
        if let Some(mat) = self.materials.get_mut(id.0 as usize) {
            mat.descriptor = descriptor;
            mat.dirty = true;
        }
    }

    /// Bind a tangent-space normal map texture to a mesh.
    ///
    /// Sets `MaterialDescriptor::has_normal_map = true` automatically. The
    /// texture must remain alive for the duration of rendering — the scene
    /// holds an `Arc` clone. Pass `None` to remove the normal map.
    ///
    /// # Example
    /// ```ignore
    /// let nmap = engine.load_texture_2d("assets/wall_normal.png");
    /// if let Some(img) = nmap.get() {
    ///     scene.set_normal_map(wall_id, Some(Arc::new(img)));
    /// }
    /// ```
    pub fn set_normal_map(&mut self, id: MeshId, texture: Option<std::sync::Arc<crate::Image>>) {
        if let Some(mat) = self.materials.get_mut(id.0 as usize) {
            mat.descriptor.has_normal_map = texture.is_some();
            mat.normal_map = texture;
            mat.dirty = true;
        }
    }

    /// Return the current material descriptor for a mesh.
    pub fn material(&self, id: MeshId) -> Option<&MaterialDescriptor> {
        self.materials.get(id.0 as usize).map(|m| &m.descriptor)
    }

    /// Set a full expression-based material for a mesh.
    ///
    /// `DeferredPass` will compile a unique G-Buffer fragment shader from this
    /// material's expression tree and cache it by content hash. Each distinct
    /// material expression compiles to exactly one shader variant — two meshes
    /// with identical expressions share the same compiled variant.
    ///
    /// Textures referenced in the material (via `MaterialExpr::texture("name")`)
    /// must be bound each frame before `DeferredPass::draw`:
    /// ```ignore
    /// frame.bind_image("rock_albedo", &rock_tex);
    /// deferred.draw(&mut scene, view, proj, &output, &frame, &engine)?;
    /// ```
    pub fn set_unified_material(&mut self, id: MeshId, material: super::material::UnifiedMaterial) {
        if let Some(mat) = self.materials.get_mut(id.0 as usize) {
            mat.unified = Some(material);
        }
    }

    /// Return the expression-based material for a mesh, if one has been set.
    pub fn unified_material(&self, id: MeshId) -> Option<&super::material::UnifiedMaterial> {
        self.materials
            .get(id.0 as usize)
            .and_then(|m| m.unified.as_ref())
    }

    /// Return the expression-based material by mesh index (used internally by `DeferredPass`).
    pub(crate) fn unified_material_at(
        &self,
        mesh_idx: usize,
    ) -> Option<&super::material::UnifiedMaterial> {
        self.materials
            .get(mesh_idx)
            .and_then(|m| m.unified.as_ref())
    }

    /// Return the per-mesh normal map override, if set (used by `DeferredPass`).
    pub(crate) fn normal_map_at(&self, mesh_idx: usize) -> Option<&std::sync::Arc<crate::Image>> {
        self.materials
            .get(mesh_idx)
            .and_then(|m| m.normal_map.as_ref())
    }

    /// Return the material GPU buffer for a mesh (used by `DeferredPass`).
    pub(crate) fn material_gpu_buffer_at(&self, mesh_idx: usize) -> Option<&Buffer> {
        self.materials
            .get(mesh_idx)
            .and_then(|m| m.gpu_buffer.as_ref())
    }

    /// Return the `MaterialDomain` of the given mesh (used by the shadow pass).
    ///
    /// Returns `Opaque` for meshes without a `UnifiedMaterial`.
    pub(crate) fn domain_at(&self, mesh_idx: usize) -> super::material::MaterialDomain {
        self.materials
            .get(mesh_idx)
            .and_then(|m| m.unified.as_ref())
            .map(|u| u.domain)
            .unwrap_or(super::material::MaterialDomain::Opaque)
    }

    /// Add an object instance at the world origin. Returns an `ObjectId` for later
    /// transform updates via [`set_transform`](Self::set_transform).
    ///
    /// Must be called from the render thread (requires `&mut self`). For
    /// worker-thread object creation, use [`reserve_object_id`] +
    /// [`commands().add_object`](SceneCommands::add_object) instead.
    pub fn add_object(&mut self, mesh_id: MeshId, kind: ObjectKind) -> ObjectId {
        self.add_object_at(mesh_id, Mat4::IDENTITY, kind)
    }

    /// Add an object instance with an explicit initial transform.
    pub fn add_object_at(
        &mut self,
        mesh_id: MeshId,
        transform: Mat4,
        kind: ObjectKind,
    ) -> ObjectId {
        let id = ObjectId(self.next_object_id.fetch_add(1, Ordering::Relaxed));
        self.objects
            .push(SceneObject::new(mesh_id, transform, kind));
        id
    }

    /// Update an object's world transform. **Callable from any thread.**
    ///
    /// Uses atomic Release/Acquire semantics — the update is visible to
    /// `prepare()` without any lock. Multiple threads may call this for
    /// **different** `ObjectId`s simultaneously with no contention.
    ///
    /// For static objects the dirty flag is set so the GPU buffer is
    /// re-uploaded in the next `prepare()`.
    pub fn set_transform(&self, id: ObjectId, transform: Mat4) {
        if let Some(obj) = self.objects.get(id.0 as usize) {
            obj.set_transform_atomic(transform);
        }
    }

    // ── Thread-safe structural mutation API ───────────────────────────────────

    /// Return the thread-safe command queue for this scene.
    ///
    /// Any thread can post structural mutations (add_mesh, add_object,
    /// remove_object, set_material) here. They are applied by `prepare()`.
    pub fn commands(&self) -> &SceneCommands {
        &self.commands
    }

    /// Create a read-only view of the scene for worker-thread queries.
    ///
    /// # Safety
    ///
    /// The caller must ensure `prepare()` is not called while the returned
    /// `SceneView` is live, and that no structural mutations occur directly
    /// on the scene (commands posted to `SceneCommands` are fine).
    pub unsafe fn view(&self) -> SceneView<'_> {
        unsafe { SceneView::new(self) }
    }

    // ── Internal: command implementations (called by SceneCommands) ──────────

    /// Mark an object slot as dead so it contributes no instances.
    ///
    /// Uses a sentinel `MeshId(u32::MAX)` — the batch for that ID never exists,
    /// so the object is silently skipped in `prepare()`. Slot reclamation for
    /// ID reuse can be added later without changing the public API.
    pub(super) fn remove_object_deferred(&mut self, id: ObjectId) {
        if let Some(obj) = self.objects.get_mut(id.0 as usize) {
            // Redirect the object to a non-existent batch by changing its mesh_id
            // to a sentinel. The static_dirty flag no longer matters.
            obj.mesh_id = MeshId(u32::MAX);
        }
    }

    /// Add a camera with an offscreen `RenderTarget`. Returns a `CameraId`.
    pub fn add_camera(&mut self, camera: SceneCamera) -> CameraId {
        let id = CameraId(self.cameras.len() as u32);
        self.cameras.push(camera);
        id
    }

    /// Borrow a camera mutably — update view/projection before calling `render`.
    pub fn camera_mut(&mut self, id: CameraId) -> Option<&mut SceneCamera> {
        self.cameras.get_mut(id.0 as usize)
    }

    /// Upload instance data for all dirty batches.
    ///
    /// Call once per frame after all `set_transform` calls, before `render`.
    pub fn prepare(&mut self, engine: &Engine) -> Result<()> {
        // Drain structural mutations queued from worker threads.
        // `take_all` extracts the Vec without borrowing self, so we can then
        // pass `&mut self` to each command.
        for cmd in self.commands.take_all() {
            cmd(self);
        }

        self.gpu_cull_active = false;

        // Clear dynamic lists; static lists persist across frames.
        for batch in self.batches.values_mut() {
            batch.dynamic_instances.clear();
        }

        // First pass: detect which static batches are dirty.
        for obj in &self.objects {
            if matches!(obj.kind, ObjectKind::Static) && obj.static_dirty.load(Ordering::Acquire) {
                if let Some(batch) = self.batches.get_mut(&obj.mesh_id.0) {
                    batch.static_dirty = true;
                }
                obj.static_dirty.store(false, Ordering::Release);
            }
        }

        // Clear static lists for dirty batches before rebuilding.
        for batch in self.batches.values_mut() {
            if batch.static_dirty {
                batch.static_instances.clear();
            }
        }

        // Second pass: fill instance lists.
        // Use atomic Acquire loads for transforms — visible to any Release store
        // performed by worker threads via set_transform().
        for obj in &self.objects {
            let transform = obj.transform.load();
            let batch = self
                .batches
                .entry(obj.mesh_id.0)
                .or_insert_with(|| InstanceBatch::new(obj.mesh_id.0));
            match obj.kind {
                ObjectKind::Static => {
                    if batch.static_dirty || batch.static_instances.is_empty() {
                        batch
                            .static_instances
                            .push(InstanceData::from_transform(transform));
                    }
                }
                ObjectKind::Dynamic => {
                    batch
                        .dynamic_instances
                        .push(InstanceData::from_transform(transform));
                }
            }
        }

        // Pre-collect mesh bounding spheres so we can borrow them inside the
        // mutable batch loop without conflicting with &self.meshes.
        let mesh_spheres: Vec<crate::BoundingSphere> = self
            .meshes
            .iter()
            .map(|(mesh, _)| mesh.bounding_sphere)
            .collect();
        let gpu_cull = self.geometry_backend == GeometryBackend::ComputeIndirect;

        for batch in self.batches.values_mut() {
            batch.prepare(engine)?;
            batch.indirect_commands.clear();

            if gpu_cull {
                if let Some(sphere) = mesh_spheres.get(batch.mesh_idx as usize) {
                    let spheres: Vec<[f32; 4]> = batch
                        .static_instances
                        .iter()
                        .chain(batch.dynamic_instances.iter())
                        .map(|inst| {
                            let ws = sphere.transform(Mat4::from_cols_array_2d(&inst.model));
                            [ws.center.x, ws.center.y, ws.center.z, ws.radius]
                        })
                        .collect();
                    batch.prepare_bounds(engine, &spheres)?;
                    batch.prepare_indirect_slots(engine, batch.total_count())?;
                }
            }
        }

        // Ensure the lighting uniform buffer exists.
        if self.light_buffer.is_none() {
            self.light_buffer = Some(engine.create_buffer(BufferDesc {
                size: std::mem::size_of::<LightingUniforms>() as u64,
                usage: BufferUsage::STORAGE,
            })?);
        }

        // Create or update per-mesh material buffers.
        for mat in &mut self.materials {
            if mat.gpu_buffer.is_none() {
                mat.gpu_buffer = Some(engine.create_buffer(BufferDesc {
                    size: std::mem::size_of::<MaterialConstants>() as u64,
                    usage: BufferUsage::STORAGE,
                })?);
                mat.dirty = true;
            }
            if mat.dirty {
                if let Some(buf) = &mat.gpu_buffer {
                    let constants = MaterialConstants::from_descriptor(&mat.descriptor);
                    buf.write(0, bytemuck::bytes_of(&constants))?;
                }
                mat.dirty = false;
            }
        }

        Ok(())
    }

    /// Draw all offscreen cameras into the render frame.
    ///
    /// Call this before the main camera pass so offscreen textures are available
    /// as named frame images when the main scene draws objects that sample them.
    /// Ensure the lighting GPU buffer exists and upload + bind the current
    /// directional light for use in the deferred lighting pass.
    ///
    /// Called by `DeferredPass::draw()`. Also allocates the buffer on first call.
    pub fn prepare_deferred_lighting(
        &mut self,
        view: Mat4,
        engine: &Engine,
        frame: &RenderFrame,
    ) -> Result<()> {
        // Ensure forward-path buffer exists.
        if self.light_buffer.is_none() {
            self.light_buffer = Some(engine.create_buffer(crate::BufferDesc {
                size: std::mem::size_of::<LightingUniforms>() as u64,
                usage: crate::BufferUsage::STORAGE,
            })?);
        }
        // Ensure per-mesh material buffers exist.
        for mat in &mut self.materials {
            if mat.gpu_buffer.is_none() {
                mat.gpu_buffer = Some(engine.create_buffer(crate::BufferDesc {
                    size: std::mem::size_of::<MaterialConstants>() as u64,
                    usage: crate::BufferUsage::STORAGE,
                })?);
                mat.dirty = true;
            }
            if mat.dirty {
                if let Some(buf) = &mat.gpu_buffer {
                    let constants = MaterialConstants::from_descriptor(&mat.descriptor);
                    buf.write(0, bytemuck::bytes_of(&constants))?;
                }
                mat.dirty = false;
            }
        }

        // Build the combined GPU light array for the deferred pass.
        // Order: directional (index 0), point, spot, rect, sphere, disk.
        // The BVH covers all non-directional lights via their array indices.
        let mut gpu_lights: Vec<GpuLightData> = Vec::new();
        gpu_lights.push(GpuLightData::from_directional(&self.directional_light));
        for pl in &self.point_lights {
            gpu_lights.push(GpuLightData::from_point(pl));
        }
        for sl in &self.spot_lights {
            gpu_lights.push(GpuLightData::from_spot(sl));
        }
        for rl in &self.rect_lights {
            gpu_lights.push(GpuLightData::from_rect(rl));
        }
        for sl in &self.sphere_area_lights {
            gpu_lights.push(GpuLightData::from_sphere_area(sl));
        }
        for dl in &self.disk_lights {
            gpu_lights.push(GpuLightData::from_disk(dl));
        }

        let needed_bytes = (gpu_lights.len() * std::mem::size_of::<GpuLightData>()) as u64;
        let needs_realloc = self
            .deferred_lights_buffer
            .as_ref()
            .map(|b| b.desc().size < needed_bytes)
            .unwrap_or(true);
        if needs_realloc {
            let cap = (gpu_lights.len().next_power_of_two().max(16)
                * std::mem::size_of::<GpuLightData>()) as u64;
            self.deferred_lights_buffer = Some(engine.create_buffer(crate::BufferDesc {
                size: cap,
                usage: crate::BufferUsage::STORAGE,
            })?);
        }
        if let Some(buf) = &self.deferred_lights_buffer {
            buf.write(0, bytemuck::cast_slice(&gpu_lights))?;
            frame.bind_buffer("lights", buf);
        }

        self.update_lighting_uniform(view, frame)
    }

    /// Total number of entries in the deferred lights buffer.
    pub fn deferred_light_count(&self) -> u32 {
        (1 + self.point_lights.len()
            + self.spot_lights.len()
            + self.rect_lights.len()
            + self.sphere_area_lights.len()
            + self.disk_lights.len()) as u32
    }

    /// Compute and upload the current lighting uniform from the directional light
    /// and camera view matrix, then register the buffer as `"lighting"` in the
    /// frame so all subsequent shaders can read it by name.
    ///
    /// Called by `DeferredPass::draw()` before the G-Buffer fill pass. Apps
    /// using the classic forward path don't need to call this — `draw()` does it
    /// internally.
    pub fn update_lighting_uniform(&self, view: Mat4, frame: &RenderFrame) -> Result<()> {
        let cam_world = view.inverse() * Vec4::new(0.0, 0.0, 0.0, 1.0);
        let lighting = LightingUniforms::from_light_and_camera(
            &self.directional_light,
            Vec3::new(cam_world.x, cam_world.y, cam_world.z),
        );
        if let Some(buf) = &self.light_buffer {
            buf.write(0, bytemuck::bytes_of(&lighting))?;
            frame.bind_buffer("lighting", buf);
        }
        Ok(())
    }

    /// Render all scene objects into a G-Buffer set using the supplied
    /// `gbuffer_program` (typically created from `gbuffer_fragment.slang`).
    ///
    /// `color_targets` must be `[g0, g1, g2, g3]` in G-Buffer slot order.
    /// The per-mesh `"material_desc"` and `"instances"` buffers are bound
    /// automatically for each batch.
    ///
    /// Does **not** write to the output HDR image or run any lighting — that
    /// is the `DeferredPass`'s deferred lighting step.
    pub fn draw_gbuffer(
        &self,
        view: Mat4,
        proj: Mat4,
        color_targets: &[&GraphImage],
        depth: &GraphImage,
        gbuffer_program: &MeshProgram,
        frame: &RenderFrame,
        default_normal_map: &crate::Image,
    ) -> Result<()> {
        let vp = (proj * view).to_cols_array_2d();
        let constants = CameraConstants {
            view_proj: vp,
            previous_view_proj: vp,
            time: 0.0,
            _pad: [0.0; 3],
        };
        assert!(
            color_targets.len() >= 1,
            "draw_gbuffer requires at least one color target"
        );
        let primary = color_targets[0];
        let additional = &color_targets[1..];

        for batch in self.batches.values() {
            let instance_buf = match &batch.gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            if batch.total_count() == 0 {
                continue;
            }
            let mesh_idx = batch.mesh_idx as usize;
            let mesh = &self.meshes[mesh_idx].0;
            frame.bind_buffer("instances", instance_buf);
            if let Some(mat) = self.materials.get(mesh_idx) {
                if let Some(mat_buf) = &mat.gpu_buffer {
                    frame.bind_buffer("material_desc", mat_buf);
                }
                // Per-mesh normal map, falling back to the flat-normal default.
                let nmap: &crate::Image = mat
                    .normal_map
                    .as_ref()
                    .map(|arc| arc.as_ref())
                    .unwrap_or(default_normal_map);
                frame.bind_image("normal_map", nmap);
            } else {
                frame.bind_image("normal_map", default_normal_map);
            }
            primary.draw_mesh_instanced_mrt_with_push_constants_and_depth(
                additional,
                mesh,
                gbuffer_program,
                instance_buf,
                batch.total_count(),
                &constants,
                Some(depth),
            )?;
        }
        Ok(())
    }

    /// Draw all offscreen cameras into the render frame using the classic vertex path.
    ///
    /// Frustum culling is not applied to offscreen cameras; call `draw()` on the
    /// main camera to benefit from the `ComputeIndirect` backend.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        for camera in &self.cameras {
            let CameraOutput::Offscreen(rt) = &camera.output;
            let target = rt.as_frame_image(frame)?;
            let constants = CameraConstants::from_camera(camera);
            self.draw_batches_classic(&constants, &target, None, frame)?;
        }
        Ok(())
    }

    /// Draw the scene into `output` using the given view and projection matrices.
    ///
    /// This is the primary render path for a main camera. No camera object needs
    /// to be registered first — pass the matrices directly.
    ///
    /// A depth buffer matching `output`'s extent and sample count is allocated
    /// automatically as a frame-managed image and cleared to 1.0 before drawing.
    ///
    /// ```ignore
    /// let view = cam.view_matrix();
    /// let proj = cam.projection_matrix(width as f32 / height as f32);
    /// scene.prepare(&engine)?;
    /// scene.draw(view, proj, &hdr_output, &frame)?;
    /// ```
    pub fn draw(
        &mut self,
        view: Mat4,
        proj: Mat4,
        output: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> Result<()> {
        let out_desc = output.desc();

        if matches!(
            out_desc.format,
            Format::Depth32Float | Format::Depth24Stencil8
        ) {
            return Err(Error::InvalidInput(format!(
                "Scene::draw output '{}' has depth format {:?}; use a colour-renderable format",
                output.name(),
                out_desc.format,
            )));
        }
        if !out_desc.usage.contains(ImageUsage::RENDER_TARGET) {
            return Err(Error::InvalidInput(format!(
                "Scene::draw output '{}' requires ImageUsage::RENDER_TARGET but was created with {:?}",
                output.name(),
                out_desc.usage,
            )));
        }

        let view_proj = proj * view;
        let vp_arr = view_proj.to_cols_array_2d();
        let constants = CameraConstants {
            view_proj: vp_arr,
            previous_view_proj: vp_arr,
            time: 0.0,
            _pad: [0.0; 3],
        };

        // Extract camera world position from the view matrix inverse.
        // view * cam_pos = [0,0,0,1], so cam_pos = view_inv * [0,0,0,1].
        let cam_world = view.inverse() * Vec4::new(0.0, 0.0, 0.0, 1.0);
        let lighting = LightingUniforms::from_light_and_camera(
            &self.directional_light,
            Vec3::new(cam_world.x, cam_world.y, cam_world.z),
        );
        if let Some(buf) = &self.light_buffer {
            buf.write(0, bytemuck::bytes_of(&lighting))?;
        }

        let ext = out_desc.extent;
        let depth = frame.image(
            "_scene_depth",
            ImageDesc {
                dimension: ImageDimension::D2,
                extent: Extent3d {
                    width: ext.width,
                    height: ext.height,
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: output.desc().samples,
                format: Format::Depth32Float,
                usage: ImageUsage::DEPTH_STENCIL,
                transient: false,
                clear_value: None,
                debug_name: None,
            },
        )?;
        self.draw_batches(&constants, view_proj, output, Some(&depth), frame, engine)
    }

    /// Draw a single registered camera into an explicit output image.
    ///
    /// Use this for offscreen cameras (portal cameras, reflection probes, etc.)
    /// where the camera was registered with `add_camera`. For the main camera,
    /// prefer `Scene::draw`.
    pub fn render_camera_to(
        &self,
        camera_id: CameraId,
        output: &GraphImage,
        frame: &RenderFrame,
    ) -> Result<()> {
        let camera = self.cameras.get(camera_id.0 as usize).ok_or_else(|| {
            crate::Error::InvalidInput(format!("camera {:?} not found in scene", camera_id))
        })?;
        let constants = CameraConstants::from_camera(camera);
        self.draw_batches_classic(&constants, output, None, frame)
    }

    /// Draw path used by the main camera. Dispatches to the classic or indirect path
    /// based on `self.geometry_backend`.
    fn draw_batches(
        &mut self,
        constants: &CameraConstants,
        view_proj: Mat4,
        output: &GraphImage,
        depth: Option<&GraphImage>,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> Result<()> {
        if self.geometry_backend != GeometryBackend::ComputeIndirect {
            return self.draw_batches_classic(constants, output, depth, frame);
        }

        // ComputeIndirect path: CPU frustum cull, write one DrawIndexedIndirectCommand
        // per visible instance, upload to GPU, then issue DrawIndirect.
        let frustum = Frustum::from_view_proj(view_proj);
        for batch in self.batches.values_mut() {
            batch.indirect_commands.clear();
            let total = batch.total_count() as usize;
            if total == 0 {
                continue;
            }
            let mesh_idx = batch.mesh_idx as usize;
            let mesh = &self.meshes[mesh_idx].0;
            let index_count = if mesh.is_indexed() {
                mesh.index_count
            } else {
                mesh.vertex_count
            };

            let all_instances: Vec<_> = batch
                .static_instances
                .iter()
                .chain(batch.dynamic_instances.iter())
                .enumerate()
                .collect();
            for (instance_idx, inst) in all_instances {
                let model = Mat4::from_cols_array_2d(&inst.model);
                let world_sphere = mesh.bounding_sphere.transform(model);
                if frustum.intersects_sphere(&world_sphere) {
                    batch.indirect_commands.push(DrawIndexedIndirectCommand {
                        index_count,
                        instance_count: 1,
                        first_index: 0,
                        vertex_offset: 0,
                        first_instance: instance_idx as u32,
                    });
                }
            }
            batch.prepare_indirect(engine)?;
        }

        for batch in self.batches.values() {
            let instance_buf = match &batch.gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            // GPU cull: every slot in the indirect buffer has been written by compute
            // (visible = instance_count 1, invisible = 0). Draw all N slots.
            // CPU cull: visible commands only.
            let draw_count = if self.gpu_cull_active {
                batch.total_count()
            } else {
                batch.indirect_commands.len() as u32
            };
            if draw_count == 0 {
                continue;
            }
            let indirect_buf = match &batch.indirect_gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            let mesh_idx = batch.mesh_idx as usize;
            let (mesh, program) = &self.meshes[mesh_idx];
            frame.bind_buffer("instances", instance_buf);
            if let Some(light_buf) = &self.light_buffer {
                frame.bind_buffer("lighting", light_buf);
            }
            if let Some(mat) = self.materials.get(mesh_idx) {
                if let Some(mat_buf) = &mat.gpu_buffer {
                    frame.bind_buffer("material_desc", mat_buf);
                }
            }
            let effective_depth = if program.uses_depth { depth } else { None };
            output.draw_mesh_indirect_with_push_constants_and_depth(
                mesh,
                program,
                instance_buf,
                indirect_buf,
                draw_count,
                constants,
                effective_depth,
            )?;
        }
        Ok(())
    }

    /// Classic draw path — no frustum culling, no mutation. Used by offscreen cameras.
    fn draw_batches_classic(
        &self,
        constants: &CameraConstants,
        output: &GraphImage,
        depth: Option<&GraphImage>,
        frame: &RenderFrame,
    ) -> Result<()> {
        for batch in self.batches.values() {
            let buf = match &batch.gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            let total = batch.total_count();
            if total == 0 {
                continue;
            }
            let mesh_idx = batch.mesh_idx as usize;
            let (mesh, program) = &self.meshes[mesh_idx];
            frame.bind_buffer("instances", buf);
            if let Some(light_buf) = &self.light_buffer {
                frame.bind_buffer("lighting", light_buf);
            }
            if let Some(mat) = self.materials.get(mesh_idx) {
                if let Some(mat_buf) = &mat.gpu_buffer {
                    frame.bind_buffer("material_desc", mat_buf);
                }
            }
            let effective_depth = if program.uses_depth { depth } else { None };
            output.draw_mesh_instanced_with_push_constants_and_depth(
                mesh,
                program,
                buf,
                total,
                constants,
                effective_depth,
            )?;
        }
        Ok(())
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}
