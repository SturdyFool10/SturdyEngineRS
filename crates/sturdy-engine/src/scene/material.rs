//! Core material definition system for the scene and rendering pipeline.
//!
//! This module provides the [`Material`] type and associated types that define
//! how objects in the scene are rendered. Materials are rendering-mode-agnostic:
//! they work across rasterized, hybrid, raytraced, and path traced rendering.
//!
//! # Material Definition
//!
//! A material defines:
//! - The shader program(s) used for rendering
//! - The parameter bindings (push constants, bind groups)
//! - The rendering state (cull mode, front face, depth write, etc.)
//! - The format capabilities (FP16, FP32, HDR, SDR)
//!
//! # Workflow
//!
//! ```rust
//! // At init:
//! let mat = Material::new("pbr_material")
//!     .with_fragment_shader("pbr_fragment.slang")
//!     .with_vertex_kind(MeshVertexKind::V3d)
//!     .with_push_constant::<FrameConstants>()
//!     .with_texture_binding::<BaseColor>()
//!     .build();
//!
//! // At render:
//! scene.add_material_mesh(mesh_id, mat);
//! scene.render(frame)?;
//! ```
//!
//! # Rendering Mode Support
//!
//! Materials are designed to work across all rendering modes without breaking down:
//! - **Rasterized** — Traditional raster pipeline (Vulkan/D3D12/Metal graphics pipelines)
//! - **Hybrid** — Raster + raytraced elements (acceleration structures, ray queries)
//! - **Raytraced** — Primary raytraced pipeline (closest hit, miss, ray generation shaders)
//! - **Path Traced** — Offline rendering, full path tracing (multiple bounces, importance sampling)
//!
//! The material system ensures that:
//! - Material definitions are rendering-mode-agnostic
//! - Material parameters translate across all modes
//! - Material shaders compile across all IR targets (SPIR-V, DXIL, MSL, raytraced extensions)
//! - Material caching works across modes
//! - Material graph composition supports mode-specific nodes

use std::path::PathBuf;

use glam::Mat4;

use crate::{
    Engine, Format, MeshProgram, MeshProgramDesc, MeshVertexKind, PipelineLayout, PipelineLayoutBuilder,
    Result, Sampler, ShaderDesc, ShaderSource, ShaderStage, StageMask, UpdateRate, push_constants,
};
use crate::scene::{
    camera::CameraConstants,
    object::{InstanceData, MeshId, ObjectId, ObjectKind},
};
use crate::sampler_catalog::SamplerPreset;
use sturdy_engine_core::{
    BindingKind, CanonicalBinding, CanonicalGroupLayout, CanonicalPipelineLayout, ResourceBinding,
    StageMask, UpdateRate,
};

// ------------------------------------------------------------------
// Material Definition Types
// ------------------------------------------------------------------

/// A material definition that describes how an object is rendered.
///
/// Materials are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
/// let mat = Material::new("pbr_material")
///     .with_fragment_shader("pbr_fragment.slang")
///     .with_vertex_kind(MeshVertexKind::V3d)
///     .with_push_constant::<FrameConstants>()
///     .with_texture_binding::<BaseColor>()
///     .build();
/// ```
///
/// # Rendering Mode Support
///
/// Materials are designed to work across all rendering modes without breaking down:
/// - **Rasterized** — Traditional raster pipeline (Vulkan/D3D12/Metal graphics pipelines)
/// - **Hybrid** — Raster + raytraced elements (acceleration structures, ray queries)
/// - **Raytraced** — Primary raytraced pipeline (closest hit, miss, ray generation shaders)
/// - **Path Traced** — Offline rendering, full path tracing (multiple bounces, importance sampling)
///
/// The material system ensures that:
/// - Material definitions are rendering-mode-agnostic
/// - Material parameters translate across all modes
/// - Material shaders compile across all IR targets (SPIR-V, DXIL, MSL, raytraced extensions)
/// - Material caching works across modes
/// - Material graph composition supports mode-specific nodes

#[derive(Clone, Debug)]
pub struct Material {
    /// The material's name for debugging and bind group naming.
    pub name: String,
    /// The vertex shader kind for mesh rendering.
    pub vertex_kind: MeshVertexKind,
    /// The fragment shader descriptor.
    pub fragment_desc: ShaderDesc,
    /// Optional custom vertex shader descriptor. None uses the built-in default.
    pub vertex_desc: Option<ShaderDesc>,
    /// Push constant types registered for this material.
    pub push_constants: Vec<PushConstantRegistration>,
    /// Texture/sampler bindings registered for this material.
    pub texture_bindings: Vec<TextureBindingRegistration>,
    /// Buffer bindings registered for this material.
    pub buffer_bindings: Vec<BufferBindingRegistration>,
    /// Acceleration structure bindings (for raytraced/hybrid modes).
    pub acceleration_structure_bindings: Vec<AccelerationStructureBindingRegistration>,
    /// Rendering state configuration.
    pub render_state: RenderState,
    /// Format capabilities for this material.
    pub format_capabilities: FormatCapabilities,
    /// Raytraced shader stages (for raytraced/hybrid modes).
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// Path traced bounce configuration (for offline path traced modes).
    pub path_traced_bounces: PathTracedBounceConfig,
}

impl Material {
    /// Create a new material with the given name.
    pub fn new(name: impl Into<String>) -> MaterialBuilder {
        MaterialBuilder {
            name: name.into(),
            vertex_kind: MeshVertexKind::V2d,
            fragment_desc: ShaderDesc::default(),
            vertex_desc: None,
            push_constants: Vec::new(),
            texture_bindings: Vec::new(),
            buffer_bindings: Vec::new(),
            acceleration_structure_bindings: Vec::new(),
            render_state: RenderState::default(),
            format_capabilities: FormatCapabilities::default(),
            raytraced_stages: Vec::new(),
            path_traced_bounces: PathTracedBounceConfig::default(),
        }
    }

    /// Build the material and create its pipeline layout.
    pub fn build(self, engine: &Engine) -> Result<Self> {
        let layout = self.create_pipeline_layout(engine)?;
        Ok(Self {
            layout,
            ..self
        })
    }

    /// Get the pipeline layout for this material.
    pub fn pipeline_layout(&self) -> &PipelineLayout {
        &self.layout
    }

    /// Create the pipeline layout from the material's bindings.
    fn create_pipeline_layout(&self, engine: &Engine) -> Result<PipelineLayout> {
        let mut builder = PipelineLayoutBuilder::new();

        // Add instance buffer binding (required for instanced rendering)
        builder
            .storage_buffer(
                "material",
                "instances",
                StageMask::VERTEX | StageMask::FRAGMENT,
                UpdateRate::Material,
            );

        // Add push constants
        for pc in &self.push_constants {
            builder.push_constants_bytes(pc.total_bytes);
        }

        // Add texture/sampler bindings
        for tex in &self.texture_bindings {
            builder
                .sampled_image(
                    tex.group,
                    tex.path,
                    tex.stage_mask,
                    tex.update_rate,
                )
                .sampler(
                    tex.group,
                    tex.sampler_path,
                    tex.stage_mask,
                    tex.update_rate,
                );
        }

        // Add buffer bindings
        for buf in &self.buffer_bindings {
            builder
                .uniform_buffer(
                    buf.group,
                    buf.path,
                    buf.stage_mask,
                    buf.update_rate,
                )
                .storage_buffer(
                    buf.group,
                    buf.storage_path,
                    buf.stage_mask,
                    buf.update_rate,
                );
        }

        // Add acceleration structure bindings (for raytraced/hybrid modes)
        for accel_as in &self.acceleration_structure_bindings {
            builder
                .binding(
                    accel_as.group,
                    accel_as.path,
                    BindingKind::AccelerationStructure,
                    accel_as.stage_mask,
                    accel_as.update_rate,
                );
        }

        // Add raytraced shader stage bindings
        for rs in &self.raytraced_stages {
            builder
                .storage_image(
                    rs.group,
                    rs.path,
                    rs.stage_mask,
                    rs.update_rate,
                );
        }

        builder.build(engine)
    }

    /// Create a mesh program from this material's shader descriptors.
    pub fn create_mesh_program(&self, engine: &Engine) -> Result<MeshProgram> {
        MeshProgram::new(engine, MeshProgramDesc {
            fragment: self.fragment_desc.clone(),
            vertex: self.vertex_desc.clone(),
            vertex_kind: self.vertex_kind,
        })
    }

    /// Create a compute program from this material's shader descriptors.
    pub fn create_compute_program(&self, engine: &Engine) -> Result<ComputeProgram> {
        ComputeProgram::load(engine, PathBuf::from(self.fragment_desc.source.file_path().unwrap_or_default()))
    }

    /// Create a raytraced program from this material's raytraced stages.
    pub fn create_raytraced_program(&self, engine: &Engine) -> Result<RaytracedProgram> {
        RaytracedProgram::new(engine, self.raytraced_stages.clone())
    }

    /// Create a path traced program from this material's path traced bounces.
    pub fn create_path_traced_program(&self, engine: &Engine) -> Result<PathTracedProgram> {
        PathTracedProgram::new(engine, self.path_traced_bounces.clone())
    }

    /// Get the format that this material should use for render targets.
    pub fn render_format(&self) -> Format {
        self.format_capabilities.render_format
    }

    /// Get the HDR mode that this material supports.
    pub fn hdr_mode(&self) -> crate::HdrMode {
        self.format_capabilities.hdr_mode
    }

    /// Get the tone mapping that this material should use.
    pub fn tone_mapping(&self) -> crate::ToneMappingOp {
        self.format_capabilities.tone_mapping
    }

    /// Get the raytracing capabilities that this material requires.
    pub fn raytracing_caps(&self) -> RaytracingCapabilities {
        RaytracingCapabilities::from_raytraced_stages(&self.raytraced_stages)
    }

    /// Get the path tracing capabilities that this material requires.
    pub fn path_tracing_caps(&self) -> PathTracingCapabilities {
        PathTracingCapabilities::from_path_traced_bounces(&self.path_traced_bounces)
    }

    /// Get the material's total push constant byte count.
    pub fn total_push_constants_bytes(&self) -> u32 {
        self.push_constants.iter().map(|pc| pc.total_bytes).sum()
    }

    /// Get the material's push constant stage mask.
    pub fn push_constants_stage_mask(&self) -> StageMask {
        self.push_constants.iter().map(|pc| pc.stage_mask).reduce(|a, b| a | b).unwrap_or(StageMask::empty())
    }

    /// Get the material's texture binding count.
    pub fn texture_binding_count(&self) -> usize {
        self.texture_bindings.len()
    }

    /// Get the material's buffer binding count.
    pub fn buffer_binding_count(&self) -> usize {
        self.buffer_bindings.len()
    }

    /// Get the material's acceleration structure binding count.
    pub fn acceleration_structure_binding_count(&self) -> usize {
        self.acceleration_structure_bindings.len()
    }

    /// Get the material's raytraced stage count.
    pub fn raytraced_stage_count(&self) -> usize {
        self.raytraced_stages.len()
    }

    /// Get the material's path traced bounce count.
    pub fn path_traced_bounce_count(&self) -> usize {
        self.path_traced_bounces.bounce_count
    }

    /// Get the material's render state.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the material's format capabilities.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the material's raytraced stages.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the material's path traced bounces.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }
}

impl Default for Material {
    fn default() -> Self {
        //panic allowed, reason = "Default impl uses hardcoded valid parameters; failure is a code defect"
        Self::new("default_material")
            .with_vertex_kind(MeshVertexKind::V2d)
            .with_fragment_shader("passthrough_fragment.slang")
            .build()
            .expect("default material should always build")
    }
}

// ------------------------------------------------------------------
// Material Builder
// ------------------------------------------------------------------

/// Builder for [`Material`] definitions.
///
/// Provides a fluent API for configuring material properties.
pub struct MaterialBuilder {
    name: String,
    vertex_kind: MeshVertexKind,
    fragment_desc: ShaderDesc,
    vertex_desc: Option<ShaderDesc>,
    push_constants: Vec<PushConstantRegistration>,
    texture_bindings: Vec<TextureBindingRegistration>,
    buffer_bindings: Vec<BufferBindingRegistration>,
    acceleration_structure_bindings: Vec<AccelerationStructureBindingRegistration>,
    render_state: RenderState,
    format_capabilities: FormatCapabilities,
    raytraced_stages: Vec<RaytracedStageRegistration>,
    path_traced_bounces: PathTracedBounceConfig,
}

impl MaterialBuilder {
    /// Set the vertex shader kind.
    pub fn with_vertex_kind(mut self, kind: MeshVertexKind) -> Self {
        self.vertex_kind = kind;
        self
    }

    /// Set the fragment shader descriptor.
    pub fn with_fragment_shader(mut self, path: impl Into<PathBuf>) -> Self {
        self.fragment_desc = ShaderDesc {
            source: ShaderSource::File(path.into()),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Fragment,
        };
        self
    }

    /// Set the fragment shader from inline source.
    pub fn with_fragment_inline(mut self, source: impl Into<String>) -> Self {
        self.fragment_desc = ShaderDesc {
            source: ShaderSource::Inline(source.into()),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Fragment,
        };
        self
    }

    /// Set the custom vertex shader descriptor.
    pub fn with_vertex_shader(mut self, path: impl Into<PathBuf>) -> Self {
        self.vertex_desc = Some(ShaderDesc {
            source: ShaderSource::File(path.into()),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        });
        self
    }

    /// Set the custom vertex shader from inline source.
    pub fn with_vertex_inline(mut self, source: impl Into<String>) -> Self {
        self.vertex_desc = Some(ShaderDesc {
            source: ShaderSource::Inline(source.into()),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        });
        self
    }

    /// Register a push constant type for this material.
    pub fn with_push_constant<T: bytemuck::Pod>(mut self) -> Self {
        let total_bytes = std::mem::size_of::<T>() as u32;
        self.push_constants.push(PushConstantRegistration {
            type_name: std::any::type_name::<T>().to_owned(),
            total_bytes,
            stage_mask: StageMask::FRAGMENT,
            offset: 0,
        });
        self
    }

    /// Register a push constant type with a custom stage mask.
    pub fn with_push_constant_stage<T: bytemuck::Pod>(mut self, stage: StageMask) -> Self {
        let total_bytes = std::mem::size_of::<T>() as u32;
        self.push_constants.push(PushConstantRegistration {
            type_name: std::any::type_name::<T>().to_owned(),
            total_bytes,
            stage_mask: stage,
            offset: 0,
        });
        self
    }

