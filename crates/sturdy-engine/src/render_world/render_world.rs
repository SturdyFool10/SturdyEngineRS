use std::collections::{BTreeMap, HashMap};
use parking_lot::Mutex;
use rayon::prelude::*;

use glam::Mat4;

use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, GpuInstanceData, Result,
    ecs::{Transform, World},
};

use super::{
    GpuObjectAllocator, GpuObjectId, GpuTransformSourceData, LocalToWorld, PreviousTransform,
    RenderBounds, RenderDirtyFlags, RenderExtractionStats, RenderMaterial, RenderMesh,
    RenderObjectState, RenderVisibility, RenderWorldBatchRange, RenderWorldCommand,
    RenderWorldCommands, RenderWorldGpuBinData, RenderWorldGpuCullCaps,
    RenderWorldGpuCullOutputStats, RenderWorldGpuCullSettings, RenderWorldGpuDrawGenerationStats,
    RenderWorldGpuDrawOutput, RenderWorldGpuMatrixCaps, RenderWorldGpuMatrixSettings,
    RenderWorldGpuMatrixStats, RenderWorldGpuMeshDrawInfo, RenderWorldGpuSceneData,
    RenderWorldGpuSceneState, RenderWorldGpuSceneStats, RenderWorldGpuTransformSourceData,
    RenderWorldPersistentBins, VisibilityFlags,
};

/// Thread-safe CPU staging world for persistent GPU-renderable objects.
///
/// Gameplay/ECS code can reserve object IDs and queue compact render mutations
/// from any thread. The render-extract phase drains those commands into a
/// deterministic CPU snapshot; later GPU passes can upload dirty slots and let
/// compute build matrices, bounds, visibility, and indirect commands.
pub struct RenderWorld {
    objects: GpuObjectAllocator,
    commands: RenderWorldCommands,
    states: Mutex<HashMap<GpuObjectId, RenderObjectState>>,
    gpu_scene: Mutex<RenderWorldGpuSceneState>,
}

impl RenderWorld {
    pub fn new() -> Self {
        Self {
            objects: GpuObjectAllocator::new(),
            commands: RenderWorldCommands::new(),
            states: Mutex::new(HashMap::new()),
            gpu_scene: Mutex::new(RenderWorldGpuSceneState::new()),
        }
    }

    /// Access the object slot allocator.
    pub fn objects(&self) -> &GpuObjectAllocator {
        &self.objects
    }

    /// Cloneable command handle for worker threads.
    pub fn commands(&self) -> RenderWorldCommands {
        self.commands.clone()
    }

    /// Reserve a GPU object slot and queue creation of its staging state.
    pub fn reserve_object(&self) -> GpuObjectId {
        let object = self.objects.reserve();
        self.commands.create_object(object);
        object
    }

    /// Queue release of a GPU object slot.
    pub fn release_object(&self, object: GpuObjectId) {
        self.commands.release_object(object);
    }

    pub fn set_transform(&self, object: GpuObjectId, transform: Transform) {
        self.commands.set_transform(object, transform);
    }

    pub fn set_previous_transform(&self, object: GpuObjectId, previous: PreviousTransform) {
        self.commands.set_previous_transform(object, previous);
    }

    pub fn set_mesh(&self, object: GpuObjectId, mesh: RenderMesh) {
        self.commands.set_mesh(object, mesh);
    }

    pub fn set_material(&self, object: GpuObjectId, material: RenderMaterial) {
        self.commands.set_material(object, material);
    }

    pub fn set_bounds(&self, object: GpuObjectId, bounds: RenderBounds) {
        self.commands.set_bounds(object, bounds);
    }

    pub fn set_visibility(&self, object: GpuObjectId, visibility: RenderVisibility) {
        self.commands.set_visibility(object, visibility);
    }

