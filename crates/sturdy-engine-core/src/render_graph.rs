use std::collections::{HashMap, HashSet};

use crate::{
    BindGroupHandle, BufferDesc, BufferHandle, Error, ImageDesc, ImageHandle, PipelineHandle,
    Result, ShaderHandle,
};


#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum QueueType {
    Graphics,
    Compute,
    Transfer,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Access {
    Read,
    Write,
    ReadWrite,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RgState {
    Undefined,
    ShaderRead,
    ShaderWrite,
    RenderTarget,
    DepthRead,
    DepthWrite,
    CopySrc,
    CopyDst,
    Present,
    UniformRead,
    VertexRead,
    IndexRead,
    IndirectRead,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SubresourceRange {
    pub base_mip: u16,
    pub mip_count: u16,
    pub base_layer: u16,
    pub layer_count: u16,
}

impl SubresourceRange {
    pub const WHOLE: Self = Self {
        base_mip: 0,
        mip_count: u16::MAX,
        base_layer: 0,
        layer_count: u16::MAX,
    };
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ResourceUse {
    pub image: ImageHandle,
    pub access: Access,
    pub state: RgState,
    pub subresource: SubresourceRange,
}

pub type ImageUse = ResourceUse;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BufferUse {
    pub buffer: BufferHandle,
    pub access: Access,
    pub state: RgState,
    pub offset: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PassBindings {
    pub bind_groups: Vec<BindGroupHandle>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DispatchDesc {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DrawDesc {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
    pub vertex_buffer: Option<VertexBufferBinding>,
    pub index_buffer: Option<IndexBufferBinding>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VertexBufferBinding {
    pub buffer: BufferHandle,
    pub binding: u32,
    pub offset: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IndexFormat {
    Uint16,
    Uint32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct IndexBufferBinding {
    pub buffer: BufferHandle,
    pub offset: u64,
    pub format: IndexFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CopyImageToBufferDesc {
    pub image: ImageHandle,
    pub buffer: BufferHandle,
    pub buffer_offset: u64,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum PassWork {
    #[default]
    None,
    Dispatch(DispatchDesc),
    Draw(DrawDesc),
    CopyImageToBuffer(CopyImageToBufferDesc),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassDesc {
    pub name: String,
    pub queue: QueueType,
    pub shader: Option<ShaderHandle>,
    pub pipeline: Option<PipelineHandle>,
    pub bind_groups: Vec<BindGroupHandle>,
    pub work: PassWork,
    pub reads: Vec<ImageUse>,
    pub writes: Vec<ImageUse>,
    pub buffer_reads: Vec<BufferUse>,
    pub buffer_writes: Vec<BufferUse>,
    /// Per-attachment clear colors (RGBA) applied before this pass executes.
    /// Backends that require explicit load ops (Metal MTLLoadAction, D3D12 ClearRTV)
    /// should clear the listed images. Images not listed use Load or DontCare based
    /// on the before-barrier state (Undefined → DontCare, any other → Load).
    pub clear_colors: Vec<(ImageHandle, [u32; 4])>,
    /// Clear depth (bits reinterpreted as f32) and stencil applied before this pass.
    pub clear_depth: Option<(ImageHandle, u32, u8)>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Barrier {
    pub image: ImageHandle,
    pub subresource: SubresourceRange,
    pub before: RgState,
    pub after: RgState,
    pub queue: QueueType,
}

pub type ImageBarrier = Barrier;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BufferBarrier {
    pub buffer: BufferHandle,
    pub before: RgState,
    pub after: RgState,
    pub queue: QueueType,
    pub offset: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualImage {
    pub handle: ImageHandle,
    pub desc: ImageDesc,
    pub imported: bool,
    pub first_use: u32,
    pub last_use: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualBuffer {
    pub handle: BufferHandle,
    pub desc: BufferDesc,
    pub imported: bool,
    pub first_use: u32,
    pub last_use: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordBatch {
    pub queue: QueueType,
    pub pass_indices: Vec<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AliasPlan {
    pub transient_image_count: usize,
    pub transient_buffer_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledGraph {
    pub passes: Vec<PassDesc>,
    pub images: Vec<VirtualImage>,
    pub buffers: Vec<VirtualBuffer>,
    pub barriers_per_pass: Vec<Vec<ImageBarrier>>,
    pub buffer_barriers_per_pass: Vec<Vec<BufferBarrier>>,
    pub batches: Vec<RecordBatch>,
    pub alias_plan: AliasPlan,
}

#[derive(Clone, Debug, Default)]
pub struct RenderGraph {
    image_set: HashSet<ImageHandle>,
    buffer_set: HashSet<BufferHandle>,
    images: Vec<VirtualImage>,
    buffers: Vec<VirtualBuffer>,
    passes: Vec<PassDesc>,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Import an external image so it can be used in passes.
    /// Calling this more than once for the same handle is a no-op.
    pub fn import_image(&mut self, handle: ImageHandle, desc: ImageDesc) -> Result<()> {
        if self.image_set.contains(&handle) {
            return Ok(());
        }
        desc.validate()?;
        self.image_set.insert(handle);
        self.images.push(VirtualImage {
            handle,
            desc,
            imported: true,
            first_use: u32::MAX,
            last_use: 0,
        });
        Ok(())
    }

    /// Import an external buffer so it can be used in passes.
    /// Calling this more than once for the same handle is a no-op.
    pub fn import_buffer(&mut self, handle: BufferHandle, desc: BufferDesc) -> Result<()> {
        if self.buffer_set.contains(&handle) {
            return Ok(());
        }
        desc.validate()?;
        self.buffer_set.insert(handle);
        self.buffers.push(VirtualBuffer {
            handle,
            desc,
            imported: true,
            first_use: u32::MAX,
            last_use: 0,
        });
        Ok(())
    }

    pub fn add_pass(&mut self, pass: PassDesc) -> Result<()> {
        if pass.name.trim().is_empty() {
            return Err(Error::InvalidInput("pass name must be non-empty".into()));
        }
        if let PassWork::Dispatch(dispatch) = pass.work {
            if dispatch.x == 0 || dispatch.y == 0 || dispatch.z == 0 {
                return Err(Error::InvalidInput(
                    "dispatch dimensions must be non-zero".into(),
                ));
            }
            if pass.pipeline.is_none() {
                return Err(Error::InvalidInput(
                    "dispatch pass requires a compute pipeline".into(),
                ));
            }
        }
        if let PassWork::Draw(draw) = pass.work {
            if draw.vertex_count == 0 || draw.instance_count == 0 {
                return Err(Error::InvalidInput(
                    "draw vertex_count and instance_count must be non-zero".into(),
                ));
            }
            if pass.pipeline.is_none() {
                return Err(Error::InvalidInput(
                    "draw pass requires a graphics pipeline".into(),
                ));
            }
        }
        if let PassWork::CopyImageToBuffer(copy) = pass.work {
            if copy.width == 0 || copy.height == 0 || copy.depth == 0 {
                return Err(Error::InvalidInput(
                    "copy image extent must be non-zero".into(),
                ));
            }
        }
        self.passes.push(pass);
        Ok(())
    }

    pub fn compile(&self) -> Result<CompiledGraph> {
        let order = self.topological_order()?;
        let mut images = self.images.clone();
        let mut buffers = self.buffers.clone();
        let mut image_indices = HashMap::new();
        let mut buffer_indices = HashMap::new();

        for (index, image) in images.iter_mut().enumerate() {
            image.first_use = u32::MAX;
            image.last_use = 0;
            image_indices.insert(image.handle, index);
        }
        for (index, buffer) in buffers.iter_mut().enumerate() {
            buffer.first_use = u32::MAX;
            buffer.last_use = 0;
            buffer_indices.insert(buffer.handle, index);
        }

        for (ordered_index, pass_index) in order.iter().copied().enumerate() {
            let pass = &self.passes[pass_index as usize];
            for usage in pass.reads.iter().chain(pass.writes.iter()) {
                let Some(image_index) = image_indices.get(&usage.image).copied() else {
                    return Err(Error::InvalidHandle);
                };
                let image = &mut images[image_index];
                image.first_use = image.first_use.min(ordered_index as u32);
                image.last_use = image.last_use.max(ordered_index as u32);
            }
            for usage in pass.buffer_reads.iter().chain(pass.buffer_writes.iter()) {
                let Some(buffer_index) = buffer_indices.get(&usage.buffer).copied() else {
                    return Err(Error::InvalidHandle);
                };
                let buffer = &mut buffers[buffer_index];
                buffer.first_use = buffer.first_use.min(ordered_index as u32);
                buffer.last_use = buffer.last_use.max(ordered_index as u32);
            }
        }

        let mut state_by_image: HashMap<ImageHandle, RgState> = HashMap::new();
        let mut state_by_buffer: HashMap<BufferHandle, RgState> = HashMap::new();
        let mut barriers_per_pass = Vec::with_capacity(order.len());
        let mut buffer_barriers_per_pass = Vec::with_capacity(order.len());

        for pass_index in order.iter().copied() {
            let pass = &self.passes[pass_index as usize];
            let mut pass_barriers = Vec::new();
            let mut pass_buffer_barriers = Vec::new();

            for usage in pass.reads.iter().chain(pass.writes.iter()) {
                let before = state_by_image
                    .get(&usage.image)
                    .copied()
                    .unwrap_or(RgState::Undefined);
                if before != usage.state {
                    pass_barriers.push(ImageBarrier {
                        image: usage.image,
                        subresource: usage.subresource,
                        before,
                        after: usage.state,
                        queue: pass.queue,
                    });
                    state_by_image.insert(usage.image, usage.state);
                }
            }

            for usage in pass.buffer_reads.iter().chain(pass.buffer_writes.iter()) {
                let before = state_by_buffer
                    .get(&usage.buffer)
                    .copied()
                    .unwrap_or(RgState::Undefined);
                if before != usage.state {
                    pass_buffer_barriers.push(BufferBarrier {
                        buffer: usage.buffer,
                        before,
                        after: usage.state,
                        queue: pass.queue,
                        offset: usage.offset,
                        size: usage.size,
                    });
                    state_by_buffer.insert(usage.buffer, usage.state);
                }
            }

            barriers_per_pass.push(pass_barriers);
            buffer_barriers_per_pass.push(pass_buffer_barriers);
        }

        let passes = order
            .iter()
            .map(|index| self.passes[*index as usize].clone())
            .collect::<Vec<_>>();
        let batches = build_batches(&passes);
        let alias_plan = AliasPlan {
            transient_image_count: images.iter().filter(|image| !image.imported).count(),
            transient_buffer_count: buffers.iter().filter(|buffer| !buffer.imported).count(),
        };

        Ok(CompiledGraph {
            passes,
            images,
            buffers,
            barriers_per_pass,
            buffer_barriers_per_pass,
            batches,
            alias_plan,
        })
    }

    fn topological_order(&self) -> Result<Vec<u32>> {
        let mut edges: HashMap<usize, HashSet<usize>> = HashMap::new();
        let mut last_image_writer: HashMap<ImageHandle, usize> = HashMap::new();
        let mut image_readers_since_write: HashMap<ImageHandle, Vec<usize>> = HashMap::new();
        let mut last_buffer_writer: HashMap<BufferHandle, usize> = HashMap::new();
        let mut buffer_readers_since_write: HashMap<BufferHandle, Vec<usize>> = HashMap::new();

        for (pass_index, pass) in self.passes.iter().enumerate() {
            for usage in &pass.reads {
                if let Some(writer) = last_image_writer.get(&usage.image).copied() {
                    edges.entry(writer).or_default().insert(pass_index);
                }
                image_readers_since_write
                    .entry(usage.image)
                    .or_default()
                    .push(pass_index);
            }
            for usage in &pass.buffer_reads {
                if let Some(writer) = last_buffer_writer.get(&usage.buffer).copied() {
                    edges.entry(writer).or_default().insert(pass_index);
                }
                buffer_readers_since_write
                    .entry(usage.buffer)
                    .or_default()
                    .push(pass_index);
            }

            for usage in &pass.writes {
                if let Some(writer) = last_image_writer.insert(usage.image, pass_index) {
                    edges.entry(writer).or_default().insert(pass_index);
                }
                if let Some(readers) = image_readers_since_write.remove(&usage.image) {
                    for reader in readers {
                        edges.entry(reader).or_default().insert(pass_index);
                    }
                }
            }
            for usage in &pass.buffer_writes {
                if let Some(writer) = last_buffer_writer.insert(usage.buffer, pass_index) {
                    edges.entry(writer).or_default().insert(pass_index);
                }
                if let Some(readers) = buffer_readers_since_write.remove(&usage.buffer) {
                    for reader in readers {
                        edges.entry(reader).or_default().insert(pass_index);
                    }
                }
            }
        }

        let mut indegree = vec![0usize; self.passes.len()];
        for targets in edges.values() {
            for target in targets {
                indegree[*target] += 1;
            }
        }

        let mut ready = indegree
            .iter()
            .enumerate()
            .filter_map(|(index, degree)| (*degree == 0).then_some(index))
            .collect::<Vec<_>>();
        let mut order = Vec::with_capacity(self.passes.len());

        while let Some(index) = ready.pop() {
            order.push(index as u32);
            if let Some(targets) = edges.get(&index) {
                for target in targets {
                    indegree[*target] -= 1;
                    if indegree[*target] == 0 {
                        ready.push(*target);
                    }
                }
            }
        }

        if order.len() != self.passes.len() {
            return Err(Error::InvalidInput("render graph contains a cycle".into()));
        }

        Ok(order)
    }
}

fn build_batches(passes: &[PassDesc]) -> Vec<RecordBatch> {
    let mut batches: Vec<RecordBatch> = Vec::new();
    for (index, pass) in passes.iter().enumerate() {
        match batches.last_mut() {
            Some(batch) if batch.queue == pass.queue => batch.pass_indices.push(index as u32),
            _ => batches.push(RecordBatch {
                queue: pass.queue,
                pass_indices: vec![index as u32],
            }),
        }
    }
    batches
}