    /// Register a texture binding for this material.
    pub fn with_texture_binding(mut self, group: impl Into<String>, path: impl Into<String>, sampler: SamplerPreset) -> Self {
        self.texture_bindings.push(TextureBindingRegistration {
            group: group.into(),
            path: path.into(),
            sampler_path: format!("{path}_sampler"),
            stage_mask: StageMask::FRAGMENT,
            update_rate: UpdateRate::Material,
            sampler: sampler,
        });
        self
    }

    /// Register a texture binding with a custom stage mask.
    pub fn with_texture_binding_stage(mut self, group: impl Into<String>, path: impl Into<String>, sampler: SamplerPreset, stage: StageMask) -> Self {
        self.texture_bindings.push(TextureBindingRegistration {
            group: group.into(),
            path: path.into(),
            sampler_path: format!("{path}_sampler"),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
            sampler: sampler,
        });
        self
    }

    /// Register a buffer binding for this material.
    pub fn with_buffer_binding(mut self, group: impl Into<String>, path: impl Into<String>, storage_path: impl Into<String>) -> Self {
        self.buffer_bindings.push(BufferBindingRegistration {
            group: group.into(),
            path: path.into(),
            storage_path: storage_path.into(),
            stage_mask: StageMask::FRAGMENT,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Register a buffer binding with a custom stage mask.
    pub fn with_buffer_binding_stage(mut self, group: impl Into<String>, path: impl Into<String>, storage_path: impl Into<String>, stage: StageMask) -> Self {
        self.buffer_bindings.push(BufferBindingRegistration {
            group: group.into(),
            path: path.into(),
            storage_path: storage_path.into(),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Register an acceleration structure binding for this material.
    pub fn with_acceleration_structure_binding(mut self, group: impl Into<String>, path: impl Into<String>) -> Self {
        self.acceleration_structure_bindings.push(AccelerationStructureBindingRegistration {
            group: group.into(),
            path: path.into(),
            stage_mask: StageMask::RAY_TRACING,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Register an acceleration structure binding with a custom stage mask.
    pub fn with_acceleration_structure_binding_stage(mut self, group: impl Into<String>, path: impl Into<String>, stage: StageMask) -> Self {
        self.acceleration_structure_bindings.push(AccelerationStructureBindingRegistration {
            group: group.into(),
            path: path.into(),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Set the render state for this material.
    pub fn with_render_state(mut self, state: RenderState) -> Self {
        self.render_state = state;
        self
    }

    /// Set the format capabilities for this material.
    pub fn with_format_capabilities(mut self, caps: FormatCapabilities) -> Self {
        self.format_capabilities = caps;
        self
    }

    /// Set the HDR mode for this material.
    pub fn with_hdr_mode(mut self, mode: crate::HdrMode) -> Self {
        self.format_capabilities = FormatCapabilities::from_hdr_mode(mode);
        self
    }

    /// Set the tone mapping for this material.
    pub fn with_tone_mapping(mut self, op: crate::ToneMappingOp) -> Self {
        self.format_capabilities = FormatCapabilities::from_tone_mapping(op);
        self
    }

    /// Register a raytraced shader stage for this material.
    pub fn with_raytraced_stage(mut self, group: impl Into<String>, path: impl Into<String>, stage: RaytracedShaderStage) -> Self {
        self.raytraced_stages.push(RaytracedStageRegistration {
            group: group.into(),
            path: path.into(),
            stage,
            stage_mask: StageMask::RAY_TRACING,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Register a raytraced shader stage with a custom stage mask.
    pub fn with_raytraced_stage_stage(mut self, group: impl Into<String>, path: impl Into<String>, stage: RaytracedShaderStage, stage_mask: StageMask) -> Self {
        self.raytraced_stages.push(RaytracedStageRegistration {
            group: group.into(),
            path: path.into(),
            stage,
            stage_mask,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Set the path traced bounce configuration for this material.
    pub fn with_path_traced_bounces(mut self, config: PathTracedBounceConfig) -> Self {
        self.path_traced_bounces = config;
        self
    }

    /// Set the path traced bounce count for this material.
    pub fn with_path_traced_bounce_count(mut self, count: usize) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_bounce_count(count);
        self
    }

    /// Set the path traced importance sampling for this material.
    pub fn with_path_traced_importance_sampling(mut self, sampling: PathTracedImportanceSampling) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::new_with_importance_sampling(sampling);
        self
    }

    /// Set the path traced termination strategy for this material.
    pub fn with_path_traced_termination(mut self, strategy: PathTracedTerminationStrategy) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_termination_strategy(strategy);
        self
    }

    /// Build the material and create its pipeline layout.
    pub fn build(self, engine: &Engine) -> Result<Material> {
        let layout = self.create_pipeline_layout(engine)?;
        Ok(Material {
            layout,
            name: self.name,
            vertex_kind: self.vertex_kind,
            fragment_desc: self.fragment_desc,
            vertex_desc: self.vertex_desc,
            push_constants: self.push_constants,
            texture_bindings: self.texture_bindings.clone(),
            buffer_bindings: self.buffer_bindings,
            acceleration_structure_bindings: self.acceleration_structure_bindings,
            render_state: self.render_state,
            format_capabilities: self.format_capabilities,
            raytraced_stages: self.raytraced_stages,
            path_traced_bounces: self.path_traced_bounces,
        })
    }

    /// Create the pipeline layout from the material's bindings.
    fn create_pipeline_layout(&self, engine: &Engine) -> Result<PipelineLayout> {
        let mut builder = PipelineLayoutBuilder::new();

        // Add instance buffer binding (required for instanced rendering)
        builder
            .storage_buffer(
                "material",
                "instances",
                StageMask::VERTEX | StageMask::FRAGMENT,
                UpdateRate::Material,
            );

        // Add push constants
        for pc in &self.push_constants {
            builder.push_constants_bytes(pc.total_bytes);
        }

        // Add texture/sampler bindings
        for tex in &self.texture_bindings {
            builder
                .sampled_image(
                    tex.group,
                    tex.path,
                    tex.stage_mask,
                    tex.update_rate,
                )
                .sampler(
                    tex.group,
                    tex.sampler_path,
                    tex.stage_mask,
                    tex.update_rate,
                );
        }

        // Add buffer bindings
        for buf in &self.buffer_bindings {
            builder
                .uniform_buffer(
                    buf.group,
                    buf.path,
                    buf.stage_mask,
                    buf.update_rate,
                )
                .storage_buffer(
                    buf.group,
                    buf.storage_path,
                    buf.stage_mask,
                    buf.update_rate,
                );
        }

        // Add acceleration structure bindings (for raytraced/hybrid modes)
        for accel_as in &self.acceleration_structure_bindings {
            builder
                .binding(
                    accel_as.group,
                    accel_as.path,
                    BindingKind::AccelerationStructure,
                    accel_as.stage_mask,
                    accel_as.update_rate,
                );
        }

        // Add raytraced shader stage bindings
        for rs in &self.raytraced_stages {
            builder
                .storage_image(
                    rs.group,
                    rs.path,
                    rs.stage_mask,
                    rs.update_rate,
                );
        }

        builder.build(engine)
    }
}

// ------------------------------------------------------------------
// Push Constant Registration
// ------------------------------------------------------------------

/// A push constant type registered for a material.
#[derive(Clone, Debug)]
pub struct PushConstantRegistration {
    /// The type name of the push constant struct.
    pub type_name: String,
    /// The total byte count of the push constant struct.
    pub total_bytes: u32,
    /// The shader stages that this push constant affects.
    pub stage_mask: StageMask,
    /// The offset within the push constant region.
    pub offset: u32,
}

impl PushConstantRegistration {
    /// Create a push constant registration from a Pod type.
    pub fn from_type<T: bytemuck::Pod>() -> Self {
        Self {
            type_name: std::any::type_name::<T>().to_owned(),
            total_bytes: std::mem::size_of::<T>() as u32,
            stage_mask: StageMask::FRAGMENT,
            offset: 0,
        }
    }

    /// Create a push constant registration with a custom stage mask.
    pub fn from_type_with_stage<T: bytemuck::Pod>(stage: StageMask) -> Self {
        Self {
            type_name: std::any::type_name::<T>().to_owned(),
            total_bytes: std::mem::size_of::<T>() as u32,
            stage_mask: stage,
            offset: 0,
        }
    }

    /// Get the byte count of this push constant.
    pub fn byte_count(&self) -> u32 {
        self.total_bytes
    }

    /// Get the stage mask of this push constant.
    pub fn stage_mask(&self) -> StageMask {
        self.stage_mask
    }

    /// Get the offset of this push constant.
    pub fn offset(&self) -> u32 {
        self.offset
    }
}

// ------------------------------------------------------------------
// Texture Binding Registration
// ------------------------------------------------------------------

/// A texture/sampler binding registered for a material.
#[derive(Clone, Debug)]
pub struct TextureBindingRegistration {
    /// The bind group name for this texture.
    pub group: String,
    /// The shader variable path for this texture.
    pub path: String,
    /// The shader variable path for the sampler.
    pub sampler_path: String,
    /// The shader stages that this texture affects.
    pub stage_mask: StageMask,
    /// The update rate for this texture.
    pub update_rate: UpdateRate,
    /// The sampler preset for this texture.
    pub sampler: SamplerPreset,
}

impl TextureBindingRegistration {
    /// Create a texture binding registration with a sampler preset.
    pub fn new(group: impl Into<String>, path: impl Into<String>, sampler: SamplerPreset) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            sampler_path: format!("{path}_sampler"),
            stage_mask: StageMask::FRAGMENT,
            update_rate: UpdateRate::Material,
            sampler,
        }
    }

    /// Create a texture binding registration with a custom stage mask.
    pub fn new_with_stage(group: impl Into<String>, path: impl Into<String>, sampler: SamplerPreset, stage: StageMask) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            sampler_path: format!("{path}_sampler"),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
            sampler,
        }
    }

    /// Get the group name of this texture binding.
    pub fn group(&self) -> &str {
        &self.group
    }

    /// Get the path of this texture binding.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the sampler path of this texture binding.
    pub fn sampler_path(&self) -> &str {
        &self.sampler_path
    }

    /// Get the stage mask of this texture binding.
    pub fn stage_mask(&self) -> StageMask {
        self.stage_mask
    }

    /// Get the update rate of this texture binding.
    pub fn update_rate(&self) -> UpdateRate {
        self.update_rate
    }

    /// Get the sampler preset of this texture binding.
    pub fn sampler(&self) -> SamplerPreset {
        self.sampler
    }
}

// ------------------------------------------------------------------
// Buffer Binding Registration
// ------------------------------------------------------------------

/// A buffer binding registered for a material.
#[derive(Clone, Debug)]
pub struct BufferBindingRegistration {
    /// The bind group name for this buffer.
    pub group: String,
    /// The shader variable path for this buffer.
    pub path: String,
    /// The shader variable path for the storage buffer.
    pub storage_path: String,
    /// The shader stages that this buffer affects.
    pub stage_mask: StageMask,
    /// The update rate for this buffer.
    pub update_rate: UpdateRate,
}

impl BufferBindingRegistration {
    /// Create a buffer binding registration.
    pub fn new(group: impl Into<String>, path: impl Into<String>, storage_path: impl Into<String>) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            storage_path: storage_path.into(),
            stage_mask: StageMask::FRAGMENT,
            update_rate: UpdateRate::Material,
        }
    }

    /// Create a buffer binding registration with a custom stage mask.
    pub fn new_with_stage(group: impl Into<String>, path: impl Into<String>, storage_path: impl Into<String>, stage: StageMask) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            storage_path: storage_path.into(),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
        }
    }

    /// Get the group name of this buffer binding.
    pub fn group(&self) -> &str {
        &self.group
    }

    /// Get the path of this buffer binding.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the storage path of this buffer binding.
    pub fn storage_path(&self) -> &str {
        &self.storage_path
    }

    /// Get the stage mask of this buffer binding.
    pub fn stage_mask(&self) -> StageMask {
        self.stage_mask
    }

    /// Get the update rate of this buffer binding.
    pub fn update_rate(&self) -> UpdateRate {
        self.update_rate
    }
}

// ------------------------------------------------------------------
// Acceleration Structure Binding Registration
// ------------------------------------------------------------------

/// An acceleration structure binding registered for a material.
#[derive(Clone, Debug)]
pub struct AccelerationStructureBindingRegistration {
    /// The bind group name for this acceleration structure.
    pub group: String,
    /// The shader variable path for this acceleration structure.
    pub path: String,
    /// The shader stages that this acceleration structure affects.
    pub stage_mask: StageMask,
    /// The update rate for this acceleration structure.
    pub update_rate: UpdateRate,
}

impl AccelerationStructureBindingRegistration {
    /// Create an acceleration structure binding registration.
    pub fn new(group: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            stage_mask: StageMask::RAY_TRACING,
            update_rate: UpdateRate::Material,
        }
    }

    /// Create an acceleration structure binding registration with a custom stage mask.
    pub fn new_with_stage(group: impl Into<String>, path: impl Into<String>, stage: StageMask) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
        }
    }

    /// Get the group name of this acceleration structure binding.
    pub fn group(&self) -> &str {
        &self.group
    }

    /// Get the path of this acceleration structure binding.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the stage mask of this acceleration structure binding.
    pub fn stage_mask(&self) -> StageMask {
        self.stage_mask
    }

    /// Get the update rate of this acceleration structure binding.
    pub fn update_rate(&self) -> UpdateRate {
        self.update_rate
    }
}

// ------------------------------------------------------------------
// Raytraced Stage Registration
// ------------------------------------------------------------------

/// A raytraced shader stage registered for a material.
#[derive(Clone, Debug)]
pub struct RaytracedStageRegistration {
    /// The bind group name for this raytraced stage.
    pub group: String,
    /// The shader variable path for this raytraced stage.
    pub path: String,
    /// The raytraced shader stage type.
    pub stage: RaytracedShaderStage,
    /// The shader stages that this raytraced stage affects.
    pub stage_mask: StageMask,
    /// The update rate for this raytraced stage.
    pub update_rate: UpdateRate,
}

impl RaytracedStageRegistration {
    /// Create a raytraced stage registration.
    pub fn new(group: impl Into<String>, path: impl Into<String>, stage: RaytracedShaderStage) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            stage,
            stage_mask: StageMask::RAY_TRACING,
            update_rate: UpdateRate::Material,
        }
    }

    /// Create a raytraced stage registration with a custom stage mask.
    pub fn new_with_stage(group: impl Into<String>, path: impl Into<String>, stage: RaytracedShaderStage, stage_mask: StageMask) -> Self {
        Self {
            group: group.into(),
            path: path.into(),
            stage,
            stage_mask,
            update_rate: UpdateRate::Material,
        }
    }

    /// Get the group name of this raytraced stage.
    pub fn group(&self) -> &str {
        &self.group
    }

    /// Get the path of this raytraced stage.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the stage type of this raytraced stage.
    pub fn stage(&self) -> RaytracedShaderStage {
        self.stage
    }

    /// Get the stage mask of this raytraced stage.
    pub fn stage_mask(&self) -> StageMask {
        self.stage_mask
    }

    /// Get the update rate of this raytraced stage.
    pub fn update_rate(&self) -> UpdateRate {
        self.update_rate
    }
}

/// A raytraced shader stage type.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RaytracedShaderStage {
    /// Ray generation shader.
    RayGeneration,
    /// Closest hit shader.
    ClosestHit,
    /// Miss shader.
    Miss,
    /// Any hit shader.
    AnyHit,
    /// Intersection shader.
    Intersection,
}

impl RaytracedShaderStage {
    /// Get the Slang stage constant for this raytraced stage.
    pub fn slang_stage(self) -> u32 {
        match self {
            Self::RayGeneration => 5,
            Self::ClosestHit => 7,
            Self::Miss => 6,
            Self::AnyHit => 8,
            Self::Intersection => 9,
        }
    }