    /// Drain pending commands into the staged render-world state.
    ///
    /// Returns the number of commands applied.
    pub fn apply_pending(&self) -> usize {
        let commands = self.commands.take_all();
        let applied = commands.len();
        if commands.is_empty() {
            return 0;
        }

        let mut states = self.lock_states();

        for command in commands {
            match command {
                RenderWorldCommand::CreateObject(object) => {
                    states
                        .entry(object)
                        .or_insert_with(|| RenderObjectState::new(object));
                }
                RenderWorldCommand::ReleaseObject(object) => {
                    if states.remove(&object).is_some() {
                        self.objects.release(object);
                    }
                }
                RenderWorldCommand::SetTransform(object, transform) => {
                    let state = state_for(&mut states, object);
                    state.transform = Some(transform);
                    state.dirty.insert(RenderDirtyFlags::TRANSFORM);
                }
                RenderWorldCommand::SetPreviousTransform(object, previous) => {
                    let state = state_for(&mut states, object);
                    state.previous_transform = Some(previous);
                    state.dirty.insert(RenderDirtyFlags::PREVIOUS_TRANSFORM);
                }
                RenderWorldCommand::SetMesh(object, mesh) => {
                    let state = state_for(&mut states, object);
                    state.mesh = Some(mesh);
                    state.dirty.insert(RenderDirtyFlags::MESH);
                }
                RenderWorldCommand::SetMaterial(object, material) => {
                    let state = state_for(&mut states, object);
                    state.material = Some(material);
                    state.dirty.insert(RenderDirtyFlags::MATERIAL);
                }
                RenderWorldCommand::SetBounds(object, bounds) => {
                    let state = state_for(&mut states, object);
                    state.bounds = Some(bounds);
                    state.dirty.insert(RenderDirtyFlags::BOUNDS);
                }
                RenderWorldCommand::SetVisibility(object, visibility) => {
                    let state = state_for(&mut states, object);
                    state.visibility = visibility;
                    state.dirty.insert(RenderDirtyFlags::VISIBILITY);
                }
            }
        }

        applied
    }

    /// Allocate GPU object links for entities that have `RenderMesh` but do not
    /// yet have `LocalToWorld`.
    pub fn allocate_missing_objects(&self, world: &mut World) -> usize {
        let missing: Vec<_> = world
            .entities_with::<RenderMesh>()
            .filter(|&entity| !world.has::<LocalToWorld>(entity))
            .collect();

        for entity in &missing {
            let object = self.reserve_object();
            world.insert(*entity, LocalToWorld::new(object));
        }

        missing.len()
    }

    /// Extract compact ECS render components into this render world.
    ///
    /// Missing `LocalToWorld` links are allocated lazily for entities with
    /// `RenderMesh`, then all linked entities have their render components
    /// mirrored into queued render-world commands and applied immediately.
    pub fn extract_from_world(&self, world: &mut World) -> RenderExtractionStats {
        // Phase 1 (sequential): allocate GPU slots for new entities.
        let allocated_objects = self.allocate_missing_objects(world);

        // Phase 2 (parallel): extract components from all linked entities.
        // World::get<T> is read-only (&self) and World: Sync, so this is safe
        // to parallelize across rayon threads.  self.commands is thread-safe.
        let linked: Vec<_> = world
            .query::<LocalToWorld>()
            .map(|(entity, link)| (entity, link.object))
            .collect();

        // Parallel extraction: compute all commands in parallel, then push in a single
        // batch (one lock acquisition instead of N×6 per entity).
        let world_ref: &World = world;
        let all_commands: Vec<RenderWorldCommand> = linked
            .par_iter()
            .flat_map_iter(|(entity, object)| {
                let mut cmds = Vec::with_capacity(6);
                if let Some(t) = world_ref.get::<Transform>(*entity) {
                    cmds.push(RenderWorldCommand::SetTransform(*object, t.clone()));
                }
                if let Some(p) = world_ref.get::<PreviousTransform>(*entity) {
                    cmds.push(RenderWorldCommand::SetPreviousTransform(*object, p.clone()));
                }
                if let Some(m) = world_ref.get::<RenderMesh>(*entity) {
                    cmds.push(RenderWorldCommand::SetMesh(*object, *m));
                }
                if let Some(mat) = world_ref.get::<RenderMaterial>(*entity) {
                    cmds.push(RenderWorldCommand::SetMaterial(*object, *mat));
                }
                if let Some(b) = world_ref.get::<RenderBounds>(*entity) {
                    cmds.push(RenderWorldCommand::SetBounds(*object, *b));
                }
                if let Some(v) = world_ref.get::<RenderVisibility>(*entity) {
                    cmds.push(RenderWorldCommand::SetVisibility(*object, *v));
                }
                cmds
            })
            .collect();
        // Single lock acquisition to push all collected commands.
        self.commands.push_batch(all_commands);

        RenderExtractionStats {
            allocated_objects,
            extracted_entities: linked.len(),
            applied_commands: self.apply_pending(),
        }
    }

