use parking_lot::Mutex;
use std::sync::Arc;

use crate::ecs::Transform;

use super::{
    GpuObjectId, PreviousTransform, RenderBounds, RenderMaterial, RenderMesh, RenderVisibility,
    RenderWorldCommand,
};

/// Thread-safe render-world command queue.
///
/// Clone this handle and send it to worker threads. Commands are applied by
/// [`RenderWorld::apply_pending`](super::RenderWorld::apply_pending), normally
/// during the render-extract phase barrier.
#[derive(Clone, Debug)]
pub struct RenderWorldCommands {
    queue: Arc<Mutex<Vec<RenderWorldCommand>>>,
}

impl RenderWorldCommands {
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn push(&self, command: RenderWorldCommand) {
        self.queue.lock().push(command);
    }

    /// Push multiple commands in a single lock acquisition — much cheaper than
    /// calling `push` N times when many commands are generated concurrently.
    pub fn push_batch(&self, commands: impl IntoIterator<Item = RenderWorldCommand>) {
        self.queue.lock().extend(commands);
    }

    pub fn create_object(&self, object: GpuObjectId) {
        self.push(RenderWorldCommand::CreateObject(object));
    }

    pub fn release_object(&self, object: GpuObjectId) {
        self.push(RenderWorldCommand::ReleaseObject(object));
    }

    pub fn set_transform(&self, object: GpuObjectId, transform: Transform) {
        self.push(RenderWorldCommand::SetTransform(object, transform));
    }

    pub fn set_previous_transform(&self, object: GpuObjectId, previous: PreviousTransform) {
        self.push(RenderWorldCommand::SetPreviousTransform(object, previous));
    }

    pub fn set_mesh(&self, object: GpuObjectId, mesh: RenderMesh) {
        self.push(RenderWorldCommand::SetMesh(object, mesh));
    }

    pub fn set_material(&self, object: GpuObjectId, material: RenderMaterial) {
        self.push(RenderWorldCommand::SetMaterial(object, material));
    }

    pub fn set_bounds(&self, object: GpuObjectId, bounds: RenderBounds) {
        self.push(RenderWorldCommand::SetBounds(object, bounds));
    }

    pub fn set_visibility(&self, object: GpuObjectId, visibility: RenderVisibility) {
        self.push(RenderWorldCommand::SetVisibility(object, visibility));
    }

    pub fn pending_count(&self) -> usize {
        self.queue.lock().len()
    }

    pub(super) fn take_all(&self) -> Vec<RenderWorldCommand> {
        let mut queue = self.queue.lock();
        std::mem::take(&mut *queue)
    }
}

impl Default for RenderWorldCommands {
    fn default() -> Self {
        Self::new()
    }
}
