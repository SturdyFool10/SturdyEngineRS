use glam::{Mat4, Vec3, Vec4};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{
    batch::InstanceBatch,
    camera::{CameraId, CameraOutput, SceneCamera},
    commands::{SceneCommands, SceneView},
    gpu_constants::{CameraConstants, LightingUniforms},
    lights::{
        DirectionalLight, DiskLight, GpuLightData, PointLight, RectLight, SphereLight, SpotLight,
    },
    material_state::{MaterialConstants, MaterialDescriptor, MeshMaterial},
    object::{InstanceData, MeshId, ObjectId, ObjectKind, SceneObject},
};
use crate::{
    Buffer, BufferDesc, BufferUsage, ComputeProgram, DrawIndexedIndirectCommand, Engine, Error,
    Format, Frustum, GeometryBackend, GraphImage, HizHistory, HizPass, ImageDesc, ImageDimension,
    ImageUsage, Mesh, MeshProgram, MultiMeshDrawBinItem, RenderFrame, Result,
    render_world::{
        GpuObjectId, HizOcclusionInput, MaterialShaderClass, PipelineClass, RenderStateClass,
        RenderWorld, RenderWorldGpuCullPass, RenderWorldGpuCullSettings,
        RenderWorldGpuDrawGenerationPass, RenderWorldGpuMatrixSettings, RenderWorldGpuMeshDrawInfo,
        RenderWorldGpuTransformBuildPass, RenderWorldPersistentBins, VertexLayoutClass,
        VisibilityFlags,
    },
};
use sturdy_engine_core::Extent3d;

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
    pub(super) materials: Vec<MeshMaterial>,
    pub(super) objects: Vec<SceneObject>,
    pub(super) cameras: Vec<SceneCamera>,
    pub(super) batches: HashMap<u32, InstanceBatch>,
    /// Atomic ID counter so `reserve_object_id()` works from any thread.
    next_object_id: AtomicU32,
    /// Atomic ID counter so `reserve_mesh_id()` works from any thread.
    next_mesh_id: AtomicU32,
    /// Thread-safe queue for structural mutations posted from worker threads.
    commands: SceneCommands,
    /// Compatibility map from persistent render-world objects to legacy scene slots.
    pub(super) render_world_objects: HashMap<GpuObjectId, ObjectId>,
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
    pub(super) geometry_backend: GeometryBackend,
    /// Lazily created GPU frustum culling compute program.
    pub(super) culling_program: Option<ComputeProgram>,
    /// Lazily created GPU transform build compute pass (expands TRS sources → matrices + bounds).
    transform_build_pass: Option<RenderWorldGpuTransformBuildPass>,
    /// Lazily created GPU frustum cull pass (writes visibility flags from world_bounds).
    cull_pass: Option<RenderWorldGpuCullPass>,
    /// Lazily created GPU draw-generation pass (visibility flags + bins → indirect commands).
    draw_generation_pass: Option<RenderWorldGpuDrawGenerationPass>,
    /// Persistent-bin snapshot matching the prepared render-world draw-generation buffers.
    gpu_draw_bins: RenderWorldPersistentBins,
    /// Set by `prepare_internal()` each frame; tells the draw path to use GPU indirect.
    pub(super) gpu_cull_active: bool,
    /// Hi-Z depth pyramid builder (lazily created alongside the cull pass).
    hiz_pass: Option<HizPass>,
    /// Ping-pong history for the Hi-Z pyramid — retains last frame's levels for
    /// use as the occlusion input on the next frame.
    hiz_history: HizHistory,
    /// Elapsed application time in seconds. Written by `set_app_time` and read
    /// by `draw()` / `draw_gbuffer()` to populate `CameraConstants::time` so
    /// material expressions that use `cam.time` animate correctly.
    pub app_time_secs: f32,
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
            render_world_objects: HashMap::new(),
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
            transform_build_pass: None,
            cull_pass: None,
            draw_generation_pass: None,
            gpu_draw_bins: RenderWorldPersistentBins::default(),
            gpu_cull_active: false,
            hiz_pass: None,
            hiz_history: HizHistory::new(),
            app_time_secs: 0.0,
        }
    }

    /// Select the geometry front-end for draw submission.
    ///
    /// Call this once at init (after checking [`GeometryRendererCaps`]) to opt
    /// into frustum culling and indirect draw. Safe to change between frames.
    ///
    /// ```ignore
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

    fn build_render_world_draw_bins(
        &self,
        render_world: &RenderWorld,
    ) -> RenderWorldPersistentBins {
        RenderWorldPersistentBins::from_states(
            &render_world.snapshot(),
            GeometryBackend::ComputeIndirect,
            PipelineClass::GBuffer,
            MaterialShaderClass::PbrOpaque,
            VertexLayoutClass::StaticMesh,
            RenderStateClass::OpaqueDepthWrite,
        )
    }

    fn render_world_mesh_draws(&self) -> Vec<RenderWorldGpuMeshDrawInfo> {
        self.meshes
            .iter()
            .enumerate()
            .map(|(mesh_idx, (mesh, _))| {
                let index_count = if mesh.is_indexed() {
                    mesh.index_count
                } else {
                    mesh.vertex_count
                };
                RenderWorldGpuMeshDrawInfo::indexed(MeshId::from_raw(mesh_idx as u32), index_count)
            })
            .collect()
    }

    /// Returns `true` when GPU-driven culling and draw-generation are active for
    /// this scene, meaning [`draw_gbuffer_render_world_bins`] will use the GPU
    /// indirect path instead of falling back to the legacy batch draw.
    pub fn gpu_cull_active(&self) -> bool {
        self.gpu_cull_active
    }

    /// Total triangles that would be submitted if every persistent bin's objects
    /// were visible.  Computed from `index_count / 3` per mesh × the bin's
    /// object count.  Returns `None` when no GPU-cull bins have been built.
    pub fn submitted_triangle_count(&self) -> Option<u64> {
        if self.gpu_draw_bins.is_empty() {
            return None;
        }
        let count: u64 = self.gpu_draw_bins.bins.iter().fold(0u64, |acc, bin| {
            let mesh_idx = bin.key.mesh.index();
            let tri_per_mesh = self
                .meshes
                .get(mesh_idx)
                .map(|(mesh, _)| mesh.index_count as u64 / 3)
                .unwrap_or(0);
            acc.saturating_add(bin.object_count as u64 * tri_per_mesh)
        });
        Some(count)
    }

    fn render_world_draw_programs_ready(&self) -> bool {
        !self.gpu_draw_bins.is_empty()
            && self.gpu_draw_bins.bins.iter().all(|bin| {
                self.meshes
                    .get(bin.key.mesh.index())
                    .map(|(_, program)| program.uses_render_world_instances())
                    .unwrap_or(false)
            })
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

    /// Mirror a [`RenderWorld`] snapshot into this legacy `Scene`.
    ///
    /// This is the compatibility bridge while rendering transitions from
    /// object-oriented `Scene` authoring to ECS-authored persistent render-world
    /// data. It drains pending render-world commands, creates or updates scene
    /// objects for visible render-world objects, and marks no-longer-live or
    /// hidden objects as removed in the legacy scene.
    ///
    /// Returns the number of visible render-world objects synced into scene
    /// object slots.
    pub fn sync_render_world(&mut self, render_world: &RenderWorld) -> usize {
        render_world.apply_pending();

        let snapshot = render_world.snapshot();
        let mut synced = 0;
        let mut visible_objects = HashSet::new();

        for state in snapshot {
            let visible = state.visibility.flags.contains(VisibilityFlags::VISIBLE);
            let Some(mesh) = state.mesh else {
                if let Some(scene_object) = self.render_world_objects.remove(&state.object) {
                    self.remove_object_deferred(scene_object);
                }
                continue;
            };

            let mesh_valid = mesh.mesh.index() < self.meshes.len();
            if !visible || !mesh_valid {
                if let Some(scene_object) = self.render_world_objects.remove(&state.object) {
                    self.remove_object_deferred(scene_object);
                }
                continue;
            }

            visible_objects.insert(state.object);
            let transform = state
                .transform
                .as_ref()
                .map(|transform| transform.to_mat4())
                .unwrap_or(Mat4::IDENTITY);
            let kind = if state.visibility.flags.contains(VisibilityFlags::STATIC)
                && !state.visibility.flags.contains(VisibilityFlags::DYNAMIC)
            {
                ObjectKind::Static
            } else {
                ObjectKind::Dynamic
            };

            let scene_object = match self.render_world_objects.get(&state.object).copied() {
                Some(id) if id.index() < self.objects.len() => id,
                _ => {
                    let id = self.add_object_at(mesh.mesh, transform, kind);
                    self.render_world_objects.insert(state.object, id);
                    id
                }
            };

            if let Some(object) = self.objects.get_mut(scene_object.index()) {
                if object.mesh_id != mesh.mesh || object.kind != kind {
                    object.mesh_id = mesh.mesh;
                    object.kind = kind;
                    object.static_dirty.store(true, Ordering::Release);
                }
                object.set_render_metadata(
                    state.bounds,
                    state.material.map(|material| material.material.as_u32()),
                    state.visibility.flags,
                    0.0,
                );
                object.set_transform_atomic(transform);
            }

            synced += 1;
        }

        let stale: Vec<_> = self
            .render_world_objects
            .keys()
            .copied()
            .filter(|object| !visible_objects.contains(object))
            .collect();
        for object in stale {
            if let Some(scene_object) = self.render_world_objects.remove(&object) {
                self.remove_object_deferred(scene_object);
            }
        }

        synced
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

    /// Upload legacy per-batch instance data.
    ///
    /// Call once per frame after all `set_transform` calls, before `render`.
    /// Use [`prepare_render_world`](Self::prepare_render_world) when this scene
    /// is mirroring a [`RenderWorld`], so the flat GPU scene buffer is owned and
    /// uploaded by the render world.
    pub fn prepare(&mut self, engine: &Engine) -> Result<()> {
        self.prepare_internal(engine, None)
    }

    /// Upload legacy per-batch instance data and render-world-owned GPU scene data.
    pub fn prepare_render_world(
        &mut self,
        engine: &Engine,
        render_world: &RenderWorld,
    ) -> Result<()> {
        self.prepare_internal(engine, Some(render_world))
    }

    fn prepare_internal(
        &mut self,
        engine: &Engine,
        render_world: Option<&RenderWorld>,
    ) -> Result<()> {
        // Drain structural mutations queued from worker threads.
        // `take_all` extracts the Vec without borrowing self, so we can then
        // pass `&mut self` to each command.
        for cmd in self.commands.take_all() {
            cmd(self);
        }

        self.gpu_cull_active = false;

        // Pre-collect mesh bounding spheres so we can borrow them while filling
        // batches and later while uploading GPU scene data.
        let mesh_spheres: Vec<crate::BoundingSphere> = self
            .meshes
            .iter()
            .map(|(mesh, _)| mesh.bounding_sphere)
            .collect();

        // Rebuild compatibility batches from scene objects every frame. This is
        // simpler and more correct for the render-world bridge than preserving
        // stale static slots when objects are hidden, released, or change mesh.
        for batch in self.batches.values_mut() {
            batch.clear_instances();
        }

        // Fill instance lists and per-instance render metadata.
        // Use atomic Acquire loads for transforms — visible to any Release store
        // performed by worker threads via set_transform().
        for obj in &self.objects {
            if obj.mesh_id.index() >= self.meshes.len()
                || !obj.visibility_flags.contains(VisibilityFlags::VISIBLE)
            {
                continue;
            }

            let transform = obj.transform.load();
            let fallback_sphere = mesh_spheres
                .get(obj.mesh_id.index())
                .copied()
                .unwrap_or(crate::BoundingSphere::EMPTY);
            let metadata = obj.instance_metadata(fallback_sphere);
            let batch = self
                .batches
                .entry(obj.mesh_id.0)
                .or_insert_with(|| InstanceBatch::new(obj.mesh_id.0));
            match obj.kind {
                ObjectKind::Static => {
                    batch
                        .static_instances
                        .push(InstanceData::from_transform(transform));
                    batch.static_metadata.push(metadata);
                    obj.static_dirty.store(false, Ordering::Release);
                }
                ObjectKind::Dynamic => {
                    batch
                        .dynamic_instances
                        .push(InstanceData::from_transform(transform));
                    batch.dynamic_metadata.push(metadata);
                }
            }
        }
        if let Some(render_world) = render_world {
            render_world.prepare_gpu_scene(engine, self.meshes.len())?;
            render_world.prepare_gpu_transform_sources(
                engine,
                self.meshes.len(),
                RenderWorldGpuMatrixSettings::default(),
            )?;
            render_world.prepare_gpu_cull_outputs(
                engine,
                RenderWorldGpuCullSettings::default(),
                !self.hiz_history.is_empty(),
            )?;
            self.gpu_draw_bins = self.build_render_world_draw_bins(render_world);
            render_world.prepare_gpu_draw_generation(
                engine,
                &self.gpu_draw_bins,
                self.render_world_mesh_draws(),
                engine.caps().features.draw_indirect_count,
            )?;
            if self.transform_build_pass.is_none() {
                self.transform_build_pass = Some(RenderWorldGpuTransformBuildPass::new(engine)?);
            }
            if self.cull_pass.is_none() {
                self.cull_pass = Some(RenderWorldGpuCullPass::new(engine)?);
            }
            if self.hiz_pass.is_none() {
                self.hiz_pass = Some(HizPass::new(engine)?);
            }
            if self.draw_generation_pass.is_none() {
                self.draw_generation_pass = Some(RenderWorldGpuDrawGenerationPass::new(engine)?);
            }
            self.gpu_cull_active = self.render_world_draw_programs_ready();
        } else {
            self.gpu_draw_bins = RenderWorldPersistentBins::default();
        }

        for batch in self.batches.values_mut() {
            batch.prepare(engine)?;
            batch.indirect_commands.clear();

            if let Some(render_world) = render_world {
                if let Some(range) = render_world.gpu_scene_batch_range(batch.mesh_idx) {
                    batch.scene_base_idx = range.base;
                    batch.scene_count = range.count;
                } else {
                    batch.scene_base_idx = 0;
                    batch.scene_count = 0;
                }
            } else {
                batch.scene_base_idx = 0;
                batch.scene_count = 0;
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
            time: self.app_time_secs,
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

    /// Dispatch the render-world GPU compute passes without performing any draw calls.
    ///
    /// Runs transform-build, frustum cull, and draw-generation compute shaders so
    /// their outputs (current_matrices, visibility flags, indirect commands) are ready
    /// for the subsequent [`draw_gbuffer_render_world_bins`] call.
    ///
    /// `depth` — optionally the current frame's depth render target.  When provided,
    /// the Hi-Z pyramid builder records a pass to update the Hi-Z history and the
    /// cull pass uses the **previous** frame's pyramid for occlusion testing.  On the
    /// first frame the history is empty so culling falls back to frustum-only.
    ///
    /// Must be called after [`Scene::prepare_with_render_world`] and before
    /// [`draw_gbuffer_render_world_bins`] each frame.  No-ops when the required
    /// passes are not yet initialised or when `gpu_cull_active` is false.
    pub fn dispatch_render_world_gpu_passes(
        &mut self,
        frame: &RenderFrame,
        render_world: &RenderWorld,
        view_proj: glam::Mat4,
        depth: Option<&GraphImage>,
        proj: glam::Mat4,
    ) -> Result<()> {
        if !self.gpu_cull_active {
            return Ok(());
        }
        if let Some(build_pass) = &self.transform_build_pass {
            build_pass.execute(frame, render_world)?;
        }

        // Build Hi-Z pyramid from the current depth image so the cull pass can
        // use the *previous* frame's result for occlusion testing.
        let hiz_frame = if let (Some(depth), Some(hiz_pass)) = (depth, &self.hiz_pass) {
            Some(hiz_pass.execute_history(frame, depth, &mut self.hiz_history)?)
        } else {
            None
        };

        if let Some(cull_pass) = &self.cull_pass {
            let out_ext = depth
                .map(|d| d.desc().extent)
                .unwrap_or(sturdy_engine_core::Extent3d {
                    width: 0,
                    height: 0,
                    depth: 1,
                });
            let hiz_ref = hiz_frame
                .as_ref()
                .and_then(|hf| hf.previous.as_ref())
                .map(|prev| HizOcclusionInput {
                    pyramid: prev,
                    proj_y_scale: proj.col(1).y.abs(),
                    proj_z_scale: proj.col(2).z,
                    screen_size: [out_ext.width, out_ext.height],
                });
            cull_pass.execute(frame, render_world, view_proj, hiz_ref)?;
        }
        let _ = hiz_frame; // keep alive until after cull dispatch is recorded

        if let Some(draw_gen) = &self.draw_generation_pass {
            draw_gen.execute(frame, render_world)?;
        }
        Ok(())
    }

    /// Draw the G-Buffer using GPU-generated indirect draw commands from the render world.
    ///
    /// Call after [`dispatch_render_world_gpu_passes`].  For each persistent bin
    /// issues one indirect draw into the G-Buffer MRT targets using the
    /// `gbuffer_rw_program` (which must have a vertex shader that reads
    /// `render_world_visible_instances` and `current_matrices`).
    ///
    /// Returns immediately when `gpu_cull_active` is false.
    pub fn draw_gbuffer_render_world_bins(
        &self,
        constants: &CameraConstants,
        color_targets: &[&GraphImage],
        depth: &GraphImage,
        frame: &RenderFrame,
        render_world: &RenderWorld,
        gbuffer_rw_program: &MeshProgram,
        default_normal_map: &crate::Image,
    ) -> Result<()> {
        if !self.gpu_cull_active {
            return Ok(());
        }
        assert!(
            !color_targets.is_empty(),
            "draw_gbuffer_render_world_bins requires at least one color target"
        );
        let primary = color_targets[0];
        let additional = &color_targets[1..];

        render_world.with_gpu_draw_output(|draw_output| -> Result<()> {
            let Some(draw_output) = draw_output else {
                return Ok(());
            };
            frame.bind_buffer(
                "render_world_visible_instances",
                draw_output.visible_instances,
            );
            frame.bind_buffer("current_matrices", draw_output.current_matrices);
            // Bind previous-frame model matrices for per-object motion vectors.
            // Falls back to current matrices on the first frame (motion = 0).
            render_world.with_gpu_previous_matrix_buffer(|prev| {
                let prev_buf = prev.unwrap_or(draw_output.current_matrices);
                frame.bind_buffer("previous_matrices", prev_buf);
            });

            const INDIRECT_STRIDE: u64 =
                std::mem::size_of::<crate::DrawIndexedIndirectCommand>() as u64;

            // Build the bin item list for a single-pass multi-mesh indirect draw.
            // All bins share the same program and push constants; only vertex/index
            // buffers and indirect-command offset differ per bin.
            // Per-bin material bindings (legacy material_desc / normal_map) are set
            // on the frame before calling so the bind-group builder picks them up.
            // Note: with the material table (bindless path), per-bin bindings go
            // away and this becomes a pure single-descriptor-set pass.
            let mut bin_items: Vec<MultiMeshDrawBinItem<'_>> = Vec::with_capacity(
                self.gpu_draw_bins.bins.len()
            );
            for (bin_index, bin) in self.gpu_draw_bins.bins.iter().enumerate() {
                let mesh_idx = bin.key.mesh.index();
                let Some((mesh, _)) = self.meshes.get(mesh_idx) else {
                    continue;
                };
                // Bind per-bin material resources so the deferred resolver captures
                // the last registered binding when building the shared bind group.
                if let Some(mat) = self.materials.get(mesh_idx) {
                    if let Some(mat_buf) = &mat.gpu_buffer {
                        frame.bind_buffer("material_desc", mat_buf);
                    }
                    let nmap: &crate::Image = mat
                        .normal_map
                        .as_ref()
                        .map(|arc| arc.as_ref())
                        .unwrap_or(default_normal_map);
                    frame.bind_image("normal_map", nmap);
                } else {
                    frame.bind_image("normal_map", default_normal_map);
                }
                bin_items.push(MultiMeshDrawBinItem {
                    mesh,
                    indirect_buffer: draw_output.indirect_commands,
                    indirect_offset: bin_index as u64 * INDIRECT_STRIDE,
                    draw_count: 1,
                });
            }

            primary.draw_multi_mesh_indirect_mrt(
                additional,
                &bin_items,
                gbuffer_rw_program,
                constants,
                Some(depth),
            )?;
            Ok(())
        })
    }

    /// Draw all offscreen cameras into the render frame using the classic vertex path.
    ///
    /// Frustum culling is not applied to offscreen cameras; call `draw()` on the
    /// main camera to benefit from the `ComputeIndirect` backend.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        for camera in &self.cameras {
            let CameraOutput::Offscreen(rt) = &camera.output;
            let target = rt.as_frame_image(frame)?;
            let constants = CameraConstants::from_camera(camera).with_time(self.app_time_secs);
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
        self.draw_inner(view, proj, output, frame, engine, None)
    }

    /// Draw using render-world-owned GPU scene and indirect buffers.
    pub fn draw_render_world(
        &mut self,
        view: Mat4,
        proj: Mat4,
        output: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
        render_world: &RenderWorld,
    ) -> Result<()> {
        self.draw_inner(view, proj, output, frame, engine, Some(render_world))
    }

    fn draw_inner(
        &mut self,
        view: Mat4,
        proj: Mat4,
        output: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
        render_world: Option<&RenderWorld>,
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
            time: self.app_time_secs,
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
                compression: Default::default(),
                min_lod_bits: None,
                msaa_resolve_to_single_sampled: false,
                drm_format_modifier: None,
            },
        )?;
        self.draw_batches(
            &constants,
            proj,
            view_proj,
            output,
            Some(&depth),
            frame,
            engine,
            render_world,
        )
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
        let constants = CameraConstants::from_camera(camera).with_time(self.app_time_secs);
        self.draw_batches_classic(&constants, output, None, frame)
    }

    /// Draw path used by the main camera. Dispatches to the classic or indirect path
    /// based on `self.geometry_backend`.
    fn draw_batches(
        &mut self,
        constants: &CameraConstants,
        proj: Mat4,
        view_proj: Mat4,
        output: &GraphImage,
        depth: Option<&GraphImage>,
        frame: &RenderFrame,
        engine: &Engine,
        render_world: Option<&RenderWorld>,
    ) -> Result<()> {
        if self.geometry_backend != GeometryBackend::ComputeIndirect {
            return self.draw_batches_classic(constants, output, depth, frame);
        }

        // GPU-driven compute: expand transforms and write visibility flags every frame.
        // These run unconditionally when a render world is present so the derived GPU
        // tables (matrices, world bounds, visibility flags) are always current, even
        // while the draw path still uses the CPU-generated indirect commands.
        if let Some(render_world) = render_world {
            if let Some(build_pass) = &self.transform_build_pass {
                build_pass.execute(frame, render_world)?;
            }

            // Build Hi-Z pyramid from the current depth image via ping-pong history.
            // The current frame writes a fresh pyramid; the returned previous-frame
            // levels are used by the cull pass below for occlusion testing.
            // `hiz_frame` is kept alive here so the `previous` GraphImages remain valid
            // when the cull pass dispatches and the render graph records resource reads.
            let hiz_frame = if let (Some(depth), Some(hiz_pass)) = (depth, &self.hiz_pass) {
                Some(hiz_pass.execute_history(frame, depth, &mut self.hiz_history)?)
            } else {
                None
            };

            if let Some(cull_pass) = &self.cull_pass {
                let out_ext = output.desc().extent;
                let hiz_ref = hiz_frame
                    .as_ref()
                    .and_then(|hf| hf.previous.as_ref())
                    .map(|prev| HizOcclusionInput {
                        pyramid: prev,
                        // proj[1][1]: Y scale = 1/tan(fov_y/2); abs handles y-flip.
                        proj_y_scale: proj.col(1).y.abs(),
                        // proj[2][2]: depth scale for near-plane NDC depth derivation.
                        proj_z_scale: proj.col(2).z,
                        screen_size: [out_ext.width, out_ext.height],
                    });
                cull_pass.execute(frame, render_world, view_proj, hiz_ref)?;
            }
            let _ = hiz_frame; // keep alive until after the cull dispatch is recorded

            if let Some(draw_generation_pass) = &self.draw_generation_pass {
                draw_generation_pass.execute(frame, render_world)?;
            }
            if self.gpu_cull_active {
                return self.draw_render_world_bins(constants, output, depth, frame, render_world);
            }
        }

        // ComputeIndirect path: CPU frustum cull, write one DrawIndexedIndirectCommand
        // per visible instance, upload to GPU, then issue DrawIndirect.
        // Skipped when gpu_cull_active — the GPU already wrote the indirect buffer.
        if !self.gpu_cull_active {
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

                for (instance_idx, (inst, metadata)) in batch
                    .static_instances
                    .iter()
                    .zip(batch.static_metadata.iter())
                    .chain(
                        batch
                            .dynamic_instances
                            .iter()
                            .zip(batch.dynamic_metadata.iter()),
                    )
                    .enumerate()
                {
                    let model = Mat4::from_cols_array_2d(&inst.model);
                    let world_sphere = metadata.local_sphere.transform(model);
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
        } // end !gpu_cull_active CPU cull block

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
            if self.gpu_cull_active {
                let Some(render_world) = render_world else {
                    continue;
                };
                const INDIRECT_STRIDE: u64 =
                    std::mem::size_of::<DrawIndexedIndirectCommand>() as u64;
                let indirect_offset = batch.scene_base_idx as u64 * INDIRECT_STRIDE;
                render_world.with_gpu_indirect_buffer(|indirect_buf| -> Result<()> {
                    let Some(indirect_buf) = indirect_buf else {
                        return Ok(());
                    };
                    output.draw_mesh_indirect_range_with_push_constants_and_depth(
                        mesh,
                        program,
                        instance_buf,
                        indirect_buf,
                        indirect_offset,
                        draw_count,
                        constants,
                        effective_depth,
                    )
                })?;
            } else {
                let indirect_buf = match &batch.indirect_gpu_buffer {
                    Some(b) => b,
                    None => continue,
                };
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
        }
        Ok(())
    }

    fn draw_render_world_bins(
        &self,
        constants: &CameraConstants,
        output: &GraphImage,
        depth: Option<&GraphImage>,
        frame: &RenderFrame,
        render_world: &RenderWorld,
    ) -> Result<()> {
        render_world.with_gpu_draw_output(|draw_output| -> Result<()> {
            let Some(draw_output) = draw_output else {
                return Ok(());
            };
            frame.bind_buffer(
                "render_world_visible_instances",
                draw_output.visible_instances,
            );
            frame.bind_buffer("current_matrices", draw_output.current_matrices);
            render_world.with_gpu_previous_matrix_buffer(|prev| {
                let prev_buf = prev.unwrap_or(draw_output.current_matrices);
                frame.bind_buffer("previous_matrices", prev_buf);
            });

            const INDIRECT_STRIDE: u64 = std::mem::size_of::<DrawIndexedIndirectCommand>() as u64;
            for (bin_index, bin) in self.gpu_draw_bins.bins.iter().enumerate() {
                let mesh_idx = bin.key.mesh.index();
                let Some((mesh, program)) = self.meshes.get(mesh_idx) else {
                    continue;
                };
                if !program.uses_render_world_instances() {
                    continue;
                }
                if let Some(light_buf) = &self.light_buffer {
                    frame.bind_buffer("lighting", light_buf);
                }
                if let Some(mat) = self.materials.get(mesh_idx) {
                    if let Some(mat_buf) = &mat.gpu_buffer {
                        frame.bind_buffer("material_desc", mat_buf);
                    }
                }
                let effective_depth = if program.uses_depth { depth } else { None };
                output.draw_mesh_indirect_range_with_push_constants_and_depth(
                    mesh,
                    program,
                    draw_output.current_matrices,
                    draw_output.indirect_commands,
                    bin_index as u64 * INDIRECT_STRIDE,
                    1,
                    constants,
                    effective_depth,
                )?;
            }
            Ok(())
        })
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