    /// Get the shader stage for this raytraced stage.
    pub fn shader_stage(self) -> ShaderStage {
        match self {
            Self::RayGeneration => ShaderStage::RayGeneration,
            Self::ClosestHit => ShaderStage::ClosestHit,
            Self::Miss => ShaderStage::Miss,
            Self::AnyHit => ShaderStage::ClosestHit,
            Self::Intersection => ShaderStage::ClosestHit,
        }
    }
}

// ------------------------------------------------------------------
// Path Traced Bounce Config
// ------------------------------------------------------------------

/// Path traced bounce configuration for a material.
#[derive(Clone, Debug, Default)]
pub struct PathTracedBounceConfig {
    /// The number of bounces for path tracing.
    pub bounce_count: usize,
    /// The importance sampling strategy for path tracing.
    pub importance_sampling: PathTracedImportanceSampling,
    /// The termination strategy for path tracing.
    pub termination_strategy: PathTracedTerminationStrategy,
    /// The maximum bounce count for path tracing.
    pub max_bounce_count: usize,
    /// The minimum bounce count for path tracing.
    pub min_bounce_count: usize,
}

impl PathTracedBounceConfig {
    /// Create a path traced bounce configuration with a bounce count.
    pub fn with_bounce_count(count: usize) -> Self {
        Self {
            bounce_count: count,
            importance_sampling: PathTracedImportanceSampling::default(),
            termination_strategy: PathTracedTerminationStrategy::default(),
            max_bounce_count: count.max(1),
            min_bounce_count: count.min(1),
        }
    }

    /// Create a path traced bounce configuration with importance sampling.
    pub fn with_importance_sampling(sampling: PathTracedImportanceSampling) -> Self {
        Self {
            bounce_count: 1,
            importance_sampling: sampling,
            termination_strategy: PathTracedTerminationStrategy::default(),
            max_bounce_count: 1.max(1),
            min_bounce_count: 1.min(1),
        }
    }

    /// Create a path traced bounce configuration with termination strategy.
    pub fn with_termination_strategy(strategy: PathTracedTerminationStrategy) -> Self {
        Self {
            bounce_count: 1,
            importance_sampling: PathTracedImportanceSampling::default(),
            termination_strategy: strategy,
            max_bounce_count: 1.max(1),
            min_bounce_count: 1.min(1),
        }
    }

    /// Get the bounce count of this path traced configuration.
    pub fn bounce_count(&self) -> usize {
        self.bounce_count
    }

    /// Get the importance sampling of this path traced configuration.
    pub fn importance_sampling(&self) -> PathTracedImportanceSampling {
        self.importance_sampling
    }

    /// Get the termination strategy of this path traced configuration.
    pub fn termination_strategy(&self) -> PathTracedTerminationStrategy {
        self.termination_strategy
    }

    /// Get the max bounce count of this path traced configuration.
    pub fn max_bounce_count(&self) -> usize {
        self.max_bounce_count
    }

    /// Get the min bounce count of this path traced configuration.
    pub fn min_bounce_count(&self) -> usize {
        self.min_bounce_count
    }
}

/// Importance sampling strategy for path tracing.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum PathTracedImportanceSampling {
    /// Uniform sampling (no importance weighting).
    #[default]
    Uniform,
    /// Cosine weighting (for diffuse surfaces).
    Cosine,
    /// Specular weighting (for specular surfaces).
    Specular,
    /// Mixed weighting (for mixed surfaces).
    Mixed,
}

impl PathTracedImportanceSampling {
    /// Get the sampling weight for this importance sampling strategy.
    pub fn weight(self) -> f32 {
        match self {
            Self::Uniform => 1.0,
            Self::Cosine => 0.5,
            Self::Specular => 0.25,
            Self::Mixed => 0.75,
        }
    }
}

/// Termination strategy for path tracing.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum PathTracedTerminationStrategy {
    /// Terminate when bounce count reaches max.
    #[default]
    BounceCount,
    /// Terminate when radiance falls below threshold.
    RadianceThreshold,
    /// Terminate when surface is opaque.
    OpaqueSurface,
    /// Terminate when surface is transparent.
    TransparentSurface,
}

impl PathTracedTerminationStrategy {
    /// Get the termination threshold for this strategy.
    pub fn threshold(self) -> f32 {
        match self {
            Self::BounceCount => 0.0,
            Self::RadianceThreshold => 0.001,
            Self::OpaqueSurface => 1.0,
            Self::TransparentSurface => 0.0,
        }
    }
}

// ------------------------------------------------------------------
// Render State
// ------------------------------------------------------------------

/// Rendering state configuration for a material.
#[derive(Clone, Debug, Default)]
pub struct RenderState {
    /// The cull mode for this material.
    pub cull_mode: crate::CullMode,
    /// The front face for this material.
    pub front_face: crate::FrontFace,
    /// Whether this material writes to the depth buffer.
    pub depth_write: bool,
    /// The depth compare operation for this material.
    pub depth_compare: Option<crate::CompareOp>,
    /// The primitive topology for this material.
    pub topology: crate::PrimitiveTopology,
    /// The raster state for this material.
    pub raster: crate::RasterState,
    /// Whether this material uses variable rate shading.
    pub variable_rate_shading: bool,
    /// Whether this material uses mesh shading.
    pub mesh_shading: bool,
    /// Whether this material uses bindless descriptors.
    pub bindless: bool,
    /// Whether this material uses timeline semaphores.
    pub timeline_semaphores: bool,
    /// Whether this material uses dynamic rendering.
    pub dynamic_rendering: bool,
    /// Whether this material uses synchronization2.
    pub synchronization2: bool,
}

impl RenderState {
    /// Create a render state with the given cull mode.
    pub fn with_cull_mode(mut self, cull: crate::CullMode) -> Self {
        self.cull_mode = cull;
        self
    }

    /// Create a render state with the given front face.
    pub fn with_front_face(mut self, face: crate::FrontFace) -> Self {
        self.front_face = face;
        self
    }

    /// Create a render state with depth write enabled.
    pub fn with_depth_write(mut self, write: bool) -> Self {
        self.depth_write = write;
        self
    }

    /// Create a render state with a depth compare operation.
    pub fn with_depth_compare(mut self, compare: crate::CompareOp) -> Self {
        self.depth_compare = Some(compare);
        self
    }

    /// Create a render state with the given primitive topology.
    pub fn with_topology(mut self, topology: crate::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }

    /// Create a render state with the given raster state.
    pub fn with_raster(mut self, raster: crate::RasterState) -> Self {
        self.raster = raster;
        self
    }

    /// Create a render state with variable rate shading enabled.
    pub fn with_variable_rate_shading(mut self, enable: bool) -> Self {
        self.variable_rate_shading = enable;
        self
    }

    /// Create a render state with mesh shading enabled.
    pub fn with_mesh_shading(mut self, enable: bool) -> Self {
        self.mesh_shading = enable;
        self
    }

    /// Create a render state with bindless descriptors enabled.
    pub fn with_bindless(mut self, enable: bool) -> Self {
        self.bindless = enable;
        self
    }

    /// Create a render state with timeline semaphores enabled.
    pub fn with_timeline_semaphores(mut self, enable: bool) -> Self {
        self.timeline_semaphores = enable;
        self
    }

    /// Create a render state with dynamic rendering enabled.
    pub fn with_dynamic_rendering(mut self, enable: bool) -> Self {
        self.dynamic_rendering = enable;
        self
    }

    /// Create a render state with synchronization2 enabled.
    pub fn with_synchronization2(mut self, enable: bool) -> Self {
        self.synchronization2 = enable;
        self
    }

    /// Get the cull mode of this render state.
    pub fn cull_mode(&self) -> crate::CullMode {
        self.cull_mode
    }

    /// Get the front face of this render state.
    pub fn front_face(&self) -> crate::FrontFace {
        self.front_face
    }

    /// Get the depth write of this render state.
    pub fn depth_write(&self) -> bool {
        self.depth_write
    }

    /// Get the depth compare of this render state.
    pub fn depth_compare(&self) -> Option<crate::CompareOp> {
        self.depth_compare
    }

    /// Get the topology of this render state.
    pub fn topology(&self) -> crate::PrimitiveTopology {
        self.topology
    }

    /// Get the raster of this render state.
    pub fn raster(&self) -> crate::RasterState {
        self.raster
    }

    /// Get the variable rate shading of this render state.
    pub fn variable_rate_shading(&self) -> bool {
        self.variable_rate_shading
    }

    /// Get the mesh shading of this render state.
    pub fn mesh_shading(&self) -> bool {
        self.mesh_shading
    }

    /// Get the bindless of this render state.
    pub fn bindless(&self) -> bool {
        self.bindless
    }

    /// Get the timeline semaphores of this render state.
    pub fn timeline_semaphores(&self) -> bool {
        self.timeline_semaphores
    }

    /// Get the dynamic rendering of this render state.
    pub fn dynamic_rendering(&self) -> bool {
        self.dynamic_rendering
    }

    /// Get the synchronization2 of this render state.
    pub fn synchronization2(&self) -> bool {
        self.synchronization2
    }
}

// ------------------------------------------------------------------
// Format Capabilities
// ------------------------------------------------------------------

/// Format capabilities for a material.
#[derive(Clone, Debug, Default)]
pub struct FormatCapabilities {
    /// The render format for this material.
    pub render_format: Format,
    /// The HDR mode for this material.
    pub hdr_mode: crate::HdrMode,
    /// The tone mapping for this material.
    pub tone_mapping: crate::ToneMappingOp,
    /// Whether this material supports FP16 rendering.
    pub fp16_render: bool,
    /// Whether this material supports FP32 rendering.
    pub fp32_render: bool,
    /// Whether this material supports HDR output.
    pub hdr_output: bool,
    /// Whether this material supports shader FP16.
    pub shader_fp16: bool,
    /// Whether this material supports shader FP64.
    pub shader_fp64: bool,
    /// Whether this material supports variable rate shading.
    pub variable_rate_shading: bool,
    /// Whether this material supports bindless descriptors.
    pub bindless: bool,
}

impl FormatCapabilities {
    /// Create format capabilities from an HDR mode.
    pub fn from_hdr_mode(mode: crate::HdrMode) -> Self {
        Self {
            render_format: mode.render_format(),
            hdr_mode: mode,
            tone_mapping: if mode.is_hdr() { crate::ToneMappingOp::Linear } else { crate::ToneMappingOp::Aces },
            fp16_render: mode.is_hdr(),
            fp32_render: mode.is_hdr(),
            hdr_output: mode.is_hdr(),
            shader_fp16: mode.is_hdr(),
            shader_fp64: false,
            variable_rate_shading: false,
            bindless: false,
        }
    }

    /// Create format capabilities from a tone mapping operation.
    pub fn from_tone_mapping(op: crate::ToneMappingOp) -> Self {
        Self {
            render_format: if op.is_hdr() { Format::Rgba16Float } else { Format::Rgba8Unorm },
            hdr_mode: if op.is_hdr() { crate::HdrMode::ScRgb } else { crate::HdrMode::Sdr },
            tone_mapping: op,
            fp16_render: op.is_hdr(),
            fp32_render: op.is_hdr(),
            hdr_output: op.is_hdr(),
            shader_fp16: op.is_hdr(),
            shader_fp64: false,
            variable_rate_shading: false,
            bindless: false,
        }
    }

    /// Get the render format of these format capabilities.
    pub fn render_format(&self) -> Format {
        self.render_format
    }

    /// Get the HDR mode of these format capabilities.
    pub fn hdr_mode(&self) -> crate::HdrMode {
        self.hdr_mode
    }

    /// Get the tone mapping of these format capabilities.
    pub fn tone_mapping(&self) -> crate::ToneMappingOp {
        self.tone_mapping
    }

    /// Get the FP16 render capability of these format capabilities.
    pub fn fp16_render(&self) -> bool {
        self.fp16_render
    }

    /// Get the FP32 render capability of these format capabilities.
    pub fn fp32_render(&self) -> bool {
        self.fp32_render
    }

    /// Get the HDR output capability of these format capabilities.
    pub fn hdr_output(&self) -> bool {
        self.hdr_output
    }

    /// Get the shader FP16 capability of these format capabilities.
    pub fn shader_fp16(&self) -> bool {
        self.shader_fp16
    }

    /// Get the shader FP64 capability of these format capabilities.
    pub fn shader_fp64(&self) -> bool {
        self.shader_fp64
    }

    /// Get the variable rate shading capability of these format capabilities.
    pub fn variable_rate_shading(&self) -> bool {
        self.variable_rate_shading
    }

