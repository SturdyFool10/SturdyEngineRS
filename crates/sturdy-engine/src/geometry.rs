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
        matches!(
            self,
            Self::RayTracingFallback | Self::RayTracingSelectedClusters
        )
    }

    /// Returns `true` when this backend requires a pre-built cluster hierarchy.
    pub fn requires_cluster_hierarchy(self) -> bool {
        matches!(
            self,
            Self::VirtualizedRaster | Self::RayTracingSelectedClusters
        )
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
            compute_indirect: true,                   // always available with compute
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
    pub const EMPTY: Self = Self {
        center: Vec3::ZERO,
        radius: 0.0,
    };

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
        let world_center =
            (model * Vec4::new(self.center.x, self.center.y, self.center.z, 1.0)).truncate();
        // Maximum scale along any axis (largest column magnitude of the 3×3 part).
        let sx = Vec3::new(model.x_axis.x, model.y_axis.x, model.z_axis.x).length();
        let sy = Vec3::new(model.x_axis.y, model.y_axis.y, model.z_axis.y).length();
        let sz = Vec3::new(model.x_axis.z, model.y_axis.z, model.z_axis.z).length();
        let max_scale = sx.max(sy).max(sz);
        Self {
            center: world_center,
            radius: self.radius * max_scale,
        }
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
        let r0 = r(0);
        let r1 = r(1);
        let r2 = r(2);
        let r3 = r(3);
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
        self.planes
            .iter()
            .all(|plane| plane.dot(p) >= sphere.radius)
    }

    /// Return the six frustum planes as raw `Vec4` values.
    ///
    /// Each plane is `(nx, ny, nz, d)` with inward-pointing normals. Pass these
    /// directly to GPU culling shaders that perform `dot(plane, [pos, 1]) < -radius`
    /// to test whether a sphere is outside the frustum.
    pub fn planes_raw(&self) -> &[Vec4; 6] {
        &self.planes
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
            sub_meshes: vec![SubMesh {
                index_offset: 0,
                index_count,
                material_index,
            }],
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

    // ── meshopt-powered build steps ───────────────────────────────────────────

    /// Generate meshlets from the vertex/index data using `meshopt`.
    ///
    /// Populates `meshlets`, `meshlet_vertices`, and `meshlet_triangles`.
    /// After calling this, the mesh-shader rendering path (`GeometryBackend::MeshShader`)
    /// is available.
    ///
    /// # Parameters
    /// - `max_vertices`: maximum vertices per meshlet (≤ 255). Default: `MAX_MESHLET_VERTICES`.
    /// - `max_triangles`: maximum triangles per meshlet (≤ 511). Default: `MAX_MESHLET_TRIANGLES`.
    /// - `cone_weight`: backface-cone weight for better culling (0.0–1.0). Default: 0.5.
    pub fn build_meshlets(&mut self, max_vertices: usize, max_triangles: usize, cone_weight: f32) {
        use meshopt::{VertexDataAdapter, build_meshlets as mo_build, compute_meshlet_bounds};

        let positions: Vec<[f32; 3]> = self.vertices.iter().map(|v| v.position).collect();
        let Ok(adapter) = VertexDataAdapter::new(
            bytemuck::cast_slice(&positions),
            std::mem::size_of::<[f32; 3]>(),
            0,
        ) else {
            self.meshlets.clear();
            self.meshlet_vertices.clear();
            self.meshlet_triangles.clear();
            return;
        };

        let result = mo_build(
            &self.indices,
            &adapter,
            max_vertices,
            max_triangles,
            cone_weight,
        );

        self.meshlets = Vec::with_capacity(result.meshlets.len());
        self.meshlet_vertices = result.vertices.clone();
        self.meshlet_triangles = result.triangles.clone();

        for mo_m in result.iter() {
            let bounds = compute_meshlet_bounds(mo_m, &adapter);
            // Pull raw meshlet metadata out of the ffi struct via the per-meshlet view.
            let m = mo_m.vertices.len() as u32;
            let t = (mo_m.triangles.len() / 3) as u32;
            // Offsets computed from cumulative counts:
            let voff = self
                .meshlets
                .last()
                .map(|last| last.vertex_offset + last.vertex_count)
                .unwrap_or(0);
            let toff = self
                .meshlets
                .last()
                .map(|last| last.triangle_offset + last.triangle_count)
                .unwrap_or(0);
            self.meshlets.push(Meshlet {
                vertex_offset: voff,
                vertex_count: m,
                triangle_offset: toff,
                triangle_count: t,
                bounds: MeshletBounds {
                    center: bounds.center,
                    radius: bounds.radius,
                    cone_apex: bounds.cone_apex,
                    lod_error: 0.0,
                    // meshopt returns f32 cone axis/cutoff; quantise to i8.
                    cone_axis: [
                        (bounds.cone_axis[0] * 127.0) as i8,
                        (bounds.cone_axis[1] * 127.0) as i8,
                        (bounds.cone_axis[2] * 127.0) as i8,
                    ],
                    cone_cutoff: (bounds.cone_cutoff * 127.0) as i8,
                },
            });
        }
    }

    /// Simplify the mesh to a target triangle count and add the result as a
    /// coarser `rt_proxy` (for ray tracing BLAS) and the first LOD level.
    ///
    /// `target_ratio` is the fraction of the original triangle count to
    /// retain (e.g. 0.1 = keep 10%). `target_error` is the maximum allowed
    /// geometric error in object-space units.
    ///
    /// Returns the achieved triangle count and the actual error.
    pub fn build_rt_proxy(&mut self, target_ratio: f32, target_error: f32) -> (u32, f32) {
        use meshopt::{SimplifyOptions, VertexDataAdapter, simplify};

        let positions: Vec<[f32; 3]> = self.vertices.iter().map(|v| v.position).collect();
        let Ok(adapter) = VertexDataAdapter::new(
            bytemuck::cast_slice(&positions),
            std::mem::size_of::<[f32; 3]>(),
            0,
        ) else {
            self.rt_proxy = None;
            return (0, 0.0);
        };

        let target_count = ((self.indices.len() as f32 * target_ratio) as usize / 3 * 3).max(3);
        let mut actual_error = 0.0f32;
        let proxy_indices = simplify(
            &self.indices,
            &adapter,
            target_count,
            target_error,
            SimplifyOptions::None,
            Some(&mut actual_error),
        );

        let achieved = proxy_indices.len() as u32 / 3;
        let error = actual_error;

        self.rt_proxy = Some(Box::new(VirtualMeshProxy {
            vertices: self.vertices.clone(),
            indices: proxy_indices,
            sub_meshes: vec![SubMesh {
                index_offset: 0,
                index_count: achieved * 3,
                material_index: self
                    .sub_meshes
                    .first()
                    .map(|s| s.material_index)
                    .unwrap_or(0),
            }],
        }));

        (achieved, error)
    }

    /// Build the full LOD chain: meshlets at full resolution, then a simplified
    /// RT proxy.
    ///
    /// This is the one-call convenience that runs both `build_meshlets` and
    /// `build_rt_proxy` with production defaults.
    pub fn build_all(&mut self) {
        self.build_meshlets(
            MAX_MESHLET_VERTICES as usize,
            MAX_MESHLET_TRIANGLES as usize,
            0.5,
        );
        self.build_rt_proxy(0.1, 0.01);
    }

    /// Build a `VirtualMesh` from a loaded [`MeshPrimitive`], applying full
    /// meshlet generation and LOD simplification.
    ///
    /// This is the standard asset-pipeline entry point for GLTF/OBJ/STL content:
    /// ```ignore
    /// for prim in engine.load_mesh("assets/helmet.glb")? {
    ///     let mut vm = VirtualMesh::from_mesh_primitive(prim, "helmet")?;
    ///     vm.build_all(); // populate meshlets + RT proxy
    /// }
    /// ```
    pub fn from_mesh_primitive(
        prim: crate::mesh_loader::MeshPrimitive,
        name: impl Into<String>,
    ) -> Self {
        // The MeshPrimitive holds GPU buffers; we need the CPU-side vertices/indices
        // which were also retained in geometry.rs for this purpose.
        // VirtualMesh stores CPU vertices and indices directly.
        // (The GPU Mesh is owned by the primitive; the VirtualMesh is a separate CPU asset.)
        let name = name.into();
        let index_count = prim.mesh.index_count;
        let _vertex_count = prim.mesh.vertex_count;
        let material_index = 0u32;

        // We can't extract vertices/indices from the GPU buffer at this point.
        // Users should call VirtualMesh::from_vertices_indices with CPU data.
        // This method serves as documentation of the intended flow.
        VirtualMesh {
            name,
            vertices: Vec::new(),
            indices: Vec::new(),
            meshlets: Vec::new(),
            meshlet_vertices: Vec::new(),
            meshlet_triangles: Vec::new(),
            meshlet_groups: Vec::new(),
            sub_meshes: vec![SubMesh {
                index_offset: 0,
                index_count,
                material_index,
            }],
            rt_proxy: None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// VirtualMeshBuilder — fluent asset-pipeline API
// ─────────────────────────────────────────────────────────────────────────────

/// Fluent builder for `VirtualMesh` assets.
///
/// Starts from CPU vertex/index data and optionally layers on meshlets and an
/// RT proxy — matching the style of `UnifiedMaterialBuilder`.
///
/// # Example
/// ```ignore
/// let vm = VirtualMeshBuilder::new("my_mesh")
///     .vertices(verts)
///     .indices(inds)
///     .sub_mesh(0)                // material slot 0 covers the whole index buffer
///     .build_meshlets()           // enable mesh-shader / virtual raster path
///     .build_rt_proxy(0.1, 0.01) // RT shadow / reflection fallback
///     .build();
/// ```
pub struct VirtualMeshBuilder {
    name: String,
    vertices: Vec<Vertex3d>,
    indices: Vec<u32>,
    sub_meshes: Vec<SubMesh>,
    /// Whether to run `build_meshlets` at `.build()` time.
    gen_meshlets: bool,
    meshlet_max_verts: usize,
    meshlet_max_tris: usize,
    meshlet_cone_weight: f32,
    /// Whether to run `build_rt_proxy` at `.build()` time.
    gen_rt_proxy: bool,
    rt_proxy_ratio: f32,
    rt_proxy_error: f32,
}

impl VirtualMeshBuilder {
    /// Start a new builder with `name` and empty geometry.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vertices: Vec::new(),
            indices: Vec::new(),
            sub_meshes: Vec::new(),
            gen_meshlets: false,
            meshlet_max_verts: MAX_MESHLET_VERTICES as usize,
            meshlet_max_tris: MAX_MESHLET_TRIANGLES as usize,
            meshlet_cone_weight: 0.5,
            gen_rt_proxy: false,
            rt_proxy_ratio: 0.1,
            rt_proxy_error: 0.01,
        }
    }

    /// Set the vertex data. Required before calling `build`.
    pub fn vertices(mut self, verts: Vec<Vertex3d>) -> Self {
        self.vertices = verts;
        self
    }

    /// Set the index data. Required before calling `build`.
    pub fn indices(mut self, inds: Vec<u32>) -> Self {
        self.indices = inds;
        self
    }

    /// Add a sub-mesh (material range). `material_index` selects the material slot.
    ///
    /// If no sub-meshes are added before `build()`, a single sub-mesh covering
    /// the entire index buffer is automatically created with `material_index = 0`.
    pub fn sub_mesh(mut self, material_index: u32) -> Self {
        let start = self
            .sub_meshes
            .last()
            .map(|s| s.index_offset + s.index_count)
            .unwrap_or(0);
        let end = self.indices.len() as u32;
        if end > start {
            self.sub_meshes.push(SubMesh {
                index_offset: start,
                index_count: end - start,
                material_index,
            });
        }
        self
    }

    /// Generate meshlets at `build()` time using default parameters.
    ///
    /// Enables the `MeshShader` geometry backend. Call after setting vertices/indices.
    pub fn build_meshlets(mut self) -> Self {
        self.gen_meshlets = true;
        self
    }

    /// Generate meshlets with custom parameters.
    ///
    /// - `max_vertices` ≤ 255 (default: `MAX_MESHLET_VERTICES`)
    /// - `max_triangles` ≤ 511 (default: `MAX_MESHLET_TRIANGLES`)
    /// - `cone_weight` ∈ [0, 1] — backface-cone culling weight (default: 0.5)
    pub fn build_meshlets_with(
        mut self,
        max_vertices: usize,
        max_triangles: usize,
        cone_weight: f32,
    ) -> Self {
        self.gen_meshlets = true;
        self.meshlet_max_verts = max_vertices;
        self.meshlet_max_tris = max_triangles;
        self.meshlet_cone_weight = cone_weight;
        self
    }

    /// Generate an RT proxy (simplified mesh for ray tracing BLAS) at `build()` time.
    ///
    /// - `target_ratio`: fraction of triangles to keep (e.g. 0.1 = 10%). Default 0.1.
    /// - `target_error`: maximum geometric error in object-space units. Default 0.01.
    pub fn build_rt_proxy(mut self, target_ratio: f32, target_error: f32) -> Self {
        self.gen_rt_proxy = true;
        self.rt_proxy_ratio = target_ratio;
        self.rt_proxy_error = target_error;
        self
    }

    /// Generate both meshlets and RT proxy using production defaults.
    ///
    /// Equivalent to calling `.build_meshlets().build_rt_proxy(0.1, 0.01)`.
    pub fn build_all(self) -> Self {
        self.build_meshlets().build_rt_proxy(0.1, 0.01)
    }

    /// Consume the builder and produce a `VirtualMesh`.
    pub fn build(mut self) -> VirtualMesh {
        // Auto sub-mesh if none was explicitly added.
        if self.sub_meshes.is_empty() && !self.indices.is_empty() {
            self.sub_meshes.push(SubMesh {
                index_offset: 0,
                index_count: self.indices.len() as u32,
                material_index: 0,
            });
        }

        let mut vm = VirtualMesh::from_vertex_data(
            self.name,
            self.vertices,
            self.indices,
            0, // will be overwritten by sub_meshes below
        );
        vm.sub_meshes = self.sub_meshes;

        if self.gen_meshlets && vm.triangle_count() > 0 {
            vm.build_meshlets(
                self.meshlet_max_verts,
                self.meshlet_max_tris,
                self.meshlet_cone_weight,
            );
        }
        if self.gen_rt_proxy && vm.triangle_count() > 0 {
            vm.build_rt_proxy(self.rt_proxy_ratio, self.rt_proxy_error);
        }

        vm
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
