use std::path::{Path, PathBuf};

use crate::{BindingKind, CanonicalPipelineLayout, Error, Result, StageMask, UpdateRate};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
    Mesh,
    Task,
    RayGeneration,
    Miss,
    ClosestHit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderSource {
    /// Slang source stored in memory. Kept as the ergonomic default for generated shaders.
    Inline(String),
    /// Slang source loaded from a native development file path.
    File(PathBuf),
    /// Slang source loaded from a borrowed native development file path.
    FilePath(&'static Path),
    /// Slang source addressed through the engine asset system. Runtime compilation
    /// requires an asset resolver; direct device creation rejects unresolved virtual paths.
    VirtualAssetPath(&'static Path),
    /// Borrowed UTF-8 Slang source supplied by the caller.
    MemoryUtf8(&'static str),
    /// Borrowed bytes supplied by the caller. UTF-8 bytes are compiled as Slang
    /// source; SPIR-V bytes are accepted for SPIR-V targets.
    MemoryBytes(&'static [u8]),
    Spirv(Vec<u32>),
    /// Pre-compiled DXIL bytecode for D3D12 backends.
    Dxil(Vec<u8>),
    /// Pre-compiled MSL source or Metal library bytes for Metal backends.
    Msl(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderDesc {
    pub source: ShaderSource,
    pub entry_point: String,
    pub stage: ShaderStage,
    /// Shader uses inline ray query instructions.
    ///
    /// Requires `BackendFeatures::ray_query` on the selected backend.
    pub requires_ray_query: bool,
    /// Shader uses cooperative matrix multiply-accumulate instructions (`VK_KHR_cooperative_matrix`).
    ///
    /// Requires `BackendFeatures::cooperative_matrix` or one of the NV fallback flags.
    pub requires_cooperative_matrix: bool,
    /// Shader uses Shader Execution Reordering (`VK_EXT_ray_tracing_invocation_reorder`, SER).
    ///
    /// Enables the `ShaderInvocationReorderNV` SPIR-V capability. Only valid in hit/callable
    /// shader stages of a ray-tracing pipeline. Requires `BackendFeatures::shader_execution_reordering`.
    pub uses_ser: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShaderTarget {
    Spirv,
    Dxil,
    Msl,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledShaderArtifact {
    pub target: ShaderTarget,
    pub bytes: Vec<u8>,
}

/// Precomputed properties derived from shader reflection and stage.
///
/// Computed once at program creation; zero-cost to query at dispatch time.
/// Used by `dispatch_compute_auto` queue routing and the `RenderStrategySelector`.
///
/// All fields are static — they never change after the shader is compiled.
#[derive(Clone, Debug)]
pub struct ShaderCapabilityProfile {
    /// True when the shader can run on the async compute queue without waiting for the
    /// graphics fence. False only when the shader samples images at sets > 0 (named
    /// per-frame or per-pass bindings that could be current-frame render targets).
    ///
    /// Bindless heap images (set 0, e.g. `g_bindless_textures[]`) are excluded from
    /// this check — they are persistent and never depend on same-frame render target output.
    pub async_compute_eligible: bool,
    /// True when the shader has at least one sampled image binding (any set).
    pub has_sampled_images: bool,
    /// True when the shader writes to at least one storage image.
    pub has_storage_image_writes: bool,
    /// True when the shader entry point is a ray tracing stage (raygen, miss, closest-hit).
    pub requires_ray_tracing: bool,
    /// True when the shader entry point is a mesh or task stage.
    pub requires_mesh_shading: bool,
    /// Declared push constant range size in bytes. Zero when unused.
    pub push_constant_bytes: u32,
    /// Declared compute workgroup dimensions from `OpExecutionMode LocalSize`.
    /// `[1, 1, 1]` for graphics-only shaders.
    pub workgroup_size: [u32; 3],
    /// True when all non-push-constant resource bindings are at set 0 (the bindless heap)
    /// or the shader has no resource bindings at all. When true, no per-frame or per-pass
    /// descriptor set allocation is needed.
    pub bindless_only: bool,
    /// True when the SPIR-V declares any subgroup/wave intrinsic capability.
    /// The pipeline must respect the device's preferred subgroup size when true.
    pub wave_ops_used: bool,
    /// Names of sampled image bindings at sets > 0 (per-frame/per-pass named bindings).
    ///
    /// Cached here to avoid re-scanning `ShaderReflection::parameters` at every dispatch.
    /// Used by the async compute eligibility slow path: a pass is blocked from async compute
    /// only when one of these names is registered as a frame image in the current frame.
    ///
    /// Bindless array names (e.g. `g_bindless_textures` at set 0) are intentionally excluded.
    pub sampled_image_names: Box<[Box<str>]>,
    /// Names of storage images that are written (mutable, not read-only).
    pub storage_write_image_names: Box<[Box<str>]>,
    /// Heuristic wave occupancy estimate (0–100).
    ///
    /// Derived from the declared workgroup size: smaller workgroups leave more room for
    /// other waves to co-schedule, yielding higher occupancy. Used by the strategy
    /// selector to rank pass cost without a live GPU query.
    pub estimated_wave_occupancy: u8,
}

impl Default for ShaderCapabilityProfile {
    fn default() -> Self {
        Self {
            async_compute_eligible: true,
            has_sampled_images: false,
            has_storage_image_writes: false,
            requires_ray_tracing: false,
            requires_mesh_shading: false,
            push_constant_bytes: 0,
            workgroup_size: [1, 1, 1],
            bindless_only: false,
            wave_ops_used: false,
            sampled_image_names: Box::new([]),
            storage_write_image_names: Box::new([]),
            estimated_wave_occupancy: 100,
        }
    }
}

impl ShaderCapabilityProfile {
    pub fn from_reflection(reflection: &ShaderReflection, stage: ShaderStage) -> Self {
        let mut has_sampled_images = false;
        let mut has_storage_image_writes = false;
        // Sampled images at sets > 0 (named per-frame/per-pass bindings, not bindless heap).
        let mut sampled_image_names: Vec<Box<str>> = Vec::new();
        let mut storage_write_image_names: Vec<Box<str>> = Vec::new();
        // True when any resource binding is at set > 0 (non-bindless descriptor set).
        let mut has_non_bindless_resource = false;

        for param in &reflection.parameters {
            match &param.kind {
                ShaderParameterKind::Resource(BindingKind::SampledImage) => {
                    has_sampled_images = true;
                    // Only track as a potential frame render target when NOT on the bindless heap.
                    // Bindless heap (set 0) images are persistent arrays, never frame render targets.
                    let on_bindless_heap = param.set == Some(0);
                    if !on_bindless_heap {
                        sampled_image_names.push(param.name.clone().into_boxed_str());
                    }
                    if param.set.map_or(false, |s| s > 0) {
                        has_non_bindless_resource = true;
                    }
                }
                ShaderParameterKind::Resource(BindingKind::StorageImage) => {
                    if param.access != ShaderResourceAccess::Read {
                        has_storage_image_writes = true;
                        storage_write_image_names.push(param.name.clone().into_boxed_str());
                    }
                    if param.set.map_or(false, |s| s > 0) {
                        has_non_bindless_resource = true;
                    }
                }
                ShaderParameterKind::Resource(_) => {
                    if param.set.map_or(false, |s| s > 0) {
                        has_non_bindless_resource = true;
                    }
                }
                ShaderParameterKind::PushConstant => {}
            }
        }

        // A pass is async compute eligible when it has no sampled images that could be
        // same-frame render targets (named per-frame/per-pass bindings at sets > 0).
        // Bindless heap images are always safe — they're persistent, not frame outputs.
        let async_compute_eligible = sampled_image_names.is_empty();

        // Bindless-only: no resource bindings at sets > 0 (i.e., no per-frame/per-pass descriptors).
        let bindless_only = !has_non_bindless_resource;

        let requires_ray_tracing = matches!(
            stage,
            ShaderStage::RayGeneration | ShaderStage::Miss | ShaderStage::ClosestHit
        );
        let requires_mesh_shading = matches!(stage, ShaderStage::Mesh | ShaderStage::Task);

        let workgroup_size = reflection.workgroup_size.unwrap_or([1, 1, 1]);
        let total_invocations = workgroup_size[0]
            .max(1)
            .saturating_mul(workgroup_size[1].max(1))
            .saturating_mul(workgroup_size[2].max(1));
        // Heuristic: workgroups ≤ 64 get full estimated occupancy; larger workgroups
        // reduce the estimate (fewer waves can co-schedule on one CU/SM).
        let estimated_wave_occupancy = if total_invocations <= 64 {
            100
        } else {
            let excess = (total_invocations - 64) / 4;
            100u32.saturating_sub(excess).min(100) as u8
        };

        Self {
            async_compute_eligible,
            has_sampled_images,
            has_storage_image_writes,
            requires_ray_tracing,
            requires_mesh_shading,
            push_constant_bytes: reflection.layout.push_constants_bytes,
            workgroup_size,
            bindless_only,
            wave_ops_used: reflection.wave_ops_used,
            sampled_image_names: sampled_image_names.into_boxed_slice(),
            storage_write_image_names: storage_write_image_names.into_boxed_slice(),
            estimated_wave_occupancy,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderReflection {
    pub layout: CanonicalPipelineLayout,
    pub entry_points: Vec<String>,
    pub parameters: Vec<ShaderParameterReflection>,
    /// Vertex input attributes reflected from a vertex shader's SPIR-V.
    /// Empty for fragment and compute shaders.
    pub vertex_inputs: Vec<VertexInputReflection>,
    /// Declared compute workgroup dimensions from `OpExecutionMode LocalSize`.
    /// `None` for non-compute shaders or when the SPIR-V did not declare a local size.
    pub workgroup_size: Option<[u32; 3]>,
    /// True when the SPIR-V declares any subgroup/wave intrinsic capability.
    pub wave_ops_used: bool,
}

impl Default for ShaderReflection {
    fn default() -> Self {
        Self {
            layout: CanonicalPipelineLayout::default(),
            entry_points: Vec::new(),
            parameters: Vec::new(),
            vertex_inputs: Vec::new(),
            workgroup_size: None,
            wave_ops_used: false,
        }
    }
}

/// One vertex shader input attribute as reflected from SPIR-V.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VertexInputReflection {
    pub name: String,
    pub location: u32,
    pub format: crate::VertexFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ShaderResourceAccess {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderParameterKind {
    Resource(BindingKind),
    PushConstant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderParameterReflection {
    pub name: String,
    pub kind: ShaderParameterKind,
    pub stage_mask: StageMask,
    pub access: ShaderResourceAccess,
    pub set: Option<u32>,
    pub binding: Option<u32>,
    pub count: u32,
    pub update_rate: Option<UpdateRate>,
    pub size_bytes: Option<u32>,
    /// Per-field detail for `PushConstant` parameters. Empty for resource bindings.
    pub push_constant_fields: Vec<crate::PushConstantField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderModule {
    pub desc: ShaderDesc,
    pub reflection: ShaderReflection,
    pub artifacts: Vec<CompiledShaderArtifact>,
}

impl Default for ShaderDesc {
    fn default() -> Self {
        Self {
            source: ShaderSource::Inline(String::new()),
            entry_point: String::new(),
            stage: ShaderStage::Compute,
            requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
        }
    }
}

impl ShaderDesc {
    pub fn validate(&self) -> Result<()> {
        if self.entry_point.trim().is_empty() {
            return Err(Error::InvalidInput(
                "shader entry_point must be non-empty".into(),
            ));
        }
        match &self.source {
            ShaderSource::Inline(source) if source.trim().is_empty() => Err(Error::InvalidInput(
                "shader source must be non-empty".into(),
            )),
            ShaderSource::MemoryUtf8(source) if source.trim().is_empty() => Err(
                Error::InvalidInput("shader source must be non-empty".into()),
            ),
            ShaderSource::File(path) if path.as_os_str().is_empty() => Err(Error::InvalidInput(
                "shader file path must be non-empty".into(),
            )),
            ShaderSource::FilePath(path) if path.as_os_str().is_empty() => Err(
                Error::InvalidInput("shader file path must be non-empty".into()),
            ),
            ShaderSource::VirtualAssetPath(path) if path.as_os_str().is_empty() => Err(
                Error::InvalidInput("shader virtual asset path must be non-empty".into()),
            ),
            ShaderSource::MemoryBytes(bytes) if bytes.is_empty() => Err(Error::InvalidInput(
                "shader byte source must be non-empty".into(),
            )),
            ShaderSource::Spirv(words) if words.is_empty() => Err(Error::InvalidInput(
                "SPIR-V shader source must be non-empty".into(),
            )),
            ShaderSource::Spirv(words) if words.first().copied() != Some(0x0723_0203) => Err(
                Error::InvalidInput("SPIR-V shader source has an invalid magic number".into()),
            ),
            ShaderSource::Dxil(bytes) if bytes.is_empty() => Err(Error::InvalidInput(
                "DXIL shader source must be non-empty".into(),
            )),
            ShaderSource::Msl(bytes) if bytes.is_empty() => Err(Error::InvalidInput(
                "MSL shader source must be non-empty".into(),
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanonicalPipelineLayout, StageMask};

    fn make_param(
        name: &str,
        kind: BindingKind,
        access: ShaderResourceAccess,
        set: u32,
    ) -> ShaderParameterReflection {
        ShaderParameterReflection {
            name: name.to_owned(),
            kind: ShaderParameterKind::Resource(kind),
            stage_mask: StageMask::COMPUTE,
            access,
            set: Some(set),
            binding: Some(0),
            count: 1,
            update_rate: None,
            size_bytes: None,
            push_constant_fields: Vec::new(),
        }
    }

    #[test]
    fn profile_pure_storage_is_async_eligible() {
        let reflection = ShaderReflection {
            parameters: vec![
                make_param(
                    "input_buf",
                    BindingKind::StorageBuffer,
                    ShaderResourceAccess::Read,
                    1,
                ),
                make_param(
                    "output_buf",
                    BindingKind::StorageBuffer,
                    ShaderResourceAccess::ReadWrite,
                    1,
                ),
            ],
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(profile.async_compute_eligible);
        assert!(!profile.has_sampled_images);
        assert!(!profile.has_storage_image_writes);
        assert!(!profile.requires_ray_tracing);
        assert!(!profile.requires_mesh_shading);
    }

    #[test]
    fn profile_sampled_image_at_set1_not_async_eligible() {
        let reflection = ShaderReflection {
            parameters: vec![make_param(
                "scene_color",
                BindingKind::SampledImage,
                ShaderResourceAccess::Read,
                1,
            )],
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(!profile.async_compute_eligible);
        assert!(profile.has_sampled_images);
        assert!(!profile.has_storage_image_writes);
        assert_eq!(profile.sampled_image_names.len(), 1);
        assert_eq!(profile.sampled_image_names[0].as_ref(), "scene_color");
    }

    #[test]
    fn profile_bindless_sampled_image_is_async_eligible() {
        // Bindless heap images (set 0) are NOT blocking for async compute.
        let reflection = ShaderReflection {
            parameters: vec![make_param(
                "g_bindless_textures",
                BindingKind::SampledImage,
                ShaderResourceAccess::Read,
                0,
            )],
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(profile.async_compute_eligible);
        assert!(profile.has_sampled_images);
        assert!(
            profile.sampled_image_names.is_empty(),
            "bindless textures must not block async compute"
        );
    }

    #[test]
    fn profile_storage_image_write_detected() {
        let reflection = ShaderReflection {
            parameters: vec![make_param(
                "output_img",
                BindingKind::StorageImage,
                ShaderResourceAccess::ReadWrite,
                1,
            )],
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(profile.async_compute_eligible);
        assert!(profile.has_storage_image_writes);
        assert_eq!(profile.storage_write_image_names.len(), 1);
    }

    #[test]
    fn profile_read_only_storage_image_not_flagged_as_write() {
        let reflection = ShaderReflection {
            parameters: vec![make_param(
                "src_img",
                BindingKind::StorageImage,
                ShaderResourceAccess::Read,
                1,
            )],
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(profile.async_compute_eligible);
        assert!(!profile.has_storage_image_writes);
        assert!(profile.storage_write_image_names.is_empty());
    }

    #[test]
    fn profile_bindless_only_when_no_set_gt_0() {
        let reflection = ShaderReflection {
            parameters: vec![
                make_param(
                    "g_bindless_textures",
                    BindingKind::SampledImage,
                    ShaderResourceAccess::Read,
                    0,
                ),
                make_param(
                    "g_bindless_samplers",
                    BindingKind::Sampler,
                    ShaderResourceAccess::Read,
                    0,
                ),
            ],
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(profile.bindless_only);
    }

    #[test]
    fn profile_not_bindless_only_when_has_set1_binding() {
        let reflection = ShaderReflection {
            parameters: vec![
                make_param(
                    "g_bindless_textures",
                    BindingKind::SampledImage,
                    ShaderResourceAccess::Read,
                    0,
                ),
                make_param(
                    "scene_data",
                    BindingKind::UniformBuffer,
                    ShaderResourceAccess::Read,
                    1,
                ),
            ],
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(!profile.bindless_only);
    }

    #[test]
    fn profile_ray_tracing_stage_detected() {
        let reflection = ShaderReflection::default();
        let profile =
            ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::RayGeneration);
        assert!(profile.requires_ray_tracing);
        assert!(!profile.requires_mesh_shading);
    }

    #[test]
    fn profile_mesh_stage_detected() {
        let reflection = ShaderReflection::default();
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Mesh);
        assert!(profile.requires_mesh_shading);
        assert!(!profile.requires_ray_tracing);
    }

    #[test]
    fn profile_push_constant_bytes_from_layout() {
        let reflection = ShaderReflection {
            layout: CanonicalPipelineLayout {
                push_constants_bytes: 128,
                push_constants_stage_mask: StageMask::COMPUTE,
                ..CanonicalPipelineLayout::default()
            },
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert_eq!(profile.push_constant_bytes, 128);
    }

    #[test]
    fn profile_workgroup_size_propagated() {
        let reflection = ShaderReflection {
            workgroup_size: Some([8, 8, 1]),
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert_eq!(profile.workgroup_size, [8, 8, 1]);
    }

    #[test]
    fn profile_wave_ops_propagated() {
        let reflection = ShaderReflection {
            wave_ops_used: true,
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(profile.wave_ops_used);
    }

    #[test]
    fn profile_occupancy_full_for_small_workgroup() {
        let reflection = ShaderReflection {
            workgroup_size: Some([8, 8, 1]), // 64 invocations
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert_eq!(profile.estimated_wave_occupancy, 100);
    }

    #[test]
    fn profile_occupancy_reduced_for_large_workgroup() {
        let reflection = ShaderReflection {
            workgroup_size: Some([16, 16, 1]), // 256 invocations
            ..ShaderReflection::default()
        };
        let profile = ShaderCapabilityProfile::from_reflection(&reflection, ShaderStage::Compute);
        assert!(profile.estimated_wave_occupancy < 100);
    }
}