    /// Return a cloned object state snapshot for diagnostics/tests.
    pub fn object(&self, object: GpuObjectId) -> Option<RenderObjectState> {
        self.lock_states().get(&object).cloned()
    }

    /// Return a cloned snapshot of all staged object states.
    pub fn snapshot(&self) -> Vec<RenderObjectState> {
        let mut snapshot: Vec<_> = self.lock_states().values().cloned().collect();
        snapshot.sort_by_key(|state| state.object);
        snapshot
    }

    /// Build CPU-side GPU scene data from staged render-world objects.
    ///
    /// Objects are grouped by mesh ID and ordered by stable `GpuObjectId` within
    /// each mesh. This matches the compatibility `Scene` object creation order
    /// after `Scene::sync_render_world`, so per-batch indirect `first_instance`
    /// values still index the legacy per-mesh instance buffers correctly.
    pub fn build_gpu_scene_data(&self, valid_mesh_count: usize) -> RenderWorldGpuSceneData {
        let entries = gpu_scene_entries_from_states(&self.snapshot(), valid_mesh_count);
        let instances = entries.iter().map(|(_, instance)| *instance).collect();
        let ranges = gpu_scene_ranges(&entries);
        RenderWorldGpuSceneData { instances, ranges }
    }

    /// Upload staged render-world object data into the render-world-owned GPU scene buffer.
    ///
    /// Structural changes rebuild and upload the whole compact scene buffer.
    /// Non-structural changes reuse stable object slots and update only changed
    /// `GpuInstanceData` entries.
    pub fn prepare_gpu_scene(
        &self,
        engine: &Engine,
        valid_mesh_count: usize,
    ) -> Result<RenderWorldGpuSceneStats> {
        self.apply_pending();

        let mut states = self.lock_states();
        let snapshot: Vec<_> = states.values().cloned().collect();
        let entries = gpu_scene_entries_from_states(&snapshot, valid_mesh_count);
        let instances: Vec<_> = entries.iter().map(|(_, instance)| *instance).collect();
        let ranges = gpu_scene_ranges(&entries);
        let object_slots = gpu_scene_object_slots(&entries);
        let instance_count = instances.len();
        let batch_count = ranges.len();
        let stride = std::mem::size_of::<GpuInstanceData>();

        let mut gpu_scene = self.lock_gpu_scene();
        let mut reallocated = false;
        if instance_count > gpu_scene.capacity || gpu_scene.buffer.is_none() {
            let new_capacity = instance_count.next_power_of_two().max(4);
            gpu_scene.buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_capacity * stride) as u64,
                usage: BufferUsage::STORAGE,
            })?);
            gpu_scene.capacity = new_capacity;
            reallocated = true;
        }

        let indirect_stride = std::mem::size_of::<crate::DrawIndexedIndirectCommand>();
        let mut indirect_reallocated = false;
        if instance_count > gpu_scene.indirect_capacity || gpu_scene.indirect_buffer.is_none() {
            let new_capacity = instance_count.next_power_of_two().max(4);
            gpu_scene.indirect_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_capacity * indirect_stride) as u64,
                usage: BufferUsage::INDIRECT | BufferUsage::STORAGE,
            })?);
            gpu_scene.indirect_capacity = new_capacity;
            indirect_reallocated = true;
        }

        let ranges_changed = gpu_scene.ranges != ranges;
        let slots_changed = gpu_scene.object_slots != object_slots;
        let structural_dirty = states.values()
            .collect::<Vec<_>>()
            .par_iter()
            .any(|state| {
                state.dirty.contains(RenderDirtyFlags::STRUCTURAL)
                    || state.dirty.contains(RenderDirtyFlags::MESH)
                    || state.dirty.contains(RenderDirtyFlags::VISIBILITY)
            });
        let full_rebuild = reallocated || ranges_changed || slots_changed || structural_dirty;

        let mut uploaded_instances = 0;
        if let Some(buffer) = &gpu_scene.buffer {
            if full_rebuild {
                if !instances.is_empty() {
                    buffer.write(0, bytemuck::cast_slice(&instances))?;
                }
                uploaded_instances = instances.len();
            } else {
                for (object, instance) in &entries {
                    let Some(state) = states.get(object) else {
                        continue;
                    };
                    if state.dirty.is_empty() {
                        continue;
                    }
                    let Some(slot) = gpu_scene.object_slots.get(object).copied() else {
                        continue;
                    };
                    buffer.write(slot as u64 * stride as u64, bytemuck::bytes_of(instance))?;
                    uploaded_instances += 1;
                }
            }
        }

        gpu_scene.ranges = ranges;
        gpu_scene.object_slots = object_slots;
        for state in states.values_mut() {
            state.clear_dirty();
        }

        Ok(RenderWorldGpuSceneStats {
            instance_count,
            batch_count,
            capacity: gpu_scene.capacity,
            reallocated,
            indirect_reallocated,
            full_rebuild,
            uploaded_instances,
        })
    }

    pub fn gpu_scene_batch_range(&self, mesh_id: u32) -> Option<RenderWorldBatchRange> {
        self.lock_gpu_scene().ranges.get(&mesh_id).copied()
    }

    pub fn with_gpu_scene_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().buffer.as_ref())
    }

    pub fn with_gpu_indirect_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().indirect_buffer.as_ref())
    }

    pub fn with_gpu_transform_source_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().transform_source_buffer.as_ref())
    }

    pub fn with_gpu_current_matrix_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().current_matrix_buffer.as_ref())
    }

    pub fn with_gpu_previous_matrix_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().previous_matrix_buffer.as_ref())
    }

    pub fn with_gpu_normal_matrix_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().normal_matrix_buffer.as_ref())
    }

    pub fn with_gpu_world_bounds_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().world_bounds_buffer.as_ref())
    }

    pub fn with_gpu_visibility_flags_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().visibility_flags_buffer.as_ref())
    }

    pub fn with_gpu_draw_bin_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().draw_bin_buffer.as_ref())
    }

    pub fn with_gpu_draw_indirect_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().draw_indirect_buffer.as_ref())
    }

    pub fn with_gpu_draw_count_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().draw_count_buffer.as_ref())
    }

    pub fn with_gpu_visible_instance_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        f(self.lock_gpu_scene().visible_instance_buffer.as_ref())
    }

    pub fn with_gpu_draw_output<R>(
        &self,
        f: impl FnOnce(Option<RenderWorldGpuDrawOutput<'_>>) -> R,
    ) -> R {
        let gpu_scene = self.lock_gpu_scene();
        let output = match (
            gpu_scene.draw_indirect_buffer.as_ref(),
            gpu_scene.draw_count_buffer.as_ref(),
            gpu_scene.visible_instance_buffer.as_ref(),
            gpu_scene.current_matrix_buffer.as_ref(),
            gpu_scene.draw_generation_plan.as_ref(),
        ) {
            (
                Some(indirect_commands),
                Some(visible_draw_count),
                Some(visible_instances),
                Some(current_matrices),
                Some(plan),
            ) => Some(RenderWorldGpuDrawOutput {
                indirect_commands,
                visible_draw_count,
                visible_instances,
                current_matrices,
                max_draw_count: plan.bin_count,
                use_indirect_count: plan.uses_indirect_count,
            }),
            _ => None,
        };
        f(output)
    }

    pub fn gpu_transform_source_slot(&self, object: GpuObjectId) -> Option<u32> {
        self.lock_gpu_scene()
            .transform_source_slots
            .get(&object)
            .copied()
    }

    pub fn gpu_matrix_plan(&self) -> Option<super::RenderWorldGpuMatrixPlan> {
        self.lock_gpu_scene().matrix_plan.clone()
    }

    pub fn gpu_cull_plan(&self) -> Option<super::RenderWorldGpuCullPlan> {
        self.lock_gpu_scene().cull_plan.clone()
    }

    pub fn gpu_draw_generation_plan(&self) -> Option<super::RenderWorldGpuDrawGenerationPlan> {
        self.lock_gpu_scene().draw_generation_plan.clone()
    }

    /// Upload compact transform-source data and allocate GPU-derived matrix/bounds buffers.
    ///
    /// This is the first-stage GPU-driven render-world path: the CPU uploads compact
    /// TRS/previous-TRS source data while GPU-visible derived buffers are allocated
    /// for a later compute expansion pass. The legacy `prepare_gpu_scene` path is
    /// intentionally left intact as a compatibility draw path.
    pub fn prepare_gpu_transform_sources(
        &self,
        engine: &Engine,
        valid_mesh_count: usize,
        settings: RenderWorldGpuMatrixSettings,
    ) -> Result<RenderWorldGpuMatrixStats> {
        self.apply_pending();

        let mut states = self.lock_states();
        let source_states = transform_source_states(&states, valid_mesh_count);
        let source_data = RenderWorldGpuTransformSourceData::from_states(&source_states);
        let object_count = source_data.len();
        let source_stride = std::mem::size_of::<GpuTransformSourceData>();
        let plan = super::RenderWorldGpuMatrixPlan::plan(
            object_count,
            RenderWorldGpuMatrixCaps::from_caps(&engine.caps()),
            settings,
        );
        let source_slots: HashMap<_, _> = source_data.object_slots.iter().copied().collect();

        let structural_dirty = states.values()
            .collect::<Vec<_>>()
            .par_iter()
            .any(|state| {
                state.dirty.contains(RenderDirtyFlags::STRUCTURAL)
                    || state.dirty.contains(RenderDirtyFlags::MESH)
                    || state.dirty.contains(RenderDirtyFlags::VISIBILITY)
            });
        let dirty_source_slots = states.iter().filter_map(|(object, state)| {
            let transform_dirty = state.dirty.contains(RenderDirtyFlags::TRANSFORM)
                || state.dirty.contains(RenderDirtyFlags::PREVIOUS_TRANSFORM);
            transform_dirty
                .then(|| source_slots.get(object).copied())
                .flatten()
        });

        let mut gpu_scene = self.lock_gpu_scene();
        let mut source_reallocated = false;
        if object_count > 0
            && (object_count > gpu_scene.transform_source_capacity
                || gpu_scene.transform_source_buffer.is_none())
        {
            let new_capacity = object_count.next_power_of_two().max(4);
            gpu_scene.transform_source_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_capacity * source_stride) as u64,
                usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
            })?);
            gpu_scene.transform_source_capacity = new_capacity;
            source_reallocated = true;
        }

        let slots_changed = gpu_scene.transform_source_slots != source_slots;
        let full_source_upload = source_reallocated || slots_changed || structural_dirty;
        let mut uploaded_source_ranges = 0usize;
        let mut uploaded_source_objects = 0usize;
        let mut uploaded_source_bytes = 0u64;

        if let Some(buffer) = &gpu_scene.transform_source_buffer {
            if full_source_upload {
                if !source_data.transforms.is_empty() {
                    buffer.write_slice(0, &source_data.transforms)?;
                    uploaded_source_ranges = 1;
                    uploaded_source_objects = source_data.transforms.len();
                    uploaded_source_bytes = (uploaded_source_objects * source_stride) as u64;
                }
            } else if plan.supports_dirty_source_uploads {
                let ranges = super::gpu_transform_dirty_ranges(
                    dirty_source_slots,
                    object_count.min(u32::MAX as usize) as u32,
                    1,
                );
                for range in ranges {
                    let start = range.start as usize;
                    let end = range.end as usize;
                    buffer.write_slice(range.byte_offset(), &source_data.transforms[start..end])?;
                    uploaded_source_ranges += 1;
                    uploaded_source_objects += range.len() as usize;
                    uploaded_source_bytes += range.byte_len();
                }
            } else if states.values().any(|state| !state.dirty.is_empty())
                && !source_data.transforms.is_empty()
            {
                buffer.write_slice(0, &source_data.transforms)?;
                uploaded_source_ranges = 1;
                uploaded_source_objects = source_data.transforms.len();
                uploaded_source_bytes = (uploaded_source_objects * source_stride) as u64;
            }
        }

        let mut derived_reallocated = false;
        // These buffers are exclusively GPU-compute outputs — the CPU never writes
        // to them after creation.  GPU_ONLY causes DEVICE_LOCAL allocation on
        // discrete GPUs for maximum GPU memory bandwidth.
        derived_reallocated |= ensure_buffer_capacity(
            engine,
            &mut gpu_scene.current_matrix_buffer,
            plan.current_matrix_bytes,
            BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
        )?;
        derived_reallocated |= ensure_buffer_capacity(
            engine,
            &mut gpu_scene.previous_matrix_buffer,
            plan.previous_matrix_bytes,
            BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
        )?;
        derived_reallocated |= ensure_buffer_capacity(
            engine,
            &mut gpu_scene.normal_matrix_buffer,
            plan.normal_matrix_bytes,
            BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
        )?;
        derived_reallocated |= ensure_buffer_capacity(
            engine,
            &mut gpu_scene.world_bounds_buffer,
            plan.bounds_bytes,
            BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
        )?;

        gpu_scene.transform_source_slots = source_slots;
        gpu_scene.matrix_plan = Some(plan.clone());
        let source_capacity = gpu_scene.transform_source_capacity;

        for state in states.values_mut() {
            state.clear_dirty();
        }

        Ok(RenderWorldGpuMatrixStats {
            object_count,
            source_capacity,
            source_reallocated,
            derived_reallocated,
            full_source_upload,
            uploaded_source_ranges,
            uploaded_source_objects,
            uploaded_source_bytes,
            uses_gpu_generation: plan.uses_gpu_generation,
            total_derived_bytes: plan.total_derived_bytes,
            degraded_reason: plan.degraded_reason,
        })
    }

    /// Allocate scene-wide GPU culling output buffers for the prepared render world.
    pub fn prepare_gpu_cull_outputs(
        &self,
        engine: &Engine,
        settings: RenderWorldGpuCullSettings,
        hiz_occlusion: bool,
    ) -> Result<RenderWorldGpuCullOutputStats> {
        let matrix_plan = self.gpu_matrix_plan().ok_or_else(|| {
            crate::Error::InvalidInput(
                "RenderWorld GPU cull outputs require prepare_gpu_transform_sources first".into(),
            )
        })?;
        let object_count = matrix_plan.object_count as usize;
        let batch_count = self.lock_gpu_scene().ranges.len().max(1);
        let cull_plan = super::RenderWorldGpuCullPlan::plan(
            object_count,
            batch_count,
            RenderWorldGpuCullCaps::from_caps(&engine.caps(), hiz_occlusion),
            settings,
        );

        let stride = std::mem::size_of::<u32>();
        let mut gpu_scene = self.lock_gpu_scene();
        let mut visibility_reallocated = false;
        if object_count > 0
            && (object_count > gpu_scene.visibility_flags_capacity
                || gpu_scene.visibility_flags_buffer.is_none())
        {
            let new_capacity = object_count.next_power_of_two().max(4);
            gpu_scene.visibility_flags_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_capacity * stride) as u64,
                usage: BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
            })?);
            gpu_scene.visibility_flags_capacity = new_capacity;
            visibility_reallocated = true;
        }
        if object_count == 0 {
            gpu_scene.cull_plan = Some(cull_plan);
            return Ok(RenderWorldGpuCullOutputStats::default());
        }
        gpu_scene.cull_plan = Some(cull_plan);

        Ok(RenderWorldGpuCullOutputStats {
            object_count,
            visibility_capacity: gpu_scene.visibility_flags_capacity,
            visibility_reallocated,
            output_bytes: (object_count * stride) as u64,
        })
    }

    /// Allocate and upload persistent-bin draw generation inputs/outputs.
    pub fn prepare_gpu_draw_generation(
        &self,
        engine: &Engine,
        bins: &RenderWorldPersistentBins,
        mesh_draws: impl IntoIterator<Item = RenderWorldGpuMeshDrawInfo>,
        use_indirect_count: bool,
    ) -> Result<RenderWorldGpuDrawGenerationStats> {
        if self.lock_gpu_scene().visibility_flags_buffer.is_none() && !bins.is_empty() {
            return Err(crate::Error::InvalidInput(
                "RenderWorld GPU draw generation requires prepare_gpu_cull_outputs first".into(),
            ));
        }

        let gpu_bins = RenderWorldGpuBinData::from_bins(bins, mesh_draws)
            .map_err(crate::Error::InvalidInput)?;
        let plan = super::RenderWorldGpuDrawGenerationPlan::plan(bins, use_indirect_count, 64);

        let bin_stride = std::mem::size_of::<RenderWorldGpuBinData>();
        let indirect_stride = std::mem::size_of::<crate::DrawIndexedIndirectCommand>();
        let visible_instance_stride = std::mem::size_of::<u32>();
        let bin_count = gpu_bins.len();
        let object_count = bins.object_count as usize;
        let mut gpu_scene = self.lock_gpu_scene();

        let mut bin_buffer_reallocated = false;
        if bin_count > 0
            && (bin_count > gpu_scene.draw_bin_capacity || gpu_scene.draw_bin_buffer.is_none())
        {
            let new_capacity = bin_count.next_power_of_two().max(4);
            gpu_scene.draw_bin_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_capacity * bin_stride) as u64,
                usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
            })?);
            gpu_scene.draw_bin_capacity = new_capacity;
            bin_buffer_reallocated = true;
        }

        let mut uploaded_bin_bytes = 0;
        if let Some(buffer) = &gpu_scene.draw_bin_buffer {
            if !gpu_bins.is_empty() {
                buffer.write_slice(0, &gpu_bins)?;
                uploaded_bin_bytes = (gpu_bins.len() * bin_stride) as u64;
            }
        }

        let mut indirect_buffer_reallocated = false;
        if bin_count > 0
            && (bin_count > gpu_scene.draw_indirect_capacity
                || gpu_scene.draw_indirect_buffer.is_none())
        {
            let new_capacity = bin_count.next_power_of_two().max(4);
            gpu_scene.draw_indirect_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_capacity * indirect_stride) as u64,
                // GPU_ONLY: written exclusively by render_world_draw_generate.slang compute.
                usage: BufferUsage::INDIRECT | BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
            })?);
            gpu_scene.draw_indirect_capacity = new_capacity;
            indirect_buffer_reallocated = true;
        }

        let mut count_buffer_reallocated = false;
        if gpu_scene.draw_count_buffer.is_none() {
            gpu_scene.draw_count_buffer = Some(engine.create_buffer(BufferDesc {
                size: std::mem::size_of::<u32>() as u64,
                // GPU_ONLY: written by GPU atomic in draw generation, zeroed by vkCmdFillBuffer.
                usage: BufferUsage::INDIRECT | BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
            })?);
            count_buffer_reallocated = true;
        }

        let mut visible_instance_buffer_reallocated = false;
        if object_count > 0
            && (object_count > gpu_scene.visible_instance_capacity
                || gpu_scene.visible_instance_buffer.is_none())
        {
            let new_capacity = object_count.next_power_of_two().max(4);
            gpu_scene.visible_instance_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_capacity * visible_instance_stride) as u64,
                // GPU_ONLY: compacted by cull compute, never written by CPU.
                usage: BufferUsage::STORAGE | BufferUsage::COPY_DST | BufferUsage::GPU_ONLY,
            })?);
            gpu_scene.visible_instance_capacity = new_capacity;
            visible_instance_buffer_reallocated = true;
        }

        gpu_scene.draw_generation_plan = Some(plan.clone());

        Ok(RenderWorldGpuDrawGenerationStats {
            bin_count,
            object_count,
            bin_buffer_reallocated,
            indirect_buffer_reallocated,
            count_buffer_reallocated,
            visible_instance_buffer_reallocated,
            uploaded_bin_bytes,
            indirect_bytes: (bin_count * indirect_stride) as u64,
            visible_instance_bytes: (object_count * visible_instance_stride) as u64,
            uses_indirect_count: plan.uses_indirect_count,
            degraded_reason: plan.degraded_reason,
        })
    }

    /// Return and clear all dirty object states.
    pub fn take_dirty(&self) -> Vec<RenderObjectState> {
        let mut states = self.lock_states();
        let mut dirty = Vec::new();
        for state in states.values_mut() {
            if !state.dirty.is_empty() {
                dirty.push(state.clone());
                state.clear_dirty();
            }
        }
        dirty.sort_by_key(|state| state.object);
        dirty
    }

    pub fn object_count(&self) -> usize {
        self.states.lock().len()
    }

    pub fn pending_command_count(&self) -> usize {
        self.commands.pending_count()
    }

    // ── Private lock helpers ──────────────────────────────────────────────────

    fn lock_states(&self) -> parking_lot::MutexGuard<'_, HashMap<GpuObjectId, RenderObjectState>> {
        self.states
            .lock()
    }

    fn lock_gpu_scene(&self) -> parking_lot::MutexGuard<'_, RenderWorldGpuSceneState> {
        self.gpu_scene
            .lock()
    }
}

