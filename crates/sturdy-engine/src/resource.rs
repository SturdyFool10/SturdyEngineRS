use sturdy_engine_core as core;

use crate::{
    AccelerationStructureDesc, BindGroupDesc, BufferDesc, CanonicalPipelineLayout, HdrMetadata,
    ImageDesc, Result, SamplerDesc, ShaderDesc, SurfaceCapabilities, SurfaceEvent, SurfaceHdrCaps,
    SurfaceInfo, SurfaceRecreateDesc, SurfaceSize,
};

pub struct Image {
    pub(crate) device: core::Device,
    pub(crate) handle: core::ImageHandle,
    pub(crate) desc: ImageDesc,
    /// Stable index into the engine's global bindless sampled-image array.
    ///
    /// `Some(idx)` when the image was registered at creation time (sampled,
    /// non-depth, non-transient).  Pass this into shaders as
    /// `g_bindless_textures[NonUniformResourceIndex(idx)]`.  `None` on hardware
    /// without bindless support or for images that are not suitable for bindless
    /// access (depth/stencil, transient, storage-only).
    bindless_handle: Option<u32>,
}

impl Image {
    pub fn handle(&self) -> core::ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }

    /// The stable bindless index for this image, if registered.
    ///
    /// Non-`None` for every sampled (non-depth, non-transient) engine-created
    /// image when the device supports `VK_EXT_descriptor_indexing`.  Use this
    /// index in shaders that include `bindless.slang`:
    ///
    /// ```slang
    /// float4 color = g_bindless_textures[NonUniformResourceIndex(push.albedo_idx)]
    ///                    .Sample(g_bindless_samplers[push.sampler_idx], uv);
    /// ```
    pub fn bindless_handle(&self) -> Option<u32> {
        self.bindless_handle
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_image_debug_name(self.handle, name)
    }

    /// Build an Image that is NOT auto-registered in the bindless heap.
    ///
    /// Used internally for render targets, depth buffers, and images whose
    /// heap slot would never be reclaimed (e.g. transient frame resources).
    pub(crate) fn without_bindless(device: core::Device, handle: core::ImageHandle, desc: ImageDesc) -> Self {
        Self { device, handle, desc, bindless_handle: None }
    }

    /// Build an Image and attempt bindless registration.
    ///
    /// Auto-registers if `desc.usage` contains `SAMPLED` and does NOT contain
    /// `DEPTH_STENCIL`, and `desc.transient` is false.
    pub(crate) fn with_auto_bindless(device: core::Device, handle: core::ImageHandle, desc: ImageDesc) -> Self {
        use crate::ImageUsage;
        let should_register = desc.usage.contains(ImageUsage::SAMPLED)
            && !desc.usage.contains(ImageUsage::DEPTH_STENCIL)
            && !desc.transient;
        let bindless_handle = if should_register {
            device.register_bindless_sampled_image(handle)
        } else {
            None
        };
        Self { device, handle, desc, bindless_handle }
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        let _ = self.device.destroy_image(self.handle);
    }
}

pub struct AccelerationStructure {
    pub(crate) device: core::Device,
    pub(crate) handle: core::AccelerationStructureHandle,
    pub(crate) desc: AccelerationStructureDesc,
}

impl AccelerationStructure {
    pub fn handle(&self) -> core::AccelerationStructureHandle {
        self.handle
    }

    pub fn desc(&self) -> AccelerationStructureDesc {
        self.desc
    }
}

impl Drop for AccelerationStructure {
    fn drop(&mut self) {
        let _ = self.device.destroy_acceleration_structure(self.handle);
    }
}

pub struct Buffer {
    pub(crate) device: core::Device,
    pub(crate) handle: core::BufferHandle,
    pub(crate) desc: BufferDesc,
}

impl Buffer {
    pub fn handle(&self) -> core::BufferHandle {
        self.handle
    }

    pub fn desc(&self) -> BufferDesc {
        self.desc
    }

    pub fn write(&self, offset: u64, data: &[u8]) -> Result<()> {
        self.device.write_buffer(self.handle, offset, data)
    }

