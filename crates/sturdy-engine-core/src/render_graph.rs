use std::collections::{HashMap, HashSet};

use crate::shader_object::ShaderObjectHandle;
use crate::{
    AccelerationStructureHandle, BindGroupHandle, BufferDesc, BufferHandle, DecodeFrameDesc,
    DgcExecuteDesc, DgcPreprocessDesc, EncodeFrameDesc, Error, ImageDesc, ImageHandle,
    OpticalFlowEstimateDesc, PipelineHandle, PushConstants, Result, SamplerHandle, ShaderHandle,
    VertexFormat,
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
    /// Track 11b: dedicated async compute queue; runs overlapped with graphics when supported.
    /// Falls back to the standard compute queue when a dedicated async compute family is absent.
    AsyncCompute,
    /// Track 11b: dedicated DMA/transfer queue for texture uploads and buffer copies.
    /// Falls back to the transfer queue when a dedicated DMA-only family is absent.
    Dma,
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
    /// Buffer/AS being written by a BLAS or TLAS build command.
    AccelerationStructureBuild,
    /// Acceleration structure available for shader reads (e.g. traceRayEXT).
    AccelerationStructureRead,
    /// Image used as an attachment-tier fragment shading rate image.
    ///
    /// Required barrier state before `PassDesc::shading_rate_image` is consumed.
    /// Requires `BackendFeatures::vrs_attachment`.
    ShadingRateAttachment,
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

    pub fn new(base_mip: u16, mip_count: u16, base_layer: u16, layer_count: u16) -> Self {
        Self {
            base_mip,
            mip_count,
            base_layer,
            layer_count,
        }
    }

    pub fn mip(mip: u16) -> Self {
        Self {
            base_mip: mip,
            mip_count: 1,
            base_layer: 0,
            layer_count: u16::MAX,
        }
    }

    pub fn layer(layer: u16) -> Self {
        Self {
            base_mip: 0,
            mip_count: u16::MAX,
            base_layer: layer,
            layer_count: 1,
        }
    }

    pub fn overlaps(self, other: Self) -> bool {
        ranges_overlap(
            self.base_mip,
            self.mip_count,
            other.base_mip,
            other.mip_count,
        ) && ranges_overlap(
            self.base_layer,
            self.layer_count,
            other.base_layer,
            other.layer_count,
        )
    }
}

fn ranges_overlap(a_base: u16, a_count: u16, b_base: u16, b_count: u16) -> bool {
    if a_count == 0 || b_count == 0 {
        return false;
    }
    let a_end = range_end(a_base, a_count);
    let b_end = range_end(b_base, b_count);
    u32::from(a_base) < b_end && u32::from(b_base) < a_end
}

fn range_end(base: u16, count: u16) -> u32 {
    if count == u16::MAX {
        u32::from(u16::MAX) + 1
    } else {
        u32::from(base).saturating_add(u32::from(count))
    }
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

/// Direct draw with explicit vertex/index buffers.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DrawDesc {
    pub vertex_count: u32,
    pub instance_count: u32,
    pub first_vertex: u32,
    pub first_instance: u32,
    pub vertex_buffer: Option<VertexBufferBinding>,
    pub index_buffer: Option<IndexBufferBinding>,
    /// Optional viewport override: `[x, y, width, height]` in pixels.
    ///
    /// When `None`, the viewport covers the full attachment extent.
    /// Use this for atlas shadow maps where each cascade occupies a tile.
    pub viewport: Option<[u32; 4]>,
}

/// Indirect draw: GPU-populated command buffer drives vertex counts and
/// instance ranges. The render graph tracks `indirect_buffer` as
/// `RgState::IndirectRead`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DrawIndirectDesc {
    pub indirect_buffer: BufferHandle,
    pub offset: u64,
    /// Number of draw commands packed in the buffer.
    pub draw_count: u32,
    /// Stride between commands in bytes (≥ 16 for unindexed, ≥ 20 for indexed).
    pub stride: u32,
    pub indexed: bool,
    pub vertex_buffer: Option<VertexBufferBinding>,
    pub index_buffer: Option<IndexBufferBinding>,
}

