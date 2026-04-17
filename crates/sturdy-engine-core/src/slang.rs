use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::sync::OnceLock;
use std::{path::PathBuf, process::Command};

use crate::{
    BindingKind, CanonicalBinding, CanonicalGroupLayout, CanonicalPipelineLayout,
    CompiledShaderArtifact, Error, Result, ShaderDesc, ShaderReflection, ShaderSource, ShaderStage,
    ShaderTarget, StageMask, UpdateRate,
};

#[cfg(not(target_arch = "wasm32"))]
mod sys {
    use std::ffi::{c_char, c_int, c_uint};

    pub type SlangResult = i32;
    pub type SlangInt = i64;
    pub type SlangUInt = u64;
    pub type SlangBindingType = u32;

    pub const SLANG_OK: SlangResult = 0;

    // SlangCompileTarget enum ordinals (verified against slang.h from libslang-compiler 2026.x)
    // SLANG_TARGET_NONE=0, SLANG_GLSL=1, SLANG_GLSL_VULKAN_DEPRECATED=2,
    // SLANG_GLSL_VULKAN_ONE_DESC_DEPRECATED=3, SLANG_HLSL=4
    pub const SLANG_SPIRV: c_int = 5;
    // SLANG_SPIRV_ASM=6, SLANG_DXBC=7, SLANG_DXBC_ASM=8
    pub const SLANG_DXIL: c_int = 9;
    // SLANG_DXIL_ASM=10, ... SLANG_CPP_PYTORCH_BINDING=22
    pub const SLANG_METAL: c_int = 23;

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
    pub const SLANG_BINDING_TYPE_MUTABLE_FLAG: SlangBindingType = 0x100;

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
#[cfg(not(target_arch = "wasm32"))]
pub fn reflect_pipeline_layout(desc: &ShaderDesc) -> Result<ShaderReflection> {
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
            sys::spAddTranslationUnitSourceString(
                request,
                tu,
                std::ptr::null(),
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

        let layout = extract_layout(reflection, engine_stage)?;

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
) -> Result<CanonicalPipelineLayout> {
    let param_count = unsafe { sys::spReflection_GetParameterCount(reflection) };
    let stage_mask = shader_stage_to_mask(stage);

    // BTreeMap keeps groups sorted by set index
    let mut groups: BTreeMap<u32, (String, Vec<CanonicalBinding>)> = BTreeMap::new();

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

        let type_layout = unsafe { sys::spReflectionVariableLayout_GetTypeLayout(param) };
        if type_layout.is_null() {
            continue;
        }

        let range_count = unsafe { sys::spReflectionTypeLayout_getBindingRangeCount(type_layout) };

        for r in 0..range_count {
            let binding_type =
                unsafe { sys::spReflectionTypeLayout_getBindingRangeType(type_layout, r) };
            let set_offset = unsafe {
                sys::spReflectionTypeLayout_getBindingRangeDescriptorSetIndex(type_layout, r)
            };
            let count =
                unsafe { sys::spReflectionTypeLayout_getBindingRangeBindingCount(type_layout, r) }
                    as u32;

            let kind = match slang_binding_type_to_kind(binding_type) {
                Some(k) => k,
                None => continue,
            };

            let set_index = binding_space + set_offset as u32;
            let binding_name = if range_count == 1 {
                name.clone()
            } else {
                format!("{name}.{r}")
            };

            let group = groups
                .entry(set_index)
                .or_insert_with(|| (format!("set{set_index}"), Vec::new()));
            group.1.push(CanonicalBinding {
                path: binding_name,
                kind,
                count: count.max(1),
                stage_mask,
                update_rate: UpdateRate::Frame,
            });
        }
    }

    Ok(CanonicalPipelineLayout {
        groups: groups
            .into_values()
            .map(|(name, bindings)| CanonicalGroupLayout { name, bindings })
            .collect(),
        push_constants_bytes: 0,
    })
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
            sys::spAddTranslationUnitSourceString(
                request,
                tu,
                std::ptr::null(),
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

        let compiled_source = match target {
            ShaderTarget::Spirv => ShaderSource::Spirv(spirv_words_from_bytes(&code_bytes)?),
            ShaderTarget::Dxil => ShaderSource::Dxil(code_bytes),
            ShaderTarget::Msl => ShaderSource::Msl(code_bytes),
        };

        // Reflect pipeline layout (works regardless of output target)
        let reflection_ptr = sys::spGetReflection(request);
        let (layout, entry_points) = if reflection_ptr.is_null() {
            (CanonicalPipelineLayout::default(), Vec::new())
        } else {
            let layout = extract_layout(reflection_ptr, engine_stage)?;
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
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reflect_vertex_shader_no_bindings() {
        let desc = ShaderDesc {
            source: ShaderSource::File(testbed_shader("triangle_vertex.slang")),
            entry_point: "main".into(),
            stage: ShaderStage::Vertex,
        };
        let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
        assert!(
            reflection.layout.groups.is_empty(),
            "vertex shader with no resource bindings should have empty layout"
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
