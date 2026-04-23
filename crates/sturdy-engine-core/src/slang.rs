use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::sync::OnceLock;
use std::{path::PathBuf, process::Command};

use crate::{
    BindingKind, CanonicalBinding, CanonicalGroupLayout, CanonicalPipelineLayout,
    CompiledShaderArtifact, Error, Result, ShaderDesc, ShaderReflection, ShaderSource, ShaderStage,
    ShaderTarget, StageMask, UpdateRate,
};

#[path = "slang/spirv_push_constants.rs"]
mod spirv_push_constants;

#[cfg(not(target_arch = "wasm32"))]
mod sys {
    use std::ffi::{c_char, c_int, c_uint};

    pub type SlangResult = i32;
    pub type SlangInt = i64;
    pub type SlangUInt = u64;
    pub type SlangBindingType = u32;
    pub type SlangParameterCategory = u32;

    pub const SLANG_OK: SlangResult = 0;

    // SlangCompileTarget enum ordinals (verified against slang.h from libslang-compiler 2026.x)
    // SLANG_TARGET_UNKNOWN=0, SLANG_TARGET_NONE=1, SLANG_GLSL=2,
    // SLANG_GLSL_VULKAN_DEPRECATED=3, SLANG_GLSL_VULKAN_ONE_DESC_DEPRECATED=4, SLANG_HLSL=5
    pub const SLANG_SPIRV: c_int = 6;
    // SLANG_SPIRV_ASM=7, SLANG_DXBC=8, SLANG_DXBC_ASM=9
    pub const SLANG_DXIL: c_int = 10;
    // SLANG_DXIL_ASM=11, ... SLANG_CPP_PYTORCH_BINDING=23
    pub const SLANG_METAL: c_int = 24;

    // SlangSourceLanguage enum ordinals
    pub const SLANG_SOURCE_LANGUAGE_SLANG: c_int = 1;

    // SlangStage enum ordinals
    pub const SLANG_STAGE_VERTEX: c_int = 1;
    pub const SLANG_STAGE_FRAGMENT: c_int = 5;
    pub const SLANG_STAGE_COMPUTE: c_int = 6;
    pub const SLANG_STAGE_RAY_GENERATION: c_int = 7;
    pub const SLANG_STAGE_CLOSEST_HIT: c_int = 10;
    pub const SLANG_STAGE_MISS: c_int = 11;
    pub const SLANG_STAGE_MESH: c_int = 13;
    pub const SLANG_STAGE_AMPLIFICATION: c_int = 14;

    // SlangBindingType enum ordinals
    pub const SLANG_BINDING_TYPE_SAMPLER: SlangBindingType = 1;
    pub const SLANG_BINDING_TYPE_TEXTURE: SlangBindingType = 2;
    pub const SLANG_BINDING_TYPE_CONSTANT_BUFFER: SlangBindingType = 3;
    pub const SLANG_BINDING_TYPE_TYPED_BUFFER: SlangBindingType = 5;
    pub const SLANG_BINDING_TYPE_RAW_BUFFER: SlangBindingType = 6;
    pub const SLANG_BINDING_TYPE_RAY_TRACING_ACCELERATION_STRUCTURE: SlangBindingType = 10;
    pub const SLANG_BINDING_TYPE_PUSH_CONSTANT: SlangBindingType = 14;
    pub const SLANG_BINDING_TYPE_MUTABLE_FLAG: SlangBindingType = 0x100;
    pub const SLANG_PARAMETER_CATEGORY_DESCRIPTOR_TABLE_SLOT: SlangParameterCategory = 9;
    pub const SLANG_PARAMETER_CATEGORY_UNIFORM: SlangParameterCategory = 8;
    pub const SLANG_PARAMETER_CATEGORY_PUSH_CONSTANT_BUFFER: SlangParameterCategory = 11;

    // Opaque types
    #[repr(C)]
    pub struct SlangSession {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct SlangCompileRequest {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct SlangReflection {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct SlangReflectionVariableLayout {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct SlangReflectionTypeLayout {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct SlangReflectionVariable {
        _opaque: [u8; 0],
    }
    #[repr(C)]
    pub struct SlangReflectionEntryPoint {
        _opaque: [u8; 0],
    }