/// Indirect draw with a GPU-populated count buffer.
///
/// `max_draw_count` is the upper bound consumed from `indirect_buffer`; the
/// actual command count is read from `count_buffer`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DrawIndirectCountDesc {
    pub indirect_buffer: BufferHandle,
    pub indirect_offset: u64,
    pub count_buffer: BufferHandle,
    pub count_offset: u64,
    pub max_draw_count: u32,
    /// Stride between commands in bytes (>= 16 for unindexed, >= 20 for indexed).
    pub stride: u32,
    pub indexed: bool,
    pub vertex_buffer: Option<VertexBufferBinding>,
    pub index_buffer: Option<IndexBufferBinding>,
}

/// Indirect compute dispatch: GPU-populated buffer drives group counts.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DispatchIndirectDesc {
    pub indirect_buffer: BufferHandle,
    pub offset: u64,
}

/// Direct mesh-shader dispatch.
/// Launches `(x, y, z)` mesh-shader workgroups without a task shader.
/// Requires `BackendFeatures::mesh_shading`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DrawMeshShaderDesc {
    pub group_count_x: u32,
    pub group_count_y: u32,
    pub group_count_z: u32,
}

/// Indirect mesh-shader dispatch: GPU-populated buffer drives workgroup counts.
/// The task shader typically populates this via `EmitMeshTasksEXT` /
/// `DispatchMesh`; alternatively a prior compute pass writes the counts.
/// Requires `BackendFeatures::mesh_shading`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DrawMeshShaderIndirectDesc {
    pub indirect_buffer: BufferHandle,
    pub offset: u64,
    pub draw_count: u32,
    pub stride: u32,
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ResolveImageDesc {
    pub src: ImageHandle,
    pub dst: ImageHandle,
    pub src_mip_level: u32,
    pub dst_mip_level: u32,
    pub src_base_layer: u32,
    pub dst_base_layer: u32,
    pub layer_count: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShadingRate {
    Rate1x1,
    Rate1x2,
    Rate2x1,
    Rate2x2,
    Rate2x4,
    Rate4x2,
    Rate4x4,
}

/// Build mode for acceleration structure construction commands.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AccelerationStructureBuildMode {
    /// Full build from scratch.
    Build,
    /// Incremental update of an existing structure.
    Update,
    /// Compact an existing structure into a smaller allocation.
    ///
    /// Build-size queries for compaction use `src` only and return a conservative
    /// upper bound equal to the source allocation size. Querying the exact
    /// compacted size requires GPU readback after a real build and is not exposed
    /// by the synchronous size-query API.
    Compact,
}

/// Geometry entry for a BLAS build.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BlasGeometryDesc {
    pub vertex_buffer: BufferHandle,
    pub vertex_offset: u64,
    pub vertex_count: u32,
    pub vertex_stride: u32,
    pub vertex_format: VertexFormat,
    pub index_buffer: Option<BufferHandle>,
    pub index_offset: u64,
    pub index_count: u32,
    pub index_format: Option<IndexFormat>,
    pub transform_buffer: Option<BufferHandle>,
    pub transform_offset: u64,
}

/// Describes a bottom-level acceleration structure build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlasBuildDesc {
    /// Destination acceleration structure for build/update/execute-time compact.
    /// Ignored by `Device::blas_build_sizes` when `mode` is `Compact`, because
    /// callers usually need the compact destination size before creating it.
    pub dst: AccelerationStructureHandle,
    /// Source acceleration structure for update/compact operations.
    pub src: Option<AccelerationStructureHandle>,
    /// Optional caller-provided scratch buffer. When `None`, backends may allocate
    /// transient scratch for the submitted frame.
    pub scratch_buffer: Option<BufferHandle>,
    pub geometries: Vec<BlasGeometryDesc>,
    pub mode: AccelerationStructureBuildMode,
    /// Opacity micromap to attach to each geometry triangle for hardware any-hit elimination.
    /// Requires `BackendFeatures::opacity_micromap`. When `Some`, each geometry's triangles
    /// reference sub-triangle opacity data in the micromap, eliminating any-hit shader
    /// invocations for fully opaque or fully transparent sub-triangles.
    pub opacity_micromap: Option<MicromapAttachDesc>,
}

/// Descriptor for building a per-triangle opacity micromap.
///
/// Micromaps assign per-sub-triangle opacity classifications (opaque/transparent/unknown)
/// for alpha-tested geometry. When attached to a BLAS, the RT hardware skips any-hit shaders
/// for fully classified triangles — critical for foliage with dense alpha testing.
///
/// Requires `BackendFeatures::opacity_micromap`.
#[derive(Clone, Debug)]
pub struct MicromapBuildDesc {
    /// Buffer containing the micromap index/usage data in `VkMicromapUsageEXT` format.
    pub data_buffer: BufferHandle,
    /// Byte offset into `data_buffer` for the micromap data.
    pub data_offset: u64,
    /// Total size of micromap data in bytes.
    pub data_size: u64,
    /// Format of the per-sub-triangle opacity data.
    pub format: MicromapFormat,
    /// Sub-division level (0–3); determines per-triangle sub-triangle count.
    pub subdivision_level: u32,
}

