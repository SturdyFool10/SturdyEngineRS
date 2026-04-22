//! Core engine crate.
//!
//! This crate owns the renderer's backend-neutral model: devices, capabilities,
//! opaque handles, images, shaders, frame graphs, and submission semantics. It
//! intentionally has no C ABI concerns and no high-level ergonomic wrappers.

pub mod adapter_info;
pub mod adapter_kind;
pub mod adapter_selection;
pub mod backend;
pub mod backend_features;
pub mod binding;
pub mod buffer;
pub mod caps;
pub mod device;
pub mod error;
pub mod external_resource;
pub mod gpu_capture;
pub mod handles;
pub mod image;
pub mod limits;
pub mod native_handles;
pub mod pipeline;
pub mod push_constants;
pub mod raw_capabilities;
pub mod render_graph;
pub mod sampler;
pub mod shader;
pub mod slang;
pub mod surface;

pub use adapter_info::AdapterInfo;
pub use adapter_kind::AdapterKind;
pub use adapter_selection::AdapterSelection;
pub use backend::{Backend, BackendKind, auto_backend_preference_order, available_backend_kinds};
pub use backend_features::BackendFeatures;
pub use binding::{
    BINDLESS_COUNT, BindGroupDesc, BindGroupEntry, BindingKind, CanonicalBinding,
    CanonicalGroupLayout, CanonicalPipelineLayout, ResourceBinding, StageMask, UpdateRate,
};
pub use buffer::{BufferDesc, BufferUsage};
pub use caps::Caps;
pub use device::{Device, DeviceDesc, Frame, enumerate_adapters};
pub use error::{Error, Result};
pub use external_resource::{
    ExternalBufferDesc, ExternalBufferHandle, ExternalImageDesc, ExternalImageHandle,
    VulkanExternalBuffer, VulkanExternalImage,
};
pub use gpu_capture::{GpuCaptureDesc, GpuCaptureTool};
pub use handles::{
    BindGroupHandle, BufferHandle, DeviceHandle, FrameHandle, ImageHandle, PassHandle,
    PipelineHandle, PipelineLayoutHandle, SamplerHandle, ShaderHandle, SubmissionHandle,
    SurfaceHandle,
};
pub use image::{
    Extent3d, Format, FormatCapabilities, ImageBuilder, ImageClearValue, ImageDesc, ImageDimension,
    ImageRole, ImageUsage,
};
pub use limits::Limits;
pub use native_handles::{
    NativeHandleCapabilities, NativeHandleCapability, NativeHandleKind, NativeHandleOwnership,
    native_handle_capabilities_for_backend,
};
pub use pipeline::{
    BlendMode, ColorTargetDesc, ComputePipelineDesc, CullMode, FrontFace, GraphicsPipelineDesc,
    PrimitiveTopology, RasterState, VertexAttributeDesc, VertexBufferLayout, VertexFormat,
    VertexInputRate,
};
pub use push_constants::PushConstants;
pub use raw_capabilities::{
    BackendRawCapabilities, D3d12RawCapabilities, MetalRawCapabilities, VulkanRawCapabilities,
};
pub use render_graph::{
    Access, AliasPlan, Barrier, BufferBarrier, BufferStateKey, BufferUse, CompiledGraph,
    CopyBufferToImageDesc, CopyImageToBufferDesc, DispatchDesc, DrawDesc, ImageBarrier,
    ImageStateKey, ImageUse, IndexBufferBinding, IndexFormat, PassDesc, PassWork, QueueType,
    RecordBatch, RenderGraph, ResourceLifetime, ResourceUse, RgState, SubresourceRange,
    VertexBufferBinding,
};
pub use sampler::{AddressMode, BorderColor, CompareOp, FilterMode, MipmapMode, SamplerDesc};
pub use shader::{
    CompiledShaderArtifact, ShaderDesc, ShaderModule, ShaderReflection, ShaderSource, ShaderStage,
    ShaderTarget,
};
pub use slang::{
    SlangCompileDesc, compile_and_reflect, compile_slang, compile_slang_to_file,
    compile_slang_to_spirv, reflect_pipeline_layout, reflect_pipeline_layout_with_caps,
    spirv_words_from_bytes,
};
#[cfg(not(target_arch = "wasm32"))]
pub use surface::NativeSurfaceDesc;
pub use surface::{
    SurfaceCapabilities, SurfaceColorSpace, SurfaceEvent, SurfaceFormatInfo, SurfaceHdrCaps,
    SurfaceHdrPreference, SurfaceInfo, SurfacePresentMode, SurfaceRecreateDesc, SurfaceSize,
};