    /// Write one plain-old-data value into this buffer.
    pub fn write_pod<T: bytemuck::Pod>(&self, offset: u64, value: &T) -> Result<()> {
        self.write(offset, bytemuck::bytes_of(value))
    }

    /// Write a typed plain-old-data slice into this buffer.
    pub fn write_slice<T: bytemuck::Pod>(&self, offset: u64, values: &[T]) -> Result<()> {
        self.write(offset, bytemuck::cast_slice(values))
    }

    pub fn read(&self, offset: u64, out: &mut [u8]) -> Result<()> {
        self.device.read_buffer(self.handle, offset, out)
    }

    pub fn device_address(&self) -> Result<Option<u64>> {
        self.device.buffer_device_address(self.handle)
    }

    /// Stable bindless index for this buffer in the engine's global heap.
    ///
    /// `Some(idx)` for every non-transient storage buffer when the device
    /// supports bindless.  Use this in shaders that include `bindless.slang`:
    ///
    /// ```slang
    /// uint data = bindless_load_uint(push.buf_idx, byte_offset);
    /// ```
    pub fn bindless_handle(&self) -> Option<u32> {
        self.device.buffer_bindless_index(self.handle)
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_buffer_debug_name(self.handle, name)
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let _ = self.device.destroy_buffer(self.handle);
    }
}

pub struct Sampler {
    pub(crate) device: core::Device,
    pub(crate) handle: core::SamplerHandle,
    pub(crate) desc: SamplerDesc,
}

impl Sampler {
    pub fn handle(&self) -> core::SamplerHandle {
        self.handle
    }

    pub fn desc(&self) -> SamplerDesc {
        self.desc
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        let _ = self.device.destroy_sampler(self.handle);
    }
}

pub struct Shader {
    pub(crate) device: core::Device,
    pub(crate) handle: core::ShaderHandle,
    pub(crate) desc: ShaderDesc,
}

impl Shader {
    pub fn handle(&self) -> core::ShaderHandle {
        self.handle
    }

    pub fn desc(&self) -> &ShaderDesc {
        &self.desc
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        let _ = self.device.destroy_shader(self.handle);
    }
}

pub struct BindGroup {
    pub(crate) device: core::Device,
    pub(crate) handle: core::BindGroupHandle,
    pub(crate) desc: BindGroupDesc,
}

impl BindGroup {
    pub fn handle(&self) -> core::BindGroupHandle {
        self.handle
    }

    pub fn desc(&self) -> &BindGroupDesc {
        &self.desc
    }
}

impl Drop for BindGroup {
    fn drop(&mut self) {
        let _ = self.device.destroy_bind_group(self.handle);
    }
}

pub struct PipelineLayout {
    pub(crate) device: core::Device,
    pub(crate) handle: core::PipelineLayoutHandle,
    pub(crate) layout: CanonicalPipelineLayout,
}

impl PipelineLayout {
    pub fn handle(&self) -> core::PipelineLayoutHandle {
        self.handle
    }

    pub fn layout(&self) -> &CanonicalPipelineLayout {
        &self.layout
    }
}

impl Drop for PipelineLayout {
    fn drop(&mut self) {
        let _ = self.device.destroy_pipeline_layout(self.handle);
    }
}

pub struct Pipeline {
    pub(crate) device: core::Device,
    pub(crate) handle: core::PipelineHandle,
}

impl Pipeline {
    pub fn handle(&self) -> core::PipelineHandle {
        self.handle
    }

    pub fn set_debug_name(&self, name: &str) -> Result<()> {
        self.device.set_pipeline_debug_name(self.handle, name)
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        let _ = self.device.destroy_pipeline(self.handle);
    }
}

pub struct RayTracingShaderBindingTable {
    pub(crate) device: core::Device,
    pub(crate) table: core::ShaderBindingTable,
}

impl RayTracingShaderBindingTable {
    pub fn table(&self) -> core::ShaderBindingTable {
        self.table
    }
}

impl Drop for RayTracingShaderBindingTable {
    fn drop(&mut self) {
        let _ = self.device.destroy_buffer(self.table.raygen.buffer);
    }
}