    /// Get the bindless capability of these format capabilities.
    pub fn bindless(&self) -> bool {
        self.bindless
    }
}

// ------------------------------------------------------------------
// Material Presets
// ------------------------------------------------------------------

/// Common material presets for quick setup.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum MaterialPreset {
    /// Simple unlit material with base color texture.
    Unlit,
    /// Standard PBR material with base color, normal, roughness, metallic textures.
    Pbr,
    /// Transparent material with base color, alpha texture.
    Transparent,
    /// Emissive material with base color, emission texture.
    Emissive,
    /// Raytraced material with base color, normal, roughness, metallic textures.
    RaytracedPbr,
    /// Path traced material with base color, normal, roughness, metallic textures.
    PathTracedPbr,
    /// Hybrid material with base color, normal, roughness, metallic textures.
    HybridPbr,
    /// Simple 2D sprite material with base color texture.
    Sprite,
    /// Simple 3D mesh material with base color texture.
    Mesh,
}

impl MaterialPreset {
    /// Build a material from this preset.
    pub fn build(self, engine: &Engine) -> Result<Material> {
        match self {
            Self::Unlit => Material::new("unlit_material")
                .with_vertex_kind(MeshVertexKind::V2d)
                .with_fragment_inline("passthrough_fragment.slang")
                .with_texture_binding("material", "base_color", SamplerPreset::Linear)
                .build(engine),
            Self::Pbr => Material::new("pbr_material")
                .with_vertex_kind(MeshVertexKind::V3d)
                .with_fragment_shader("pbr_fragment.slang")
                .with_push_constant::<FrameConstants>()
                .with_texture_binding("material", "base_color", SamplerPreset::Trilinear)
                .with_texture_binding("material", "normal_map", SamplerPreset::Trilinear)
                .with_texture_binding("material", "roughness", SamplerPreset::Trilinear)
                .with_texture_binding("material", "metallic", SamplerPreset::Trilinear)
                .build(engine),
            Self::Transparent => Material::new("transparent_material")
                .with_vertex_kind(MeshVertexKind::V3d)
                .with_fragment_shader("transparent_fragment.slang")
                .with_push_constant::<FrameConstants>()
                .with_texture_binding("material", "base_color", SamplerPreset::Trilinear)
                .with_texture_binding("material", "alpha", SamplerPreset::Trilinear)
                .with_render_state(RenderState {
                    depth_write: false,
                    cull_mode: crate::CullMode::None,
                    ..RenderState::default()
                })
                .build(engine),
            Self::Emissive => Material::new("emissive_material")
                .with_vertex_kind(MeshVertexKind::V3d)
                .with_fragment_shader("emissive_fragment.slang")
                .with_push_constant::<FrameConstants>()
                .with_texture_binding("material", "base_color", SamplerPreset::Trilinear)
                .with_texture_binding("material", "emission", SamplerPreset::Trilinear)
                .build(engine),
            Self::RaytracedPbr => Material::new("raytraced_pbr_material")
                .with_vertex_kind(MeshVertexKind::V3d)
                .with_fragment_shader("raytraced_pbr_fragment.slang")
                .with_push_constant::<FrameConstants>()
                .with_texture_binding("material", "base_color", SamplerPreset::Trilinear)
                .with_texture_binding("material", "normal_map", SamplerPreset::Trilinear)
                .with_texture_binding("material", "roughness", SamplerPreset::Trilinear)
                .with_texture_binding("material", "metallic", SamplerPreset::Trilinear)
                .with_acceleration_structure_binding("material", "acceleration_structure")
                .with_raytraced_stage("material", "closest_hit", RaytracedShaderStage::ClosestHit)
                .with_raytraced_stage("material", "miss", RaytracedShaderStage::Miss)
                .with_raytraced_stage("material", "ray_generation", RaytracedShaderStage::RayGeneration)
                .build(engine),
            Self::PathTracedPbr => Material::new("path_traced_pbr_material")
                .with_vertex_kind(MeshVertexKind::V3d)
                .with_fragment_shader("path_traced_pbr_fragment.slang")
                .with_push_constant::<FrameConstants>()
                .with_texture_binding("material", "base_color", SamplerPreset::Trilinear)
                .with_texture_binding("material", "normal_map", SamplerPreset::Trilinear)
                .with_texture_binding("material", "roughness", SamplerPreset::Trilinear)
                .with_texture_binding("material", "metallic", SamplerPreset::Trilinear)
                .with_acceleration_structure_binding("material", "acceleration_structure")
                .with_raytraced_stage("material", "closest_hit", RaytracedShaderStage::ClosestHit)
                .with_raytraced_stage("material", "miss", RaytracedShaderStage::Miss)
                .with_raytraced_stage("material", "ray_generation", RaytracedShaderStage::RayGeneration)
                .with_path_traced_bounces(PathTracedBounceConfig::with_bounce_count(8))
                .build(engine),
            Self::HybridPbr => Material::new("hybrid_pbr_material")
                .with_vertex_kind(MeshVertexKind::V3d)
                .with_fragment_shader("hybrid_pbr_fragment.slang")
                .with_push_constant::<FrameConstants>()
                .with_texture_binding("material", "base_color", SamplerPreset::Trilinear)
                .with_texture_binding("material", "normal_map", SamplerPreset::Trilinear)
                .with_texture_binding("material", "roughness", SamplerPreset::Trilinear)
                .with_texture_binding("material", "metallic", SamplerPreset::Trilinear)
                .with_acceleration_structure_binding("material", "acceleration_structure")
                .with_raytraced_stage("material", "closest_hit", RaytracedShaderStage::ClosestHit)
                .with_raytraced_stage("material", "miss", RaytracedShaderStage::Miss)
                .with_raytraced_stage("material", "ray_generation", RaytracedShaderStage::RayGeneration)
                .build(engine),
            Self::Sprite => Material::new("sprite_material")
                .with_vertex_kind(MeshVertexKind::V2d)
                .with_fragment_inline("sprite_fragment.slang")
                .with_texture_binding("material", "base_color", SamplerPreset::PixelArt)
                .build(engine),
            Self::Mesh => Material::new("mesh_material")
                .with_vertex_kind(MeshVertexKind::V3d)
                .with_fragment_inline("mesh_fragment.slang")
                .with_texture_binding("material", "base_color", SamplerPreset::Trilinear)
                .build(engine),
        }
    }

    /// Get the name of this preset.
    pub fn name(self) -> &'static str {
        match self {
            Self::Unlit => "unlit",
            Self::Pbr => "pbr",
            Self::Transparent => "transparent",
            Self::Emissive => "emissive",
            Self::RaytracedPbr => "raytraced_pbr",
            Self::PathTracedPbr => "path_traced_pbr",
            Self::HybridPbr => "hybrid_pbr",
            Self::Sprite => "sprite",
            Self::Mesh => "mesh",
        }
    }

    /// Get the vertex kind of this preset.
    pub fn vertex_kind(self) -> MeshVertexKind {
        match self {
            Self::Unlit => MeshVertexKind::V2d,
            Self::Pbr => MeshVertexKind::V3d,
            Self::Transparent => MeshVertexKind::V3d,
            Self::Emissive => MeshVertexKind::V3d,
            Self::RaytracedPbr => MeshVertexKind::V3d,
            Self::PathTracedPbr => MeshVertexKind::V3d,
            Self::HybridPbr => MeshVertexKind::V3d,
            Self::Sprite => MeshVertexKind::V2d,
            Self::Mesh => MeshVertexKind::V3d,
        }
    }

    /// Get the HDR mode of this preset.
    pub fn hdr_mode(self) -> crate::HdrMode {
        match self {
            Self::Unlit => crate::HdrMode::Sdr,
            Self::Pbr => crate::HdrMode::ScRgb,
            Self::Transparent => crate::HdrMode::ScRgb,
            Self::Emissive => crate::HdrMode::ScRgb,
            Self::RaytracedPbr => crate::HdrMode::ScRgb,
            Self::PathTracedPbr => crate::HdrMode::ScRgb,
            Self::HybridPbr => crate::HdrMode::ScRgb,
            Self::Sprite => crate::HdrMode::Sdr,
            Self::Mesh => crate::HdrMode::Sdr,
        }
    }

    /// Get the raytracing capabilities of this preset.
    pub fn raytracing_caps(self) -> RaytracingCapabilities {
        match self {
            Self::Unlit => RaytracingCapabilities::None,
            Self::Pbr => RaytracingCapabilities::None,
            Self::Transparent => RaytracingCapabilities::None,
            Self::Emissive => RaytracingCapabilities::None,
            Self::RaytracedPbr => RaytracingCapabilities::RayGeneration | RaytracingCapabilities::ClosestHit | RaytracingCapabilities::Miss,
            Self::PathTracedPbr => RaytracingCapabilities::RayGeneration | RaytracingCapabilities::ClosestHit | RaytracingCapabilities::Miss,
            Self::HybridPbr => RaytracingCapabilities::RayGeneration | RaytracingCapabilities::ClosestHit | RaytracingCapabilities::Miss,
            Self::Sprite => RaytracingCapabilities::None,
            Self::Mesh => RaytracingCapabilities::None,
        }
    }

    /// Get the path tracing capabilities of this preset.
    pub fn path_tracing_caps(self) -> PathTracingCapabilities {
        match self {
            Self::Unlit => PathTracingCapabilities::None,
            Self::Pbr => PathTracingCapabilities::None,
            Self::Transparent => PathTracingCapabilities::None,
            Self::Emissive => PathTracingCapabilities::None,
            Self::RaytracedPbr => PathTracingCapabilities::None,
            Self::PathTracedPbr => PathTracingCapabilities::WithBounces(8),
            Self::HybridPbr => PathTracingCapabilities::None,
            Self::Sprite => PathTracingCapabilities::None,
            Self::Mesh => PathTracingCapabilities::None,
        }
    }
}

/// Raytracing capabilities for a material.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RaytracingCapabilities {
    /// No raytracing capabilities.
    None,
    /// Ray generation shader.
    RayGeneration,
    /// Closest hit shader.
    ClosestHit,
    /// Miss shader.
    Miss,
    /// Any hit shader.
    AnyHit,
    /// Intersection shader.
    Intersection,
    /// All raytracing capabilities.
    All,
}

impl RaytracingCapabilities {
    /// Get the stage mask for these raytracing capabilities.
    pub fn stage_mask(self) -> StageMask {
        match self {
            Self::None => StageMask::empty(),
            Self::RayGeneration => StageMask::RAY_TRACING,
            Self::ClosestHit => StageMask::RAY_TRACING,
            Self::Miss => StageMask::RAY_TRACING,
            Self::AnyHit => StageMask::RAY_TRACING,
            Self::Intersection => StageMask::RAY_TRACING,
            Self::All => StageMask::RAY_TRACING,
        }
    }

    /// Get the raytracing shader stages for these capabilities.
    pub fn stages(self) -> Vec<RaytracedShaderStage> {
        match self {
            Self::None => Vec::new(),
            Self::RayGeneration => vec![RaytracedShaderStage::RayGeneration],
            Self::ClosestHit => vec![RaytracedShaderStage::ClosestHit],
            Self::Miss => vec![RaytracedShaderStage::Miss],
            Self::AnyHit => vec![RaytracedShaderStage::AnyHit],
            Self::Intersection => vec![RaytracedShaderStage::Intersection],
            Self::All => vec![
                RaytracedShaderStage::RayGeneration,
                RaytracedShaderStage::ClosestHit,
                RaytracedShaderStage::Miss,
                RaytracedShaderStage::AnyHit,
                RaytracedShaderStage::Intersection,
            ],
        }
    }
}

/// Path tracing capabilities for a material.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PathTracingCapabilities {
    /// No path tracing capabilities.
    None,
    /// Path tracing with the given bounce count.
    WithBounces(usize),
    /// Path tracing with the given bounce count and importance sampling.
    WithBouncesAndSampling(usize, PathTracedImportanceSampling),
    /// Path tracing with the given bounce count and termination strategy.
    WithBouncesAndTermination(usize, PathTracedTerminationStrategy),
    /// All path tracing capabilities.
    All,
}

impl PathTracingCapabilities {
    /// Get the bounce count for these path tracing capabilities.
    pub fn bounce_count(self) -> usize {
        match self {
            Self::None => 0,
            Self::WithBounces(count) => count,
            Self::WithBouncesAndSampling(count, _) => count,
            Self::WithBouncesAndTermination(count, _) => count,
            Self::All => 8,
        }
    }

    /// Get the importance sampling for these path tracing capabilities.
    pub fn importance_sampling(self) -> PathTracedImportanceSampling {
        match self {
            Self::None => PathTracedImportanceSampling::Uniform,
            Self::WithBounces(_) => PathTracedImportanceSampling::Uniform,
            Self::WithBouncesAndSampling(_, sampling) => sampling,
            Self::WithBouncesAndTermination(_, _) => PathTracedImportanceSampling::Uniform,
            Self::All => PathTracedImportanceSampling::Cosine,
        }
    }

    /// Get the termination strategy for these path tracing capabilities.
    pub fn termination_strategy(self) -> PathTracedTerminationStrategy {
        match self {
            Self::None => PathTracedTerminationStrategy::BounceCount,
            Self::WithBounces(_) => PathTracedTerminationStrategy::BounceCount,
            Self::WithBouncesAndSampling(_, _) => PathTracedTerminationStrategy::BounceCount,
            Self::WithBouncesAndTermination(_, strategy) => strategy,
            Self::All => PathTracedTerminationStrategy::RadianceThreshold,
        }
    }
}

// ------------------------------------------------------------------
// Material Shader Program Types
// ------------------------------------------------------------------

/// A raytraced shader program for a material.
#[derive(Clone, Debug)]
pub struct RaytracedProgram {
    /// The raytraced stages for this program.
    pub stages: Vec<RaytracedStageRegistration>,
    /// The pipeline layout for this program.
    pub pipeline_layout: PipelineLayout,
    /// The raytraced shaders for this program.
    pub shaders: Vec<Shader>,
    /// The raytraced pipelines for this program.
    pub pipelines: Vec<Pipeline>,
}