/// Sub-triangle opacity classification format for opacity micromaps.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MicromapFormat {
    /// 2-state: each sub-triangle is fully opaque or fully transparent (1 bit/sub-triangle).
    Unorm2State,
    /// 4-state: opaque / transparent / unknown-opaque / unknown-transparent (2 bits/sub-triangle).
    Unorm4State,
}

/// Attach an existing opacity micromap to a BLAS geometry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MicromapAttachDesc {
    /// Buffer containing the built micromap data.
    pub micromap_buffer: BufferHandle,
    /// Buffer containing the index/triangle mapping for attaching the micromap.
    pub index_buffer: Option<BufferHandle>,
    /// Byte offset for index data.
    pub index_offset: u64,
}

/// Describes a top-level acceleration structure build.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TlasBuildDesc {
    /// Destination acceleration structure for build/update/execute-time compact.
    /// Ignored by `Device::tlas_build_sizes` when `mode` is `Compact`, because
    /// callers usually need the compact destination size before creating it.
    pub dst: AccelerationStructureHandle,
    /// Source acceleration structure for update/compact operations.
    pub src: Option<AccelerationStructureHandle>,
    /// Optional caller-provided scratch buffer. When `None`, backends may allocate
    /// transient scratch for the submitted frame.
    pub scratch_buffer: Option<BufferHandle>,
    pub instance_buffer: BufferHandle,
    pub instance_offset: u64,
    pub instance_count: u32,
    pub mode: AccelerationStructureBuildMode,
}

/// One region of a shader binding table buffer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ShaderBindingTableRegion {
    pub buffer: BufferHandle,
    pub offset: u64,
    pub stride: u64,
    pub size: u64,
}

/// Full shader binding table for a TraceRays command.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ShaderBindingTable {
    pub raygen: ShaderBindingTableRegion,
    pub miss: ShaderBindingTableRegion,
    pub hit: ShaderBindingTableRegion,
    pub callable: Option<ShaderBindingTableRegion>,
}

/// GFX-3c: Describes a cluster acceleration structure build (VK_NV_cluster_acceleration_structure).
///
/// All inputs and outputs are GPU virtual addresses; callers must create
/// `SHADER_DEVICE_ADDRESS`-capable buffers and pass their device addresses.
/// Requires `BackendFeatures::cluster_acceleration_structure`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ClusterAccelerationStructureBuildDesc {
    /// Maximum number of cluster AS objects to build in this command.
    pub max_as_count: u32,
    /// Build flags (same `BuildAccelerationStructureFlags` as BLAS/TLAS).
    pub build_flags: u32,
    /// Operation type (build / update / move objects).
    pub op_type: u32,
    /// Operation mode (implicit / explicit / compute sizes).
    pub op_mode: u32,
    /// GPU address of the implicit-data destination, or 0.
    pub dst_implicit_data_address: u64,
    /// GPU address of the scratch buffer (from `cluster_as_build_sizes`).
    pub scratch_address: u64,
    /// Strided array of destination cluster AS addresses `{ address, size, stride }`.
    pub dst_addresses: [u64; 3],
    /// Strided array of destination size entries `{ address, size, stride }`.
    pub dst_sizes: [u64; 3],
    /// Strided array of source info structures `{ address, size, stride }`.
    pub src_infos: [u64; 3],
    /// GPU address of the source-info count (u32 on the GPU), or 0.
    pub src_infos_count_address: u64,
}

