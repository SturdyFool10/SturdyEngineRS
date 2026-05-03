// Unified virtual mesh geometry system.
//
// A `VirtualMesh` is the single source-of-truth geometry asset. Multiple
// rendering backends consume it without re-authoring:
//
//   Classic raster      — vertex/index buffers, traditional draw calls
//   GPU-indirect raster — compute culling + `DrawIndirect`
//   Mesh shader raster  — task/mesh pipeline, per-meshlet culling
//   Virtual raster      — cluster hierarchy, LOD cut, `DrawMeshShaderIndirect`
//   RT fallback         — `rt_proxy` coarse mesh for BLAS construction
//   RT clustered        — selected cluster BLAS rebuilt each frame
//
// The geometry front-end is selected at runtime from `GeometryRendererCaps`
// (derived from `Caps`). Material/pixel shading is decoupled: all backends
// feed the same G-Buffer, depth, shadow, and visibility-buffer attachments.
//
// Roadmap: Track 7.

use glam::{Mat4, Vec3, Vec4};

use crate::mesh::Vertex3d;

// ─────────────────────────────────────────────────────────────────────────────
// GeometryBackend — runtime selection
// ─────────────────────────────────────────────────────────────────────────────

/// Which geometry front-end to use for a given render pass.
///
/// Selected by the engine at init time from [`GeometryRendererCaps`].
/// Games can override this per-pass when the API allows it.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash)]
pub enum GeometryBackend {
    /// Vertex + index buffer draw calls. Supported everywhere.
    /// Zero special hardware requirements; always a valid fallback.
    #[default]
    ClassicVertex,
    /// Compute shader writes culling results + indirect draw parameters into
    /// a GPU buffer; one `DrawIndirect` per surviving batch.
    /// Requires compute shaders (universally available).
    ComputeIndirect,
    /// Task + mesh shader pipeline.
    /// Task shader culls and selects meshlets; mesh shader emits triangles.
    /// Requires `BackendFeatures::mesh_shading`.
    MeshShader,
    /// Cluster-hierarchy traversal + mesh shader emission (Nanite-style).
    /// Per-frame LOD cut selected on GPU; only surviving clusters rasterized.
    /// Requires `BackendFeatures::mesh_shading` and a built `ClusterHierarchy`.
    VirtualizedRaster,
    /// Ray tracing using a coarse proxy/fallback mesh.
    /// Used when mesh-shader clusters would be too expensive to rebuild as BLAS.
    /// Requires `BackendFeatures::ray_tracing`.
    RayTracingFallback,
    /// Ray tracing using a BLAS built from the current frame's selected
    /// cluster subset. More accurate than `RayTracingFallback` but costs
    /// a BLAS rebuild/refit each frame.
    /// Requires both `ray_tracing` and a built `ClusterHierarchy`.
    RayTracingSelectedClusters,
}

impl GeometryBackend {
    /// Returns `true` when this backend requires mesh shader hardware.
    pub fn requires_mesh_shading(self) -> bool {
        matches!(self, Self::MeshShader | Self::VirtualizedRaster)
    }

    /// Returns `true` when this backend requires ray tracing hardware.
    pub fn requires_ray_tracing(self) -> bool {
        matches!(self, Self::RayTracingFallback | Self::RayTracingSelectedClusters)
    }

    /// Returns `true` when this backend requires a pre-built cluster hierarchy.
    pub fn requires_cluster_hierarchy(self) -> bool {
        matches!(self, Self::VirtualizedRaster | Self::RayTracingSelectedClusters)
    }
}

/// Capabilities available for geometry front-end selection,
/// derived from the device's [`Caps`](sturdy_engine_core::Caps).
#[derive(Copy, Clone, Debug, Default)]
pub struct GeometryRendererCaps {
    pub mesh_shading: bool,
    pub task_shading: bool,
    pub compute_indirect: bool,
    pub indirect_draw: bool,
    pub ray_tracing: bool,
}

impl GeometryRendererCaps {
    pub fn from_caps(caps: &sturdy_engine_core::Caps) -> Self {
        Self {
            mesh_shading: caps.supports_mesh_shading,
            task_shading: caps.supports_mesh_shading, // task is part of EXT_mesh_shader
            compute_indirect: true,                    // always available with compute
            indirect_draw: true,
            ray_tracing: caps.supports_raytracing,
        }
    }

