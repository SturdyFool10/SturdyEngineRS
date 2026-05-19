use std::collections::HashMap;
use std::sync::Mutex;

use crate::ecs::{Transform, World};

use super::{
    GpuObjectAllocator, GpuObjectId, LocalToWorld, PreviousTransform, RenderBounds,
    RenderDirtyFlags, RenderExtractionStats, RenderMaterial, RenderMesh, RenderObjectState,
    RenderVisibility, RenderWorldCommand, RenderWorldCommands,
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
}

impl RenderWorld {
    pub fn new() -> Self {
        Self {
            objects: GpuObjectAllocator::new(),
            commands: RenderWorldCommands::new(),
            states: Mutex::new(HashMap::new()),
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
            .expect("render-world state mutex poisoned");

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
            .expect("render-world state mutex poisoned")
            .get(&object)
            .cloned()
    }

    /// Return a cloned snapshot of all staged object states.
    pub fn snapshot(&self) -> Vec<RenderObjectState> {
        let mut snapshot: Vec<_> = self
            .states
            .lock()
            .expect("render-world state mutex poisoned")
            .values()
            .cloned()
            .collect();
        snapshot.sort_by_key(|state| state.object);
        snapshot
    }

    /// Return and clear all dirty object states.
    pub fn take_dirty(&self) -> Vec<RenderObjectState> {
        let mut states = self
            .states
            .lock()
            .expect("render-world state mutex poisoned");
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