pub struct Surface {
    pub(crate) device: core::Device,
    pub(crate) handle: core::SurfaceHandle,
    pub(crate) info: SurfaceInfo,
    /// Native window and display handles used to create this surface.
    /// Stored so the surface can be recreated after a backend restart.
    /// `None` for programmatically-created surfaces (tests, offscreen).
    pub(crate) native_desc: Option<core::NativeSurfaceDesc>,
}

impl Surface {
    pub fn handle(&self) -> core::SurfaceHandle {
        self.handle
    }

    pub fn size(&self) -> SurfaceSize {
        self.info.size
    }

    pub fn info(&self) -> SurfaceInfo {
        self.info
    }

    pub fn resize(&mut self, size: SurfaceSize) -> Result<()> {
        self.device.resize_surface(self.handle, size)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(())
    }

    pub fn recreate(&mut self, desc: SurfaceRecreateDesc) -> Result<()> {
        self.device.recreate_surface(self.handle, desc)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(())
    }

    pub fn drain_events(&mut self) -> Result<Vec<SurfaceEvent>> {
        let events = self.device.drain_surface_events(self.handle)?;
        self.info = self.device.surface_info(self.handle)?;
        Ok(events)
    }

    pub fn hdr_caps(&self) -> Result<SurfaceHdrCaps> {
        self.device.surface_hdr_caps(self.handle)
    }

    pub fn capabilities(&self) -> Result<SurfaceCapabilities> {
        self.device.query_surface_capabilities(self.handle)
    }

    /// Set SMPTE ST 2086 / CTA 861.3 HDR mastering display metadata on this surface.
    ///
    /// Has no effect when `SurfaceCapabilities::hdr_metadata_supported` is `false` or
    /// the swapchain is not in an HDR10 color space. Silently ignored rather than
    /// returning an error to simplify call sites that run on mixed display setups.
    pub fn set_hdr_metadata(&self, metadata: HdrMetadata) -> Result<()> {
        self.device.set_surface_hdr_metadata(self.handle, metadata)
    }

    /// Block until the NVIDIA Reflex driver signals the optimal frame-start time.
    ///
    /// Call once per frame before input sampling. No-op when Reflex is unavailable.
    pub fn latency_sleep(&self) -> Result<()> {
        self.device.latency_sleep(self.handle)
    }

    pub fn acquire_image(&self) -> Result<SurfaceImage> {
        let (handle, slot) = self.device.acquire_surface_image(self.handle)?;
        let desc = self.device.image_desc(handle)?;
        Ok(SurfaceImage {
            device: self.device.clone(),
            handle,
            desc,
            slot,
        })
    }

    pub fn present(&self) -> Result<()> {
        self.device.present_surface(self.handle)
    }

    pub(crate) fn auto_present_info(&self) -> (core::Device, core::SurfaceHandle) {
        (self.device.clone(), self.handle)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        let _ = self.device.destroy_surface(self.handle);
    }
}

pub struct SurfaceImage {
    pub(crate) device: core::Device,
    pub(crate) handle: core::ImageHandle,
    pub(crate) desc: ImageDesc,
    /// Stable swapchain image index (0..swapchain_image_count).
    pub(crate) slot: u64,
}

impl SurfaceImage {
    pub fn handle(&self) -> core::ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }
}

impl Drop for SurfaceImage {
    fn drop(&mut self) {
        let _ = self.device.destroy_image(self.handle);
    }
}

/// RAII wrapper for a compiled `VkShaderEXT` object.
///
/// Shader objects replace pipeline state objects on hardware and drivers that
/// support `VK_EXT_shader_object`.  Obtain via [`Engine::create_shader_object`].
pub struct ShaderObject {
    pub(crate) device: core::Device,
    pub(crate) handle: core::shader_object::ShaderObjectHandle,
}

impl ShaderObject {
    /// The raw handle, used when building `ShaderBinding::ShaderObjects`.
    pub fn handle(&self) -> core::shader_object::ShaderObjectHandle {
        self.handle
    }
}

impl Drop for ShaderObject {
    fn drop(&mut self) {
        self.device.destroy_shader_object(self.handle);
    }
}