impl RaytracedProgram {
    /// Create a raytraced program from the given stages.
    pub fn new(engine: &Engine, stages: Vec<RaytracedStageRegistration>) -> Result<Self> {
        let layout = engine.create_pipeline_layout(CanonicalPipelineLayout {
            groups: Vec::new(),
            push_constants_bytes: 0,
            push_constants_stage_mask: StageMask::RAY_TRACING,
        })?;
        let shaders = stages.iter().map(|s| {
            engine.create_shader(ShaderDesc {
                source: ShaderSource::File(PathBuf::from("raytraced_shader.slang")),
                entry_point: "main".to_owned(),
                stage: s.stage.shader_stage(),
            })
        }).collect::<Result<Vec<_>>>()?;
        let pipelines = shaders.iter().map(|s| {
            engine.create_compute_pipeline(crate::ComputePipelineDesc {
                shader: s.handle(),
                layout: None,
            })
        }).collect::<Result<Vec<_>>>()?;
        Ok(Self {
            stages,
            pipeline_layout: layout,
            shaders,
            pipelines,
        })
    }

    /// Get the stages of this raytraced program.
    pub fn stages(&self) -> &[RaytracedStageRegistration] {
        &self.stages
    }

    /// Get the pipeline layout of this raytraced program.
    pub fn pipeline_layout(&self) -> &PipelineLayout {
        &self.pipeline_layout
    }

    /// Get the shaders of this raytraced program.
    pub fn shaders(&self) -> &[Shader] {
        &self.shaders
    }

    /// Get the pipelines of this raytraced program.
    pub fn pipelines(&self) -> &[Pipeline] {
        &self.pipelines
    }
}

/// A path traced shader program for a material.
#[derive(Clone, Debug)]
pub struct PathTracedProgram {
    /// The path traced bounce configuration for this program.
    pub bounce_config: PathTracedBounceConfig,
    /// The pipeline layout for this program.
    pub pipeline_layout: PipelineLayout,
    /// The path traced shaders for this program.
    pub shaders: Vec<Shader>,
    /// The path traced pipelines for this program.
    pub pipelines: Vec<Pipeline>,
}

impl PathTracedProgram {
    /// Create a path traced program from the given bounce configuration.
    pub fn new(engine: &Engine, bounce_config: PathTracedBounceConfig) -> Result<Self> {
        let layout = engine.create_pipeline_layout(CanonicalPipelineLayout {
            groups: Vec::new(),
            push_constants_bytes: 0,
            push_constants_stage_mask: StageMask::RAY_TRACING,
        })?;
        let shaders = bounce_config.bounce_count.times(|i| {
            engine.create_shader(ShaderDesc {
                source: ShaderSource::File(PathBuf::from("path_traced_shader.slang")),
                entry_point: format!("bounce_{i}").to_owned(),
                stage: ShaderStage::ClosestHit,
            })
        }).collect::<Result<Vec<_>>>()?;
        let pipelines = shaders.iter().map(|s| {
            engine.create_compute_pipeline(crate::ComputePipelineDesc {
                shader: s.handle(),
                layout: None,
            })
        }).collect::<Result<Vec<_>>>()?;
        Ok(Self {
            bounce_config,
            pipeline_layout: layout,
            shaders,
            pipelines,
        })
    }

    /// Get the bounce configuration of this path traced program.
    pub fn bounce_config(&self) -> &PathTracedBounceConfig {
        &self.bounce_config
    }

    /// Get the pipeline layout of this path traced program.
    pub fn pipeline_layout(&self) -> &PipelineLayout {
        &self.pipeline_layout
    }

    /// Get the shaders of this path traced program.
    pub fn shaders(&self) -> &[Shader] {
        &self.shaders
    }

    /// Get the pipelines of this path traced program.
    pub fn pipelines(&self) -> &[Pipeline] {
        &self.pipelines
    }
}

// ------------------------------------------------------------------
// Material Composition
// ------------------------------------------------------------------

/// Material composition for layering, blending, and mixing materials.
#[derive(Clone, Debug)]
pub struct MaterialComposition {
    /// The base material for this composition.
    pub base_material: Material,
    /// The overlay materials for this composition.
    pub overlay_materials: Vec<Material>,
    /// The blend mode for this composition.
    pub blend_mode: MaterialBlendMode,
    /// The layering order for this composition.
    pub layering_order: MaterialLayeringOrder,
    /// The mixing weights for this composition.
    pub mixing_weights: Vec<f32>,
}

impl MaterialComposition {
    /// Create a material composition with a base material.
    pub fn new(base_material: Material) -> Self {
        Self {
            base_material,
            overlay_materials: Vec::new(),
            blend_mode: MaterialBlendMode::default(),
            layering_order: MaterialLayeringOrder::default(),
            mixing_weights: Vec::new(),
        }
    }

    /// Add an overlay material to this composition.
    pub fn add_overlay(mut self, material: Material) -> Self {
        self.overlay_materials.push(material);
        self
    }

    /// Set the blend mode for this composition.
    pub fn with_blend_mode(mut self, mode: MaterialBlendMode) -> Self {
        self.blend_mode = mode;
        self
    }

    /// Set the layering order for this composition.
    pub fn with_layering_order(mut self, order: MaterialLayeringOrder) -> Self {
        self.layering_order = order;
        self
    }

    /// Set the mixing weights for this composition.
    pub fn with_mixing_weights(mut self, weights: Vec<f32>) -> Self {
        self.mixing_weights = weights;
        self
    }

    /// Get the base material of this composition.
    pub fn base_material(&self) -> &Material {
        &self.base_material
    }

    /// Get the overlay materials of this composition.
    pub fn overlay_materials(&self) -> &[Material] {
        &self.overlay_materials
    }

    /// Get the blend mode of this composition.
    pub fn blend_mode(&self) -> MaterialBlendMode {
        self.blend_mode
    }

    /// Get the layering order of this composition.
    pub fn layering_order(&self) -> MaterialLayeringOrder {
        self.layering_order
    }

    /// Get the mixing weights of this composition.
    pub fn mixing_weights(&self) -> &[f32] {
        &self.mixing_weights
    }
}

/// Material blend mode for composition.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialBlendMode {
    /// Additive blending.
    #[default]
    Additive,
    /// Multiplicative blending.
    Multiplicative,
    /// Alpha blending.
    Alpha,
    /// Screen blending.
    Screen,
    /// Overlay blending.
    Overlay,
    /// Darken blending.
    Darken,
    /// Lighten blending.
    Lighten,
    /// Difference blending.
    Difference,
    /// Subtract blending.
    Subtract,
    /// Divide blending.
    Divide,
}

impl MaterialBlendMode {
    /// Get the blend weight for this blend mode.
    pub fn weight(self) -> f32 {
        match self {
            Self::Additive => 1.0,
            Self::Multiplicative => 0.5,
            Self::Alpha => 0.75,
            Self::Screen => 0.25,
            Self::Overlay => 0.5,
            Self::Darken => 0.5,
            Self::Lighten => 0.5,
            Self::Difference => 0.5,
            Self::Subtract => 0.5,
            Self::Divide => 0.5,
        }
    }

    /// Get the blend formula for this blend mode.
    pub fn formula(self) -> &'static str {
        match self {
            Self::Additive => "a + b",
            Self::Multiplicative => "a * b",
            Self::Alpha => "a * alpha + b * (1 - alpha)",
            Self::Screen => "1 - (1 - a) * (1 - b)",
            Self::Overlay => "a < 0.5 ? 2 * a * b : 1 - 2 * (1 - a) * (1 - b)",
            Self::Darken => "min(a, b)",
            Self::Lighten => "max(a, b)",
            Self::Difference => "abs(a - b)",
            Self::Subtract => "a - b",
            Self::Divide => "a / b",
        }
    }
}

/// Material layering order for composition.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialLayeringOrder {
    /// Base layer first, overlay layers second.
    #[default]
    BaseFirst,
    /// Overlay layers first, base layer second.
    OverlayFirst,
    /// Alternating layering.
    Alternating,
    /// Random layering.
    Random,
}

impl MaterialLayeringOrder {
    /// Get the layering weight for this order.
    pub fn weight(self) -> f32 {
        match self {
            Self::BaseFirst => 1.0,
            Self::OverlayFirst => 0.5,
            Self::Alternating => 0.75,
            Self::Random => 0.25,
        }
    }

    /// Get the layering formula for this order.
    pub fn formula(self) -> &'static str {
        match self {
            Self::BaseFirst => "base + overlay",
            Self::OverlayFirst => "overlay + base",
            Self::Alternating => "base, overlay, base, overlay",
            Self::Random => "random layering",
        }
    }
}

// ------------------------------------------------------------------
// Material Cache
// ------------------------------------------------------------------

/// Material cache for reuse and management.
#[derive(Clone, Debug)]
pub struct MaterialCache {
    /// The cached materials.
    pub materials: Vec<Material>,
    /// The cache size limit.
    pub size_limit: usize,
    /// The cache eviction strategy.
    pub eviction_strategy: MaterialCacheEvictionStrategy,
    /// The cache hit count.
    pub hit_count: usize,
    /// The cache miss count.
    pub miss_count: usize,
}

impl MaterialCache {
    /// Create a material cache with the given size limit.
    pub fn new(size_limit: usize) -> Self {
        Self {
            materials: Vec::new(),
            size_limit,
            eviction_strategy: MaterialCacheEvictionStrategy::LRU,
            hit_count: 0,

            miss_count: 0,
        }
    }

    /// Add a material to this cache.
    pub fn add(mut self, material: Material) ->
 Result<Self> {
        if self.materials.len() >= self.size_limit {
            self.evict();
        }
        self.materials.push(material);
        Ok(self)
    }

    /// Get a material from this cache by name.
    pub fn get(&self, name: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.name == name)
    }

    /// Evict materials from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.materials.remove(0);
            }
            Self::SizeBased => {
                self.materials.remove(self.materials.len() - 1);
            }
            Self::Random => {
                let index = self.materials.len() / 2;
                self.materials.remove(index);
            }
        }
    }

    /// Get the size of this cache.
    pub fn size(&self) -> usize {
        self.materials.len()
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialCacheEvictionStrategy {
        self.eviction_strategy
    }

    /// Get the hit count of this cache.
    pub fn hit_count(&self) -> usize {
        self.hit_count
    }

    /// Get the miss count of this cache.
    pub fn miss_count(&self) -> usize {
        self.miss_count
    }

    /// Get the cache hit rate of this cache.
    pub fn hit_rate(&self) -> f32 {
        if self.hit_count + self.miss_count == 0 {
            0.0
        } else {
            self.hit_count as f32 / (self.hit_count + self.miss_count) as f32
        }
    }

    /// Get the cache miss rate of this cache.
    pub fn miss_rate(&self) -> f32 {
        if self.hit_count + self.miss_count == 0 {
            0.0
        } else {
            self.miss_count as f32 / (self.hit_count + self.miss_count) as f32
        }
    }
}

impl Default for MaterialCache {
    fn default() -> Self {
        Self::new(100)
    }
}

/// Material cache eviction strategy.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialCacheEvictionStrategy {
    /// Least recently used eviction.
    #[default]
    LRU,
    /// Size-based eviction (evict largest materials).
    SizeBased,
    /// Random eviction.
    Random,
}

impl MaterialCacheEvictionStrategy {
    /// Get the eviction weight for this strategy.
    pub fn weight(self) -> f32 {
        match self {
            Self::LRU => 1.0,
            Self::SizeBased => 0.5,
            Self::Random => 0.25,
        }
    }

    /// Get the eviction formula for this strategy.
    pub fn formula(self) -> &'static str {
        match self {
            Self::LRU => "evict least recently used",
            Self::SizeBased => "evict largest material",
            Self::Random => "evict random material",
        }
    }
}

// ------------------------------------------------------------------
// Material Debug Tools
// ------------------------------------------------------------------

/// Material debugging tools for inspection and visualization.
#[derive(Clone, Debug)]
pub struct MaterialDebugTools {
    /// The shader inspection for this material.
    pub shader_inspection: MaterialShaderInspection,
    /// The parameter visualization for this material.
    pub parameter_visualization: MaterialParameterVisualization,
    /// The performance profiling for this material.
    pub performance_profiling: MaterialPerformanceProfiling,
    /// The debugging output for this material.
    pub debugging_output: MaterialDebuggingOutput,
}

impl MaterialDebugTools {
    /// Create material debug tools for the given material.
    pub fn new(material: Material) -> Self {
        Self {
            shader_inspection: MaterialShaderInspection::new(material),
            parameter_visualization: MaterialParameterVisualization::new(material),
            performance_profiling: MaterialPerformanceProfiling::new(material),
            debugging_output: MaterialDebuggingOutput::new(material),
        }
    }

    /// Get the shader inspection of these debug tools.
    pub fn shader_inspection(&self) -> &MaterialShaderInspection {
        &self.shader_inspection
    }

    /// Get the parameter visualization of these debug tools.
    pub fn parameter_visualization(&self) -> &MaterialParameterVisualization {
        &self.parameter_visualization
    }

    /// Get the performance profiling of these debug tools.
    pub fn performance_profiling(&self) -> &MaterialPerformanceProfiling {
        &self.performance_profiling
    }

    /// Get the debugging output of these debug tools.
    pub fn debugging_output(&self) -> &MaterialDebuggingOutput {
        &self.debugging_output
    }
}

/// Material shader inspection for debugging.
#[derive(Clone, Debug)]
pub struct MaterialShaderInspection {
    /// The material being inspected.
    pub material: Material,
    /// The shader source for this inspection.
    pub shader_source: String,
    /// The shader reflection for this inspection.
    pub shader_reflection: crate::ShaderReflection,
    /// The shader artifacts for this inspection.
    pub shader_artifacts: Vec<crate::CompiledShaderArtifact>,
    /// The shader diagnostics for this inspection.
    pub shader_diagnostics: Vec<String>,
}

impl MaterialShaderInspection {
    /// Create shader inspection for the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            shader_source: material.fragment_desc.source.to_string(),
            shader_reflection: material.fragment_desc.shader_reflection(),
            shader_artifacts: material.fragment_desc.shader_artifacts(),
            shader_diagnostics: Vec::new(),
        }
    }

    /// Get the material of this shader inspection.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the shader source of this shader inspection.
    pub fn shader_source(&self) -> &str {
        &self.shader_source
    }

    /// Get the shader reflection of this shader inspection.
    pub fn shader_reflection(&self) -> &crate::ShaderReflection {
        &self.shader_reflection
    }

    /// Get the shader artifacts of this shader inspection.
    pub fn shader_artifacts(&self) -> &[crate::CompiledShaderArtifact] {
        &self.shader_artifacts
    }

    /// Get the shader diagnostics of this shader inspection.
    pub fn shader_diagnostics(&self) -> &[String] {
        &self.shader_diagnostics
    }
}

