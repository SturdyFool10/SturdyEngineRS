#![allow(non_camel_case_types)]

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use sturdy_engine_core::{
    Access, BackendKind, BufferDesc, BufferHandle, BufferUsage, BufferUse, Device, DeviceDesc,
    Extent3d, Format, Frame, ImageDesc, ImageHandle, ImageUsage, PassDesc, PassWork, QueueType,
    ResourceUse, Result, RgState, ShaderDesc, ShaderHandle, ShaderSource, ShaderStage,
    SubresourceRange,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_device_t {
    pub h: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_image_t {
    pub h: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_buffer_t {
    pub h: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_shader_t {
    pub h: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_frame_t {
    pub h: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_result_t {
    GFX_OK = 0,
    GFX_ERR_INVALID_HANDLE = 1,
    GFX_ERR_UNSUPPORTED = 2,
    GFX_ERR_COMPILE_FAILED = 3,
    GFX_ERR_OUT_OF_MEMORY = 4,
    GFX_ERR_INVALID_INPUT = 5,
    GFX_ERR_BACKEND = 6,
    GFX_ERR_UNKNOWN = 0x7fff_ffff,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_format_t {
    GFX_FORMAT_UNKNOWN = 0,
    GFX_FORMAT_RGBA8_UNORM = 1,
    GFX_FORMAT_BGRA8_UNORM = 2,
    GFX_FORMAT_RGBA16_FLOAT = 3,
    GFX_FORMAT_RGBA32_FLOAT = 4,
    GFX_FORMAT_DEPTH32_FLOAT = 100,
    GFX_FORMAT_DEPTH24_STENCIL8 = 101,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_image_usage_t {
    GFX_IMAGE_USAGE_SAMPLED = 1 << 0,
    GFX_IMAGE_USAGE_STORAGE = 1 << 1,
    GFX_IMAGE_USAGE_RENDER_TARGET = 1 << 2,
    GFX_IMAGE_USAGE_DEPTH_STENCIL = 1 << 3,
    GFX_IMAGE_USAGE_PRESENT = 1 << 4,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_buffer_usage_t {
    GFX_BUFFER_USAGE_COPY_SRC = 1 << 0,
    GFX_BUFFER_USAGE_COPY_DST = 1 << 1,
    GFX_BUFFER_USAGE_UNIFORM = 1 << 2,
    GFX_BUFFER_USAGE_STORAGE = 1 << 3,
    GFX_BUFFER_USAGE_VERTEX = 1 << 4,
    GFX_BUFFER_USAGE_INDEX = 1 << 5,
    GFX_BUFFER_USAGE_INDIRECT = 1 << 6,
    GFX_BUFFER_USAGE_ACCELERATION_STRUCTURE = 1 << 7,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_shader_stage_t {
    GFX_SHADER_STAGE_VERTEX = 0,
    GFX_SHADER_STAGE_FRAGMENT = 1,
    GFX_SHADER_STAGE_COMPUTE = 2,
    GFX_SHADER_STAGE_MESH = 3,
    GFX_SHADER_STAGE_TASK = 4,
    GFX_SHADER_STAGE_RAY_GENERATION = 5,
    GFX_SHADER_STAGE_MISS = 6,
    GFX_SHADER_STAGE_CLOSEST_HIT = 7,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_queue_type_t {
    GFX_QUEUE_GRAPHICS = 0,
    GFX_QUEUE_COMPUTE = 1,
    GFX_QUEUE_TRANSFER = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_access_t {
    GFX_ACCESS_READ = 0,
    GFX_ACCESS_WRITE = 1,
    GFX_ACCESS_READ_WRITE = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum gfx_rg_state_t {
    GFX_RG_STATE_UNDEFINED = 0,
    GFX_RG_STATE_SHADER_READ = 1,
    GFX_RG_STATE_SHADER_WRITE = 2,
    GFX_RG_STATE_RENDER_TARGET = 3,
    GFX_RG_STATE_DEPTH_READ = 4,
    GFX_RG_STATE_DEPTH_WRITE = 5,
    GFX_RG_STATE_COPY_SRC = 6,
    GFX_RG_STATE_COPY_DST = 7,
    GFX_RG_STATE_PRESENT = 8,
    GFX_RG_STATE_UNIFORM_READ = 9,
    GFX_RG_STATE_VERTEX_READ = 10,
    GFX_RG_STATE_INDEX_READ = 11,
    GFX_RG_STATE_INDIRECT_READ = 12,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_caps_t {
    pub supports_raytracing: u32,
    pub supports_mesh_shading: u32,
    pub supports_bindless: u32,
    pub max_mip_levels: u32,
    pub max_frames_in_flight: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_image_desc_t {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub mip_levels: u16,
    pub layers: u16,
    pub samples: u8,
    pub format: u32,
    pub usage_flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct gfx_buffer_desc_t {
    pub size: u64,
    pub usage_flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct gfx_shader_desc_t {
    pub source_utf8: *const c_char,
    pub file_path_utf8: *const c_char,
    pub entry_point_utf8: *const c_char,
    pub stage: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct gfx_subresource_range_t {
    pub base_mip: u16,
    pub mip_count: u16,
    pub base_layer: u16,
    pub layer_count: u16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct gfx_resource_use_t {
    pub image: gfx_image_t,
    pub access: u32,
    pub state: u32,
    pub subresource: gfx_subresource_range_t,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct gfx_buffer_use_t {
    pub buffer: gfx_buffer_t,
    pub access: u32,
    pub state: u32,
    pub offset: u64,
    pub size: u64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct gfx_pass_desc_t {
    pub name_utf8: *const c_char,
    pub queue: u32,
    pub shader: gfx_shader_t,
    pub reads: *const gfx_resource_use_t,
    pub read_count: usize,
    pub writes: *const gfx_resource_use_t,
    pub write_count: usize,
    pub buffer_reads: *const gfx_buffer_use_t,
    pub buffer_read_count: usize,
    pub buffer_writes: *const gfx_buffer_use_t,
    pub buffer_write_count: usize,
}

#[derive(Default)]
struct Registry {
    next: u64,
    devices: HashMap<u64, Device>,
    images: HashMap<u64, RegisteredImage>,
    buffers: HashMap<u64, RegisteredBuffer>,
    shaders: HashMap<u64, RegisteredShader>,
    frames: HashMap<u64, Frame>,
}

#[derive(Copy, Clone)]
struct RegisteredImage {
    device: u64,
    core: ImageHandle,
}

#[derive(Copy, Clone)]
struct RegisteredBuffer {
    device: u64,
    core: BufferHandle,
}

#[derive(Copy, Clone)]
struct RegisteredShader {
    device: u64,
    core: ShaderHandle,
}

impl Registry {
    fn alloc(&mut self) -> u64 {
        self.next = self.next.max(1);
        let handle = self.next;
        self.next += 1;
        handle
    }

    fn insert_device(&mut self, device: Device) -> gfx_device_t {
        let handle = self.alloc();
        self.devices.insert(handle, device);
        gfx_device_t { h: handle }
    }

    fn insert_frame(&mut self, frame: Frame) -> gfx_frame_t {
        let handle = self.alloc();
        self.frames.insert(handle, frame);
        gfx_frame_t { h: handle }
    }
}

static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn ffi_result(result: Result<()>) -> gfx_result_t {
    match result {
        Ok(()) => gfx_result_t::GFX_OK,
        Err(error) => match error.code() {
            1 => gfx_result_t::GFX_ERR_INVALID_HANDLE,
            2 => gfx_result_t::GFX_ERR_UNSUPPORTED,
            3 => gfx_result_t::GFX_ERR_COMPILE_FAILED,
            4 => gfx_result_t::GFX_ERR_OUT_OF_MEMORY,
            5 => gfx_result_t::GFX_ERR_INVALID_INPUT,
            6 => gfx_result_t::GFX_ERR_BACKEND,
            _ => gfx_result_t::GFX_ERR_UNKNOWN,
        },
    }
}

fn no_panic(f: impl FnOnce() -> Result<()>) -> gfx_result_t {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => ffi_result(result),
        Err(_) => gfx_result_t::GFX_ERR_UNKNOWN,
    }
}

fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let value = unsafe { CStr::from_ptr(ptr) };
    Some(value.to_string_lossy().into_owned())
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_create_device(out: *mut gfx_device_t) -> gfx_result_t {
    no_panic(|| {
        if out.is_null() {
            return Err(sturdy_engine_core::Error::InvalidInput(
                "out device pointer is null".into(),
            ));
        }

        let device = Device::create(DeviceDesc {
            backend: BackendKind::Auto,
            validation: cfg!(debug_assertions),
        })?;
        let handle = registry()
            .lock()
            .expect("ffi registry mutex poisoned")
            .insert_device(device);
        unsafe {
            *out = handle;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_destroy_device(dev: gfx_device_t) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        registry.images.retain(|_, image| image.device != dev.h);
        registry.buffers.retain(|_, buffer| buffer.device != dev.h);
        registry.shaders.retain(|_, shader| shader.device != dev.h);
        registry
            .devices
            .remove(&dev.h)
            .map(|_| ())
            .ok_or(sturdy_engine_core::Error::InvalidHandle)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_get_caps(dev: gfx_device_t, out_caps: *mut gfx_caps_t) -> gfx_result_t {
    no_panic(|| {
        if out_caps.is_null() {
            return Err(sturdy_engine_core::Error::InvalidInput(
                "out caps pointer is null".into(),
            ));
        }
        let registry = registry().lock().expect("ffi registry mutex poisoned");
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let caps = device.caps();
        unsafe {
            *out_caps = gfx_caps_t {
                supports_raytracing: caps.supports_raytracing as u32,
                supports_mesh_shading: caps.supports_mesh_shading as u32,
                supports_bindless: caps.supports_bindless as u32,
                max_mip_levels: caps.max_mip_levels,
                max_frames_in_flight: caps.max_frames_in_flight,
            };
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_create_image(
    dev: gfx_device_t,
    desc: *const gfx_image_desc_t,
    out: *mut gfx_image_t,
) -> gfx_result_t {
    no_panic(|| {
        if desc.is_null() || out.is_null() {
            return Err(sturdy_engine_core::Error::InvalidInput(
                "image desc and out pointers must be non-null".into(),
            ));
        }
        let desc = unsafe { *desc };
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let image = device.create_image(ImageDesc {
            extent: Extent3d {
                width: desc.width,
                height: desc.height,
                depth: desc.depth,
            },
            mip_levels: desc.mip_levels,
            layers: desc.layers,
            samples: desc.samples,
            format: ffi_format(desc.format),
            usage: ImageUsage(desc.usage_flags),
        })?;
        let ffi_image = registry.alloc();
        registry.images.insert(
            ffi_image,
            RegisteredImage {
                device: dev.h,
                core: image,
            },
        );
        unsafe {
            *out = gfx_image_t { h: ffi_image };
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_destroy_image(dev: gfx_device_t, image: gfx_image_t) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let registered = *registry
            .images
            .get(&image.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        if registered.device != dev.h {
            return Err(sturdy_engine_core::Error::InvalidHandle);
        }
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        device.destroy_image(registered.core)?;
        registry.images.remove(&image.h);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_create_buffer(
    dev: gfx_device_t,
    desc: *const gfx_buffer_desc_t,
    out: *mut gfx_buffer_t,
) -> gfx_result_t {
    no_panic(|| {
        if desc.is_null() || out.is_null() {
            return Err(sturdy_engine_core::Error::InvalidInput(
                "buffer desc and out pointers must be non-null".into(),
            ));
        }
        let desc = unsafe { *desc };
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let buffer = device.create_buffer(BufferDesc {
            size: desc.size,
            usage: BufferUsage(desc.usage_flags),
        })?;
        let ffi_buffer = registry.alloc();
        registry.buffers.insert(
            ffi_buffer,
            RegisteredBuffer {
                device: dev.h,
                core: buffer,
            },
        );
        unsafe {
            *out = gfx_buffer_t { h: ffi_buffer };
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_destroy_buffer(dev: gfx_device_t, buffer: gfx_buffer_t) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let registered = *registry
            .buffers
            .get(&buffer.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        if registered.device != dev.h {
            return Err(sturdy_engine_core::Error::InvalidHandle);
        }
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        device.destroy_buffer(registered.core)?;
        registry.buffers.remove(&buffer.h);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_create_shader(
    dev: gfx_device_t,
    desc: *const gfx_shader_desc_t,
    out: *mut gfx_shader_t,
) -> gfx_result_t {
    no_panic(|| {
        if desc.is_null() || out.is_null() {
            return Err(sturdy_engine_core::Error::InvalidInput(
                "shader desc and out pointers must be non-null".into(),
            ));
        }

        let desc = unsafe { *desc };
        let source = match (
            cstr_to_string(desc.source_utf8),
            cstr_to_string(desc.file_path_utf8),
        ) {
            (Some(source), _) => ShaderSource::Inline(source),
            (None, Some(path)) => ShaderSource::File(PathBuf::from(path)),
            (None, None) => {
                return Err(sturdy_engine_core::Error::InvalidInput(
                    "shader source or file path is required".into(),
                ));
            }
        };
        let entry_point = cstr_to_string(desc.entry_point_utf8).ok_or_else(|| {
            sturdy_engine_core::Error::InvalidInput("shader entry point is required".into())
        })?;

        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let shader = device.create_shader(ShaderDesc {
            source,
            entry_point,
            stage: ffi_shader_stage(desc.stage)?,
        })?;
        let ffi_shader = registry.alloc();
        registry.shaders.insert(
            ffi_shader,
            RegisteredShader {
                device: dev.h,
                core: shader,
            },
        );
        unsafe {
            *out = gfx_shader_t { h: ffi_shader };
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_destroy_shader(dev: gfx_device_t, shader: gfx_shader_t) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let registered = *registry
            .shaders
            .get(&shader.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        if registered.device != dev.h {
            return Err(sturdy_engine_core::Error::InvalidHandle);
        }
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        device.destroy_shader(registered.core)?;
        registry.shaders.remove(&shader.h);
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_begin_frame(dev: gfx_device_t, out: *mut gfx_frame_t) -> gfx_result_t {
    no_panic(|| {
        if out.is_null() {
            return Err(sturdy_engine_core::Error::InvalidInput(
                "out frame pointer is null".into(),
            ));
        }

        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let device = registry
            .devices
            .get(&dev.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let frame = device.begin_frame()?;
        let handle = registry.insert_frame(frame);
        unsafe {
            *out = handle;
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_destroy_frame(frame: gfx_frame_t) -> gfx_result_t {
    no_panic(|| {
        registry()
            .lock()
            .expect("ffi registry mutex poisoned")
            .frames
            .remove(&frame.h)
            .map(|_| ())
            .ok_or(sturdy_engine_core::Error::InvalidHandle)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_frame_import_image(frame: gfx_frame_t, image: gfx_image_t) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let registered = *registry
            .images
            .get(&image.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let device = registry
            .devices
            .get(&registered.device)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let desc = device.image_desc(registered.core)?;
        let frame = registry
            .frames
            .get_mut(&frame.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        frame.graph_mut(|graph| graph.import_image(registered.core, desc))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_frame_import_buffer(
    frame: gfx_frame_t,
    buffer: gfx_buffer_t,
) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let registered = *registry
            .buffers
            .get(&buffer.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let device = registry
            .devices
            .get(&registered.device)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        let desc = device.buffer_desc(registered.core)?;
        let frame = registry
            .frames
            .get_mut(&frame.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        frame.graph_mut(|graph| graph.import_buffer(registered.core, desc))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_frame_add_pass(
    frame: gfx_frame_t,
    desc: *const gfx_pass_desc_t,
) -> gfx_result_t {
    no_panic(|| {
        if desc.is_null() {
            return Err(sturdy_engine_core::Error::InvalidInput(
                "pass desc pointer is null".into(),
            ));
        }
        let desc = unsafe { *desc };
        let name = cstr_to_string(desc.name_utf8).ok_or_else(|| {
            sturdy_engine_core::Error::InvalidInput("pass name is required".into())
        })?;
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let reads = ffi_resource_uses(&registry, desc.reads, desc.read_count)?;
        let writes = ffi_resource_uses(&registry, desc.writes, desc.write_count)?;
        let buffer_reads = ffi_buffer_uses(&registry, desc.buffer_reads, desc.buffer_read_count)?;
        let buffer_writes =
            ffi_buffer_uses(&registry, desc.buffer_writes, desc.buffer_write_count)?;
        let shader = if desc.shader.h == 0 {
            None
        } else {
            Some(
                registry
                    .shaders
                    .get(&desc.shader.h)
                    .ok_or(sturdy_engine_core::Error::InvalidHandle)?
                    .core,
            )
        };
        let frame = registry
            .frames
            .get_mut(&frame.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        frame.graph_mut(|graph| {
            graph.add_pass(PassDesc {
                name,
                queue: ffi_queue_type(desc.queue)?,
                shader,
                pipeline: None,
                bind_groups: Vec::new(),
                push_constants: None,
                work: PassWork::None,
                reads,
                writes,
                buffer_reads,
                buffer_writes,
                clear_colors: Vec::new(),
                clear_depth: None,
            })
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_frame_flush(frame: gfx_frame_t) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let frame = registry
            .frames
            .get_mut(&frame.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        frame.flush().map(|_| ())
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_frame_present(frame: gfx_frame_t) -> gfx_result_t {
    no_panic(|| {
        let mut registry = registry().lock().expect("ffi registry mutex poisoned");
        let frame = registry
            .frames
            .get_mut(&frame.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        frame.present()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn gfx_frame_wait(frame: gfx_frame_t) -> gfx_result_t {
    no_panic(|| {
        let registry = registry().lock().expect("ffi registry mutex poisoned");
        let frame = registry
            .frames
            .get(&frame.h)
            .ok_or(sturdy_engine_core::Error::InvalidHandle)?;
        frame.wait()
    })
}

fn ffi_format(value: u32) -> Format {
    match value {
        1 => Format::Rgba8Unorm,
        2 => Format::Bgra8Unorm,
        3 => Format::Rgba16Float,
        4 => Format::Rgba32Float,
        100 => Format::Depth32Float,
        101 => Format::Depth24Stencil8,
        _ => Format::Unknown,
    }
}

fn ffi_shader_stage(value: u32) -> Result<ShaderStage> {
    match value {
        0 => Ok(ShaderStage::Vertex),
        1 => Ok(ShaderStage::Fragment),
        2 => Ok(ShaderStage::Compute),
        3 => Ok(ShaderStage::Mesh),
        4 => Ok(ShaderStage::Task),
        5 => Ok(ShaderStage::RayGeneration),
        6 => Ok(ShaderStage::Miss),
        7 => Ok(ShaderStage::ClosestHit),
        _ => Err(sturdy_engine_core::Error::InvalidInput(
            "unknown shader stage".into(),
        )),
    }
}

fn ffi_resource_uses(
    registry: &Registry,
    ptr: *const gfx_resource_use_t,
    count: usize,
) -> Result<Vec<ResourceUse>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err(sturdy_engine_core::Error::InvalidInput(
            "resource use array pointer is null".into(),
        ));
    }

    let uses = unsafe { std::slice::from_raw_parts(ptr, count) };
    uses.iter()
        .map(|usage| {
            Ok(ResourceUse {
                image: registry
                    .images
                    .get(&usage.image.h)
                    .ok_or(sturdy_engine_core::Error::InvalidHandle)?
                    .core,
                access: ffi_access(usage.access)?,
                state: ffi_rg_state(usage.state)?,
                subresource: SubresourceRange {
                    base_mip: usage.subresource.base_mip,
                    mip_count: usage.subresource.mip_count,
                    base_layer: usage.subresource.base_layer,
                    layer_count: usage.subresource.layer_count,
                },
            })
        })
        .collect()
}

fn ffi_buffer_uses(
    registry: &Registry,
    ptr: *const gfx_buffer_use_t,
    count: usize,
) -> Result<Vec<BufferUse>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if ptr.is_null() {
        return Err(sturdy_engine_core::Error::InvalidInput(
            "buffer use array pointer is null".into(),
        ));
    }

    let uses = unsafe { std::slice::from_raw_parts(ptr, count) };
    uses.iter()
        .map(|usage| {
            Ok(BufferUse {
                buffer: registry
                    .buffers
                    .get(&usage.buffer.h)
                    .ok_or(sturdy_engine_core::Error::InvalidHandle)?
                    .core,
                access: ffi_access(usage.access)?,
                state: ffi_rg_state(usage.state)?,
                offset: usage.offset,
                size: usage.size,
            })
        })
        .collect()
}

fn ffi_queue_type(value: u32) -> Result<QueueType> {
    match value {
        0 => Ok(QueueType::Graphics),
        1 => Ok(QueueType::Compute),
        2 => Ok(QueueType::Transfer),
        _ => Err(sturdy_engine_core::Error::InvalidInput(
            "unknown queue type".into(),
        )),
    }
}

fn ffi_access(value: u32) -> Result<Access> {
    match value {
        0 => Ok(Access::Read),
        1 => Ok(Access::Write),
        2 => Ok(Access::ReadWrite),
        _ => Err(sturdy_engine_core::Error::InvalidInput(
            "unknown resource access".into(),
        )),
    }
}

fn ffi_rg_state(value: u32) -> Result<RgState> {
    match value {
        0 => Ok(RgState::Undefined),
        1 => Ok(RgState::ShaderRead),
        2 => Ok(RgState::ShaderWrite),
        3 => Ok(RgState::RenderTarget),
        4 => Ok(RgState::DepthRead),
        5 => Ok(RgState::DepthWrite),
        6 => Ok(RgState::CopySrc),
        7 => Ok(RgState::CopyDst),
        8 => Ok(RgState::Present),
        9 => Ok(RgState::UniformRead),
        10 => Ok(RgState::VertexRead),
        11 => Ok(RgState::IndexRead),
        12 => Ok(RgState::IndirectRead),
        _ => Err(sturdy_engine_core::Error::InvalidInput(
            "unknown render graph state".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::ptr;

    use super::*;

    #[test]
    fn ffi_declares_and_flushes_a_frame() {
        let mut device = gfx_device_t::default();
        assert_eq!(gfx_create_device(&mut device), gfx_result_t::GFX_OK);

        let image_desc = gfx_image_desc_t {
            width: 64,
            height: 64,
            depth: 1,
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: gfx_format_t::GFX_FORMAT_RGBA8_UNORM as u32,
            usage_flags: gfx_image_usage_t::GFX_IMAGE_USAGE_RENDER_TARGET as u32
                | gfx_image_usage_t::GFX_IMAGE_USAGE_SAMPLED as u32,
        };
        let mut image = gfx_image_t::default();
        assert_eq!(
            gfx_create_image(device, &image_desc, &mut image),
            gfx_result_t::GFX_OK
        );
        let buffer_desc = gfx_buffer_desc_t {
            size: 256,
            usage_flags: gfx_buffer_usage_t::GFX_BUFFER_USAGE_UNIFORM as u32,
        };
        let mut buffer = gfx_buffer_t::default();
        assert_eq!(
            gfx_create_buffer(device, &buffer_desc, &mut buffer),
            gfx_result_t::GFX_OK
        );

        let mut frame = gfx_frame_t::default();
        assert_eq!(gfx_begin_frame(device, &mut frame), gfx_result_t::GFX_OK);
        assert_eq!(gfx_frame_import_image(frame, image), gfx_result_t::GFX_OK);
        assert_eq!(gfx_frame_import_buffer(frame, buffer), gfx_result_t::GFX_OK);

        let name = CString::new("clear").unwrap();
        let write = gfx_resource_use_t {
            image,
            access: gfx_access_t::GFX_ACCESS_WRITE as u32,
            state: gfx_rg_state_t::GFX_RG_STATE_RENDER_TARGET as u32,
            subresource: gfx_subresource_range_t {
                base_mip: 0,
                mip_count: 1,
                base_layer: 0,
                layer_count: 1,
            },
        };
        let pass = gfx_pass_desc_t {
            name_utf8: name.as_ptr(),
            queue: gfx_queue_type_t::GFX_QUEUE_GRAPHICS as u32,
            shader: gfx_shader_t::default(),
            reads: ptr::null(),
            read_count: 0,
            writes: &write,
            write_count: 1,
            buffer_reads: ptr::null(),
            buffer_read_count: 0,
            buffer_writes: ptr::null(),
            buffer_write_count: 0,
        };
        assert_eq!(gfx_frame_add_pass(frame, &pass), gfx_result_t::GFX_OK);
        assert_eq!(gfx_frame_flush(frame), gfx_result_t::GFX_OK);
        assert_eq!(gfx_frame_wait(frame), gfx_result_t::GFX_OK);

        assert_eq!(gfx_destroy_frame(frame), gfx_result_t::GFX_OK);
        assert_eq!(gfx_destroy_buffer(device, buffer), gfx_result_t::GFX_OK);
        assert_eq!(gfx_destroy_image(device, image), gfx_result_t::GFX_OK);
        assert_eq!(gfx_destroy_device(device), gfx_result_t::GFX_OK);
    }
}
