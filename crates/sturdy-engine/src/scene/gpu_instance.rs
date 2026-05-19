// GpuInstanceData — unified per-object GPU data structure (Track 8b).
//
// One `GpuInstanceData` entry lives in the render-world GPU scene buffer for every
// render-world object. It consolidates data that is currently split across three
// separate per-batch buffers:
//
//   InstanceData  (matrix only, uploaded per-batch per-frame)
//   bounds float4 (world-space sphere, computed and uploaded per-batch)
//   — future: material params, LOD bias, visibility flags
//
// By merging these into one flat buffer, `cull_compute.slang` can read all
// it needs from a single structured buffer, enabling a **single dispatch over
// all scene objects** rather than one dispatch per batch.
//
// ## Memory layout
//
//   model[0]     : float4  ( 16 bytes )  ─┐
//   model[1]     : float4  ( 16 bytes )   │  column-major model matrix
//   model[2]     : float4  ( 16 bytes )   │
//   model[3]     : float4  ( 16 bytes )  ─┘  total: 64 bytes
//   bounds       : float4  ( 16 bytes )       xyz = world-space centre, w = radius
//   mesh_id      : u32     (  4 bytes )       index into Scene::meshes
//   material_id  : u32     (  4 bytes )       currently same as mesh_id
//   lod_bias     : f32     (  4 bytes )       0.0 = normal detail
//   flags        : u32     (  4 bytes )       bit 0 = static, bit 1 = cast_shadow
//   ─────────────────────────────────────────── total: 96 bytes / object
//
// ## Upload policy
//
// `RenderWorld::prepare_gpu_scene` currently re-uploads all `GpuInstanceData`
// every frame in one contiguous write. A future refinement will dirty-track
// individual slots/pages so only changed objects are re-uploaded.
//
// ## Batch ordering
//
// To enable culling over a flat scene buffer, `RenderWorld::prepare_gpu_scene`
// fills the scene buffer with objects grouped by `mesh_id`: all instances of
// mesh 0 first, then mesh 1, etc. Each compatibility `InstanceBatch` records
// its `scene_base_idx` — the first index in the flat buffer that belongs to it
// — and `scene_count` — how many entries. The culling shader receives these as
// push constants.

/// Per-object GPU data block uploaded to the render-world GPU scene buffer.
///
/// All fields are `f32`/`u32` so the struct is `bytemuck::Pod`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuInstanceData {
    /// World-space model matrix in **column-major** layout.
    ///
    /// Matches the output of [`glam::Mat4::to_cols_array_2d`].
    pub model: [[f32; 4]; 4],

    /// World-space bounding sphere: `xyz` = centre, `w` = radius.
    ///
    /// Computed by transforming the mesh's local-space bounding sphere by
    /// `model`. Used by the GPU culling compute shader.
    pub bounds: [f32; 4],

    /// Index into `Scene::meshes` (and `Scene::materials`).
    ///
    /// Currently the material is addressed by the same index as the mesh.
    /// When the full bindless material system lands (Track 8a complete), this
    /// will index into a `StructuredBuffer<MaterialData>` instead.
    pub mesh_id: u32,

    /// Material slot index. Currently equal to `mesh_id`; reserved for
    /// per-instance material overrides when the bindless heap is used.
    pub material_id: u32,

    /// LOD selection bias. Positive = finer LOD (more detail), negative =
    /// coarser LOD (less detail). `0.0` means the automatically selected level.
    pub lod_bias: f32,

    /// Visibility / classification flags.
    ///
    /// | Bit | Meaning |
    /// |-----|---------|
    /// |  0  | `STATIC` — object does not move; cull result can be cached across frames |
    /// |  1  | `CAST_SHADOW` — participate in shadow map rendering |
    /// |  2  | `RECEIVE_SHADOW` — apply shadowing during lighting |
    /// |  3  | `DYNAMIC` — object may move every frame |
    pub flags: u32,
}

impl GpuInstanceData {
    /// Flag bit: object is static and does not move.
    pub const FLAG_STATIC: u32 = 1 << 0;
    /// Flag bit: object casts shadows.
    pub const FLAG_CAST_SHADOW: u32 = 1 << 1;
    /// Flag bit: object receives shadows.
    pub const FLAG_RECEIVE_SHADOW: u32 = 1 << 2;
    /// Flag bit: object moves every frame (may not be culled from cache).
    pub const FLAG_DYNAMIC: u32 = 1 << 3;
}