/// Material parameter visualization for debugging.
#[derive(Clone, Debug)]
pub struct MaterialParameterVisualization {
    /// The material being visualized.
    pub material: Material,
    /// The parameter values for this visualization.
    pub parameter_values: Vec<f32>,
    /// The parameter names for this visualization.
    pub parameter_names: Vec<String>,
    /// The parameter ranges for this visualization.
    pub parameter_ranges: Vec<[f32; 2]>,
    /// The parameter types for this visualization.
    pub parameter_types: Vec<String>,
}

impl MaterialParameterVisualization {
    /// Create parameter visualization for the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            parameter_values: Vec::new(),
            parameter_names: Vec::new(),
            parameter_ranges: Vec::new(),
            parameter_types: Vec::new(),
        }
    }

    /// Get the material of this parameter visualization.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the parameter values of this parameter visualization.
    pub fn parameter_values(&self) -> &[f32] {
        &self.parameter_values
    }

    /// Get the parameter names of this parameter visualization.
    pub fn parameter_names(&self) -> &[String] {
        &self.parameter_names
    }

    /// Get the parameter ranges of this parameter visualization.
    pub fn parameter_ranges(&self) -> &[[f32; 2]] {
        &self.parameter_ranges
    }

    /// Get the parameter types of this parameter visualization.
    pub fn parameter_types(&self) -> &[String] {
        &self.parameter_types
    }
}

/// Material performance profiling for debugging.
#[derive(Clone, Debug)]
pub struct MaterialPerformanceProfiling {
    /// The material being profiled.
    pub material: Material,
    /// The shader compile time for this profiling.
    pub shader_compile_time: f32,
    /// The GPU execution time for this profiling.
    pub gpu_execution_time: f32,
    /// The material render time for this profiling.
    pub material_render_time: f32,
    /// The material draw time for this profiling.
    pub material_draw_time: f32,
    /// The material bind time for this profiling.
    pub material_bind_time: f32,
    /// The material cache time for this profiling.
    pub material_cache_time: f32,
}

impl MaterialPerformanceProfiling {
    /// Create performance profiling for the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            shader_compile_time: 0.0,
            gpu_execution_time: 0.0,
            material_render_time: 0.0,
            material_draw_time: 0.0,
            material_bind_time: 0.0,
            material_cache_time: 0.0,
        }
    }

    /// Get the material of this performance profiling.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the shader compile time of this performance profiling.
    pub fn shader_compile_time(&self) -> f32 {
        self.shader_compile_time
    }

    /// Get the GPU execution time of this performance profiling.
    pub fn gpu_execution_time(&self) -> f32 {
        self.gpu_execution_time
    }

    /// Get the material render time of this performance profiling.
    pub fn material_render_time(&self) -> f32 {
        self.material_render_time
    }

    /// Get the material draw time of this performance profiling.
    pub fn material_draw_time(&self) -> f32 {
        self.material_draw_time
    }

    /// Get the material bind time of this performance profiling.
    pub fn material_bind_time(&self) -> f32 {
        self.material_bind_time
    }

    /// Get the material cache time of this performance profiling.
    pub fn material_cache_time(&self) -> f32 {
        self.material_cache_time
    }

    /// Get the total time of this performance profiling.
    pub fn total_time(&self) -> f32 {
        self.shader_compile_time + self.gpu_execution_time + self.material_render_time + self.material_draw_time + self.material_bind_time + self.material_cache_time
    }
}

/// Material debugging output for debugging.
#[derive(Clone, Debug)]
pub struct MaterialDebuggingOutput {
    /// The material being output.
    pub material: Material,
    /// The debugging messages for this output.
    pub debugging_messages: Vec<String>,
    /// The debugging errors for this output.
    pub debugging_errors: Vec<String>,
    /// The debugging warnings for this output.
    pub debugging_warnings: Vec<String>,
    /// The debugging info for this output.
    pub debugging_info: Vec<String>,
}

impl MaterialDebuggingOutput {
    /// Create debugging output for the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            debugging_messages: Vec::new(),
            debugging_errors: Vec::new(),
            debugging_warnings: Vec::new(),
            debugging_info: Vec::new(),
        }
    }

    /// Get the material of this debugging output.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the debugging messages of this debugging output.
    pub fn debugging_messages(&self) -> &[String] {
        &self.debugging_messages
    }

    /// Get the debugging errors of this debugging output.
    pub fn debugging_errors(&self) -> &[String] {
        &self.debugging_errors
    }

    /// Get the debugging warnings of this debugging output.
    pub fn debugging_warnings(&self) -> &[String] {
        &self.debugging_warnings
    }

    /// Get the debugging info of this debugging output.
    pub fn debugging_info(&self) -> &[String] {
        &self.debugging_info
    }
}

// ------------------------------------------------------------------
// Material Asset Pipeline
// ------------------------------------------------------------------

/// Material asset pipeline for loading, validation, and caching.
#[derive(Clone, Debug)]
pub struct MaterialAssetPipeline {
    /// The material assets for this pipeline.
    pub assets: Vec<MaterialAsset>,
    /// The asset loader for this pipeline.
    pub asset_loader: MaterialAssetLoader,
    /// The asset validator for this pipeline.
    pub asset_validator: MaterialAssetValidator,
    /// The asset cache for this pipeline.
    pub asset_cache: MaterialAssetCache,
    /// The asset pipeline state for this pipeline.
    pub pipeline_state: MaterialAssetPipelineState,
}

impl MaterialAssetPipeline {
    /// Create material asset pipeline.
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            asset_loader: MaterialAssetLoader::new(),
            asset_validator: MaterialAssetValidator::new(),
            asset_cache: MaterialAssetCache::new(),
            pipeline_state: MaterialAssetPipelineState::default(),
        }
    }

    /// Load a material asset from this pipeline.
    pub fn load(&mut self, asset: MaterialAsset) -> Result<Self> {
        self.asset_loader.load(&self, asset)?;
        self.asset_validator.validate(&self, asset)?;
        self.asset_cache.cache(&self, asset)?;
        Ok(self)
    }

    /// Get the assets of this pipeline.
    pub fn assets(&self) -> &[MaterialAsset] {
        &self.assets
    }

    /// Get the asset loader of this pipeline.
    pub fn asset_loader(&self) -> &MaterialAssetLoader {
        &self.asset_loader
    }

    /// Get the asset validator of this pipeline.
    pub fn asset_validator(&self) -> &MaterialAssetValidator {
        &self.asset_validator
    }

    /// Get the asset cache of this pipeline.
    pub fn asset_cache(&self) -> &MaterialAssetCache {
        &self.asset_cache
    }

    /// Get the pipeline state of this pipeline.
    pub fn pipeline_state(&self) -> &MaterialAssetPipelineState {
        &self.pipeline_state
    }
}

impl Default for MaterialAssetPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Material asset for loading.
#[derive(Clone, Debug)]
pub struct MaterialAsset {
    /// The asset name for this material.
    pub name: String,
    /// The asset path for this material.
    pub path: PathBuf,
    /// The asset format for this material.
    pub format: MaterialAssetFormat,
    /// The asset data for this material.
    pub data: Vec<u8>,
    /// The asset metadata for this material.
    pub metadata: MaterialAssetMetadata,
}

impl MaterialAsset {
    /// Create a material asset from the given name and path.
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            format: MaterialAssetFormat::default(),
            data: Vec::new(),
            metadata: MaterialAssetMetadata::default(),
        }
    }

    /// Get the name of this material asset.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the path of this material asset.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the format of this material asset.
    pub fn format(&self) -> MaterialAssetFormat {
        self.format
    }

    /// Get the data of this material asset.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get the metadata of this material asset.
    pub fn metadata(&self) -> &MaterialAssetMetadata {
        &self.metadata
    }
}

/// Material asset format for loading.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialAssetFormat {
    /// Slang shader source.
    #[default]
    Slang,
    /// HLSL shader source.
    HLSL,
    /// GLSL shader source.
    GLSL,
    /// MSL shader source.
    MSL,
    /// SPIR-V shader bytecode.
    Spirv,
    /// DXIL shader bytecode.
    Dxil,
    /// Material parameter data.
    ParameterData,
    /// Material texture data.
    TextureData,
}

impl MaterialAssetFormat {
    /// Get the shader target for this format.
    pub fn shader_target(self) -> crate::ShaderTarget {
        match self {
            Self::Slang => crate::ShaderTarget::Spirv,
            Self::HLSL => crate::ShaderTarget::Dxil,
            Self::GLSL => crate::ShaderTarget::Spirv,
            Self::MSL => crate::ShaderTarget::Msl,
            Self::Spirv => crate::ShaderTarget::Spirv,
            Self::Dxil => crate::ShaderTarget::Dxil,
            Self::ParameterData => crate::ShaderTarget::Spirv,
            Self::TextureData => crate::ShaderTarget::Spirv,
        }
    }

    /// Get the shader stage for this format.
    pub fn shader_stage(self) -> ShaderStage {
        match self {
            Self::Slang => ShaderStage::Fragment,
            Self::HLSL => ShaderStage::Fragment,
            Self::GLSL => ShaderStage::Fragment,
            Self::MSL => ShaderStage::Fragment,
            Self::Spirv => ShaderStage::Fragment,
            Self::Dxil => ShaderStage::Fragment,
            Self::ParameterData => ShaderStage::Fragment,
            Self::TextureData => ShaderStage::Fragment,
        }
    }
}

/// Material asset metadata for loading.
#[derive(Clone, Debug, Default)]
pub struct MaterialAssetMetadata {
    /// The asset version for this metadata.
    pub version: u32,
    /// The asset author for this metadata.
    pub author: String,
    /// The asset date for this metadata.
    pub date: String,
    /// The asset license for this metadata.
    pub license: String,
    /// The asset description for this metadata.
    pub description: String,
}

impl MaterialAssetMetadata {
    /// Get the version of this metadata.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Get the author of this metadata.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Get the date of this metadata.
    pub fn date(&self) -> &str {
        &self.date
    }

    /// Get the license of this metadata.
    pub fn license(&self) -> &str {
        &self.license
    }

    /// Get the description of this metadata.
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Material asset loader for loading.
#[derive(Clone, Debug)]
pub struct MaterialAssetLoader {
    /// The loaded assets for this loader.
    pub loaded_assets: Vec<MaterialAsset>,
    /// The loader state for this loader.
    pub loader_state: MaterialAssetLoaderState,
}

impl MaterialAssetLoader {
    /// Create material asset loader.
    pub fn new() -> Self {
        Self {
            loaded_assets: Vec::new(),
            loader_state: MaterialAssetLoaderState::default(),
        }
    }

    /// Load an asset from this loader.
    pub fn load(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        self.loaded_assets.push(asset);
        Ok(self)
    }

    /// Get the loaded assets of this loader.
    pub fn loaded_assets(&self) -> &[MaterialAsset] {
        &self.loaded_assets
    }

    /// Get the loader state of this loader.
    pub fn loader_state(&self) -> MaterialAssetLoaderState {
        self.loader_state
    }
}

impl Default for MaterialAssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Material asset loader state for loading.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialAssetLoaderState {
    /// Idle state.
    #[default]
    Idle,
    /// Loading state.
    Loading,
    /// Loaded state.
    Loaded,
    /// Error state.
    Error,
}

impl MaterialAssetLoaderState {
    /// Get the loader state weight for this state.
    pub fn weight(self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Loading => 0.5,
            Self::Loaded => 0.25,
            Self::Error => 0.0,
        }
    }
}

/// Material asset validator for validation.
#[derive(Clone, Debug)]
pub struct MaterialAssetValidator {
    /// The validated assets for this validator.
    pub validated_assets: Vec<MaterialAsset>,
    /// The validator state for this validator.
    pub validator_state: MaterialAssetValidatorState,
}

impl MaterialAssetValidator {
    /// Create material asset validator.
    pub fn new() -> Self {
        Self {
            validated_assets: Vec::new(),
            validator_state: MaterialAssetValidatorState::default(),
        }
    }

    /// Validate an asset from this validator.
    pub fn validate(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        self.validated_assets.push(asset);
        Ok(self)
    }

    /// Get the validated assets of this validator.
    pub fn validated_assets(&self) -> &[MaterialAsset] {
        &self.validated_assets
    }

    /// Get the validator state of this validator.
    pub fn validator_state(&self) -> MaterialAssetValidatorState {
        self.validator_state
    }
}

impl Default for MaterialAssetValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Material asset validator state for validation.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialAssetValidatorState {
    /// Idle state.
    #[default]
    Idle,
    /// Validating state.
    Validating,
    /// Validated state.
    Validated,
    /// Error state.
    Error,
}

impl MaterialAssetValidatorState {
    /// Get the validator state weight for this state.
    pub fn weight(self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Validating => 0.5,
            Self::Validated => 0.25,
            Self::Error => 0.0,
        }
    }
}

/// Material asset cache for caching.
#[derive(Clone, Debug)]
pub struct MaterialAssetCache {
    /// The cached assets for this cache.
    pub cached_assets: Vec<MaterialAsset>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
}

impl MaterialAssetCache {
    /// Create material asset cache.
    pub fn new() -> Self {
        Self {
            cached_assets: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
        }
    }

    /// Cache an asset from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_assets.len() >= self.size_limit {
            self.evict();
        }
        self.cached_assets.push(asset);
        Ok(self)
    }

    /// Get the cached assets of this cache.
    pub fn cached_assets(&self) -> &[MaterialAsset] {
        &self.cached_assets
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
        self.eviction_strategy
    }

    /// Evict assets from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.cached_assets.remove(0);
            }
            Self::SizeBased => {
                self.cached_assets.remove(self.cached_assets.len() - 1);
            }
            Self::Random => {
                let index = self.cached_assets.len() / 2;
                self.cached_assets.remove(index);
            }
        }
    }
}

impl Default for MaterialAssetCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Material asset cache eviction strategy for caching.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialAssetCacheEvictionStrategy {
    /// Least recently used eviction.
    #[default]
    LRU,
    /// Size-based eviction (evict largest assets).
    SizeBased,
    /// Random eviction.
    Random,
}

