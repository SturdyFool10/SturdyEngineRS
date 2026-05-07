use super::object::InstanceData;
use crate::{Buffer, BufferDesc, BufferUsage, DrawIndexedIndirectCommand, Engine, Result};

/// Accumulated instance data for a single mesh, split into static and dynamic halves.
///
/// Buffer layout on the GPU:  `[ static_0 … static_N | dynamic_0 … dynamic_M ]`
///
/// The static half is uploaded only when its content changes. The dynamic half
/// is always re-uploaded, touching only the bytes after the static portion.
///
/// When the scene uses `GeometryBackend::ComputeIndirect`, the bounds buffer is
/// filled with pre-transformed world-space bounding spheres each frame; the GPU
/// culling compute shader (`cull_compute.slang`) writes one
/// `DrawIndexedIndirectCommand` per slot — visible instances get instance_count=1,
/// invisible ones get instance_count=0 (skipped by the GPU driver automatically).
pub(super) struct InstanceBatch {
    /// Index into `Scene::meshes`.
    pub mesh_idx: u32,
    pub static_instances:  Vec<InstanceData>,
    pub dynamic_instances: Vec<InstanceData>,
    pub gpu_buffer: Option<Buffer>,
    buffer_capacity: usize,
    pub static_dirty: bool,
    /// CPU-side indirect commands; rebuilt every frame by the CPU cull path.
    pub indirect_commands: Vec<DrawIndexedIndirectCommand>,
    /// GPU-resident indirect command buffer.
    ///
    /// For the **CPU culling path** (`ComputeIndirect` before GPU cull):
    /// sized to `indirect_commands.len()` and uploaded each frame.
    ///
    /// For the **GPU culling path** (`GeometryBackend::GpuCulled`):
    /// sized to `total_count()` — one slot per instance; the compute shader
    /// writes each slot. The CPU draws with `draw_count = total_count()`.
    pub indirect_gpu_buffer: Option<Buffer>,
    indirect_capacity: usize,

    // ── GPU culling additions ─────────────────────────────────────────────────

    /// Pre-transformed world-space bounding spheres (float4: xyz=center, w=radius).
    /// One entry per instance in the same order as `gpu_buffer`.
    /// Filled during `Scene::prepare()` when GPU culling is enabled.
    pub bounds_gpu_buffer: Option<Buffer>,
    bounds_capacity: usize,
}

impl InstanceBatch {
    pub fn new(mesh_idx: u32) -> Self {
        Self {
            mesh_idx,
            static_instances:  Vec::new(),
            dynamic_instances: Vec::new(),
            gpu_buffer: None,
            buffer_capacity: 0,
            static_dirty: false,
            indirect_commands: Vec::new(),
            indirect_gpu_buffer: None,
            indirect_capacity: 0,
            bounds_gpu_buffer: None,
            bounds_capacity: 0,
        }
    }

    pub fn total_count(&self) -> u32 {
        (self.static_instances.len() + self.dynamic_instances.len()) as u32
    }

    /// Ensure the instance GPU buffer is large enough and upload dirty data.
    pub fn prepare(&mut self, engine: &Engine) -> Result<()> {
        let total = self.total_count() as usize;
        if total == 0 { return Ok(()); }

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

        //panic allowed, reason = "guaranteed Some above"
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

    /// Upload `indirect_commands` to the GPU (CPU culling path).
    pub fn prepare_indirect(&mut self, engine: &Engine) -> Result<()> {
        let count = self.indirect_commands.len();
        if count == 0 { return Ok(()); }
        let stride = std::mem::size_of::<DrawIndexedIndirectCommand>();
        if count > self.indirect_capacity || self.indirect_gpu_buffer.is_none() {
            let new_cap = count.next_power_of_two().max(4);
            self.indirect_gpu_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_cap * stride) as u64,
                usage: BufferUsage::INDIRECT | BufferUsage::STORAGE,
            })?);
            self.indirect_capacity = new_cap;
        }
        if let Some(buf) = &self.indirect_gpu_buffer {
            buf.write(0, bytemuck::cast_slice(&self.indirect_commands))?;
        }
        Ok(())
    }

    /// Ensure the bounds GPU buffer exists and upload world-space spheres.
    ///
    /// `spheres` must have one entry per instance in instance-buffer order
    /// (static instances first, then dynamic).
    pub fn prepare_bounds(&mut self, engine: &Engine, spheres: &[[f32; 4]]) -> Result<()> {
        let count = spheres.len();
        if count == 0 { return Ok(()); }
        let stride = std::mem::size_of::<[f32; 4]>();

        if count > self.bounds_capacity || self.bounds_gpu_buffer.is_none() {
            let new_cap = count.next_power_of_two().max(4);
            self.bounds_gpu_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_cap * stride) as u64,
                usage: BufferUsage::STORAGE,
            })?);
            self.bounds_capacity = new_cap;
        }
        if let Some(buf) = &self.bounds_gpu_buffer {
            buf.write(0, bytemuck::cast_slice(spheres))?;
        }
        Ok(())
    }

    /// Ensure the indirect command buffer has `total_count` slots (GPU cull path).
    ///
    /// Called before the culling compute dispatch. The compute shader writes
    /// every slot; the CPU then draws all `total_count` commands (invisible
    /// ones have instance_count = 0 and are skipped by the GPU).
    pub fn prepare_indirect_slots(&mut self, engine: &Engine, total: u32) -> Result<()> {
        let count = total as usize;
        if count == 0 { return Ok(()); }
        let stride = std::mem::size_of::<DrawIndexedIndirectCommand>();
        if count > self.indirect_capacity || self.indirect_gpu_buffer.is_none() {
            let new_cap = count.next_power_of_two().max(4);
            self.indirect_gpu_buffer = Some(engine.create_buffer(BufferDesc {
                size: (new_cap * stride) as u64,
                usage: BufferUsage::INDIRECT | BufferUsage::STORAGE,
            })?);
            self.indirect_capacity = new_cap;
        }
        Ok(())
    }
}
