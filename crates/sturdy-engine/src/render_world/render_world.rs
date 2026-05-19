use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use glam::Mat4;

use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, GpuInstanceData, Result,
    ecs::{Transform, World},
};

use super::{
    GpuObjectAllocator, GpuObjectId, LocalToWorld, PreviousTransform, RenderBounds,
    RenderDirtyFlags, RenderExtractionStats, RenderMaterial, RenderMesh, RenderObjectState,
    RenderVisibility, RenderWorldBatchRange, RenderWorldCommand, RenderWorldCommands,
    RenderWorldGpuSceneData, RenderWorldGpuSceneState, RenderWorldGpuSceneStats, VisibilityFlags,
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

        let mut states = self
            .states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

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
        let allocated_objects = self.allocate_missing_objects(world);
        let linked: Vec<_> = world
            .query::<LocalToWorld>()
            .map(|(entity, link)| (entity, link.object))
            .collect();

        for (entity, object) in &linked {
            if let Some(transform) = world.get::<Transform>(*entity) {
                self.set_transform(*object, transform.clone());
            }
            if let Some(previous) = world.get::<PreviousTransform>(*entity) {
                self.set_previous_transform(*object, previous.clone());
            }
            if let Some(mesh) = world.get::<RenderMesh>(*entity) {
                self.set_mesh(*object, *mesh);
            }
            if let Some(material) = world.get::<RenderMaterial>(*entity) {
                self.set_material(*object, *material);
            }
            if let Some(bounds) = world.get::<RenderBounds>(*entity) {
                self.set_bounds(*object, *bounds);
            }
            if let Some(visibility) = world.get::<RenderVisibility>(*entity) {
                self.set_visibility(*object, *visibility);
            }
        }

        RenderExtractionStats {
            allocated_objects,
            extracted_entities: linked.len(),
            applied_commands: self.apply_pending(),
        }
    }

    /// Return a cloned object state snapshot for diagnostics/tests.
    pub fn object(&self, object: GpuObjectId) -> Option<RenderObjectState> {
        self.states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&object)
            .cloned()
    }

    /// Return a cloned snapshot of all staged object states.
    pub fn snapshot(&self) -> Vec<RenderObjectState> {
        let mut snapshot: Vec<_> = self
            .states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect();
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

        let mut states = self
            .states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot: Vec<_> = states.values().cloned().collect();
        let entries = gpu_scene_entries_from_states(&snapshot, valid_mesh_count);
        let instances: Vec<_> = entries.iter().map(|(_, instance)| *instance).collect();
        let ranges = gpu_scene_ranges(&entries);
        let object_slots = gpu_scene_object_slots(&entries);
        let instance_count = instances.len();
        let batch_count = ranges.len();
        let stride = std::mem::size_of::<GpuInstanceData>();

        let mut gpu_scene = self
            .gpu_scene
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let structural_dirty = states.values().any(|state| {
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
        self.gpu_scene
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .ranges
            .get(&mesh_id)
            .copied()
    }

    pub fn with_gpu_scene_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        let gpu_scene = self
            .gpu_scene
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(gpu_scene.buffer.as_ref())
    }

    pub fn with_gpu_indirect_buffer<R>(&self, f: impl FnOnce(Option<&Buffer>) -> R) -> R {
        let gpu_scene = self
            .gpu_scene
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(gpu_scene.indirect_buffer.as_ref())
    }

    /// Return and clear all dirty object states.
    pub fn take_dirty(&self) -> Vec<RenderObjectState> {
        let mut states = self
            .states
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        self.states
            .lock()
            .map(|states| states.len())
            .unwrap_or_default()
    }

    pub fn pending_command_count(&self) -> usize {
        self.commands.pending_count()
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

fn gpu_scene_entries_from_states(
    states: &[RenderObjectState],
    valid_mesh_count: usize,
) -> Vec<(GpuObjectId, GpuInstanceData)> {
    let mut by_mesh: BTreeMap<u32, Vec<(GpuObjectId, GpuInstanceData)>> = BTreeMap::new();

    for state in states {
        if !state.visibility.flags.contains(VisibilityFlags::VISIBLE) {
            continue;
        }
        let Some(mesh) = state.mesh else {
            continue;
        };
        if mesh.mesh.index() >= valid_mesh_count {
            continue;
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

        by_mesh.entry(mesh.mesh.as_u32()).or_default().push((
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
        ));
    }

    let mut entries = Vec::new();
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