    /// Best available backend for opaque geometry rendering.
    pub fn best_opaque_backend(&self) -> GeometryBackend {
        if self.mesh_shading && self.task_shading {
            GeometryBackend::MeshShader
        } else if self.indirect_draw {
            GeometryBackend::ComputeIndirect
        } else {
            GeometryBackend::ClassicVertex
        }
    }

    /// Best available backend for ray-traced effects.
    pub fn best_rt_backend(&self) -> Option<GeometryBackend> {
        if self.ray_tracing {
            Some(GeometryBackend::RayTracingFallback)
        } else {
            None
        }
    }

    /// Returns `true` when the given backend is fully supported.
    pub fn supports(&self, backend: GeometryBackend) -> bool {
        match backend {
            GeometryBackend::ClassicVertex => true,
            GeometryBackend::ComputeIndirect => self.compute_indirect,
            GeometryBackend::MeshShader => self.mesh_shading && self.task_shading,
            GeometryBackend::VirtualizedRaster => self.mesh_shading && self.task_shading,
            GeometryBackend::RayTracingFallback => self.ray_tracing,
            GeometryBackend::RayTracingSelectedClusters => self.ray_tracing,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// BoundingSphere + Frustum (CPU culling)
// ─────────────────────────────────────────────────────────────────────────────

/// A sphere that conservatively bounds a mesh in object space.
///
/// Used by the `ComputeIndirect` and `MeshShader` paths for CPU-side and
/// GPU-side (task shader) frustum culling respectively.
#[derive(Copy, Clone, Debug)]
pub struct BoundingSphere {
    pub center: Vec3,
    pub radius: f32,
}

impl BoundingSphere {
    pub const EMPTY: Self = Self { center: Vec3::ZERO, radius: 0.0 };

    /// Compute a tight bounding sphere from a flat vertex position list.
    /// Uses Ritter's algorithm: fast, one pass, radius slightly conservative.
    pub fn from_positions(positions: &[[f32; 3]]) -> Self {
        if positions.is_empty() {
            return Self::EMPTY;
        }
        // Initial sphere: average center, max distance as radius.
        let center = positions
            .iter()
            .fold(Vec3::ZERO, |acc, p| acc + Vec3::from_array(*p))
            / positions.len() as f32;
        let radius = positions
            .iter()
            .map(|p| center.distance(Vec3::from_array(*p)))
            .fold(0.0_f32, f32::max);
        Self { center, radius }
    }

    /// Transform this sphere into world space given an instance model matrix.
    /// The radius is scaled by the maximum column-length of the 3×3 rotation+scale
    /// sub-matrix, giving a conservative world-space sphere.
    pub fn transform(&self, model: Mat4) -> Self {
        let world_center = (model * Vec4::new(self.center.x, self.center.y, self.center.z, 1.0)).truncate();
        // Maximum scale along any axis (largest column magnitude of the 3×3 part).
        let sx = Vec3::new(model.x_axis.x, model.y_axis.x, model.z_axis.x).length();
        let sy = Vec3::new(model.x_axis.y, model.y_axis.y, model.z_axis.y).length();
        let sz = Vec3::new(model.x_axis.z, model.y_axis.z, model.z_axis.z).length();
        let max_scale = sx.max(sy).max(sz);
        Self { center: world_center, radius: self.radius * max_scale }
    }
}

/// Six-plane view frustum for conservative sphere-frustum culling.
///
/// Planes are stored as `Vec4(nx, ny, nz, d)` in the convention
/// `dot(normal, point) + d >= 0` means the point is on the **inside**.
/// Extracted from the combined view-projection matrix (clip space).
#[derive(Copy, Clone, Debug)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// Extract frustum planes from a column-major view-projection matrix
    /// using Gribb/Hartmann's method. Plane normals point **inward**.
    pub fn from_view_proj(vp: Mat4) -> Self {
        let m = vp.to_cols_array_2d(); // column-major: m[col][row]
        // Gribb-Hartmann: row vectors of the matrix are r0..r3.
        // m[col][row] → row r = (m[0][r], m[1][r], m[2][r], m[3][r])
        let r = |row: usize| Vec4::new(m[0][row], m[1][row], m[2][row], m[3][row]);
        let r0 = r(0); let r1 = r(1); let r2 = r(2); let r3 = r(3);
        let mut planes = [
            r3 + r0, // left
            r3 - r0, // right
            r3 + r1, // bottom
            r3 - r1, // top
            r3 + r2, // near
            r3 - r2, // far
        ];
        // Normalize so the xyz length is 1 (makes the dot product a signed distance in world units).
        for p in &mut planes {
            let len = Vec3::new(p.x, p.y, p.z).length();
            if len > 1e-6 {
                *p /= len;
            }
        }
        Self { planes }
    }

    /// Returns `true` when the sphere is **not** fully outside any frustum plane
    /// (i.e. it may be visible — conservative, no false negatives).
    #[inline]
    pub fn intersects_sphere(&self, sphere: &BoundingSphere) -> bool {
        let p = Vec4::new(sphere.center.x, sphere.center.y, sphere.center.z, 1.0);
        for plane in &self.planes {
            if plane.dot(p) < -sphere.radius {
                return false; // fully outside this plane
            }
        }
        true
    }

    /// Returns `true` when the sphere is fully inside all planes (no clipping).
    #[inline]
    pub fn contains_sphere(&self, sphere: &BoundingSphere) -> bool {
        let p = Vec4::new(sphere.center.x, sphere.center.y, sphere.center.z, 1.0);
        self.planes.iter().all(|plane| plane.dot(p) >= sphere.radius)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Meshlet / cluster primitives
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum vertices per meshlet (matches Vulkan EXT_mesh_shader recommended limit).
pub const MAX_MESHLET_VERTICES: u32 = 64;
/// Maximum triangles per meshlet (a multiple of 4 for alignment; 124 leaves
/// room for a 4-byte header without exceeding a 128-triangle workgroup).
pub const MAX_MESHLET_TRIANGLES: u32 = 124;

/// A single meshlet: a spatially coherent cluster of ≤ 128 triangles.
///
/// Stored in `VirtualMesh::meshlets`. The task/amplification shader iterates
/// over meshlets; surviving ones launch mesh-shader workgroups that load
/// `VirtualMesh::meshlet_vertices` and `VirtualMesh::meshlet_triangles`.
#[derive(Clone, Debug)]
pub struct Meshlet {
    /// Byte offset into `VirtualMesh::meshlet_vertices`.
    pub vertex_offset: u32,
    /// Number of vertex indices for this meshlet (≤ `MAX_MESHLET_VERTICES`).
    pub vertex_count: u32,
    /// Byte offset into `VirtualMesh::meshlet_triangles` (3 bytes per triangle).
    pub triangle_offset: u32,
    /// Number of triangles (≤ `MAX_MESHLET_TRIANGLES`).
    pub triangle_count: u32,
    /// Pre-computed culling data for task shader evaluation.
    pub bounds: MeshletBounds,
}

/// Culling data for one meshlet, laid out for GPU upload.
///
/// Sent to the GPU as part of a per-meshlet storage buffer; the task/amplification
/// shader reads this data to decide whether to launch a mesh-shader workgroup.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshletBounds {
    /// Bounding sphere center in object space.
    pub center: [f32; 3],
    /// Bounding sphere radius.
    pub radius: f32,
    /// Normal cone apex in object space.
    pub cone_apex: [f32; 3],
    /// Geometric error at this LOD level (world-space units at the cluster's
    /// projected scale). Used for LOD selection: if `lod_error / distance < threshold`,
    /// this meshlet is detailed enough.
    pub lod_error: f32,
    /// Normal cone axis (quantized i8, range [-128, 127] → [-1, 1]).
    pub cone_axis: [i8; 3],
    /// Backface culling half-angle cutoff (quantized i8).
    /// A meshlet is fully back-facing and can be culled when
    /// `dot(view_dir, cone_axis) >= cone_cutoff / 127`.
    pub cone_cutoff: i8,
}

/// A group of meshlets that must make compatible LOD decisions.
///
/// Adjacent meshlets share geometric boundaries. If they chose different LOD
/// levels independently, visible cracks would appear along shared edges.
/// Groups enforce that all member meshlets transition together between LOD levels.
///
/// This mirrors the Nanite "group" concept: each group corresponds to one
/// simplification operation in the cluster DAG.
#[derive(Clone, Debug)]
pub struct MeshletGroup {
    /// Index of the first meshlet in this group (into `VirtualMesh::meshlets`).
    pub meshlet_offset: u32,
    /// Number of meshlets in this group.
    pub meshlet_count: u32,
    /// Bounding sphere center of the group in object space.
    pub group_center: [f32; 3],
    /// Bounding sphere radius of the group.
    pub group_radius: f32,
    /// The geometric error of this group's LOD level.
    pub self_lod_error: f32,
    /// The geometric error of the parent group (the coarser representation
    /// that covers the same region). Used to determine when to transition up.
    pub parent_lod_error: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Sub-mesh and proxy types
// ─────────────────────────────────────────────────────────────────────────────

/// A contiguous range of triangles in the index buffer sharing one material.
#[derive(Clone, Debug)]
pub struct SubMesh {
    /// Byte offset into `VirtualMesh::indices`.
    pub index_offset: u32,
    /// Number of indices in this sub-mesh (triangles × 3).
    pub index_count: u32,
    /// Material slot index (into the scene's material array).
    pub material_index: u32,
}

/// A coarser mesh used as a ray tracing BLAS proxy.
///
/// When full-resolution geometry would be too expensive to rebuild as
/// a BLAS each frame, the engine uses this simplified representation for
/// RT shadows, reflections, and AO instead.
#[derive(Clone, Debug)]
pub struct VirtualMeshProxy {
    pub vertices: Vec<Vertex3d>,
    pub indices: Vec<u32>,
    pub sub_meshes: Vec<SubMesh>,
}

// ─────────────────────────────────────────────────────────────────────────────
// VirtualMesh — the unified geometry asset
// ─────────────────────────────────────────────────────────────────────────────

/// The unified geometry asset type.
///
/// All rendering backends consume this same struct without re-authoring:
///
/// ```text
/// Classic raster   → vertices + indices
/// Compute indirect → vertices + indices + indirect metadata
/// Mesh shader      → meshlets + meshlet_vertices + meshlet_triangles + bounds
/// Virtual raster   → meshlet_groups (LOD DAG) + above
/// RT fallback      → rt_proxy (coarser mesh for BLAS)
/// RT clustered     → selected clusters from the LOD DAG → BLAS
/// ```
///
/// # Building
///
/// The asset pipeline produces `VirtualMesh` by:
/// 1. Loading a source mesh (GLTF, OBJ, etc.).
/// 2. Generating meshlets from the triangle list (e.g. via `meshopt`).
/// 3. Building a cluster simplification hierarchy/DAG for virtual raster / RT LOD.
/// 4. Generating a simplified proxy mesh for ray-tracing BLAS.
/// 5. Packing everything into `VirtualMesh`.
///
/// # Runtime
///
/// The engine selects `GeometryBackend` from `GeometryRendererCaps` and
/// dispatches the appropriate render pass. The material / pixel shading side
/// is unchanged regardless of which backend runs.
#[derive(Clone, Debug)]
pub struct VirtualMesh {
    /// Debug name used in GPU resource labels and diagnostics.
    pub name: String,

    // ── Classic vertex data ───────────────────────────────────────────────────
    /// Vertex positions, normals, and UVs. All backends reference this pool.
    pub vertices: Vec<Vertex3d>,
    /// Triangle index list for the classic / compute-indirect path.
    pub indices: Vec<u32>,

    // ── Meshlet data (mesh shader / virtual raster paths) ─────────────────────
    /// Per-cluster descriptor including bounds and triangle counts.
    /// Empty when meshlets have not been built.
    pub meshlets: Vec<Meshlet>,
    /// Flat list of vertex indices, partitioned per meshlet.
    /// Entry `i` maps to an index in `vertices`; each meshlet's range is
    /// `[meshlet.vertex_offset .. meshlet.vertex_offset + meshlet.vertex_count]`.
    pub meshlet_vertices: Vec<u32>,
    /// Packed triangle data: 3 bytes per triangle (local vertex indices 0..63).
    /// Each meshlet's range is
    /// `[meshlet.triangle_offset * 3 .. (meshlet.triangle_offset + meshlet.triangle_count) * 3]`.
    pub meshlet_triangles: Vec<u8>,

    // ── LOD hierarchy (virtual raster / cluster LOD paths) ────────────────────
    /// Groups of meshlets that must make the same LOD decision.
    /// Empty when the cluster hierarchy has not been built.
    pub meshlet_groups: Vec<MeshletGroup>,

    // ── Sub-mesh / material ranges ────────────────────────────────────────────
    /// Partitions the index buffer by material. At least one entry is required.
    pub sub_meshes: Vec<SubMesh>,

    // ── RT proxy ─────────────────────────────────────────────────────────────
    /// Optional coarser mesh for ray tracing BLAS construction.
    /// When `None`, the engine falls back to the full `vertices` / `indices`.
    pub rt_proxy: Option<Box<VirtualMeshProxy>>,
}

impl VirtualMesh {
    /// Create a `VirtualMesh` from flat vertex/index data with a single
    /// sub-mesh and no meshlets or LOD hierarchy.
    ///
    /// Suitable for the `ClassicVertex` and `ComputeIndirect` backends.
    /// Call `build_meshlets()` (Track 7 asset pipeline) to enable mesh shaders.
    pub fn from_vertex_data(
        name: impl Into<String>,
        vertices: Vec<Vertex3d>,
        indices: Vec<u32>,
        material_index: u32,
    ) -> Self {
        let index_count = indices.len() as u32;
        Self {
            name: name.into(),
            vertices,
            indices,
            meshlets: Vec::new(),
            meshlet_vertices: Vec::new(),
            meshlet_triangles: Vec::new(),
            meshlet_groups: Vec::new(),
            sub_meshes: vec![SubMesh { index_offset: 0, index_count, material_index }],
            rt_proxy: None,
        }
    }

    /// Returns `true` when meshlet data has been built (mesh shader path is available).
    pub fn has_meshlets(&self) -> bool {
        !self.meshlets.is_empty()
    }

    /// Returns `true` when the LOD cluster hierarchy has been built
    /// (virtual raster and RT-clustered paths are available).
    pub fn has_cluster_hierarchy(&self) -> bool {
        !self.meshlet_groups.is_empty()
    }

    /// Choose the best available `GeometryBackend` for this mesh given the
    /// hardware capabilities. The mesh's available data is checked first;
    /// if meshlets are absent, mesh-shader backends are not selected even
    /// if the hardware supports them.
    pub fn best_backend(&self, caps: &GeometryRendererCaps) -> GeometryBackend {
        if caps.mesh_shading && self.has_meshlets() {
            if self.has_cluster_hierarchy() {
                GeometryBackend::VirtualizedRaster
            } else {
                GeometryBackend::MeshShader
            }
        } else if caps.indirect_draw {
            GeometryBackend::ComputeIndirect
        } else {
            GeometryBackend::ClassicVertex
        }
    }

    /// Total triangle count across all sub-meshes.
    pub fn triangle_count(&self) -> u32 {
        self.indices.len() as u32 / 3
    }

    /// Total meshlet count.
    pub fn meshlet_count(&self) -> u32 {
        self.meshlets.len() as u32
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GPU-side indirect command structures
// ─────────────────────────────────────────────────────────────────────────────
// These match the Vulkan/D3D12 ABI. Compute culling passes write them into
// storage buffers; the render graph then issues `DrawIndirect` / mesh-shader
// indirect dispatch from those buffers.

/// Matches `VkDrawIndirectCommand` / D3D12 `D3D12_DRAW_ARGUMENTS`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawIndirectCommand {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
}

/// Matches `VkDrawIndexedIndirectCommand` / D3D12 `D3D12_DRAW_INDEXED_ARGUMENTS`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawIndexedIndirectCommand {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub first_instance: u32,
}

/// Matches `VkDispatchIndirectCommand`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DispatchIndirectCommand {
    pub group_count_x: u32,
    pub group_count_y: u32,
    pub group_count_z: u32,
}

/// Matches `VkDrawMeshTasksIndirectCommandEXT` (EXT_mesh_shader).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawMeshTasksIndirectCommand {
    pub group_count_x: u32,
    pub group_count_y: u32,
    pub group_count_z: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Hi-Z pyramid (input to occlusion culling)
// ─────────────────────────────────────────────────────────────────────────────

/// Descriptor for a hierarchical-Z depth pyramid used by GPU culling passes.
///
/// A compute pass builds this from the previous frame's depth buffer.
/// The task shader samples it to cull occluded meshlets before emission.
#[derive(Clone, Debug)]
pub struct HizDesc {
    /// Name of the Hi-Z image in the render graph.
    pub image_name: &'static str,
    /// Width of the full-resolution level (matches depth buffer).
    pub width: u32,
    /// Height of the full-resolution level.
    pub height: u32,
    /// Number of mip levels (ceil(log2(max(w, h))) + 1).
    pub mip_levels: u32,
}