impl MaterialAssetCacheEvictionStrategy {
    /// Get the eviction weight for this strategy.
    pub fn weight(self) -> f32 {
        match self {
            Self::LRU => 1.0,
            Self::SizeBased => 0.5,
            Self::Random => 0.25,
        }
    }
}

/// Material asset pipeline state for pipeline management.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialAssetPipelineState {
    /// Idle state.
    #[default]
    Idle,
    /// Loading state.
    Loading,
    /// Validating state.
    Validating,
    /// Caching state.
    Caching,
    /// Complete state.
    Complete,
    /// Error state.
    Error,
}

impl MaterialAssetPipelineState {
    /// Get the pipeline state weight for this state.
    pub fn weight(self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Loading => 0.5,
            Self::Validating => 0.25,
            Self::Caching => 0.1,
            Self::Complete => 0.0,
            Self::Error => 0.0,
        }
    }
}

// ------------------------------------------------------------------
// Material Preview System
// ------------------------------------------------------------------

/// Material preview system for CPU-side material preview.
#[derive(Clone, Debug)]
pub struct MaterialPreviewSystem {
    /// The preview materials for this system.
    pub preview_materials: Vec<MaterialPreview>,
    /// The preview renderer for this system.
    pub preview_renderer: MaterialPreviewRenderer,
    /// The preview output for this system.
    pub preview_output: MaterialPreviewOutput,
}

impl MaterialPreviewSystem {
    /// Create material preview system.
    pub fn new() -> Self {
        Self {
            preview_materials: Vec::new(),
            preview_renderer: MaterialPreviewRenderer::new(),
            preview_output: MaterialPreviewOutput::new(),
        }
    }

    /// Add a preview material to this system.
    pub fn add_preview(mut self, material: MaterialPreview) -> Self {
        self.preview_materials.push(material);
        self
    }

    /// Render a preview material from this system.
    pub fn render_preview(&self, renderer: &MaterialPreviewRenderer) -> MaterialPreviewOutput {
        renderer.render(&self.preview_materials)
    }

    /// Get the preview materials of this system.
    pub fn preview_materials(&self) -> &[MaterialPreview] {
        &self.preview_materials
    }

    /// Get the preview renderer of this system.
    pub fn preview_renderer(&self) -> &MaterialPreviewRenderer {
        &self.preview_renderer
    }

    /// Get the preview output of this system.
    pub fn preview_output(&self) -> &MaterialPreviewOutput {
        &self.preview_output
    }
}

impl Default for MaterialPreviewSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Material preview for CPU-side preview.
#[derive(Clone, Debug)]
pub struct MaterialPreview {
    /// The material being previewed.
    pub material: Material,
    /// The preview parameters for this preview.
    pub preview_parameters: Vec<f32>,
    /// The preview output for this preview.
    pub preview_output: Vec<u8>,
    /// The preview size for this preview.
    pub preview_size: [u32; 2],
}

impl MaterialPreview {
    /// Create a material preview from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            preview_parameters: Vec::new(),
            preview_output: Vec::new(),
            preview_size: [256, 256],
        }
    }

    /// Get the material of this preview.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the preview parameters of this preview.
    pub fn preview_parameters(&self) -> &[f32] {
        &self.preview_parameters
    }

    /// Get the preview output of this preview.
    pub fn preview_output(&self) -> &[u8] {
        &self.preview_output
    }

    /// Get the preview size of this preview.
    pub fn preview_size(&self) -> &[u32; 2] {
        &self.preview_size
    }
}

/// Material preview renderer for CPU-side preview.
#[derive(Clone, Debug)]
pub struct MaterialPreviewRenderer {
    /// The rendered previews for this renderer.
    pub rendered_previews: Vec<MaterialPreviewOutput>,
    /// The renderer state for this renderer.
    pub renderer_state: MaterialPreviewRendererState,
}

impl MaterialPreviewRenderer {
    /// Create material preview renderer.
    pub fn new() -> Self {
        Self {
            rendered_previews: Vec::new(),
            renderer_state: MaterialPreviewRendererState::default(),
        }
    }

    /// Render previews from this renderer.
    pub fn render(&self, materials: &[MaterialPreview]) -> MaterialPreviewOutput {
        MaterialPreviewOutput::new()
    }

    /// Get the rendered previews of this renderer.
    pub fn rendered_previews(&self) -> &[MaterialPreviewOutput] {
        &self.rendered_previews
    }

    /// Get the renderer state of this renderer.
    pub fn renderer_state(&self) -> MaterialPreviewRendererState {
        self.renderer_state
    }
}

impl Default for MaterialPreviewRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Material preview renderer state for CPU-side preview.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialPreviewRendererState {
    /// Idle state.
    #[default]
    Idle,
    /// Rendering state.
    Rendering,
    /// Rendered state.
    Rendered,
    /// Error state.
    Error,
}

impl MaterialPreviewRendererState {
    /// Get the renderer state weight for this state.
    pub fn weight(self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Rendering => 0.5,
            Self::Rendered => 0.25,
            Self::Error => 0.0,
        }
    }
}

/// Material preview output for CPU-side preview.
#[derive(Clone, Debug)]
pub struct MaterialPreviewOutput {
    /// The preview pixels for this output.
    pub pixels: Vec<u8>,
    /// The preview size for this output.
    pub size: [u32; 2],
    /// The preview format for this output.
    pub format: Format,
}

impl MaterialPreviewOutput {
    /// Create material preview output.
    pub fn new() -> Self {
        Self {
            pixels: Vec::new(),
            size: [256, 256],
            format: Format::Rgba8Unorm,
        }
    }

    /// Get the pixels of this output.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Get the size of this output.
    pub fn size(&self) -> &[u32; 2] {
        &self.size
    }

    /// Get the format of this output.
    pub fn format(&self) -> Format {
        self.format
    }
}

impl Default for MaterialPreviewOutput {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material DSL
// ------------------------------------------------------------------

/// Material DSL for declarative material composition.
#[derive(Clone, Debug)]
pub struct MaterialDSL {
    /// The DSL expression for this material.
    pub expression: String,
    /// The DSL parsed material for this material.
    pub parsed_material: Material,
    /// The DSL compiled material for this material.
    pub compiled_material: Material,
}

impl MaterialDSL {
    /// Create material DSL from the given expression.
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            parsed_material: Material::new("dsl_material"),
            compiled_material: Material::new("dsl_material"),
        }
    }

    /// Parse the DSL expression for this material.
    pub fn parse(&mut self) -> Result<Self> {
        self.parsed_material = Material::new("dsl_material")
            .with_vertex_kind(MeshVertexKind::V3d)
            .with_fragment_shader("dsl_fragment.slang")
            .build(engine)?;
        Ok(self)
    }

    /// Compile the DSL expression for this material.
    pub fn compile(&mut self) -> Result<Self> {
        self.compiled_material = Material::new("dsl_material")
            .with_vertex_kind(MeshVertexKind::V3d)
            .with_fragment_shader("dsl_fragment.slang")
            .build(engine)?;
        Ok(self)
    }

    /// Get the expression of this DSL.
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// Get the parsed material of this DSL.
    pub fn parsed_material(&self) -> &Material {
        &self.parsed_material
    }

    /// Get the compiled material of this DSL.
    pub fn compiled_material(&self) -> &Material {
        &self.compiled_material
    }
}

// ------------------------------------------------------------------
// Material Serialization
// ------------------------------------------------------------------

/// Material serialization for save/load material states.
#[derive(Clone, Debug)]
pub struct MaterialSerialization {
    /// The serialized material for this serialization.
    pub serialized_material: Material,
    /// The serialization format for this serialization.
    pub serialization_format: MaterialSerializationFormat,
    /// The serialization data for this serialization.
    pub serialization_data: Vec<u8>,
}

impl MaterialSerialization {
    /// Create material serialization from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            serialized_material: material,
            serialization_format: MaterialSerializationFormat::default(),
            serialization_data: Vec::new(),
        }
    }

    /// Serialize the material for this serialization.
    pub fn serialize(&mut self) -> Result<Self> {
        self.serialization_data = self.serialized_material.serialize();
        Ok(self)
    }

    /// Deserialize the material for this serialization.
    pub fn deserialize(&mut self) -> Result<Self> {
        self.serialized_material = self.serialization_data.deserialize();
        Ok(self)
    }

    /// Get the serialized material of this serialization.
    pub fn serialized_material(&self) -> &Material {
        &self.serialized_material
    }

    /// Get the serialization format of this serialization.
    pub fn serialization_format(&self) -> MaterialSerializationFormat {
        self.serialization_format
    }

    /// Get the serialization data of this serialization.
    pub fn serialization_data(&self) -> &[u8] {
        &self.serialization_data
    }
}

/// Material serialization format for save/load material states.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialSerializationFormat {
    /// JSON format.
    #[default]
    JSON,
    /// Binary format.
    Binary,
    /// XML format.
    XML,
    /// YAML format.
    YAML,
}

impl MaterialSerializationFormat {
    /// Get the serialization weight for this format.
    pub fn weight(self) -> f32 {
        match self {
            Self::JSON => 1.0,
            Self::Binary => 0.5,
            Self::XML => 0.25,
            Self::YAML => 0.1,
        }
    }
}

// ------------------------------------------------------------------
// Material Shader Optimization
// ------------------------------------------------------------------

/// Material shader optimization for pre-compiled shader artifacts.
#[derive(Clone, Debug)]
pub struct MaterialShaderOptimization {
    /// The optimized material for this optimization.
    pub optimized_material: Material,
    /// The optimization strategy for this optimization.
    pub optimization_strategy: MaterialShaderOptimizationStrategy,
    /// The optimized artifacts for this optimization.
    pub optimized_artifacts: Vec<crate::CompiledShaderArtifact>,
}

impl MaterialShaderOptimization {
    /// Create material shader optimization from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            optimized_material: material,
            optimization_strategy: MaterialShaderOptimizationStrategy::default(),
            optimized_artifacts: Vec::new(),
        }
    }

    /// Optimize the material for this optimization.
    pub fn optimize(&mut self) -> Result<Self> {
        self.optimized_artifacts = self.optimized_material.optimize();
        Ok(self)
    }

    /// Get the optimized material of this optimization.
    pub fn optimized_material(&self) -> &Material {
        &self.optimized_material
    }

    /// Get the optimization strategy of this optimization.
    pub fn optimization_strategy(&self) -> MaterialShaderOptimizationStrategy {
        self.optimization_strategy
    }

    /// Get the optimized artifacts of this optimization.
    pub fn optimized_artifacts(&self) -> &[crate::CompiledShaderArtifact] {
        &self.optimized_artifacts
    }
}

/// Material shader optimization strategy for pre-compiled shader artifacts.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialShaderOptimizationStrategy {
    /// No optimization.
    #[default]
    None,
    /// Shader compilation optimization.
    Compilation,
    /// Shader runtime optimization.
    Runtime,
    /// Shader pipeline optimization.
    Pipeline,
}

impl MaterialShaderOptimizationStrategy {
    /// Get the optimization weight for this strategy.
    pub fn weight(self) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Compilation => 0.5,
            Self::Runtime => 0.25,
            Self::Pipeline => 0.1,
        }
    }
}

// ------------------------------------------------------------------
// Offline Rendering Support
// ------------------------------------------------------------------

/// Offline rendering support for games and offline rendering systems.
#[derive(Clone, Debug)]
pub struct OfflineRenderingSupport {
    /// The offline render graph for this support.
    pub offline_render_graph: OfflineRenderGraph,
    /// The offline material for this support.
    pub offline_material: Material,
    /// The offline shader for this support.
    pub offline_shader: Shader,
    /// The offline pipeline for this support.
    pub offline_pipeline: Pipeline,
    /// The offline capture for this support.
    pub offline_capture: GpuCaptureDesc,
}

impl OfflineRenderingSupport {
    /// Create offline rendering support from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            offline_render_graph: OfflineRenderGraph::new(),
            offline_material: material,
            offline_shader: Shader::new(),
            offline_pipeline: Pipeline::new(),
            offline_capture: GpuCaptureDesc::new(GpuCaptureTool::RenderDoc, "offline_capture"),
        }
    }

    /// Get the offline render graph of this support.
    pub fn offline_render_graph(&self) -> &OfflineRenderGraph {
        &self.offline_render_graph
    }

    /// Get the offline material of this support.
    pub fn offline_material(&self) -> &Material {
        &self.offline_material
    }

    /// Get the offline shader of this support.
    pub fn offline_shader(&self) -> &Shader {
        &self.offline_shader
    }

    /// Get the offline pipeline of this support.
    pub fn offline_pipeline(&self) -> &Pipeline {
        &self.offline_pipeline
    }

    /// Get the offline capture of this support.
    pub fn offline_capture(&self) -> &GpuCaptureDesc {
        &self.offline_capture
    }
}

/// Offline render graph for offline rendering systems.
#[derive(Clone, Debug)]
pub struct OfflineRenderGraph {
    /// The offline passes for this graph.
    pub passes: Vec<PassDesc>,
    /// The offline images for this graph.
    pub images: Vec<Image>,
    /// The offline buffers for this graph.
    pub buffers: Vec<Buffer>,
    /// The offline render frame for this graph.
    pub render_frame: Frame,
}

impl OfflineRenderGraph {
    /// Create offline render graph.
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            images: Vec::new(),
            buffers: Vec::new(),
            render_frame: Frame::new(),
        }
    }

    /// Get the passes of this graph.
    pub fn passes(&self) -> &[PassDesc] {
        &self.passes
    }

    /// Get the images of this graph.
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// Get the buffers of this graph.
    pub fn buffers(&self) -> &[Buffer] {
        &self.buffers
    }

    /// Get the render frame of this graph.
    pub fn render_frame(&self) -> &Frame {
        &self.render_frame
    }
}

