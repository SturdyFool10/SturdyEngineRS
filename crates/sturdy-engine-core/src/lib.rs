//! Core engine crate.
//!
//! This crate owns the renderer's backend-neutral model: devices, capabilities,
//! opaque handles, images, shaders, frame graphs, and submission semantics. It
//! intentionally has no C ABI concerns and no high-level ergonomic wrappers.

pub mod backend;
pub mod binding;
pub mod buffer;
pub mod caps;
pub mod device;
pub mod error;
pub mod handles;
pub mod image;
pub mod pipeline;
pub mod render_graph;
pub mod shader;
pub mod slang;
pub mod surface;

pub use backend::{Backend, BackendKind, auto_backend_preference_order, available_backend_kinds};
pub use binding::{
    BindGroupDesc, BindGroupEntry, BindingKind, CanonicalBinding, CanonicalGroupLayout,
    CanonicalPipelineLayout, ResourceBinding, StageMask, UpdateRate,
};
pub use buffer::{BufferDesc, BufferUsage};
pub use caps::Caps;
pub use device::{Device, DeviceDesc, Frame};
pub use error::{Error, Result};
pub use handles::{
    BindGroupHandle, BufferHandle, DeviceHandle, FrameHandle, ImageHandle, PassHandle,
    PipelineHandle, PipelineLayoutHandle, ShaderHandle, SurfaceHandle,
};
pub use image::{Extent3d, Format, ImageDesc, ImageUsage};
pub use pipeline::{
    ColorTargetDesc, ComputePipelineDesc, CullMode, FrontFace, GraphicsPipelineDesc,
    PrimitiveTopology, RasterState, VertexAttributeDesc, VertexBufferLayout, VertexFormat,
    VertexInputRate,
};
pub use render_graph::{
    Access, Barrier, BufferBarrier, BufferUse, CompiledGraph, CopyImageToBufferDesc, DispatchDesc,
    DrawDesc, ImageBarrier, ImageUse, IndexBufferBinding, IndexFormat, PassDesc, PassWork,
    QueueType, RecordBatch, RenderGraph, ResourceUse, RgState, SubresourceRange,
    VertexBufferBinding,
};
pub use shader::{
    CompiledShaderArtifact, ShaderDesc, ShaderModule, ShaderReflection, ShaderSource, ShaderStage,
    ShaderTarget,
};
pub use slang::{
    SlangCompileDesc, compile_and_reflect, compile_slang, compile_slang_to_file,
    compile_slang_to_spirv, reflect_pipeline_layout, spirv_words_from_bytes,
};
#[cfg(not(target_arch = "wasm32"))]
pub use surface::NativeSurfaceDesc;
pub use surface::SurfaceSize;