/// Describes a ray dispatch (vkCmdTraceRaysKHR).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TraceRaysDesc {
    pub pipeline: PipelineHandle,
    pub sbt: ShaderBindingTable,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    /// When `Some`, use `vkCmdTraceRaysIndirectKHR2` — dispatch dimensions and SBT regions
    /// are read from this GPU buffer at the given byte offset.
    /// Requires `BackendFeatures::ray_tracing_maintenance1`.
    /// The buffer must contain a `VkTraceRaysIndirectCommand2KHR` structure.
    pub indirect: Option<(BufferHandle, u64)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PassWork {
    #[default]
    None,
    /// Direct compute dispatch.
    Dispatch(DispatchDesc),
    /// Indirect compute dispatch — group counts come from a GPU buffer.
    DispatchIndirect(DispatchIndirectDesc),
    /// Direct vertex/index draw.
    Draw(DrawDesc),
    /// Indirect vertex/index draw — draw commands come from a GPU buffer.
    DrawIndirect(DrawIndirectDesc),
    /// Indirect vertex/index draw limited by a GPU-written command count.
    DrawIndirectCount(DrawIndirectCountDesc),
    /// Direct mesh-shader dispatch (no task shader).
    DrawMeshShader(DrawMeshShaderDesc),
    /// Indirect mesh-shader dispatch — workgroup counts come from a GPU buffer.
    DrawMeshShaderIndirect(DrawMeshShaderIndirectDesc),
    CopyImageToBuffer(CopyImageToBufferDesc),
    CopyBufferToImage(CopyBufferToImageDesc),
    ResolveImage(ResolveImageDesc),
    /// Generate a full mip chain for a single image by blitting each level
    /// from the previous one. The image must have `COPY_SRC | COPY_DST` usage
    /// and `mip_levels > 1`. The Vulkan backend uses `vkCmdBlitImage` with a
    /// linear filter; all mip levels end in `SHADER_READ_ONLY_OPTIMAL`.
    GenerateMipmaps {
        image: ImageHandle,
        mip_count: u32,
    },
    /// Blit a single mip level to another mip level of the same image (linear filter).
    ///
    /// Used for explicit level-by-level downsample chains (e.g. HizPass) where
    /// a compute shader is not needed and the blit filter is acceptable.
    BlitMip {
        image: ImageHandle,
        src_mip: u32,
        dst_mip: u32,
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
        aspect: SubresourceRange,
    },
    /// Build a bottom-level acceleration structure from triangle geometry.
    BuildBlas(BlasBuildDesc),
    /// Build a top-level acceleration structure from instance data.
    BuildTlas(TlasBuildDesc),
    /// Dispatch rays using a ray-tracing pipeline and a shader binding table.
    TraceRays(TraceRaysDesc),
    /// Decode a compressed video frame into an output image.
    ///
    /// Video graph passes are design-only scaffolding until backend video
    /// parameters, DPB image management, and command recording are wired.
    /// `RenderGraph::add_pass` rejects this work with `Error::Unsupported` so
    /// callers cannot accidentally compile a graph that will fail during command
    /// recording.
    DecodeVideoFrame(DecodeFrameDesc),
    /// Encode an input image into a compressed video bitstream.
    ///
    /// Video graph passes are design-only scaffolding until backend video
    /// parameters, DPB image management, and command recording are wired.
    /// `RenderGraph::add_pass` rejects this work with `Error::Unsupported` so
    /// callers cannot accidentally compile a graph that will fail during command
    /// recording.
    EncodeVideoFrame(EncodeFrameDesc),
    /// Execute GPU-generated commands from a pre-processed buffer.
    ///
    /// Device-generated command passes are design-only scaffolding until the
    /// backend owns command layouts, preprocess buffers, and recording through
    /// the selected DGC extension. `RenderGraph::add_pass` rejects this work
    /// with `Error::Unsupported` so callers cannot compile a graph that will
    /// fail during command recording.
    ExecuteGeneratedCommands(DgcExecuteDesc),
    /// Preprocess an indirect command buffer for GPU execution.
    ///
    /// Device-generated command passes are design-only scaffolding until the
    /// backend owns command layouts, preprocess buffers, and recording through
    /// the selected DGC extension. `RenderGraph::add_pass` rejects this work
    /// with `Error::Unsupported` so callers cannot compile a graph that will
    /// fail during command recording.
    PreprocessGeneratedCommands(DgcPreprocessDesc),
    /// Estimate optical flow motion vectors between two frames.
    ///
    /// Requires a backend-owned optical-flow session and images created with
    /// optical-flow input/output usage. Vulkan records
    /// `vkBindOpticalFlowSessionImageNV` and `vkCmdOpticalFlowExecuteNV` when
    /// `VK_NV_optical_flow` is enabled.
    EstimateOpticalFlow(OpticalFlowEstimateDesc),
    /// GFX-3c: Build cluster acceleration structure(s) for NVIDIA Mega Geometry.
    ///
    /// Requires `BackendFeatures::cluster_acceleration_structure`. Records
    /// `vkCmdBuildClusterAccelerationStructureIndirectNV`.
    BuildClusterAccelerationStructure(ClusterAccelerationStructureBuildDesc),
}