    #[link(name = "slang-compiler")]
    unsafe extern "C" {
        pub fn spCreateSession(deprecated: *const c_char) -> *mut SlangSession;
        pub fn spCreateCompileRequest(session: *mut SlangSession) -> *mut SlangCompileRequest;
        pub fn spDestroyCompileRequest(request: *mut SlangCompileRequest);
        pub fn spAddCodeGenTarget(request: *mut SlangCompileRequest, target: c_int) -> c_int;
        pub fn spAddTranslationUnit(
            request: *mut SlangCompileRequest,
            language: c_int,
            name: *const c_char,
        ) -> c_int;
        pub fn spAddTranslationUnitSourceFile(
            request: *mut SlangCompileRequest,
            tu_index: c_int,
            path: *const c_char,
        );
        pub fn spAddTranslationUnitSourceString(
            request: *mut SlangCompileRequest,
            tu_index: c_int,
            path: *const c_char,
            source: *const c_char,
        );
        pub fn spAddEntryPoint(
            request: *mut SlangCompileRequest,
            tu_index: c_int,
            name: *const c_char,
            stage: c_int,
        ) -> c_int;
        pub fn spCompile(request: *mut SlangCompileRequest) -> SlangResult;
        pub fn spGetDiagnosticOutput(request: *mut SlangCompileRequest) -> *const c_char;
        pub fn spGetReflection(request: *mut SlangCompileRequest) -> *mut SlangReflection;
        pub fn spGetEntryPointCode(
            request: *mut SlangCompileRequest,
            entry_point_index: c_int,
            out_size: *mut usize,
        ) -> *const u8;

        // Top-level parameters
        pub fn spReflection_GetParameterCount(reflection: *mut SlangReflection) -> c_uint;
        pub fn spReflection_GetParameterByIndex(
            reflection: *mut SlangReflection,
            index: c_uint,
        ) -> *mut SlangReflectionVariableLayout;
        pub fn spReflection_getEntryPointCount(reflection: *mut SlangReflection) -> SlangUInt;
        pub fn spReflection_getEntryPointByIndex(
            reflection: *mut SlangReflection,
            index: SlangUInt,
        ) -> *mut SlangReflectionEntryPoint;

        // Entry point
        pub fn spReflectionEntryPoint_getName(ep: *mut SlangReflectionEntryPoint) -> *const c_char;

        // Parameter/variable layout
        pub fn spReflectionParameter_GetBindingSpace(
            param: *mut SlangReflectionVariableLayout,
        ) -> c_uint;
        pub fn spReflectionVariableLayout_GetVariable(
            var_layout: *mut SlangReflectionVariableLayout,
        ) -> *mut SlangReflectionVariable;
        pub fn spReflectionVariableLayout_GetTypeLayout(
            var_layout: *mut SlangReflectionVariableLayout,
        ) -> *mut SlangReflectionTypeLayout;

        // Variable
        pub fn spReflectionVariable_GetName(var: *mut SlangReflectionVariable) -> *const c_char;

        // Variable layout — category offset
        pub fn spReflectionVariableLayout_GetOffset(
            var_layout: *mut SlangReflectionVariableLayout,
            category: SlangParameterCategory,
        ) -> usize;

        // Type layout — binding ranges
        pub fn spReflectionTypeLayout_getBindingRangeCount(
            type_layout: *mut SlangReflectionTypeLayout,
        ) -> SlangInt;
        pub fn spReflectionTypeLayout_getBindingRangeType(
            type_layout: *mut SlangReflectionTypeLayout,
            index: SlangInt,
        ) -> SlangBindingType;
        pub fn spReflectionTypeLayout_getBindingRangeBindingCount(
            type_layout: *mut SlangReflectionTypeLayout,
            index: SlangInt,
        ) -> SlangInt;
        pub fn spReflectionTypeLayout_getBindingRangeDescriptorSetIndex(
            type_layout: *mut SlangReflectionTypeLayout,
            index: SlangInt,
        ) -> SlangInt;
        pub fn spReflectionTypeLayout_GetSize(
            type_layout: *mut SlangReflectionTypeLayout,
            category: SlangParameterCategory,
        ) -> usize;
        pub fn spReflectionTypeLayout_GetParameterCategory(
            type_layout: *mut SlangReflectionTypeLayout,
        ) -> SlangParameterCategory;
        pub fn spReflectionTypeLayout_GetCategoryCount(
            type_layout: *mut SlangReflectionTypeLayout,
        ) -> c_uint;
        pub fn spReflectionTypeLayout_GetCategoryByIndex(
            type_layout: *mut SlangReflectionTypeLayout,
            index: c_uint,
        ) -> SlangParameterCategory;
        pub fn spReflectionTypeLayout_GetFieldCount(
            type_layout: *mut SlangReflectionTypeLayout,
        ) -> c_uint;
        pub fn spReflectionTypeLayout_GetFieldByIndex(
            type_layout: *mut SlangReflectionTypeLayout,
            index: c_uint,
        ) -> *mut SlangReflectionVariableLayout;
        pub fn spReflectionTypeLayout_GetElementTypeLayout(
            type_layout: *mut SlangReflectionTypeLayout,
        ) -> *mut SlangReflectionTypeLayout;
        pub fn spReflectionTypeLayout_getBindingRangeLeafTypeLayout(
            type_layout: *mut SlangReflectionTypeLayout,
            index: SlangInt,
        ) -> *mut SlangReflectionTypeLayout;
    }
}

#[cfg(not(target_arch = "wasm32"))]
static GLOBAL_SESSION: OnceLock<std::sync::Mutex<usize>> = OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
fn with_session<T>(f: impl FnOnce(*mut sys::SlangSession) -> T) -> T {
    let lock = GLOBAL_SESSION.get_or_init(|| {
        let s = unsafe { sys::spCreateSession(std::ptr::null()) };
        assert!(!s.is_null(), "slang: spCreateSession returned null");
        std::sync::Mutex::new(s as usize)
    });
    let guard = lock.lock().expect("slang session mutex poisoned");
    f(*guard as *mut sys::SlangSession)
}

#[cfg(not(target_arch = "wasm32"))]
struct CompileRequestGuard(*mut sys::SlangCompileRequest);

#[cfg(not(target_arch = "wasm32"))]
impl Drop for CompileRequestGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { sys::spDestroyCompileRequest(self.0) };
        }
    }
}

/// Reflect the pipeline layout of a Slang shader source.
///
/// Returns an empty layout for `ShaderSource::Spirv` (no source available to reflect).
/// Pass `ray_tracing_enabled = false` to strip acceleration structure bindings from the layout.
#[cfg(not(target_arch = "wasm32"))]
pub fn reflect_pipeline_layout(desc: &ShaderDesc) -> Result<ShaderReflection> {
    reflect_pipeline_layout_inner(desc, true)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn reflect_pipeline_layout_with_caps(
    desc: &ShaderDesc,
    ray_tracing_enabled: bool,
) -> Result<ShaderReflection> {
    reflect_pipeline_layout_inner(desc, ray_tracing_enabled)
}

#[cfg(not(target_arch = "wasm32"))]
fn reflect_pipeline_layout_inner(
    desc: &ShaderDesc,
    ray_tracing_enabled: bool,
) -> Result<ShaderReflection> {
    if matches!(desc.source, ShaderSource::Spirv(_)) {
        return Ok(ShaderReflection::default());
    }

    // Prepare CStrings before acquiring the session lock
    let path_or_source_cstr = match &desc.source {
        ShaderSource::File(path) => {
            let s = path.to_str().ok_or_else(|| {
                Error::InvalidInput("shader file path contains non-UTF-8 characters".into())
            })?;
            CString::new(s)
                .map_err(|_| Error::InvalidInput("shader file path contains null bytes".into()))?
        }
        ShaderSource::Inline(src) => CString::new(src.as_str())
            .map_err(|_| Error::InvalidInput("shader source contains null bytes".into()))?,
        ShaderSource::Spirv(_) | ShaderSource::Dxil(_) | ShaderSource::Msl(_) => unreachable!(),
    };
    let entry_cstr = CString::new(desc.entry_point.as_str())
        .map_err(|_| Error::InvalidInput("entry point contains null bytes".into()))?;
    let stage = shader_stage_to_slang(desc.stage);
    let engine_stage = desc.stage;
    let is_file = matches!(desc.source, ShaderSource::File(_));

    // Hold the session lock for the entire compile request lifetime
    with_session(|session| unsafe {
        let request = sys::spCreateCompileRequest(session);
        if request.is_null() {
            return Err(Error::Backend(
                "slang: spCreateCompileRequest returned null".into(),
            ));
        }
        let _guard = CompileRequestGuard(request);

        sys::spAddCodeGenTarget(request, sys::SLANG_SPIRV);
        let tu =
            sys::spAddTranslationUnit(request, sys::SLANG_SOURCE_LANGUAGE_SLANG, std::ptr::null());

        if is_file {
            sys::spAddTranslationUnitSourceFile(request, tu, path_or_source_cstr.as_ptr());
        } else {
            let inline_path = std::ffi::CStr::from_bytes_with_nul(b"<inline>\0").unwrap();
            sys::spAddTranslationUnitSourceString(
                request,
                tu,
                inline_path.as_ptr(),
                path_or_source_cstr.as_ptr(),
            );
        }

        sys::spAddEntryPoint(request, tu, entry_cstr.as_ptr(), stage);

        let result = sys::spCompile(request);
        if result < sys::SLANG_OK {
            let diag_ptr = sys::spGetDiagnosticOutput(request);
            let msg = if diag_ptr.is_null() {
                "unknown error".into()
            } else {
                CStr::from_ptr(diag_ptr).to_string_lossy().into_owned()
            };
            return Err(Error::CompileFailed(msg));
        }

        let reflection = sys::spGetReflection(request);
        if reflection.is_null() {
            return Ok(ShaderReflection::default());
        }

        let mut layout = extract_layout(reflection, engine_stage, ray_tracing_enabled)?;
        let mut code_size: usize = 0;
        let code_ptr = sys::spGetEntryPointCode(request, 0, &mut code_size);
        if !code_ptr.is_null() && code_size != 0 {
            let code_bytes = std::slice::from_raw_parts(code_ptr, code_size);
            merge_spirv_push_constant_reflection(&mut layout, code_bytes, engine_stage)?;
        }

        let ep_count = sys::spReflection_getEntryPointCount(reflection);
        let mut entry_points = Vec::with_capacity(ep_count as usize);
        for i in 0..ep_count {
            let ep = sys::spReflection_getEntryPointByIndex(reflection, i);
            if ep.is_null() {
                continue;
            }
            let name_ptr = sys::spReflectionEntryPoint_getName(ep);
            if !name_ptr.is_null() {
                entry_points.push(CStr::from_ptr(name_ptr).to_string_lossy().into_owned());
            }
        }

        Ok(ShaderReflection {
            layout,
            entry_points,
        })
    })
}

#[cfg(target_arch = "wasm32")]
pub fn reflect_pipeline_layout(_desc: &ShaderDesc) -> Result<ShaderReflection> {
    Ok(ShaderReflection::default())
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn extract_layout(
    reflection: *mut sys::SlangReflection,
    stage: ShaderStage,
    ray_tracing_enabled: bool,
) -> Result<CanonicalPipelineLayout> {
    let param_count = unsafe { sys::spReflection_GetParameterCount(reflection) };
    let stage_mask = shader_stage_to_mask(stage);

    // BTreeMap keeps groups sorted by set index
    let mut groups: BTreeMap<u32, (String, Vec<CanonicalBinding>)> = BTreeMap::new();
    let mut push_constants_bytes = 0;

    for i in 0..param_count {
        let param = unsafe { sys::spReflection_GetParameterByIndex(reflection, i) };
        if param.is_null() {
            continue;
        }

        let var = unsafe { sys::spReflectionVariableLayout_GetVariable(param) };
        let name = if var.is_null() {
            String::new()
        } else {
            let ptr = unsafe { sys::spReflectionVariable_GetName(var) };
            if ptr.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(ptr) }
                    .to_string_lossy()
                    .into_owned()
            }
        };

        let binding_space = unsafe { sys::spReflectionParameter_GetBindingSpace(param) };
        let base_binding_slot = unsafe {
            sys::spReflectionVariableLayout_GetOffset(
                param,
                sys::SLANG_PARAMETER_CATEGORY_DESCRIPTOR_TABLE_SLOT,
            )
        } as u32;

        let type_layout = unsafe { sys::spReflectionVariableLayout_GetTypeLayout(param) };
        if type_layout.is_null() {
            continue;
        }
        if unsafe {
            type_layout_has_category(
                type_layout,
                sys::SLANG_PARAMETER_CATEGORY_PUSH_CONSTANT_BUFFER,
            )
        } {
            let size = unsafe { push_constant_size_for_type_layout(type_layout) }?;
            push_constants_bytes = push_constants_bytes.max(size);
        }

        let range_count = unsafe { sys::spReflectionTypeLayout_getBindingRangeCount(type_layout) };

        for r in 0..range_count {
            let binding_type =
                unsafe { sys::spReflectionTypeLayout_getBindingRangeType(type_layout, r) };
            if binding_type & !sys::SLANG_BINDING_TYPE_MUTABLE_FLAG
                == sys::SLANG_BINDING_TYPE_PUSH_CONSTANT
            {
                let size = unsafe { push_constant_size_for_type_layout(type_layout) }?;
                push_constants_bytes = push_constants_bytes.max(size);
                continue;
            }
            let set_offset = unsafe {
                sys::spReflectionTypeLayout_getBindingRangeDescriptorSetIndex(type_layout, r)
            };
            let raw_count =
                unsafe { sys::spReflectionTypeLayout_getBindingRangeBindingCount(type_layout, r) };
            // SLANG_UNBOUNDED_SIZE (~size_t(0)) is returned as -1 when the SlangInt is i64.
            let count: u32 = if raw_count < 0 {
                u32::MAX
            } else {
                raw_count as u32
            };

            let kind = match slang_binding_type_to_kind(binding_type) {
                Some(k) => k,
                None => continue,
            };
            if kind == BindingKind::AccelerationStructure && !ray_tracing_enabled {
                continue;
            }

            let set_index = binding_space + set_offset as u32;
            // For a parameter with one binding range its slot IS base_binding_slot.
            // For a parameter with multiple ranges, subsequent ranges follow sequentially.
            let binding_slot = base_binding_slot + r as u32;
            let binding_name = if range_count == 1 {
                name.clone()
            } else {
                format!("{name}.{r}")
            };

            let group = groups
                .entry(set_index)
                .or_insert_with(|| (format!("set{set_index}"), Vec::new()));
            // count == u32::MAX means the array is unsized (bindless/unbounded descriptor array).
            let resolved_count = if count == u32::MAX {
                u32::MAX
            } else {
                count.max(1)
            };
            group.1.push(CanonicalBinding {
                path: binding_name,
                kind,
                count: resolved_count,
                stage_mask,
                update_rate: binding_space_to_update_rate(set_index),
                binding: binding_slot,
            });
        }
    }

    Ok(CanonicalPipelineLayout {
        groups: groups
            .into_values()
            .map(|(name, bindings)| CanonicalGroupLayout { name, bindings })
            .collect(),
        push_constants_bytes,
        push_constants_stage_mask: if push_constants_bytes == 0 {
            StageMask::default()
        } else {
            stage_mask
        },
    })
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn type_layout_has_category(
    type_layout: *mut sys::SlangReflectionTypeLayout,
    category: sys::SlangParameterCategory,
) -> bool {
    if unsafe { sys::spReflectionTypeLayout_GetParameterCategory(type_layout) } == category {
        return true;
    }
    let count = unsafe { sys::spReflectionTypeLayout_GetCategoryCount(type_layout) };
    (0..count).any(|index| unsafe {
        sys::spReflectionTypeLayout_GetCategoryByIndex(type_layout, index) == category
    })
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn push_constant_size_for_type_layout(
    type_layout: *mut sys::SlangReflectionTypeLayout,
) -> Result<u32> {
    let size = unsafe { push_constant_size_for_type_layout_inner(type_layout, 0) };
    u32::try_from(size)
        .map_err(|_| Error::InvalidInput("reflected push constant size exceeds u32 range".into()))
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn push_constant_size_for_type_layout_inner(
    type_layout: *mut sys::SlangReflectionTypeLayout,
    depth: usize,
) -> usize {
    if type_layout.is_null() || depth > 8 {
        return 0;
    }

    let push_size = unsafe {
        sys::spReflectionTypeLayout_GetSize(
            type_layout,
            sys::SLANG_PARAMETER_CATEGORY_PUSH_CONSTANT_BUFFER,
        )
    };
    let uniform_size = unsafe {
        sys::spReflectionTypeLayout_GetSize(type_layout, sys::SLANG_PARAMETER_CATEGORY_UNIFORM)
    };
    let mut size = push_size.max(uniform_size);

    let element_layout = unsafe { sys::spReflectionTypeLayout_GetElementTypeLayout(type_layout) };
    size = size.max(unsafe { push_constant_size_for_type_layout_inner(element_layout, depth + 1) });

    let field_count = unsafe { sys::spReflectionTypeLayout_GetFieldCount(type_layout) };
    for field_index in 0..field_count {
        let field =
            unsafe { sys::spReflectionTypeLayout_GetFieldByIndex(type_layout, field_index) };
        if field.is_null() {
            continue;
        }
        let field_type_layout = unsafe { sys::spReflectionVariableLayout_GetTypeLayout(field) };
        size = size
            .max(unsafe { push_constant_size_for_type_layout_inner(field_type_layout, depth + 1) });
    }

    let range_count = unsafe { sys::spReflectionTypeLayout_getBindingRangeCount(type_layout) };
    for range_index in 0..range_count {
        let leaf_layout = unsafe {
            sys::spReflectionTypeLayout_getBindingRangeLeafTypeLayout(type_layout, range_index)
        };
        size =
            size.max(unsafe { push_constant_size_for_type_layout_inner(leaf_layout, depth + 1) });
    }

    size
}

#[cfg(not(target_arch = "wasm32"))]
fn merge_spirv_push_constant_reflection(
    layout: &mut CanonicalPipelineLayout,
    code_bytes: &[u8],
    stage: ShaderStage,
) -> Result<()> {
    let words = spirv_words_from_bytes(code_bytes)?;
    let push_constants = spirv_push_constants::reflect_spirv_push_constants(&words);
    if push_constants.bytes == 0 {
        return Ok(());
    }

    layout.push_constants_bytes = layout.push_constants_bytes.max(push_constants.bytes);
    layout.push_constants_stage_mask |= shader_stage_to_mask(stage);
    for group in &mut layout.groups {
        group
            .bindings
            .retain(|binding| !push_constants.names.contains(&binding.path));
    }
    layout.groups.retain(|group| !group.bindings.is_empty());
    Ok(())
}

/// Compile a Slang source and reflect its pipeline layout in a single pass.
///
/// Pass `target` to select the output IR (`Spirv` for Vulkan, `Dxil` for D3D12, `Msl` for Metal).
/// Pre-compiled sources (`Spirv`, `Dxil`, `Msl`) are returned unchanged with an empty reflection.
/// `File` and `Inline` sources are compiled via the Slang C API.
#[cfg(not(target_arch = "wasm32"))]
pub fn compile_and_reflect(
    desc: &ShaderDesc,
    target: ShaderTarget,
) -> Result<(ShaderDesc, ShaderReflection)> {
    if matches!(
        desc.source,
        ShaderSource::Spirv(_) | ShaderSource::Dxil(_) | ShaderSource::Msl(_)
    ) {
        return Ok((desc.clone(), ShaderReflection::default()));
    }

    let path_or_source_cstr = match &desc.source {
        ShaderSource::File(path) => {
            let s = path.to_str().ok_or_else(|| {
                Error::InvalidInput("shader file path contains non-UTF-8 characters".into())
            })?;
            CString::new(s)
                .map_err(|_| Error::InvalidInput("shader file path contains null bytes".into()))?
        }
        ShaderSource::Inline(src) => CString::new(src.as_str())
            .map_err(|_| Error::InvalidInput("shader source contains null bytes".into()))?,
        ShaderSource::Spirv(_) | ShaderSource::Dxil(_) | ShaderSource::Msl(_) => unreachable!(),
    };
    let entry_cstr = CString::new(desc.entry_point.as_str())
        .map_err(|_| Error::InvalidInput("entry point contains null bytes".into()))?;
    let stage = shader_stage_to_slang(desc.stage);
    let engine_stage = desc.stage;
    let is_file = matches!(desc.source, ShaderSource::File(_));
    let slang_target = shader_target_to_slang(target);

    with_session(|session| unsafe {
        let request = sys::spCreateCompileRequest(session);
        if request.is_null() {
            return Err(Error::Backend(
                "slang: spCreateCompileRequest returned null".into(),
            ));
        }
        let _guard = CompileRequestGuard(request);

        sys::spAddCodeGenTarget(request, slang_target);
        let tu =
            sys::spAddTranslationUnit(request, sys::SLANG_SOURCE_LANGUAGE_SLANG, std::ptr::null());

        if is_file {
            sys::spAddTranslationUnitSourceFile(request, tu, path_or_source_cstr.as_ptr());
        } else {
            let inline_path = std::ffi::CStr::from_bytes_with_nul(b"<inline>\0").unwrap();
            sys::spAddTranslationUnitSourceString(
                request,
                tu,
                inline_path.as_ptr(),
                path_or_source_cstr.as_ptr(),
            );
        }

        sys::spAddEntryPoint(request, tu, entry_cstr.as_ptr(), stage);

        let result = sys::spCompile(request);
        if result < sys::SLANG_OK {
            let diag_ptr = sys::spGetDiagnosticOutput(request);
            let msg = if diag_ptr.is_null() {
                "unknown slang error".into()
            } else {
                CStr::from_ptr(diag_ptr).to_string_lossy().into_owned()
            };
            return Err(Error::CompileFailed(msg));
        }

        // Extract compiled bytes for entry point 0
        let mut code_size: usize = 0;
        let code_ptr = sys::spGetEntryPointCode(request, 0, &mut code_size);
        if code_ptr.is_null() || code_size == 0 {
            return Err(Error::CompileFailed(
                "slang: spGetEntryPointCode returned empty output".into(),
            ));
        }
        let code_bytes = std::slice::from_raw_parts(code_ptr, code_size).to_vec();

        // Reflect pipeline layout (works regardless of output target)
        let reflection_ptr = sys::spGetReflection(request);
        let (layout, entry_points) = if reflection_ptr.is_null() {
            (CanonicalPipelineLayout::default(), Vec::new())
        } else {
            let mut layout = extract_layout(reflection_ptr, engine_stage, true)?;
            if target == ShaderTarget::Spirv {
                merge_spirv_push_constant_reflection(&mut layout, &code_bytes, engine_stage)?;
            }
            let ep_count = sys::spReflection_getEntryPointCount(reflection_ptr);
            let mut eps = Vec::with_capacity(ep_count as usize);
            for i in 0..ep_count {
                let ep = sys::spReflection_getEntryPointByIndex(reflection_ptr, i);
                if ep.is_null() {
                    continue;
                }
                let name_ptr = sys::spReflectionEntryPoint_getName(ep);
                if !name_ptr.is_null() {
                    eps.push(CStr::from_ptr(name_ptr).to_string_lossy().into_owned());
                }
            }
            (layout, eps)
        };

        let compiled_source = match target {
            ShaderTarget::Spirv => ShaderSource::Spirv(spirv_words_from_bytes(&code_bytes)?),
            ShaderTarget::Dxil => ShaderSource::Dxil(code_bytes),
            ShaderTarget::Msl => ShaderSource::Msl(code_bytes),
        };

        let compiled_desc = ShaderDesc {
            source: compiled_source,
            entry_point: desc.entry_point.clone(),
            stage: desc.stage,
        };
        Ok((
            compiled_desc,
            ShaderReflection {
                layout,
                entry_points,
            },
        ))
    })
}

#[cfg(target_arch = "wasm32")]
pub fn compile_and_reflect(
    desc: &ShaderDesc,
    _target: ShaderTarget,
) -> Result<(ShaderDesc, ShaderReflection)> {
    if matches!(
        desc.source,
        ShaderSource::Spirv(_) | ShaderSource::Dxil(_) | ShaderSource::Msl(_)
    ) {
        return Ok((desc.clone(), ShaderReflection::default()));
    }
    Err(Error::Unsupported(
        "Slang source compilation is not available on wasm32",
    ))
}

/// Maps descriptor set index to engine update rate by convention:
/// set 0 = Frame, set 1 = Pass, set 2 = Material, set 3 = Draw.
fn binding_space_to_update_rate(set_index: u32) -> UpdateRate {
    match set_index {
        0 => UpdateRate::Frame,
        1 => UpdateRate::Pass,
        2 => UpdateRate::Material,
        3 => UpdateRate::Draw,
        _ => UpdateRate::Frame,
    }
}

fn slang_binding_type_to_kind(bt: u32) -> Option<BindingKind> {
    let base = bt & !sys::SLANG_BINDING_TYPE_MUTABLE_FLAG;
    let mutable = (bt & sys::SLANG_BINDING_TYPE_MUTABLE_FLAG) != 0;
    match base {
        x if x == sys::SLANG_BINDING_TYPE_SAMPLER => Some(BindingKind::Sampler),
        x if x == sys::SLANG_BINDING_TYPE_TEXTURE => {
            if mutable {
                Some(BindingKind::StorageImage)
            } else {
                Some(BindingKind::SampledImage)
            }
        }
        x if x == sys::SLANG_BINDING_TYPE_CONSTANT_BUFFER => Some(BindingKind::UniformBuffer),
        x if x == sys::SLANG_BINDING_TYPE_TYPED_BUFFER => Some(BindingKind::StorageBuffer),
        x if x == sys::SLANG_BINDING_TYPE_RAW_BUFFER => Some(BindingKind::StorageBuffer),
        x if x == sys::SLANG_BINDING_TYPE_RAY_TRACING_ACCELERATION_STRUCTURE => {
            Some(BindingKind::AccelerationStructure)
        }
        _ => None,
    }
}

fn shader_target_to_slang(target: ShaderTarget) -> i32 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match target {
            ShaderTarget::Spirv => sys::SLANG_SPIRV,
            ShaderTarget::Dxil => sys::SLANG_DXIL,
            ShaderTarget::Msl => sys::SLANG_METAL,
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = target;
        0
    }
}

fn shader_stage_to_slang(stage: ShaderStage) -> i32 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        match stage {
            ShaderStage::Vertex => sys::SLANG_STAGE_VERTEX,
            ShaderStage::Fragment => sys::SLANG_STAGE_FRAGMENT,
            ShaderStage::Compute => sys::SLANG_STAGE_COMPUTE,
            ShaderStage::Mesh => sys::SLANG_STAGE_MESH,
            ShaderStage::Task => sys::SLANG_STAGE_AMPLIFICATION,
            ShaderStage::RayGeneration => sys::SLANG_STAGE_RAY_GENERATION,
            ShaderStage::Miss => sys::SLANG_STAGE_MISS,
            ShaderStage::ClosestHit => sys::SLANG_STAGE_CLOSEST_HIT,
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = stage;
        0
    }
}

fn shader_stage_to_mask(stage: ShaderStage) -> StageMask {
    match stage {
        ShaderStage::Vertex => StageMask::VERTEX,
        ShaderStage::Fragment => StageMask::FRAGMENT,
        ShaderStage::Compute => StageMask::COMPUTE,
        ShaderStage::Mesh => StageMask::MESH,
        ShaderStage::Task => StageMask::TASK,
        ShaderStage::RayGeneration | ShaderStage::Miss | ShaderStage::ClosestHit => {
            StageMask::RAY_TRACING
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlangCompileDesc {
    pub source: PathBuf,
    pub output: PathBuf,
    pub entry_point: String,
    pub stage: ShaderStage,
    pub target: ShaderTarget,
    pub profile: String,
    pub include_paths: Vec<PathBuf>,
}

impl SlangCompileDesc {
    pub fn spirv(source: impl Into<PathBuf>, output: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            output: output.into(),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Compute,
            target: ShaderTarget::Spirv,
            profile: "sm_6_6".to_owned(),
            include_paths: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.source.as_os_str().is_empty() {
            return Err(Error::InvalidInput(
                "Slang source path must be non-empty".into(),
            ));
        }
        if self.output.as_os_str().is_empty() {
            return Err(Error::InvalidInput(
                "Slang output path must be non-empty".into(),
            ));
        }
        if self.entry_point.trim().is_empty() {
            return Err(Error::InvalidInput(
                "Slang entry point must be non-empty".into(),
            ));
        }
        if self.profile.trim().is_empty() {
            return Err(Error::InvalidInput(
                "Slang profile must be non-empty".into(),
            ));
        }
        Ok(())
    }
}

pub fn compile_slang_to_file(desc: &SlangCompileDesc) -> Result<()> {
    desc.validate()?;

    let mut command = Command::new("slangc");
    command
        .arg("-target")
        .arg(slang_target(desc.target))
        .arg("-profile")
        .arg(&desc.profile)
        .arg("-entry")
        .arg(&desc.entry_point)
        .arg("-stage")
        .arg(slang_stage(desc.stage))
        .arg("-fvk-use-entrypoint-name")
        .arg("-o")
        .arg(&desc.output);

    for include_path in &desc.include_paths {
        command.arg("-I").arg(include_path);
    }

    command.arg(&desc.source);

    let output = command
        .output()
        .map_err(|error| Error::CompileFailed(format!("failed to run slangc: {error}")))?;
    if !output.status.success() {
        return Err(Error::CompileFailed(slang_diagnostics(
            output.status.code(),
            &output.stdout,
            &output.stderr,
        )));
    }

    Ok(())
}

pub fn compile_slang(desc: &SlangCompileDesc) -> Result<CompiledShaderArtifact> {
    compile_slang_to_file(desc)?;
    let bytes = std::fs::read(&desc.output).map_err(|error| {
        Error::CompileFailed(format!(
            "failed to read Slang output '{}': {error}",
            desc.output.display()
        ))
    })?;
    Ok(CompiledShaderArtifact {
        target: desc.target,
        bytes,
    })
}

pub fn spirv_words_from_bytes(bytes: &[u8]) -> Result<Vec<u32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(Error::InvalidInput(
            "SPIR-V bytecode length must be a multiple of 4".into(),
        ));
    }

    let words = bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect::<Vec<_>>();
    if words.first().copied() != Some(0x0723_0203) {
        return Err(Error::InvalidInput(
            "SPIR-V bytecode has an invalid magic number".into(),
        ));
    }
    Ok(words)
}

pub fn compile_slang_to_spirv(desc: &SlangCompileDesc) -> Result<Vec<u32>> {
    if desc.target != ShaderTarget::Spirv {
        return Err(Error::InvalidInput(
            "compile_slang_to_spirv requires ShaderTarget::Spirv".into(),
        ));
    }
    let artifact = compile_slang(desc)?;
    spirv_words_from_bytes(&artifact.bytes)
}

fn slang_target(target: ShaderTarget) -> &'static str {
    match target {
        ShaderTarget::Spirv => "spirv",
        ShaderTarget::Dxil => "dxil",
        ShaderTarget::Msl => "metal",
    }
}

fn slang_stage(stage: ShaderStage) -> &'static str {
    match stage {
        ShaderStage::Vertex => "vertex",
        ShaderStage::Fragment => "fragment",
        ShaderStage::Compute => "compute",
        ShaderStage::Mesh => "mesh",
        ShaderStage::Task => "amplification",
        ShaderStage::RayGeneration => "raygeneration",
        ShaderStage::Miss => "miss",
        ShaderStage::ClosestHit => "closesthit",
    }
}

fn slang_diagnostics(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let mut message = String::new();
    if let Some(code) = code {
        message.push_str(&format!("slangc exited with status {code}"));
    } else {
        message.push_str("slangc terminated by signal");
    }
    append_diagnostics(&mut message, "stdout", stdout.trim());
    append_diagnostics(&mut message, "stderr", stderr.trim());
    message
}

fn append_diagnostics(message: &mut String, label: &str, diagnostics: &str) {
    if diagnostics.is_empty() {
        return;
    }
    message.push_str("\n");
    message.push_str(label);
    message.push_str(":\n");
    message.push_str(diagnostics);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn testbed_shader(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/sturdy-engine-testbed/shaders")
            .join(name)
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reflect_compute_shader_binding() {
        let desc = ShaderDesc {
            source: ShaderSource::File(testbed_shader("testbed_compute.slang")),
            entry_point: "main".into(),
            stage: ShaderStage::Compute,
        };
        let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
        assert!(
            !reflection.entry_points.is_empty(),
            "should have at least one entry point"
        );
        assert_eq!(
            reflection.entry_points[0], "main",
            "entry point name mismatch"
        );
        assert!(
            !reflection.layout.groups.is_empty(),
            "compute shader with buffer binding should have at least one descriptor group"
        );
        let group = &reflection.layout.groups[0];
        assert!(
            !group.bindings.is_empty(),
            "group should have at least one binding"
        );
        let binding = &group.bindings[0];
        assert_eq!(
            binding.kind,
            BindingKind::StorageBuffer,
            "RWStructuredBuffer should reflect as StorageBuffer"
        );
        assert_eq!(binding.stage_mask, StageMask::COMPUTE);
        // Binding index should be populated from Slang reflection (not always 0 from array position)
        // For a single binding in set 0, Slang assigns binding=0
        assert_eq!(binding.binding, 0, "first binding should have slot 0");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reflect_binding_indices_are_preserved() {
        // textured_fragment.slang has:
        //   Texture2D base_color : register(t0, space0)  → binding 0
        //   SamplerState base_sampler : register(s1, space0) → binding 1
        // Verify the reflected binding slots match the register declarations.
        let desc = ShaderDesc {
            source: ShaderSource::File(testbed_shader("textured_fragment.slang")),
            entry_point: "ps_main".into(),
            stage: ShaderStage::Fragment,
        };
        let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
        let group = reflection
            .layout
            .groups
            .first()
            .expect("textured fragment shader should have a descriptor group");
        assert_eq!(group.bindings.len(), 2, "should reflect both bindings");
        let slots: Vec<u32> = group.bindings.iter().map(|b| b.binding).collect();
        // Both binding slots should be present (0 for texture, 1 for sampler).
        assert!(
            slots.contains(&0) && slots.contains(&1),
            "binding slots 0 and 1 should both appear, got {slots:?}"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reflect_vertex_shader_no_bindings() {
        let desc = ShaderDesc {
            source: ShaderSource::File(testbed_shader("triangle_vertex.slang")),
            entry_point: "vs_main".into(),
            stage: ShaderStage::Vertex,
        };
        let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
        assert!(
            reflection.layout.groups.is_empty(),
            "vertex shader with no resource bindings should have empty layout"
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reflect_push_constant_size_and_stage() {
        let desc = ShaderDesc {
            source: ShaderSource::File(testbed_shader("push_vertex.slang")),
            entry_point: "vs_main".into(),
            stage: ShaderStage::Vertex,
        };
        let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
        assert_eq!(reflection.layout.push_constants_bytes, 32);
        assert_eq!(
            reflection.layout.push_constants_stage_mask,
            StageMask::VERTEX
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reflect_separate_texture_and_sampler_bindings() {
        // textured_fragment.slang uses separate Texture2D + SamplerState
        let desc = ShaderDesc {
            source: ShaderSource::File(testbed_shader("textured_fragment.slang")),
            entry_point: "ps_main".into(),
            stage: ShaderStage::Fragment,
        };
        let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
        let group = reflection
            .layout
            .groups
            .first()
            .expect("should have a group");
        let kinds: Vec<BindingKind> = group.bindings.iter().map(|b| b.kind).collect();
        assert!(
            kinds.contains(&BindingKind::SampledImage),
            "should reflect Texture2D as SampledImage, got {kinds:?}"
        );
        assert!(
            kinds.contains(&BindingKind::Sampler),
            "should reflect SamplerState as Sampler, got {kinds:?}"
        );
    }

    #[test]
    fn reflect_spirv_returns_empty() {
        let desc = ShaderDesc {
            source: ShaderSource::Spirv(vec![0x0723_0203, 0, 0, 0, 0]),
            entry_point: "main".into(),
            stage: ShaderStage::Compute,
        };
        let reflection = reflect_pipeline_layout(&desc).expect("should not error for SPIRV source");
        assert_eq!(reflection, ShaderReflection::default());
    }
}
