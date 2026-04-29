use super::object::InstanceData;
use crate::{Buffer, BufferDesc, BufferUsage, Engine, Result};

/// Accumulated instance data for a single mesh, split into static and dynamic halves.
///
/// Buffer layout on the GPU:  `[ static_0 … static_N | dynamic_0 … dynamic_M ]`
///
/// The static half is uploaded only when its content changes. The dynamic half
/// is always re-uploaded, touching only the bytes after the static portion.
pub(super) struct InstanceBatch {
    /// Index into `Scene::meshes`.
    pub mesh_idx: u32,
    pub static_instances: Vec<InstanceData>,
    pub dynamic_instances: Vec<InstanceData>,
    pub gpu_buffer: Option<Buffer>,
    buffer_capacity: usize,
    pub static_dirty: bool,
}

impl InstanceBatch {
    pub fn new(mesh_idx: u32) -> Self {
        Self {
            mesh_idx,
            static_instances: Vec::new(),
            dynamic_instances: Vec::new(),
            gpu_buffer: None,
            buffer_capacity: 0,
            static_dirty: false,
        }
    }

    pub fn total_count(&self) -> u32 {
        (self.static_instances.len() + self.dynamic_instances.len()) as u32
    }

    /// Ensure the GPU buffer is large enough and upload dirty data.
    pub fn prepare(&mut self, engine: &Engine) -> Result<()> {
        let total = self.total_count() as usize;
        if total == 0 {
            return Ok(());
        }

        let stride = std::mem::size_of::<InstanceData>();

        if total > self.buffer_capacity || self.gpu_buffer.is_none() {
            let new_cap = total.next_power_of_two().max(4);
            self.gpu_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_cap * stride) as u64,
                usage: BufferUsage::STORAGE,
            })?);
            self.buffer_capacity = new_cap;
            self.static_dirty = true;
        }

        //panic allowed, reason = "guaranteed Some: buffer was just created or was already Some in the branch above"
        let buf = self.gpu_buffer.as_ref().unwrap();

        if self.static_dirty && !self.static_instances.is_empty() {
            buf.write(0, bytemuck::cast_slice(&self.static_instances))?;
            self.static_dirty = false;
        }

        if !self.dynamic_instances.is_empty() {
            let offset = (self.static_instances.len() * stride) as u64;
            buf.write(offset, bytemuck::cast_slice(&self.dynamic_instances))?;
        }

        Ok(())
    }
}