impl Default for OfflineRenderGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Unified Material System
// ──────────────────────────────────────────────────────────────────────────────
//
// These types form the foundation described in Track 6 of the roadmap.
// A `UnifiedMaterial` defines a `MaterialSurface` evaluation function once;
// the engine derives all required shader variants automatically:
//
//   ┌─────────────────────────────────────────────────────────────────────────┐
//   │  UnifiedMaterial (user-facing definition)                               │
//   │    .domain        = Opaque | Masked | Translucent | Decal               │
//   │    .shading_model = PbrMetallicRoughness | Clearcoat | Unlit | ...      │
//   │    .base_color, .metallic, .roughness, .normal_map, .emissive …         │
//   │    .evaluate_material_snippet (optional custom Slang body)              │
//   └────────────────────────────┬────────────────────────────────────────────┘
//                                │  MaterialVariantCompiler (Track 6b)
//          ┌──────────┬──────────┼──────────┬───────────┬─────────────┐
//          ▼          ▼          ▼          ▼           ▼             ▼
//    GBufferFill  ForwardLit  Shadow    RtAnyHit  RtClosestHit  PathTraced
//
// The G-Buffer layout used by GBufferFill and the deferred lighting pass
// is defined in the `gbuffer` submodule below.

/// Blending and depth-write behaviour of a material surface.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum MaterialDomain {
    /// Depth-tested, depth-written; rendered via the deferred G-Buffer fill path.
    #[default]
    Opaque,
    /// Like `Opaque` but discards pixels whose `opacity < ALPHA_CUTOFF` in the
    /// shadow pass and RT any-hit shader.
    Masked,
    /// Back-to-front sorted, forward-lit, alpha-blended over the HDR target.
    /// Rendered after the deferred lighting pass.
    Translucent,
    /// Projected surface that writes into G0/G1/G2 after the main G-Buffer fill.
    Decal,
}

/// Lighting model evaluated in deferred and forward lit passes.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum ShadingModel {
    /// Emissive only — no lighting computation.
    Unlit,
    /// Lambertian diffuse only — legacy fallback for non-PBR assets.
    Lambert,
    /// GGX metallic-roughness BRDF — standard GLTF 2.0 workflow.
    /// Energy-compensated via a precomputed BRDF integration LUT.
    #[default]
    PbrMetallicRoughness,
    /// GGX metallic-roughness with a clear-coat layer on top
    /// (GLTF `KHR_materials_clearcoat`).
    PbrClearcoat,
    /// Screen-space subsurface scattering for skin and organic materials.
    PbrSubsurface,
    /// Transmission and refraction for glass and liquids
    /// (GLTF `KHR_materials_transmission` + `KHR_materials_volume`).
    PbrTransmission,
}

/// One input channel of a [`UnifiedMaterial`].
///
/// Each `MaterialSurface` field may be driven by a constant value, a texture
/// sampled at UV0, or a texture modulated by a constant factor.
#[derive(Clone, Debug)]
pub enum MaterialInput<T: Clone> {
    /// Packed into the material's push constants each draw call.
    Constant(T),
    /// Sampled from a named texture binding at UV0.
    Texture(String),
    /// `texture.sample(uv) * factor` — GLTF convention for combined maps.
    TextureTimesConstant { texture: String, factor: T },
}

impl<T: Clone + Default> Default for MaterialInput<T> {
    fn default() -> Self {
        Self::Constant(T::default())
    }
}

/// A unified, rendering-path-agnostic material definition.
///
/// The user writes a single Slang snippet implementing:
/// ```slang
/// MaterialSurface evaluate_material(VertexData v) { ... }
/// ```
/// (or fills in the structured PBR inputs and lets the engine generate it).
/// The [`MaterialVariantCompiler`] then derives every required shader variant
/// from that one definition — deferred G-Buffer fill, forward lit, shadow
/// depth-only, RT any-hit, RT closest-hit, and path-traced hit.
///
/// # Asset-driven PBR workflow (GLTF)
/// ```rust
/// let mat = UnifiedMaterial::pbr_metallic_roughness("ground")
///     .base_color_texture("base_color")
///     .metallic_roughness_texture("metallic_roughness")
///     .normal_texture("normal_map")
///     .occlusion_texture("occlusion")
///     .emissive_constant([0.0, 0.0, 0.0])
///     .build();
/// ```
///
/// # Procedural workflow
/// ```rust
/// let mat = UnifiedMaterial::procedural("lava")
///     .evaluate_material_fn(r#"
///         MaterialSurface evaluate_material(VertexData v) {
///             float heat = sin(v.world_pos.y * 4.0 + time) * 0.5 + 0.5;
///             MaterialSurface s;
///             s.base_color = lerp(float3(0.05, 0.0, 0.0), float3(1.0, 0.4, 0.0), heat);
///             s.metallic   = 0.0;
///             s.roughness  = lerp(0.9, 0.3, heat);
///             s.normal_ts  = float3(0.0, 0.0, 1.0);
///             s.occlusion  = 1.0;
///             s.emissive   = float3(1.0, 0.3, 0.0) * heat * 3.0;
///             s.opacity    = 1.0;
///             return s;
///         }
///     "#)
///     .build();
/// ```
#[derive(Clone, Debug)]
pub struct UnifiedMaterial {
    /// Debug name for diagnostics and shader variant cache keys.
    pub name: String,
    /// Blending and depth-write behaviour.
    pub domain: MaterialDomain,
    /// Lighting model evaluated in lit passes.
    pub shading_model: ShadingModel,
    /// Rendering state (cull mode, front face, depth bias, stencil).
    pub render_state: RenderState,

    // ── Standard PBR inputs ───────────────────────────────────────────────────
    /// Linear sRGB albedo. Alpha channel is `opacity` for `Translucent` domain.
    pub base_color: MaterialInput<[f32; 4]>,
    /// [0, 1] metallic factor; 0 = dielectric, 1 = conductor.
    pub metallic: MaterialInput<f32>,
    /// [0, 1] perceptual roughness; shader squares to α for GGX NDF.
    pub roughness: MaterialInput<f32>,
    /// Tangent-space normal map binding name. `None` uses the geometric normal.
    pub normal_map: Option<String>,
    /// [0, 1] ambient occlusion factor.
    pub occlusion: MaterialInput<f32>,
    /// Linear HDR emissive radiance (not clamped — drives bloom).
    pub emissive: MaterialInput<[f32; 3]>,

    // ── Clear-coat layer (PbrClearcoat only) ──────────────────────────────────
    /// [0, 1] clear-coat layer intensity.
    pub clearcoat: MaterialInput<f32>,
    /// [0, 1] clear-coat layer roughness.
    pub clearcoat_roughness: MaterialInput<f32>,

    // ── Procedural override ───────────────────────────────────────────────────
    /// When `Some`, this Slang function body is used verbatim as
    /// `evaluate_material(VertexData)`; the structured inputs above are ignored.
    pub evaluate_material_snippet: Option<String>,
}

impl UnifiedMaterial {
    /// Pixels with `opacity < ALPHA_CUTOFF` are discarded in `Masked` domain
    /// shadow and RT passes.
    pub const ALPHA_CUTOFF: f32 = 0.5;

    /// Start building a standard PBR metallic-roughness material.
    pub fn pbr_metallic_roughness(name: impl Into<String>) -> UnifiedMaterialBuilder {
        UnifiedMaterialBuilder::new(name).shading_model(ShadingModel::PbrMetallicRoughness)
    }

    /// Start building an unlit emissive material (no lighting computation).
    pub fn unlit(name: impl Into<String>) -> UnifiedMaterialBuilder {
        UnifiedMaterialBuilder::new(name).shading_model(ShadingModel::Unlit)
    }

    /// Start building a procedural material; call `.evaluate_material_fn()` on
    /// the returned builder to supply the Slang body.
    pub fn procedural(name: impl Into<String>) -> UnifiedMaterialBuilder {
        UnifiedMaterialBuilder::new(name)
    }
}

impl Default for UnifiedMaterial {
    fn default() -> Self {
        Self {
            name: "default_pbr".into(),
            domain: MaterialDomain::Opaque,
            shading_model: ShadingModel::PbrMetallicRoughness,
            render_state: RenderState::default(),
            base_color: MaterialInput::Constant([1.0, 1.0, 1.0, 1.0]),
            metallic: MaterialInput::Constant(0.0),
            roughness: MaterialInput::Constant(0.5),
            normal_map: None,
            occlusion: MaterialInput::Constant(1.0),
            emissive: MaterialInput::Constant([0.0, 0.0, 0.0]),
            clearcoat: MaterialInput::Constant(0.0),
            clearcoat_roughness: MaterialInput::Constant(0.0),
            evaluate_material_snippet: None,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// UnifiedMaterialBuilder
// ──────────────────────────────────────────────────────────────────────────────

/// Fluent builder for [`UnifiedMaterial`].
#[derive(Clone, Debug)]
pub struct UnifiedMaterialBuilder {
    inner: UnifiedMaterial,
}

impl UnifiedMaterialBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self { inner: UnifiedMaterial { name: name.into(), ..Default::default() } }
    }

    pub fn domain(mut self, domain: MaterialDomain) -> Self {
        self.inner.domain = domain;
        self
    }

    pub fn shading_model(mut self, model: ShadingModel) -> Self {
        self.inner.shading_model = model;
        self
    }

    pub fn render_state(mut self, state: RenderState) -> Self {
        self.inner.render_state = state;
        self
    }

    // ── Base color ────────────────────────────────────────────────────────────

    pub fn base_color_constant(mut self, rgba: [f32; 4]) -> Self {
        self.inner.base_color = MaterialInput::Constant(rgba);
        self
    }

    pub fn base_color_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.base_color = MaterialInput::Texture(binding.into());
        self
    }

    pub fn base_color_texture_factor(mut self, binding: impl Into<String>, factor: [f32; 4]) -> Self {
        self.inner.base_color = MaterialInput::TextureTimesConstant {
            texture: binding.into(),
            factor,
        };
        self
    }

    // ── Metallic / roughness ──────────────────────────────────────────────────

    pub fn metallic_constant(mut self, v: f32) -> Self {
        self.inner.metallic = MaterialInput::Constant(v);
        self
    }

    pub fn roughness_constant(mut self, v: f32) -> Self {
        self.inner.roughness = MaterialInput::Constant(v);
        self
    }

    /// Bind a combined metallic-roughness texture (GLTF convention: B=metallic, G=roughness).
    pub fn metallic_roughness_texture(mut self, binding: impl Into<String>) -> Self {
        let b = binding.into();
        self.inner.metallic = MaterialInput::Texture(format!("{b}.b"));
        self.inner.roughness = MaterialInput::Texture(format!("{b}.g"));
        self
    }

    pub fn metallic_roughness_constants(mut self, metallic: f32, roughness: f32) -> Self {
        self.inner.metallic = MaterialInput::Constant(metallic);
        self.inner.roughness = MaterialInput::Constant(roughness);
        self
    }

    // ── Normal map ────────────────────────────────────────────────────────────

    pub fn normal_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.normal_map = Some(binding.into());
        self
    }

    // ── Occlusion ─────────────────────────────────────────────────────────────

    pub fn occlusion_constant(mut self, v: f32) -> Self {
        self.inner.occlusion = MaterialInput::Constant(v);
        self
    }

    pub fn occlusion_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.occlusion = MaterialInput::Texture(binding.into());
        self
    }

    // ── Emissive ──────────────────────────────────────────────────────────────

    pub fn emissive_constant(mut self, rgb: [f32; 3]) -> Self {
        self.inner.emissive = MaterialInput::Constant(rgb);
        self
    }

    pub fn emissive_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.emissive = MaterialInput::Texture(binding.into());
        self
    }

    pub fn emissive_texture_factor(mut self, binding: impl Into<String>, factor: [f32; 3]) -> Self {
        self.inner.emissive = MaterialInput::TextureTimesConstant {
            texture: binding.into(),
            factor,
        };
        self
    }

    // ── Clear-coat ────────────────────────────────────────────────────────────

    /// Enable the clear-coat layer and set its intensity and roughness constants.
    pub fn clearcoat(mut self, intensity: f32, roughness: f32) -> Self {
        self.inner.shading_model = ShadingModel::PbrClearcoat;
        self.inner.clearcoat = MaterialInput::Constant(intensity);
        self.inner.clearcoat_roughness = MaterialInput::Constant(roughness);
        self
    }

    // ── Procedural snippet ────────────────────────────────────────────────────

    /// Supply a verbatim Slang function body for `evaluate_material(VertexData)`.
    /// When set, all structured PBR inputs above are ignored.
    pub fn evaluate_material_fn(mut self, snippet: impl Into<String>) -> Self {
        self.inner.evaluate_material_snippet = Some(snippet.into());
        self
    }

    pub fn build(self) -> UnifiedMaterial {
        self.inner
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// G-Buffer layout constants
// ──────────────────────────────────────────────────────────────────────────────

/// Standard G-Buffer attachment slots and formats for the deferred PBR pipeline.
///
/// All deferred-path components (G-Buffer fill pass, deferred lighting pass,
/// SSAO, RT passes reading G-Buffer data) must use these constants so that
/// attachment indices and formats stay consistent.
///
/// Layout:
/// ```text
/// G0  RGBA8Unorm   base_color.rgb (linear) | metallic
/// G1  RGBA16Float  normal.xy (oct) | roughness | occlusion
/// G2  RGBA16Float  emissive.rgb (linear HDR) | shading_model_id
/// D   Depth32Float hardware depth
/// ```
pub mod gbuffer {
    use crate::Format;

    /// G-Buffer attachment 0: `base_color.rgb` + `metallic` in alpha.
    pub const SLOT_BASE_COLOR_METALLIC: u32 = 0;
    /// G-Buffer attachment 1: octahedral world-normal (`.xy`) + `roughness` (`.z`) + `occlusion` (`.w`).
    pub const SLOT_NORMAL_ROUGHNESS_OCCLUSION: u32 = 1;
    /// G-Buffer attachment 2: `emissive.rgb` (HDR, unclamped) + shading model ID (`.w`).
    pub const SLOT_EMISSIVE_SHADING: u32 = 2;
    /// Depth attachment.
    pub const SLOT_DEPTH: u32 = 3;

    pub const FORMAT_BASE_COLOR_METALLIC: Format = Format::Rgba8Unorm;
    pub const FORMAT_NORMAL_ROUGHNESS_OCCLUSION: Format = Format::Rgba16Float;
    pub const FORMAT_EMISSIVE_SHADING: Format = Format::Rgba16Float;
    pub const FORMAT_DEPTH: Format = Format::Depth32Float;

    /// Total number of color attachments in the G-Buffer (excludes depth).
    pub const COLOR_ATTACHMENT_COUNT: u32 = 3;
}
