use std::collections::{HashMap, HashSet};

use crate::{
    BindGroupHandle, BufferDesc, BufferHandle, Error, ImageDesc, ImageHandle, PipelineHandle,
    PushConstants, Result, ShaderHandle,
};

#[path = "render_graph/alias_plan.rs"]
mod alias_plan;

use alias_plan::build_alias_plan;
pub use alias_plan::{
    AliasCompatibilityClass, AliasPlacement, AliasPlan, AliasResourceKind, ResourceLifetime,
};

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
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

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
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

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct ImageStateKey {
    pub image: ImageHandle,
    pub subresource: SubresourceRange,
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct BufferStateKey {
    pub buffer: BufferHandle,
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
    pub mip_level: u32,
    pub base_layer: u32,
    pub layer_count: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct CopyBufferToImageDesc {
    pub buffer: BufferHandle,
    pub image: ImageHandle,
    pub buffer_offset: u64,
    pub mip_level: u32,
    pub base_layer: u32,
    pub layer_count: u32,
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
    CopyBufferToImage(CopyBufferToImageDesc),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassDesc {
    pub name: String,
    pub queue: QueueType,
    pub shader: Option<ShaderHandle>,
    pub pipeline: Option<PipelineHandle>,
    pub bind_groups: Vec<BindGroupHandle>,
    pub push_constants: Option<PushConstants>,
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
    pub before_queue: QueueType,
    pub after_queue: QueueType,
    pub queue: QueueType,
}

pub type ImageBarrier = Barrier;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BufferBarrier {
    pub buffer: BufferHandle,
    pub before: RgState,
    pub after: RgState,
    pub before_queue: QueueType,
    pub after_queue: QueueType,
    pub queue: QueueType,
    pub offset: u64,
    pub size: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ResourceState {
    state: RgState,
    queue: QueueType,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledGraph {
    pub passes: Vec<PassDesc>,
    pub images: Vec<VirtualImage>,
    pub buffers: Vec<VirtualBuffer>,
    pub barriers_per_pass: Vec<Vec<ImageBarrier>>,
    pub buffer_barriers_per_pass: Vec<Vec<BufferBarrier>>,
    pub batches: Vec<RecordBatch>,
    pub alias_plan: AliasPlan,
    pub final_image_states: Vec<(ImageStateKey, RgState)>,
    pub final_buffer_states: Vec<(BufferStateKey, RgState)>,
}

#[derive(Clone, Debug, Default)]
pub struct RenderGraph {
    image_set: HashSet<ImageHandle>,
    buffer_set: HashSet<BufferHandle>,
    images: Vec<VirtualImage>,
    buffers: Vec<VirtualBuffer>,
    passes: Vec<PassDesc>,
    initial_image_states: HashMap<ImageStateKey, RgState>,
    initial_buffer_states: HashMap<BufferStateKey, RgState>,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_initial_image_state(&mut self, handle: ImageHandle, state: RgState) {
        self.set_initial_image_subresource_state(handle, SubresourceRange::WHOLE, state);
    }

    pub fn set_initial_image_subresource_state(
        &mut self,
        handle: ImageHandle,
        subresource: SubresourceRange,
        state: RgState,
    ) {
        self.initial_image_states.insert(
            ImageStateKey {
                image: handle,
                subresource,
            },
            state,
        );
    }

    pub fn set_initial_buffer_state(&mut self, handle: BufferHandle, state: RgState) {
        self.set_initial_buffer_range_state(handle, 0, u64::MAX, state);
    }

    pub fn set_initial_buffer_range_state(
        &mut self,
        handle: BufferHandle,
        offset: u64,
        size: u64,
        state: RgState,
    ) {
        self.initial_buffer_states.insert(
            BufferStateKey {
                buffer: handle,
                offset,
                size,
            },
            state,
        );
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
        if let Some(push_constants) = &pass.push_constants {
            push_constants.validate()?;
            if pass.pipeline.is_none() {
                return Err(Error::InvalidInput(
                    "push constants require a pass pipeline".into(),
                ));
            }
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
            self.validate_copy_image_to_buffer(copy)?;
        }
        if let PassWork::CopyBufferToImage(copy) = pass.work {
            self.validate_copy_buffer_to_image(copy)?;
        }
        self.passes.push(pass);
        Ok(())
    }

    fn validate_copy_image_to_buffer(&self, copy: CopyImageToBufferDesc) -> Result<()> {
        validate_copy_extent(
            copy.width,
            copy.height,
            copy.depth,
            copy.layer_count,
            copy.mip_level,
            copy.base_layer,
        )?;
        let image = self
            .images
            .iter()
            .find(|image| image.handle == copy.image)
            .ok_or(Error::InvalidHandle)?;
        let buffer = self
            .buffers
            .iter()
            .find(|buffer| buffer.handle == copy.buffer)
            .ok_or(Error::InvalidHandle)?;
        validate_copy_fits_image(
            image.desc,
            copy.mip_level,
            copy.base_layer,
            copy.layer_count,
            copy.width,
            copy.height,
            copy.depth,
        )?;
        validate_copy_fits_buffer(
            image.desc,
            buffer.desc,
            copy.buffer_offset,
            copy.width,
            copy.height,
            copy.depth,
            copy.layer_count,
        )
    }

    fn validate_copy_buffer_to_image(&self, copy: CopyBufferToImageDesc) -> Result<()> {
        validate_copy_extent(
            copy.width,
            copy.height,
            copy.depth,
            copy.layer_count,
            copy.mip_level,
            copy.base_layer,
        )?;
        let image = self
            .images
            .iter()
            .find(|image| image.handle == copy.image)
            .ok_or(Error::InvalidHandle)?;
        let buffer = self
            .buffers
            .iter()
            .find(|buffer| buffer.handle == copy.buffer)
            .ok_or(Error::InvalidHandle)?;
        validate_copy_fits_image(
            image.desc,
            copy.mip_level,
            copy.base_layer,
            copy.layer_count,
            copy.width,
            copy.height,
            copy.depth,
        )?;
        validate_copy_fits_buffer(
            image.desc,
            buffer.desc,
            copy.buffer_offset,
            copy.width,
            copy.height,
            copy.depth,
            copy.layer_count,
        )
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

        let mut state_by_image = self
            .initial_image_states
            .iter()
            .map(|(key, state)| {
                (
                    *key,
                    ResourceState {
                        state: *state,
                        queue: QueueType::Graphics,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let mut state_by_buffer = self
            .initial_buffer_states
            .iter()
            .map(|(key, state)| {
                (
                    *key,
                    ResourceState {
                        state: *state,
                        queue: QueueType::Graphics,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let mut barriers_per_pass = Vec::with_capacity(order.len());
        let mut buffer_barriers_per_pass = Vec::with_capacity(order.len());

        for pass_index in order.iter().copied() {
            let pass = &self.passes[pass_index as usize];
            let mut pass_barriers = Vec::new();
            let mut pass_buffer_barriers = Vec::new();

            for usage in pass.reads.iter().chain(pass.writes.iter()) {
                let key = ImageStateKey {
                    image: usage.image,
                    subresource: usage.subresource,
                };
                let before = state_by_image.get(&key).copied().unwrap_or_else(|| {
                    state_by_image
                        .get(&ImageStateKey {
                            image: usage.image,
                            subresource: SubresourceRange::WHOLE,
                        })
                        .copied()
                        .unwrap_or(ResourceState {
                            state: RgState::Undefined,
                            queue: pass.queue,
                        })
                });
                if before.state != usage.state || before.queue != pass.queue {
                    pass_barriers.push(ImageBarrier {
                        image: usage.image,
                        subresource: usage.subresource,
                        before: before.state,
                        after: usage.state,
                        before_queue: before.queue,
                        after_queue: pass.queue,
                        queue: pass.queue,
                    });
                }
                state_by_image.insert(
                    key,
                    ResourceState {
                        state: usage.state,
                        queue: pass.queue,
                    },
                );
            }

            for usage in pass.buffer_reads.iter().chain(pass.buffer_writes.iter()) {
                let key = BufferStateKey {
                    buffer: usage.buffer,
                    offset: usage.offset,
                    size: usage.size,
                };
                let before = state_by_buffer.get(&key).copied().unwrap_or_else(|| {
                    state_by_buffer
                        .get(&BufferStateKey {
                            buffer: usage.buffer,
                            offset: 0,
                            size: u64::MAX,
                        })
                        .copied()
                        .unwrap_or(ResourceState {
                            state: RgState::Undefined,
                            queue: pass.queue,
                        })
                });
                if before.state != usage.state || before.queue != pass.queue {
                    pass_buffer_barriers.push(BufferBarrier {
                        buffer: usage.buffer,
                        before: before.state,
                        after: usage.state,
                        before_queue: before.queue,
                        after_queue: pass.queue,
                        queue: pass.queue,
                        offset: usage.offset,
                        size: usage.size,
                    });
                }
                state_by_buffer.insert(
                    key,
                    ResourceState {
                        state: usage.state,
                        queue: pass.queue,
                    },
                );
            }

            barriers_per_pass.push(pass_barriers);
            buffer_barriers_per_pass.push(pass_buffer_barriers);
        }

        let passes = order
            .iter()
            .map(|index| self.passes[*index as usize].clone())
            .collect::<Vec<_>>();
        let batches = build_batches(&passes);
        let alias_plan = build_alias_plan(&images, &buffers);
        let final_image_states = state_by_image
            .into_iter()
            .filter(|(key, _)| self.image_set.contains(&key.image))
            .map(|(key, state)| (key, state.state))
            .collect();
        let final_buffer_states = state_by_buffer
            .into_iter()
            .filter(|(key, _)| self.buffer_set.contains(&key.buffer))
            .map(|(key, state)| (key, state.state))
            .collect();

        Ok(CompiledGraph {
            passes,
            images,
            buffers,
            barriers_per_pass,
            buffer_barriers_per_pass,
            batches,
            alias_plan,
            final_image_states,
            final_buffer_states,
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
                    if writer != pass_index {
                        edges.entry(writer).or_default().insert(pass_index);
                    }
                }
                if let Some(readers) = image_readers_since_write.remove(&usage.image) {
                    for reader in readers {
                        if reader != pass_index {
                            edges.entry(reader).or_default().insert(pass_index);
                        }
                    }
                }
            }
            for usage in &pass.buffer_writes {
                if let Some(writer) = last_buffer_writer.insert(usage.buffer, pass_index) {
                    if writer != pass_index {
                        edges.entry(writer).or_default().insert(pass_index);
                    }
                }
                if let Some(readers) = buffer_readers_since_write.remove(&usage.buffer) {
                    for reader in readers {
                        if reader != pass_index {
                            edges.entry(reader).or_default().insert(pass_index);
                        }
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

fn validate_copy_extent(
    width: u32,
    height: u32,
    depth: u32,
    layer_count: u32,
    _mip_level: u32,
    _base_layer: u32,
) -> Result<()> {
    if width == 0 || height == 0 || depth == 0 {
        return Err(Error::InvalidInput(
            "copy image extent must be non-zero".into(),
        ));
    }
    if layer_count == 0 {
        return Err(Error::InvalidInput(
            "copy image layer_count must be non-zero".into(),
        ));
    }
    Ok(())
}

fn validate_copy_fits_image(
    desc: ImageDesc,
    mip_level: u32,
    base_layer: u32,
    layer_count: u32,
    width: u32,
    height: u32,
    depth: u32,
) -> Result<()> {
    if mip_level >= desc.mip_levels as u32 {
        return Err(Error::InvalidInput(format!(
            "copy mip_level {mip_level} exceeds image mip_levels {}",
            desc.mip_levels
        )));
    }
    let end_layer = base_layer
        .checked_add(layer_count)
        .ok_or_else(|| Error::InvalidInput("copy layer range overflowed".into()))?;
    if end_layer > desc.layers as u32 {
        return Err(Error::InvalidInput(format!(
            "copy layer range [{base_layer}, {end_layer}) exceeds image layers {}",
            desc.layers
        )));
    }
    let mip_width = mip_extent(desc.extent.width, mip_level);
    let mip_height = mip_extent(desc.extent.height, mip_level);
    let mip_depth = mip_extent(desc.extent.depth, mip_level);
    if width > mip_width || height > mip_height || depth > mip_depth {
        return Err(Error::InvalidInput(format!(
            "copy extent {}x{}x{} exceeds mip extent {}x{}x{}",
            width, height, depth, mip_width, mip_height, mip_depth
        )));
    }
    Ok(())
}

fn validate_copy_fits_buffer(
    image_desc: ImageDesc,
    buffer_desc: BufferDesc,
    buffer_offset: u64,
    width: u32,
    height: u32,
    depth: u32,
    layer_count: u32,
) -> Result<()> {
    let byte_count = copy_byte_count(image_desc, width, height, depth, layer_count)?;
    let end = buffer_offset
        .checked_add(byte_count)
        .ok_or_else(|| Error::InvalidInput("copy buffer range overflowed".into()))?;
    if end > buffer_desc.size {
        return Err(Error::InvalidInput(format!(
            "copy buffer range [{buffer_offset}, {end}) exceeds buffer size {}",
            buffer_desc.size
        )));
    }
    Ok(())
}

fn copy_byte_count(
    desc: ImageDesc,
    width: u32,
    height: u32,
    depth: u32,
    layer_count: u32,
) -> Result<u64> {
    let texel_size = format_texel_size(desc)?;
    [
        width as u64,
        height as u64,
        depth as u64,
        layer_count as u64,
        texel_size,
    ]
    .into_iter()
    .try_fold(1u64, |acc, value| {
        acc.checked_mul(value)
            .ok_or_else(|| Error::InvalidInput("copy byte count overflowed".into()))
    })
}

fn format_texel_size(desc: ImageDesc) -> Result<u64> {
    let size = match desc.format {
        crate::Format::Unknown => {
            return Err(Error::InvalidInput(
                "copy image format must be specified".into(),
            ));
        }
        crate::Format::Rgba8Unorm => 4,
        crate::Format::Bgra8Unorm => 4,
        crate::Format::Rgba16Float => 8,
        crate::Format::Rgba32Float => 16,
        crate::Format::Depth32Float => 4,
        crate::Format::Depth24Stencil8 => 4,
    };
    Ok(size)
}

fn mip_extent(base: u32, mip_level: u32) -> u32 {
    (base >> mip_level).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BufferUsage, Extent3d, Format, ImageUsage};

    fn image_desc() -> ImageDesc {
        ImageDesc {
            extent: Extent3d {
                width: 4,
                height: 4,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED | ImageUsage::COPY_DST | ImageUsage::COPY_SRC,
        }
    }

    fn buffer_desc(size: u64) -> BufferDesc {
        BufferDesc {
            size,
            usage: BufferUsage::COPY_SRC | BufferUsage::COPY_DST,
        }
    }

    fn transient_image_desc() -> ImageDesc {
        ImageDesc {
            extent: Extent3d {
                width: 64,
                height: 64,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba16Float,
            usage: ImageUsage::SAMPLED
                | ImageUsage::STORAGE
                | ImageUsage::RENDER_TARGET
                | ImageUsage::COPY_DST,
        }
    }

    fn register_transient_image(graph: &mut RenderGraph, handle: ImageHandle, desc: ImageDesc) {
        graph.image_set.insert(handle);
        graph.images.push(VirtualImage {
            handle,
            desc,
            imported: false,
            first_use: u32::MAX,
            last_use: 0,
        });
    }

    fn register_transient_buffer(graph: &mut RenderGraph, handle: BufferHandle, desc: BufferDesc) {
        graph.buffer_set.insert(handle);
        graph.buffers.push(VirtualBuffer {
            handle,
            desc,
            imported: false,
            first_use: u32::MAX,
            last_use: 0,
        });
    }

    #[test]
    fn copy_buffer_to_image_pass_compiles_barriers() {
        let image = ImageHandle(1);
        let buffer = BufferHandle(2);
        let mut graph = RenderGraph::new();
        graph.import_image(image, image_desc()).unwrap();
        graph.import_buffer(buffer, buffer_desc(64)).unwrap();

        graph
            .add_pass(PassDesc {
                name: "upload-texture".into(),
                queue: QueueType::Transfer,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                    buffer,
                    image,
                    buffer_offset: 0,
                    mip_level: 0,
                    base_layer: 0,
                    layer_count: 1,
                    width: 4,
                    height: 4,
                    depth: 1,
                }),
                reads: Vec::new(),
                writes: vec![ImageUse {
                    image,
                    access: Access::Write,
                    state: RgState::CopyDst,
                    subresource: SubresourceRange {
                        base_mip: 0,
                        mip_count: 1,
                        base_layer: 0,
                        layer_count: 1,
                    },
                }],
                buffer_reads: vec![BufferUse {
                    buffer,
                    access: Access::Read,
                    state: RgState::CopySrc,
                    offset: 0,
                    size: 64,
                }],
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        let compiled = graph.compile().unwrap();
        assert_eq!(compiled.passes.len(), 1);
        assert_eq!(compiled.barriers_per_pass[0].len(), 1);
        assert_eq!(compiled.barriers_per_pass[0][0].after, RgState::CopyDst);
        assert_eq!(compiled.buffer_barriers_per_pass[0].len(), 1);
        assert_eq!(
            compiled.buffer_barriers_per_pass[0][0].after,
            RgState::CopySrc
        );
    }

    #[test]
    fn copy_buffer_to_image_rejects_short_buffer() {
        let image = ImageHandle(1);
        let buffer = BufferHandle(2);
        let mut graph = RenderGraph::new();
        graph.import_image(image, image_desc()).unwrap();
        graph.import_buffer(buffer, buffer_desc(63)).unwrap();

        let err = graph
            .add_pass(PassDesc {
                name: "upload-texture".into(),
                queue: QueueType::Transfer,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                    buffer,
                    image,
                    buffer_offset: 0,
                    mip_level: 0,
                    base_layer: 0,
                    layer_count: 1,
                    width: 4,
                    height: 4,
                    depth: 1,
                }),
                reads: Vec::new(),
                writes: Vec::new(),
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn push_constants_require_aligned_byte_ranges() {
        let mut graph = RenderGraph::new();
        let err = graph
            .add_pass(PassDesc {
                name: "draw-with-bad-push".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: Some(PipelineHandle(1)),
                bind_groups: Vec::new(),
                push_constants: Some(PushConstants {
                    offset: 2,
                    stages: crate::StageMask::VERTEX,
                    bytes: vec![0, 1, 2, 3],
                }),
                work: PassWork::Draw(DrawDesc {
                    vertex_count: 3,
                    instance_count: 1,
                    first_vertex: 0,
                    first_instance: 0,
                    vertex_buffer: None,
                    index_buffer: None,
                }),
                reads: Vec::new(),
                writes: Vec::new(),
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn push_constants_require_pipeline() {
        let mut graph = RenderGraph::new();
        let err = graph
            .add_pass(PassDesc {
                name: "push-without-pipeline".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: Some(PushConstants {
                    offset: 0,
                    stages: crate::StageMask::VERTEX,
                    bytes: vec![0, 1, 2, 3],
                }),
                work: PassWork::None,
                reads: Vec::new(),
                writes: Vec::new(),
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn imported_image_uses_initial_state_for_first_barrier() {
        let image = ImageHandle(1);
        let mut graph = RenderGraph::new();
        graph.import_image(image, image_desc()).unwrap();
        graph.set_initial_image_state(image, RgState::ShaderRead);
        graph
            .add_pass(PassDesc {
                name: "read-texture".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads: vec![ImageUse {
                    image,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    subresource: SubresourceRange {
                        base_mip: 0,
                        mip_count: 1,
                        base_layer: 0,
                        layer_count: 1,
                    },
                }],
                writes: Vec::new(),
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        let compiled = graph.compile().unwrap();
        assert!(compiled.barriers_per_pass[0].is_empty());
        assert!(compiled.final_image_states.contains(&(
            ImageStateKey {
                image,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            },
            RgState::ShaderRead
        )));
    }

    #[test]
    fn imported_image_tracks_distinct_subresource_states() {
        let image = ImageHandle(1);
        let mut graph = RenderGraph::new();
        let desc = ImageDesc {
            mip_levels: 2,
            ..image_desc()
        };
        graph.import_image(image, desc).unwrap();
        graph.set_initial_image_subresource_state(
            image,
            SubresourceRange {
                base_mip: 0,
                mip_count: 1,
                base_layer: 0,
                layer_count: 1,
            },
            RgState::ShaderRead,
        );
        graph.set_initial_image_subresource_state(
            image,
            SubresourceRange {
                base_mip: 1,
                mip_count: 1,
                base_layer: 0,
                layer_count: 1,
            },
            RgState::CopyDst,
        );
        graph
            .add_pass(PassDesc {
                name: "use-mips".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads: vec![ImageUse {
                    image,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    subresource: SubresourceRange {
                        base_mip: 0,
                        mip_count: 1,
                        base_layer: 0,
                        layer_count: 1,
                    },
                }],
                writes: vec![ImageUse {
                    image,
                    access: Access::Write,
                    state: RgState::CopyDst,
                    subresource: SubresourceRange {
                        base_mip: 1,
                        mip_count: 1,
                        base_layer: 0,
                        layer_count: 1,
                    },
                }],
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        let compiled = graph.compile().unwrap();
        assert!(compiled.barriers_per_pass[0].is_empty());
        assert_eq!(compiled.final_image_states.len(), 2);
    }

    #[test]
    fn queue_changes_emit_ownership_barriers() {
        let image = ImageHandle(1);
        let buffer = BufferHandle(2);
        let mut graph = RenderGraph::new();
        graph.import_image(image, image_desc()).unwrap();
        graph.import_buffer(buffer, buffer_desc(64)).unwrap();

        graph
            .add_pass(PassDesc {
                name: "upload".into(),
                queue: QueueType::Transfer,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads: Vec::new(),
                writes: vec![ImageUse {
                    image,
                    access: Access::Write,
                    state: RgState::CopyDst,
                    subresource: SubresourceRange::WHOLE,
                }],
                buffer_reads: Vec::new(),
                buffer_writes: vec![BufferUse {
                    buffer,
                    access: Access::Write,
                    state: RgState::CopyDst,
                    offset: 0,
                    size: 64,
                }],
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();
        graph
            .add_pass(PassDesc {
                name: "sample".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads: vec![ImageUse {
                    image,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    subresource: SubresourceRange::WHOLE,
                }],
                writes: Vec::new(),
                buffer_reads: vec![BufferUse {
                    buffer,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    offset: 0,
                    size: 64,
                }],
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        let compiled = graph.compile().unwrap();
        let image_barrier = compiled.barriers_per_pass[1][0];
        assert_eq!(image_barrier.before_queue, QueueType::Transfer);
        assert_eq!(image_barrier.after_queue, QueueType::Graphics);
        assert_eq!(image_barrier.before, RgState::CopyDst);
        assert_eq!(image_barrier.after, RgState::ShaderRead);

        let buffer_barrier = compiled.buffer_barriers_per_pass[1][0];
        assert_eq!(buffer_barrier.before_queue, QueueType::Transfer);
        assert_eq!(buffer_barrier.after_queue, QueueType::Graphics);
        assert_eq!(buffer_barrier.before, RgState::CopyDst);
        assert_eq!(buffer_barrier.after, RgState::ShaderRead);
    }

    #[test]
    fn independent_queue_batches_compile_without_cross_batch_barriers() {
        let compute_buffer = BufferHandle(1);
        let transfer_image = ImageHandle(2);
        let transfer_buffer = BufferHandle(3);
        let graphics_image = ImageHandle(4);
        let mut graph = RenderGraph::new();
        graph
            .import_buffer(compute_buffer, buffer_desc(64))
            .unwrap();
        graph.import_image(transfer_image, image_desc()).unwrap();
        graph
            .import_buffer(transfer_buffer, buffer_desc(64))
            .unwrap();
        graph.import_image(graphics_image, image_desc()).unwrap();

        graph
            .add_pass(PassDesc {
                name: "compute-independent".into(),
                queue: QueueType::Compute,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads: Vec::new(),
                writes: Vec::new(),
                buffer_reads: Vec::new(),
                buffer_writes: vec![BufferUse {
                    buffer: compute_buffer,
                    access: Access::Write,
                    state: RgState::ShaderWrite,
                    offset: 0,
                    size: 64,
                }],
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();
        graph
            .add_pass(PassDesc {
                name: "transfer-independent".into(),
                queue: QueueType::Transfer,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads: Vec::new(),
                writes: vec![ImageUse {
                    image: transfer_image,
                    access: Access::Write,
                    state: RgState::CopyDst,
                    subresource: SubresourceRange::WHOLE,
                }],
                buffer_reads: vec![BufferUse {
                    buffer: transfer_buffer,
                    access: Access::Read,
                    state: RgState::CopySrc,
                    offset: 0,
                    size: 64,
                }],
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();
        graph
            .add_pass(PassDesc {
                name: "graphics-independent".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads: Vec::new(),
                writes: vec![ImageUse {
                    image: graphics_image,
                    access: Access::Write,
                    state: RgState::RenderTarget,
                    subresource: SubresourceRange::WHOLE,
                }],
                buffer_reads: Vec::new(),
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        let compiled = graph.compile().unwrap();
        assert_eq!(
            compiled.batches,
            vec![
                RecordBatch {
                    queue: QueueType::Graphics,
                    pass_indices: vec![0],
                },
                RecordBatch {
                    queue: QueueType::Transfer,
                    pass_indices: vec![1],
                },
                RecordBatch {
                    queue: QueueType::Compute,
                    pass_indices: vec![2],
                },
            ]
        );
        for (pass, barriers) in compiled.passes.iter().zip(&compiled.barriers_per_pass) {
            for barrier in barriers {
                assert_eq!(barrier.before_queue, pass.queue);
                assert_eq!(barrier.after_queue, pass.queue);
            }
        }
        for (pass, barriers) in compiled
            .passes
            .iter()
            .zip(&compiled.buffer_barriers_per_pass)
        {
            for barrier in barriers {
                assert_eq!(barrier.before_queue, pass.queue);
                assert_eq!(barrier.after_queue, pass.queue);
            }
        }
    }

    #[test]
    fn showcase_upload_push_constants_multi_queue_and_aliasing_plan() {
        let staging = BufferHandle(1);
        let uploaded = ImageHandle(10);
        let gbuffer = ImageHandle(11);
        let lighting = ImageHandle(12);
        let postprocess = ImageHandle(13);
        let scratch_a = BufferHandle(20);
        let scratch_b = BufferHandle(21);

        let image_desc = transient_image_desc();
        let scratch_desc = BufferDesc {
            size: 4096,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        };
        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };
        let mut graph = RenderGraph::new();
        graph
            .import_buffer(staging, buffer_desc(64 * 64 * 8))
            .unwrap();
        for image in [uploaded, gbuffer, lighting, postprocess] {
            register_transient_image(&mut graph, image, image_desc);
        }
        for buffer in [scratch_a, scratch_b] {
            register_transient_buffer(&mut graph, buffer, scratch_desc);
        }

        graph
            .add_pass(PassDesc {
                name: "upload-material-texture".into(),
                queue: QueueType::Transfer,
                shader: None,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                    buffer: staging,
                    image: uploaded,
                    buffer_offset: 0,
                    mip_level: 0,
                    base_layer: 0,
                    layer_count: 1,
                    width: 64,
                    height: 64,
                    depth: 1,
                }),
                reads: Vec::new(),
                writes: vec![ImageUse {
                    image: uploaded,
                    access: Access::Write,
                    state: RgState::CopyDst,
                    subresource,
                }],
                buffer_reads: vec![BufferUse {
                    buffer: staging,
                    access: Access::Read,
                    state: RgState::CopySrc,
                    offset: 0,
                    size: 64 * 64 * 8,
                }],
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        graph
            .add_pass(PassDesc {
                name: "gbuffer-draw-with-push-constants".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: Some(PipelineHandle(1)),
                bind_groups: Vec::new(),
                push_constants: Some(PushConstants {
                    offset: 0,
                    stages: crate::StageMask::VERTEX | crate::StageMask::FRAGMENT,
                    bytes: vec![0x11; 16],
                }),
                work: PassWork::Draw(DrawDesc {
                    vertex_count: 3,
                    instance_count: 2,
                    first_vertex: 0,
                    first_instance: 0,
                    vertex_buffer: None,
                    index_buffer: None,
                }),
                reads: vec![ImageUse {
                    image: uploaded,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    subresource,
                }],
                writes: vec![ImageUse {
                    image: gbuffer,
                    access: Access::Write,
                    state: RgState::RenderTarget,
                    subresource,
                }],
                buffer_reads: Vec::new(),
                buffer_writes: vec![BufferUse {
                    buffer: scratch_a,
                    access: Access::Write,
                    state: RgState::ShaderWrite,
                    offset: 0,
                    size: scratch_desc.size,
                }],
                clear_colors: vec![(gbuffer, [0, 0, 0, f32::to_bits(1.0)])],
                clear_depth: None,
            })
            .unwrap();

        graph
            .add_pass(PassDesc {
                name: "compute-lighting".into(),
                queue: QueueType::Compute,
                shader: None,
                pipeline: Some(PipelineHandle(2)),
                bind_groups: Vec::new(),
                push_constants: Some(PushConstants {
                    offset: 16,
                    stages: crate::StageMask::COMPUTE,
                    bytes: vec![0x22; 16],
                }),
                work: PassWork::Dispatch(DispatchDesc { x: 8, y: 8, z: 1 }),
                reads: vec![ImageUse {
                    image: gbuffer,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    subresource,
                }],
                writes: vec![ImageUse {
                    image: lighting,
                    access: Access::Write,
                    state: RgState::ShaderWrite,
                    subresource,
                }],
                buffer_reads: vec![BufferUse {
                    buffer: scratch_a,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    offset: 0,
                    size: scratch_desc.size,
                }],
                buffer_writes: Vec::new(),
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        graph
            .add_pass(PassDesc {
                name: "postprocess-and-presentable-output".into(),
                queue: QueueType::Graphics,
                shader: None,
                pipeline: Some(PipelineHandle(3)),
                bind_groups: Vec::new(),
                push_constants: Some(PushConstants {
                    offset: 0,
                    stages: crate::StageMask::FRAGMENT,
                    bytes: vec![0x33; 16],
                }),
                work: PassWork::Draw(DrawDesc {
                    vertex_count: 3,
                    instance_count: 1,
                    first_vertex: 0,
                    first_instance: 0,
                    vertex_buffer: None,
                    index_buffer: None,
                }),
                reads: vec![ImageUse {
                    image: lighting,
                    access: Access::Read,
                    state: RgState::ShaderRead,
                    subresource,
                }],
                writes: vec![ImageUse {
                    image: postprocess,
                    access: Access::Write,
                    state: RgState::RenderTarget,
                    subresource,
                }],
                buffer_reads: Vec::new(),
                buffer_writes: vec![BufferUse {
                    buffer: scratch_b,
                    access: Access::Write,
                    state: RgState::ShaderWrite,
                    offset: 0,
                    size: scratch_desc.size,
                }],
                clear_colors: Vec::new(),
                clear_depth: None,
            })
            .unwrap();

        let compiled = graph.compile().unwrap();

        assert_eq!(compiled.passes.len(), 4);
        assert_eq!(
            compiled.batches,
            vec![
                RecordBatch {
                    queue: QueueType::Transfer,
                    pass_indices: vec![0],
                },
                RecordBatch {
                    queue: QueueType::Graphics,
                    pass_indices: vec![1],
                },
                RecordBatch {
                    queue: QueueType::Compute,
                    pass_indices: vec![2],
                },
                RecordBatch {
                    queue: QueueType::Graphics,
                    pass_indices: vec![3],
                },
            ]
        );

        assert_eq!(
            compiled.passes[1].push_constants.as_ref().unwrap().stages,
            crate::StageMask::VERTEX | crate::StageMask::FRAGMENT
        );
        assert_eq!(
            compiled.passes[2].push_constants.as_ref().unwrap().offset,
            16
        );

        let upload_to_draw = compiled.barriers_per_pass[1]
            .iter()
            .find(|barrier| barrier.image == uploaded)
            .expect("uploaded image transitions from transfer upload to graphics sampling");
        assert_eq!(upload_to_draw.before_queue, QueueType::Transfer);
        assert_eq!(upload_to_draw.after_queue, QueueType::Graphics);
        assert_eq!(upload_to_draw.before, RgState::CopyDst);
        assert_eq!(upload_to_draw.after, RgState::ShaderRead);

        let draw_to_compute = compiled.barriers_per_pass[2]
            .iter()
            .find(|barrier| barrier.image == gbuffer)
            .expect("gbuffer transitions from graphics render target to compute input");
        assert_eq!(draw_to_compute.before_queue, QueueType::Graphics);
        assert_eq!(draw_to_compute.after_queue, QueueType::Compute);
        assert_eq!(draw_to_compute.before, RgState::RenderTarget);
        assert_eq!(draw_to_compute.after, RgState::ShaderRead);

        let compute_to_post = compiled.barriers_per_pass[3]
            .iter()
            .find(|barrier| barrier.image == lighting)
            .expect("lighting image transitions from compute output to graphics sampling");
        assert_eq!(compute_to_post.before_queue, QueueType::Compute);
        assert_eq!(compute_to_post.after_queue, QueueType::Graphics);
        assert_eq!(compute_to_post.before, RgState::ShaderWrite);
        assert_eq!(compute_to_post.after, RgState::ShaderRead);

        assert_eq!(compiled.alias_plan.transient_image_count, 4);
        assert_eq!(compiled.alias_plan.transient_buffer_count, 2);
        assert!(compiled.alias_plan.image_slot_count < 4);
        assert_eq!(compiled.alias_plan.buffer_slot_count, 1);
        assert!(compiled.alias_plan.image_savings_bytes > 0);
        assert_eq!(compiled.alias_plan.buffer_savings_bytes, scratch_desc.size);
        assert!(compiled.alias_plan.total_savings_bytes() > scratch_desc.size);
    }
}