impl Default for RenderWorld {
    fn default() -> Self {
        Self::new()
    }
}

fn state_for(
    states: &mut HashMap<GpuObjectId, RenderObjectState>,
    object: GpuObjectId,
) -> &mut RenderObjectState {
    states
        .entry(object)
        .or_insert_with(|| RenderObjectState::new(object))
}

fn ensure_buffer_capacity(
    engine: &Engine,
    buffer: &mut Option<Buffer>,
    required_bytes: u64,
    usage: BufferUsage,
) -> Result<bool> {
    if required_bytes == 0 {
        *buffer = None;
        return Ok(false);
    }

    let needs_reallocate = buffer
        .as_ref()
        .map(|buffer| buffer.desc().size < required_bytes)
        .unwrap_or(true);
    if needs_reallocate {
        *buffer = Some(engine.create_buffer(BufferDesc {
            size: required_bytes.next_power_of_two(),
            usage,
        })?);
        Ok(true)
    } else {
        Ok(false)
    }
}

fn transform_source_states(
    states: &HashMap<GpuObjectId, RenderObjectState>,
    valid_mesh_count: usize,
) -> Vec<RenderObjectState> {
    // Parallel filter+collect for large scenes.
    states
        .values()
        .collect::<Vec<_>>()
        .into_par_iter()
        .filter(|state| {
            state.visibility.flags.contains(VisibilityFlags::VISIBLE)
                && state.mesh.is_some_and(|mesh| mesh.mesh.index() < valid_mesh_count)
        })
        .cloned()
        .collect()
}

