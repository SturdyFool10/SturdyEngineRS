#[cfg(not(target_arch = "wasm32"))]
use crate::NativeSurfaceDesc;
use crate::{
    BindGroupDesc, BindGroupHandle, BufferDesc, BufferHandle, CanonicalPipelineLayout, Caps,
    CompiledGraph, ComputePipelineDesc, GraphicsPipelineDesc, ImageDesc, ImageHandle,
    PipelineHandle, PipelineLayoutHandle, Result, SamplerDesc, SamplerHandle, ShaderDesc,
    ShaderHandle, ShaderTarget, SubmissionHandle, SurfaceHandle, SurfaceSize,
};

#[cfg(target_os = "windows")]
pub mod d3d12;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "visionos"
))]
pub mod metal;
#[cfg(not(target_arch = "wasm32"))]
pub mod vulkan;

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum BackendKind {
    #[default]
    Auto,
    Vulkan,
    D3d12,
    Metal,
    Null,
}

impl BackendKind {
    pub fn is_available_on_target(self) -> bool {
        match self {
            Self::Auto | Self::Null => true,
            Self::Vulkan => cfg!(not(target_arch = "wasm32")),
            Self::D3d12 => cfg!(target_os = "windows"),
            Self::Metal => cfg!(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "visionos"
            )),
        }
    }
}

pub fn available_backend_kinds() -> Vec<BackendKind> {
    let mut backends = Vec::new();
    if BackendKind::Vulkan.is_available_on_target() {
        backends.push(BackendKind::Vulkan);
    }
    if BackendKind::D3d12.is_available_on_target() {
        backends.push(BackendKind::D3d12);
    }
    if BackendKind::Metal.is_available_on_target() {
        backends.push(BackendKind::Metal);
    }
    backends.push(BackendKind::Null);
    backends
}

pub fn auto_backend_preference_order() -> Vec<BackendKind> {
    let candidates = if cfg!(target_os = "windows") {
        vec![BackendKind::D3d12, BackendKind::Vulkan]
    } else if cfg!(target_os = "linux") {
        vec![BackendKind::Vulkan]
    } else if cfg!(target_os = "macos") {
        vec![BackendKind::Vulkan, BackendKind::Metal]
    } else if cfg!(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos"
    )) {
        vec![BackendKind::Metal]
    } else {
        vec![BackendKind::Vulkan]
    };

    candidates
        .into_iter()
        .filter(|backend| backend.is_available_on_target())
        .collect()
}

pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;
    fn adapter_name(&self) -> Option<String> {
        None
    }
    fn caps(&self) -> Caps;
    /// The shader IR format this backend requires. The device uses this to select
    /// the Slang compilation target when compiling from source.
    fn preferred_shader_ir(&self) -> ShaderTarget {
        ShaderTarget::Spirv
    }
    fn create_image(&self, _handle: ImageHandle, _desc: ImageDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_image(&self, _handle: ImageHandle) -> Result<()> {
        Ok(())
    }
    fn create_buffer(&self, _handle: BufferHandle, _desc: BufferDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_buffer(&self, _handle: BufferHandle) -> Result<()> {
        Ok(())
    }
    fn create_sampler(&self, _handle: SamplerHandle, _desc: SamplerDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_sampler(&self, _handle: SamplerHandle) -> Result<()> {
        Ok(())
    }
    fn write_buffer(&self, _handle: BufferHandle, _offset: u64, _data: &[u8]) -> Result<()> {
        Ok(())
    }
    fn read_buffer(&self, _handle: BufferHandle, _offset: u64, out: &mut [u8]) -> Result<()> {
        out.fill(0);
        Ok(())
    }
    fn create_shader(&self, _handle: ShaderHandle, _desc: &ShaderDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_shader(&self, _handle: ShaderHandle) -> Result<()> {
        Ok(())
    }
    fn create_pipeline_layout(
        &self,
        _handle: PipelineLayoutHandle,
        _layout: &CanonicalPipelineLayout,
    ) -> Result<()> {
        Ok(())
    }
    fn destroy_pipeline_layout(&self, _handle: PipelineLayoutHandle) -> Result<()> {
        Ok(())
    }
    fn create_bind_group(&self, _handle: BindGroupHandle, _desc: &BindGroupDesc) -> Result<()> {
        Ok(())
    }
    fn destroy_bind_group(&self, _handle: BindGroupHandle) -> Result<()> {
        Ok(())
    }
    fn create_compute_pipeline(
        &self,
        _handle: PipelineHandle,
        _desc: ComputePipelineDesc,
    ) -> Result<()> {
        Ok(())
    }
    fn create_graphics_pipeline(
        &self,
        _handle: PipelineHandle,
        _desc: &GraphicsPipelineDesc,
    ) -> Result<()> {
        Ok(())
    }
    fn destroy_pipeline(&self, _handle: PipelineHandle) -> Result<()> {
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn create_surface(&self, _handle: SurfaceHandle, _desc: NativeSurfaceDesc) -> Result<()> {
        Ok(())
    }
    fn resize_surface(&self, _handle: SurfaceHandle, _size: SurfaceSize) -> Result<()> {
        Ok(())
    }
    fn acquire_surface_image(
        &self,
        _surface: SurfaceHandle,
        _image: ImageHandle,
    ) -> Result<ImageDesc> {
        Err(crate::Error::Unsupported(
            "backend does not support surface image acquisition",
        ))
    }
    fn present_surface(&self, _surface: SurfaceHandle) -> Result<()> {
        Err(crate::Error::Unsupported(
            "backend does not support surface presentation",
        ))
    }
    fn destroy_surface(&self, _handle: SurfaceHandle) -> Result<()> {
        Ok(())
    }
    fn flush(&self, _graph: &CompiledGraph) -> Result<SubmissionHandle>;
    fn wait_submission(&self, _token: SubmissionHandle) -> Result<()> {
        Ok(())
    }
    fn present(&self) -> Result<()>;
    fn wait_idle(&self) -> Result<()>;
}

#[derive(Debug)]
pub(crate) struct NullBackend {
    kind: BackendKind,
    caps: Caps,
}

impl NullBackend {
    pub(crate) fn new() -> Self {
        Self::for_kind(BackendKind::Null)
    }

    pub(crate) fn for_kind(kind: BackendKind) -> Self {
        Self {
            kind,
            caps: Caps::default(),
        }
    }
}

impl Backend for NullBackend {
    fn kind(&self) -> BackendKind {
        self.kind
    }

    fn caps(&self) -> Caps {
        self.caps
    }

    fn flush(&self, _graph: &CompiledGraph) -> Result<SubmissionHandle> {
        Ok(SubmissionHandle(0))
    }

    fn present(&self) -> Result<()> {
        Ok(())
    }

    fn wait_idle(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_preference_order_matches_target_policy() {
        let order = auto_backend_preference_order();

        if cfg!(target_os = "windows") {
            assert_eq!(order, vec![BackendKind::D3d12, BackendKind::Vulkan]);
        } else if cfg!(target_os = "linux") {
            assert_eq!(order, vec![BackendKind::Vulkan]);
        } else if cfg!(target_os = "macos") {
            assert_eq!(order, vec![BackendKind::Vulkan, BackendKind::Metal]);
        } else if cfg!(any(
            target_os = "ios",
            target_os = "tvos",
            target_os = "visionos"
        )) {
            assert_eq!(order, vec![BackendKind::Metal]);
        }
    }
}