/// Inline descriptor binding pushed into the command stream without a descriptor pool.
///
/// Used with `PassDesc::push_descriptor_set` to update a single descriptor set
/// inline during command recording.  Requires `BackendFeatures::push_descriptors`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PushDescriptorSetDesc {
    /// Set index to push into.
    pub set: u32,
    pub bindings: Vec<PushDescriptorBinding>,
}

/// A single binding entry inside a `PushDescriptorSetDesc`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PushDescriptorBinding {
    SampledImage {
        binding: u32,
        image_view: ImageHandle,
        layout: RgState,
    },
    Sampler {
        binding: u32,
        sampler: SamplerHandle,
    },
    StorageBuffer {
        binding: u32,
        buffer: BufferHandle,
        offset: u64,
        range: u64,
    },
}

/// GPU-driven predicate: skip all commands in this pass when the `u32` at
/// `buffer[offset]` is zero (or non-zero when `inverted` is true).
///
/// Requires `BackendFeatures::conditional_rendering`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ConditionalRenderingDesc {
    pub buffer: BufferHandle,
    pub offset: u64,
    pub inverted: bool,
}

/// Shader binding mode for a render or compute pass.
///
/// The default path binds a compiled `VkPipeline` via `PassDesc::pipeline`.
/// `ShaderObjects` binds individually compiled `VkShaderEXT` objects instead,
/// which requires `BackendFeatures::shader_object`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderBinding {
    /// Bind a monolithic compiled pipeline (default).
    Pipeline(PipelineHandle),
    /// Bind individual shader objects per stage.
    ///
    /// Requires `BackendFeatures::shader_object`. Backends that do not support
    /// shader objects will fall back to the `PassDesc::pipeline` field. Graphics
    /// passes also use `PassDesc::pipeline` as their render-state/fallback anchor;
    /// the pipeline is not bound while shader objects are active.
    ShaderObjects(Vec<ShaderObjectHandle>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassDesc {
    pub name: String,
    pub queue: QueueType,
    pub shader: Option<ShaderHandle>,
    pub pipeline: Option<PipelineHandle>,
    pub bind_groups: Vec<BindGroupHandle>,
    pub push_constants: Option<PushConstants>,
    /// Pipeline-tier variable rate shading for graphics work.
    ///
    /// Backends ignore `None`. Callers should check
    /// `BackendFeatures::vrs_pipeline` before requesting a non-default rate.
    pub pipeline_shading_rate: Option<ShadingRate>,
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
    /// Inline descriptor updates pushed directly into the command stream.
    ///
    /// Requires `BackendFeatures::push_descriptors`. Each entry maps a
    /// binding slot to a descriptor (sampled image, sampler, or storage buffer).
    pub push_descriptor_set: Option<PushDescriptorSetDesc>,
    /// GPU-driven predicate: skip all commands in this pass when the u32 at
    /// `buffer[offset]` is zero (or non-zero when `inverted` is true).
    ///
    /// Requires `BackendFeatures::conditional_rendering`.
    pub predicate: Option<ConditionalRenderingDesc>,
    /// Explicit shader binding for this pass, overriding the `pipeline` field.
    ///
    /// When `Some(ShaderBinding::ShaderObjects(...))`, individually compiled
    /// shader objects are bound via `vkCmdBindShadersEXT` rather than a monolithic
    /// pipeline. Requires `BackendFeatures::shader_object`. Backends that do not
    /// support shader objects fall back to the `pipeline` field.
    pub shader_binding: Option<ShaderBinding>,
    /// Attachment-tier variable rate shading image for this graphics pass.
    ///
    /// When `Some`, binds the image as `VkRenderingFragmentShadingRateAttachmentInfoKHR`
    /// inside `cmd_begin_rendering`. Requires `BackendFeatures::vrs_attachment` and
    /// `BackendFeatures::dynamic_rendering`.
    pub shading_rate_image: Option<ImageHandle>,
    /// Hardware performance counters to record for this pass.
    ///
    /// Each handle is an index from `Engine::enumerate_performance_counters`. When `Some`,
    /// the backend wraps the pass's GPU work in a `VK_KHR_performance_query` begin/end pair.
    /// Requires `BackendFeatures::performance_query`; ignored otherwise.
    /// Results appear in `PassTimingReport::perf_counters` the following frame.
    pub perf_counters: Option<Vec<crate::PerfCounterHandle>>,
}

impl PassDesc {
    /// Create a pass descriptor with the common optional fields initialized to defaults.
    pub fn new(name: impl Into<String>, queue: QueueType) -> Self {
        Self {
            name: name.into(),
            queue,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        }
    }

    pub fn default_graphics(name: impl Into<String>) -> Self {
        Self::new(name, QueueType::Graphics)
    }

    pub fn default_compute(name: impl Into<String>) -> Self {
        Self::new(name, QueueType::Compute)
    }

    pub fn default_transfer(name: impl Into<String>) -> Self {
        Self::new(name, QueueType::Transfer)
    }
}

impl Default for PassDesc {
    fn default() -> Self {
        Self::default_graphics(String::new())
    }
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
        if let Some(predicate) = pass.predicate {
            if predicate.offset % 4 != 0 {
                return Err(Error::InvalidInput(
                    "conditional rendering predicate offset must be 4-byte aligned".into(),
                ));
            }
        }
        if let Some(ShaderBinding::ShaderObjects(handles)) = &pass.shader_binding {
            if handles.is_empty() {
                return Err(Error::InvalidInput(
                    "shader object binding requires at least one shader object".into(),
                ));
            }
        }
        if matches!(
            pass.work,
            PassWork::DecodeVideoFrame(_) | PassWork::EncodeVideoFrame(_)
        ) {
            return Err(Error::Unsupported(
                "render graph video passes are not executable until backend video sessions are implemented",
            ));
        }
        if matches!(
            pass.work,
            PassWork::ExecuteGeneratedCommands(_) | PassWork::PreprocessGeneratedCommands(_)
        ) {
            return Err(Error::Unsupported(
                "render graph device-generated command passes are not executable until backend DGC resources and recording are implemented",
            ));
        }
        if let PassWork::EstimateOpticalFlow(desc) = pass.work {
            if desc.input_current == desc.input_previous {
                return Err(Error::InvalidInput(
                    "optical-flow current and previous images must be distinct".into(),
                ));
            }
            if desc.output_motion_vectors == desc.input_current
                || desc.output_motion_vectors == desc.input_previous
            {
                return Err(Error::InvalidInput(
                    "optical-flow output image must be distinct from input images".into(),
                ));
            }
        }
        let has_pipeline_binding = pass.pipeline.is_some()
            || matches!(pass.shader_binding, Some(ShaderBinding::Pipeline(_)));
        let has_shader_object_binding = matches!(
            pass.shader_binding,
            Some(ShaderBinding::ShaderObjects(ref handles)) if !handles.is_empty()
        );
        let has_compute_shader_binding = has_pipeline_binding || has_shader_object_binding;
        let has_graphics_shader_binding =
            has_pipeline_binding || (has_shader_object_binding && pass.pipeline.is_some());
        if let Some(push_constants) = &pass.push_constants {
            push_constants.validate()?;
            if !has_compute_shader_binding {
                return Err(Error::InvalidInput(
                    "push constants require a pass shader binding".into(),
                ));
            }
        }
        if let Some(push_descriptors) = &pass.push_descriptor_set {
            if !has_compute_shader_binding {
                return Err(Error::InvalidInput(
                    "push descriptors require a pass shader binding".into(),
                ));
            }
            if push_descriptors.bindings.is_empty() {
                return Err(Error::InvalidInput(
                    "push descriptor set must contain at least one binding".into(),
                ));
            }
            for binding in &push_descriptors.bindings {
                if let PushDescriptorBinding::StorageBuffer { range, .. } = binding {
                    if *range == 0 {
                        return Err(Error::InvalidInput(
                            "push descriptor storage buffer range must be non-zero".into(),
                        ));
                    }
                }
            }
        }
        if pass.pipeline_shading_rate.is_some()
            && (pass.queue != QueueType::Graphics || !is_graphics_work(&pass.work))
        {
            return Err(Error::InvalidInput(
                "pipeline shading rate requires graphics pass work".into(),
            ));
        }
        if let PassWork::Dispatch(ref dispatch) = pass.work {
            if dispatch.x == 0 || dispatch.y == 0 || dispatch.z == 0 {
                return Err(Error::InvalidInput(
                    "dispatch dimensions must be non-zero".into(),
                ));
            }
            if !has_compute_shader_binding {
                return Err(Error::InvalidInput(
                    "dispatch pass requires a compute shader binding".into(),
                ));
            }
        }
        if let PassWork::Draw(ref draw) = pass.work {
            if draw.vertex_count == 0 || draw.instance_count == 0 {
                return Err(Error::InvalidInput(
                    "draw vertex_count and instance_count must be non-zero".into(),
                ));
            }
            if !has_graphics_shader_binding {
                return Err(Error::InvalidInput(
                    "draw pass requires a graphics shader binding; shader-object draws also need pass.pipeline as render-state/fallback anchor".into(),
                ));
            }
        }
        if let PassWork::DrawIndirectCount(ref draw) = pass.work {
            if draw.max_draw_count == 0 {
                return Err(Error::InvalidInput(
                    "draw-indirect-count max_draw_count must be non-zero".into(),
                ));
            }
            if draw.stride < if draw.indexed { 20 } else { 16 } || draw.stride % 4 != 0 {
                return Err(Error::InvalidInput(
                    "draw-indirect-count stride must be a multiple of 4 and large enough for the command type"
                        .into(),
                ));
            }
            if draw.indexed && draw.index_buffer.is_none() {
                return Err(Error::InvalidInput(
                    "indexed draw-indirect-count requires an index buffer binding".into(),
                ));
            }
            if !has_graphics_shader_binding {
                return Err(Error::InvalidInput(
                    "draw-indirect-count pass requires a graphics shader binding; shader-object draws also need pass.pipeline as render-state/fallback anchor".into(),
                ));
            }
        }
        if let PassWork::CopyImageToBuffer(copy) = pass.work {
            self.validate_copy_image_to_buffer(copy)?;
        }
        if let PassWork::CopyBufferToImage(copy) = pass.work {
            self.validate_copy_buffer_to_image(copy)?;
        }
        if let PassWork::ResolveImage(resolve) = pass.work {
            self.validate_resolve_image(resolve)?;
        }
        if let PassWork::BuildBlas(ref build) = pass.work {
            if build.mode == AccelerationStructureBuildMode::Compact {
                if build.src.is_none() {
                    return Err(Error::InvalidInput(
                        "BLAS compaction requires a source acceleration structure".into(),
                    ));
                }
            } else if build.geometries.is_empty() {
                return Err(Error::InvalidInput(
                    "BLAS build requires at least one geometry".into(),
                ));
            } else {
                for geometry in &build.geometries {
                    if geometry.vertex_count == 0 {
                        return Err(Error::InvalidInput(
                            "BLAS geometry vertex_count must be non-zero".into(),
                        ));
                    }
                    if geometry.vertex_stride == 0 {
                        return Err(Error::InvalidInput(
                            "BLAS geometry vertex_stride must be non-zero".into(),
                        ));
                    }
                    if geometry.index_buffer.is_some() {
                        if geometry.index_count == 0 || geometry.index_count % 3 != 0 {
                            return Err(Error::InvalidInput(
                                "indexed BLAS geometry index_count must be a non-zero multiple of 3"
                                    .into(),
                            ));
                        }
                        if geometry.index_format.is_none() {
                            return Err(Error::InvalidInput(
                                "indexed BLAS geometry requires an index format".into(),
                            ));
                        }
                    } else if geometry.vertex_count % 3 != 0 {
                        return Err(Error::InvalidInput(
                            "non-indexed BLAS geometry vertex_count must be a multiple of 3".into(),
                        ));
                    }
                }
            }
        }
        if let PassWork::BuildTlas(ref build) = pass.work {
            if build.mode == AccelerationStructureBuildMode::Compact {
                if build.src.is_none() {
                    return Err(Error::InvalidInput(
                        "TLAS compaction requires a source acceleration structure".into(),
                    ));
                }
            } else if build.instance_count == 0 {
                return Err(Error::InvalidInput(
                    "TLAS build instance_count must be non-zero".into(),
                ));
            }
        }
        if let PassWork::TraceRays(ref trace) = pass.work {
            if trace.width == 0 || trace.height == 0 || trace.depth == 0 {
                return Err(Error::InvalidInput(
                    "trace rays dimensions must be non-zero".into(),
                ));
            }
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

    fn validate_resolve_image(&self, resolve: ResolveImageDesc) -> Result<()> {
        if resolve.width == 0 || resolve.height == 0 || resolve.layer_count == 0 {
            return Err(Error::InvalidInput(
                "resolve width, height, and layer_count must be non-zero".into(),
            ));
        }
        let src = self
            .images
            .iter()
            .find(|image| image.handle == resolve.src)
            .ok_or(Error::InvalidHandle)?;
        let dst = self
            .images
            .iter()
            .find(|image| image.handle == resolve.dst)
            .ok_or(Error::InvalidHandle)?;
        if src.desc.samples <= 1 {
            return Err(Error::InvalidInput(
                "resolve source image must have more than one sample".into(),
            ));
        }
        if dst.desc.samples != 1 {
            return Err(Error::InvalidInput(
                "resolve destination image must have exactly one sample".into(),
            ));
        }
        if src.desc.format != dst.desc.format {
            return Err(Error::InvalidInput(
                "resolve source and destination formats must match".into(),
            ));
        }
        if src.desc.extent.depth != 1 || dst.desc.extent.depth != 1 {
            return Err(Error::InvalidInput(
                "resolve currently supports 2D images only".into(),
            ));
        }
        validate_copy_fits_image(
            src.desc,
            resolve.src_mip_level,
            resolve.src_base_layer,
            resolve.layer_count,
            resolve.width,
            resolve.height,
            1,
        )?;
        validate_copy_fits_image(
            dst.desc,
            resolve.dst_mip_level,
            resolve.dst_base_layer,
            resolve.layer_count,
            resolve.width,
            resolve.height,
            1,
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
                    return Err(Error::ResourceStateCorruption(format!(
                        "render graph pass '{}' references image {:?} in state {:?}, but that image was not imported into the graph",
                        pass.name, usage.image, usage.state
                    )));
                };
                let image = &mut images[image_index];
                image.first_use = image.first_use.min(ordered_index as u32);
                image.last_use = image.last_use.max(ordered_index as u32);
            }
            for usage in pass.buffer_reads.iter().chain(pass.buffer_writes.iter()) {
                let Some(buffer_index) = buffer_indices.get(&usage.buffer).copied() else {
                    return Err(Error::ResourceStateCorruption(format!(
                        "render graph pass '{}' references buffer {:?} in state {:?}, but that buffer was not imported into the graph",
                        pass.name, usage.buffer, usage.state
                    )));
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

fn is_graphics_work(work: &PassWork) -> bool {
    matches!(
        work,
        PassWork::Draw(_)
            | PassWork::DrawIndirect(_)
            | PassWork::DrawIndirectCount(_)
            | PassWork::DrawMeshShader(_)
            | PassWork::DrawMeshShaderIndirect(_)
    )
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
    if desc.format.is_block_compressed() {
        let blocks_x = (width as u64).div_ceil(4);
        let blocks_y = (height as u64).div_ceil(4);
        return [
            blocks_x,
            blocks_y,
            depth as u64,
            layer_count as u64,
            desc.format.bc_block_bytes(),
        ]
        .into_iter()
        .try_fold(1u64, |acc, value| {
            acc.checked_mul(value)
                .ok_or_else(|| Error::InvalidInput("copy byte count overflowed".into()))
        });
    }

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
    use crate::Format;
    if desc.format == Format::Unknown {
        return Err(Error::InvalidInput(
            "copy image format must be specified".into(),
        ));
    }
    if desc.format.is_block_compressed() {
        return Err(Error::InvalidInput(
            "BC-compressed copy byte counts must be computed from 4x4 blocks".into(),
        ));
    }
    let size = match desc.format {
        Format::Rgba8Unorm | Format::Bgra8Unorm => 4,
        Format::R8Unorm => 1,
        Format::Rg8Unorm => 2,
        Format::Rgba16Float => 8,
        Format::Rgba32Float => 16,
        Format::Depth32Float | Format::Depth24Stencil8 => 4,
        Format::Unknown => {
            return Err(Error::InvalidInput(
                "copy image format must be specified".into(),
            ));
        }
        // YCbCr planar: 1 byte/texel (luma only for copy sizing; chroma plane handled separately).
        Format::G8_B8R8_2PLANE_420_UNORM => 1,
        Format::Bc3Unorm
        | Format::Bc3UnormSrgb
        | Format::Bc4Unorm
        | Format::Bc5Unorm
        | Format::Bc7Unorm
        | Format::Bc7UnormSrgb
        | Format::Bc6hUfloat => {
            return Err(Error::InvalidInput(
                "BC-compressed copy byte counts must be computed from 4x4 blocks".into(),
            ));
        }
    };
    Ok(size)
}

fn mip_extent(base: u32, mip_level: u32) -> u32 {
    (base >> mip_level).max(1)
}

#[cfg(test)]
mod tests;