fn gpu_scene_entries_from_states(
    states: &[RenderObjectState],
    valid_mesh_count: usize,
) -> Vec<(GpuObjectId, GpuInstanceData)> {
    // Phase 1 (parallel): compute instance data for each visible state.
    // All reads are immutable so this is safe across rayon threads.
    let flat: Vec<(GpuObjectId, GpuInstanceData)> = states
        .par_iter()
        .filter_map(|state| {
            if !state.visibility.flags.contains(VisibilityFlags::VISIBLE) {
                return None;
            }
            let mesh = state.mesh?;
            if mesh.mesh.index() >= valid_mesh_count {
                return None;
            }
            let model = state
                .transform
                .as_ref()
                .map(|transform| transform.to_mat4())
                .unwrap_or(Mat4::IDENTITY);
            let local_sphere = state
                .bounds
                .map(|bounds| bounds.local_sphere)
                .unwrap_or(crate::BoundingSphere::EMPTY);
            let world_sphere = local_sphere.transform(model);
            let material_id = state
                .material
                .map(|material| material.material.as_u32())
                .unwrap_or(mesh.mesh.as_u32());
            let flags = gpu_instance_flags(state.visibility.flags);
            Some((
                state.object,
                GpuInstanceData {
                    model: model.to_cols_array_2d(),
                    bounds: [
                        world_sphere.center.x,
                        world_sphere.center.y,
                        world_sphere.center.z,
                        world_sphere.radius,
                    ],
                    mesh_id: mesh.mesh.as_u32(),
                    material_id,
                    lod_bias: 0.0,
                    flags,
                },
            ))
        })
        .collect();

    // Phase 2 (sequential): sort by mesh then object to produce a stable order.
    let mut by_mesh: BTreeMap<u32, Vec<(GpuObjectId, GpuInstanceData)>> = BTreeMap::new();
    for (object, instance) in flat {
        by_mesh.entry(instance.mesh_id).or_default().push((object, instance));
    }
    let mut entries = Vec::with_capacity(by_mesh.values().map(|v| v.len()).sum());
    for (_, mut mesh_entries) in by_mesh {
        mesh_entries.sort_by_key(|(object, _)| *object);
        entries.append(&mut mesh_entries);
    }
    entries
}

fn gpu_scene_ranges(
    entries: &[(GpuObjectId, GpuInstanceData)],
) -> HashMap<u32, RenderWorldBatchRange> {
    let mut ranges = HashMap::new();
    for (idx, (_, instance)) in entries.iter().enumerate() {
        ranges
            .entry(instance.mesh_id)
            .and_modify(|range: &mut RenderWorldBatchRange| range.count += 1)
            .or_insert_with(|| RenderWorldBatchRange::new(idx as u32, 1));
    }
    ranges
}

fn gpu_scene_object_slots(entries: &[(GpuObjectId, GpuInstanceData)]) -> HashMap<GpuObjectId, u32> {
    entries
        .iter()
        .enumerate()
        .map(|(idx, (object, _))| (*object, idx as u32))
        .collect()
}

fn gpu_instance_flags(visibility: VisibilityFlags) -> u32 {
    let mut flags = if visibility.contains(VisibilityFlags::STATIC)
        && !visibility.contains(VisibilityFlags::DYNAMIC)
    {
        GpuInstanceData::FLAG_STATIC
    } else {
        GpuInstanceData::FLAG_DYNAMIC
    };

    if visibility.contains(VisibilityFlags::CAST_SHADOW) {
        flags |= GpuInstanceData::FLAG_CAST_SHADOW;
    }
    if visibility.contains(VisibilityFlags::RECEIVE_SHADOW) {
        flags |= GpuInstanceData::FLAG_RECEIVE_SHADOW;
    }

    flags
}
