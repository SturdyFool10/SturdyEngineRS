//! Asset-driven material loading system for the scene and rendering pipeline.
//!
//! This module provides the [`MaterialAsset`] type and associated types that define
//! how materials are loaded from external assets (shader files, texture files,
//! parameter data, etc.). Materials are rendering-mode-agnostic: they work across
//! rasterized, hybrid, raytraced, and path traced rendering.
//!
//! # Material Asset Loading
//!
//! A material asset defines:
//! - The shader program(s) used for rendering
//! - The parameter bindings (push constants, bind groups)
//! - The rendering state (cull mode, front face, depth write, etc.)
//! - The format capabilities (FP16, FP32, HDR, SDR)
//!
//! # Workflow
//!
//! ```rust
//! // At init:
//! let pipeline = MaterialAssetPipeline::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! pipeline.load(&engine, asset)?;
//! let mat = pipeline.material_for("pbr_material")?;
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
use std::collections::HashMap;
use std::sync::Mutex;

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
use sturdy_engine_core::slang::{compile_and_reflect, compile_slang, compile_slang_to_spirv, reflect_pipeline_layout};

// ------------------------------------------------------------------
// Material Asset Types
// ------------------------------------------------------------------

/// A material asset that defines a material from external files.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! let pipeline = MaterialAssetPipeline::new();
//! pipeline.load(&engine, asset)?;
//! ```
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
pub struct MaterialAsset {
    /// The material's name for debugging and bind group naming.
    pub name: String,
    /// The shader source for this material.
    pub shader_source: ShaderSource,
    /// The shader entry point for this material.
    pub shader_entry_point: String,
    /// The shader stage for this material.
    pub shader_stage: ShaderStage,
    /// The shader target for this material.
    pub shader_target: crate::ShaderTarget,
    /// The vertex shader kind for mesh rendering.
    pub vertex_kind: MeshVertexKind,
    /// Optional custom vertex shader descriptor. None uses the built-in default.
    pub vertex_source: Option<ShaderSource>,
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
    /// The material's asset path for debugging.
    pub asset_path: PathBuf,
    /// The material's asset metadata for debugging.
    pub asset_metadata: MaterialAssetMetadata,
    /// The material's validation status for debugging.
    pub validation_status: MaterialAssetValidationStatus,
}

impl MaterialAsset {
    /// Create a new material asset with the given name and shader path.
    pub fn new(name: impl Into<String>, shader_path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            shader_source: ShaderSource::File(shader_path.into()),
            shader_entry_point: "main".to_owned(),
            shader_stage: ShaderStage::Fragment,
            shader_target: crate::ShaderTarget::Spirv,
            vertex_kind: MeshVertexKind::V2d,
            vertex_source: None,
            push_constants: Vec::new(),
            texture_bindings: Vec::new(),
            buffer_bindings: Vec::new(),
            acceleration_structure_bindings: Vec::new(),
            render_state: RenderState::default(),
            format_capabilities: FormatCapabilities::default(),
            raytraced_stages: Vec::new(),
            path_traced_bounces: PathTracedBounceConfig::default(),
            asset_path: shader_path.into(),
            asset_metadata: MaterialAssetMetadata::default(),
            validation_status: MaterialAssetValidationStatus::Unvalidated,
        }
    }

    /// Create a new material asset with the given name and shader source.
    pub fn new_with_source(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            shader_source: ShaderSource::Inline(source.into()),
            shader_entry_point: "main".to_owned(),
            shader_stage: ShaderStage::Fragment,
            shader_target: crate::ShaderTarget::Spirv,
            vertex_kind: MeshVertexKind::V2d,
            vertex_source: None,
            push_constants: Vec::new(),
            texture_bindings: Vec::new(),
            buffer_bindings: Vec::new(),
            acceleration_structure_bindings: Vec::new(),
            render_state: RenderState::default(),
            format_capabilities: FormatCapabilities::default(),
            raytraced_stages: Vec::new(),
            path_traced_bounces: PathTracedBounceConfig::default(),
            asset_path: PathBuf::new(),
            asset_metadata: MaterialAssetMetadata::default(),
            validation_status: MaterialAssetValidationStatus::Unvalidated,
        }
    }

    /// Set the shader entry point for this material asset.
    pub fn with_entry_point(mut self, entry_point: impl Into<String>) -> Self {
        self.shader_entry_point = entry_point.into();
        self
    }

    /// Set the shader stage for this material asset.
    pub fn with_shader_stage(mut self, stage: ShaderStage) -> Self {
        self.shader_stage = stage;
        self
    }

    /// Set the shader target for this material asset.
    pub fn with_shader_target(mut self, target: crate::ShaderTarget) -> Self {
        self.shader_target = target;
        self
    }

    /// Set the vertex shader kind for this material asset.
    pub fn with_vertex_kind(mut self, kind: MeshVertexKind) -> Self {
        self.vertex_kind = kind;
        self
    }

    /// Set the custom vertex shader source for this material asset.
    pub fn with_vertex_source(mut self, source: impl Into<String>) -> Self {
        self.vertex_source = Some(ShaderSource::Inline(source.into()));
        self
    }

    /// Set the custom vertex shader file for this material asset.
    pub fn with_vertex_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.vertex_source = Some(ShaderSource::File(path.into()));
        self
    }

    /// Register a push constant type for this material asset.
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

    /// Register a push constant type with a custom stage mask for this material asset.
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

    /// Register a texture binding for this material asset.
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

    /// Register a texture binding with a custom stage mask for this material asset.
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

    /// Register a buffer binding for this material asset.
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

    /// Register a buffer binding with a custom stage mask for this material asset.
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

    /// Register an acceleration structure binding for this material asset.
    pub fn with_acceleration_structure_binding(mut self, group: impl Into<String>, path: impl Into<String>) -> Self {
        self.acceleration_structure_bindings.push(AccelerationStructureBindingRegistration {
            group: group.into(),
            path: path.into(),
            stage_mask: StageMask::RAY_TRACING,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Register an acceleration structure binding with a custom stage mask for this material asset.
    pub fn with_acceleration_structure_binding_stage(mut self, group: impl Into<String>, path: impl Into<String>, stage: StageMask) -> Self {
        self.acceleration_structure_bindings.push(AccelerationStructureBindingRegistration {
            group: group.into(),
            path: path.into(),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Set the render state for this material asset.
    pub fn with_render_state(mut self, state: RenderState) -> Self {
        self.render_state = state;
        self
    }

    /// Set the format capabilities for this material asset.
    pub fn with_format_capabilities(mut self, caps: FormatCapabilities) -> Self {
        self.format_capabilities = caps;
        self
    }

    /// Set the HDR mode for this material asset.
    pub fn with_hdr_mode(mut self, mode: crate::HdrMode) -> Self {
        self.format_capabilities = FormatCapabilities::from_hdr_mode(mode);
        self
    }

    /// Set the tone mapping for this material asset.
    pub fn with_tone_mapping(mut self, op: crate::ToneMappingOp) -> Self {
        self.format_capabilities = FormatCapabilities::from_tone_mapping(op);
        self
    }

    /// Register a raytraced shader stage for this material asset.
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

    /// Register a raytraced shader stage with a custom stage mask for this material asset.
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

    /// Set the path traced bounce configuration for this material asset.
    pub fn with_path_traced_bounces(mut self, config: PathTracedBounceConfig) -> Self {
        self.path_traced_bounces = config;
        self
    }

    /// Set the path traced bounce count for this material asset.
    pub fn with_path_traced_bounce_count(mut self, count: usize) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_bounce_count(count);
        self
    }

    /// Set the path traced importance sampling for this material asset.
    pub fn with_path_traced_importance_sampling(mut self, sampling: PathTracedImportanceSampling) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_importance_sampling(sampling);
        self
    }

    /// Set the path traced termination strategy for this material asset.
    pub fn with_path_traced_termination(mut self, strategy: PathTracedTerminationStrategy) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_termination_strategy(strategy);
        self
    }

    /// Build the material from this asset.
    pub fn build(self, engine: &Engine) -> Result<Material> {
        let layout = self.create_pipeline_layout(engine)?;
        Ok(Material {
            layout,
            name: self.name,
            vertex_kind: self.vertex_kind,
            fragment_desc: ShaderDesc {
                source: self.shader_source,
                entry_point: self.shader_entry_point,
                stage: self.shader_stage,
            },
            vertex_desc: self.vertex_source.map(|s| ShaderDesc {
                source: s,
                entry_point: "main".to_owned(),
                stage: ShaderStage::Vertex,
            }),
            push_constants: self.push_constants,
            texture_bindings: self.texture_bindings,
            buffer_bindings: self.buffer_bindings,
            acceleration_structure_bindings: self.acceleration_structure_bindings,
            render_state: self.render_state,
            format_capabilities: self.format_capabilities,
            raytraced_stages: self.raytraced_stages,
            path_traced_bounces: self.path_traced_bounces,
        })
    }

    /// Get the pipeline layout for this material asset.
    pub fn pipeline_layout(&self) -> &PipelineLayout {
        &self.layout
    }

    /// Create the pipeline layout from the material asset's bindings.
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

    /// Create a mesh program from this material asset's shader descriptors.
    pub fn create_mesh_program(&self, engine: &Engine) -> Result<MeshProgram> {
        MeshProgram::new(engine, MeshProgramDesc {
            fragment: ShaderDesc {
                source: self.shader_source.clone(),
                entry_point: self.shader_entry_point.clone(),
                stage: self.shader_stage,
            },
            vertex: self.vertex_source.clone(),
            vertex_kind: self.vertex_kind,
        })
    }

    /// Create a compute program from this material asset's shader descriptors.
    pub fn create_compute_program(&self, engine: &Engine) -> Result<ComputeProgram> {
        ComputeProgram::load(engine, PathBuf::from(self.shader_source.file_path().unwrap_or_default()))
    }

    /// Create a raytraced program from this material asset's raytraced stages.
    pub fn create_raytraced_program(&self, engine: &Engine) -> Result<RaytracedProgram> {
        RaytracedProgram::new(engine, self.raytraced_stages.clone())
    }

    /// Create a path traced program from this material asset's path traced bounces.
    pub fn create_path_traced_program(&self, engine: &Engine) -> Result<PathTracedProgram> {
        PathTracedProgram::new(engine, self.path_traced_bounces.clone())
    }

    /// Get the format that this material asset should use for render targets.
    pub fn render_format(&self) -> Format {
        self.format_capabilities.render_format
    }

    /// Get the HDR mode that this material asset supports.
    pub fn hdr_mode(&self) -> crate::HdrMode {
        self.format_capabilities.hdr_mode
    }

    /// Get the tone mapping that this material asset should use.
    pub fn tone_mapping(&self) -> crate::ToneMappingOp {
        self.format_capabilities.tone_mapping
    }

    /// Get the raytracing capabilities that this material asset requires.
    pub fn raytracing_caps(&self) -> RaytracingCapabilities {
        RaytracingCapabilities::from_raytraced_stages(&self.raytraced_stages)
    }

    /// Get the path tracing capabilities that this material asset requires.
    pub fn path_tracing_caps(&self) -> PathTracingCapabilities {
        PathTracingCapabilities::from_path_traced_bounces(&self.path_traced_bounces)
    }

    /// Get the material asset's total push constant byte count.
    pub fn total_push_constants_bytes(&self) -> u32 {
        self.push_constants.iter().map(|pc| pc.total_bytes).sum()
    }

    /// Get the material asset's push constant stage mask.
    pub fn push_constants_stage_mask(&self) -> StageMask {
        self.push_constants.iter().map(|pc| pc.stage_mask).reduce(|a, b| a | b).unwrap_or(StageMask::empty())
    }

    /// Get the material asset's texture binding count.
    pub fn texture_binding_count(&self) -> usize {
        self.texture_bindings.len()
    }

    /// Get the material asset's buffer binding count.
    pub fn buffer_binding_count(&self) -> usize {
        self.buffer_bindings.len()
    }

    /// Get the material asset's acceleration structure binding count.
    pub fn acceleration_structure_binding_count(&self) -> usize {
        self.acceleration_structure_bindings.len()
    }

    /// Get the material asset's raytraced stage count.
    pub fn raytraced_stage_count(&self) -> usize {
        self.raytraced_stages.len()
    }

    /// Get the material asset's path traced bounce count.
    pub fn path_traced_bounce_count(&self) -> usize {
        self.path_traced_bounces.bounce_count
    }

    /// Get the material asset's render state.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the material asset's format capabilities.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the material asset's raytraced stages.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the material asset's path traced bounces.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }

    /// Get the material asset's shader source.
    pub fn shader_source(&self) -> &ShaderSource {
        &self.shader_source
    }

    /// Get the material asset's shader entry point.
    pub fn shader_entry_point(&self) -> &str {
        &self.shader_entry_point
    }

    /// Get the material asset's shader stage.
    pub fn shader_stage(&self) -> ShaderStage {
        self.shader_stage
    }

    /// Get the material asset's shader target.
    pub fn shader_target(&self) -> crate::ShaderTarget {
        self.shader_target
    }

    /// Get the material asset's vertex shader kind.
    pub fn vertex_kind(&self) -> MeshVertexKind {
        self.vertex_kind
    }

    /// Get the material asset's vertex source.
    pub fn vertex_source(&self) -> Option<&ShaderSource> {
        self.vertex_source.as_ref()
    }

    /// Get the material asset's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the material asset's asset path.
    pub fn asset_path(&self) -> &PathBuf {
        &self.asset_path
    }

    /// Get the material asset's asset metadata.
    pub fn asset_metadata(&self) -> &MaterialAssetMetadata {
        &self.asset_metadata
    }

    /// Get the material asset's validation status.
    pub fn validation_status(&self) -> MaterialAssetValidationStatus {
        self.validation_status
    }
}

impl Default for MaterialAsset {
    fn default() -> Self {
        Self::new("default_material", "passthrough_fragment.slang")
            .with_vertex_kind(MeshVertexKind::V2d)
            .build()
            .expect("default material asset should always build")
    }
}

// ------------------------------------------------------------------
// Material Asset Pipeline
// ------------------------------------------------------------------

/// A pipeline for loading, validating, and caching material assets.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let pipeline = MaterialAssetPipeline::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! pipeline.load(&engine, asset)?;
//! ```
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
pub struct MaterialAssetPipeline {
    /// The loaded material assets for this pipeline.
    pub loaded_assets: Vec<MaterialAsset>,
    /// The cached material assets for this pipeline.
    pub cached_assets: Vec<MaterialAsset>,
    /// The validated material assets for this pipeline.
    pub validated_assets: Vec<MaterialAsset>,
    /// The pipeline state for this pipeline.
    pub pipeline_state: MaterialAssetPipelineState,
    /// The size limit for this pipeline.
    pub size_limit: usize,
    /// The eviction strategy for this pipeline.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The asset loader for this pipeline.
    pub asset_loader: MaterialAssetLoader,
    /// The asset validator for this pipeline.
    pub asset_validator: MaterialAssetValidator,
    /// The asset cache for this pipeline.
    pub asset_cache: MaterialAssetCache,
    /// The cross-language compiler for this pipeline.
    pub cross_language_compiler: MaterialCrossLanguageCompiler,
}

impl MaterialAssetPipeline {
    /// Create a new material asset pipeline.
    pub fn new() -> Self {
        Self {
            loaded_assets: Vec::new(),
            cached_assets: Vec::new(),
            validated_assets: Vec::new(),
            pipeline_state: MaterialAssetPipelineState::default(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            asset_loader: MaterialAssetLoader::new(),
            asset_validator: MaterialAssetValidator::new(),
            asset_cache: MaterialAssetCache::new(),
            cross_language_compiler: MaterialCrossLanguageCompiler::new(),
        }
    }

    /// Load a material asset from this pipeline.
    pub fn load(&mut self, engine: &Engine, asset: MaterialAsset) -> Result<Self> {
        self.asset_loader.load(&self, asset)?;
        self.asset_validator.validate(&self, asset)?;
        self.asset_cache.cache(&self, asset)?;
        self.pipeline_state = MaterialAssetPipelineState::Complete;
        Ok(self)
    }

    /// Load multiple material assets from this pipeline.
    pub fn load_many(&mut self, engine: &Engine, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.load(engine, asset)?;
        }
        self.pipeline_state = MaterialAssetPipelineState::Complete;
        Ok(self)
    }

    /// Get a material asset from this pipeline by name.
    pub fn get(&self, name: &str) -> Option<&MaterialAsset> {
        self.loaded_assets.iter().find(|a| a.name == name)
    }

    /// Get a validated material asset from this pipeline by name.
    pub fn get_validated(&self, name: &str) -> Option<&MaterialAsset> {
        self.validated_assets.iter().find(|a| a.name == name)
    }

    /// Get a cached material asset from this pipeline by name.
    pub fn get_cached(&self, name: &str) -> Option<&MaterialAsset> {
        self.cached_assets.iter().find(|a| a.name == name)
    }

    /// Get a material from this pipeline by name.
    pub fn material_for(&self, name: &str) -> Result<Material> {
        let asset = self.get(name).ok_or_else(|| {
            crate::Error::InvalidInput(format!("material asset {:?} not found in pipeline", name))
        })?;
        asset.build(&self.asset_loader.engine)
    }

    /// Get a validated material from this pipeline by name.
    pub fn validated_material_for(&self, name: &str) -> Result<Material> {
        let asset = self.get_validated(name).ok_or_else(|| {
            crate::Error::InvalidInput(format!("validated material asset {:?} not found in pipeline", name))
        })?;
        asset.build(&self.asset_validator.engine)
    }

    /// Get a cached material from this pipeline by name.
    pub fn cached_material_for(&self, name: &str) -> Result<Material> {
        let asset = self.get_cached(name).ok_or_else(|| {
            crate::Error::InvalidInput(format!("cached material asset {:?} not found in pipeline", name))
        })?;
        asset.build(&self.asset_cache.engine)
    }

    /// Evict material assets from this pipeline.
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

    /// Get the loaded assets of this pipeline.
    pub fn loaded_assets(&self) -> &[MaterialAsset] {
        &self.loaded_assets
    }

    /// Get the cached assets of this pipeline.
    pub fn cached_assets(&self) -> &[MaterialAsset] {
        &self.cached_assets
    }

    /// Get the validated assets of this pipeline.
    pub fn validated_assets(&self) -> &[MaterialAsset] {
        &self.validated_assets
    }

    /// Get the pipeline state of this pipeline.
    pub fn pipeline_state(&self) -> MaterialAssetPipelineState {
        self.pipeline_state
    }

    /// Get the size limit of this pipeline.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this pipeline.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
        self.eviction_strategy
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

    /// Get the cross-language compiler of this pipeline.
    pub fn cross_language_compiler(&self) -> &MaterialCrossLanguageCompiler {
        &self.cross_language_compiler
    }

    /// Get the loaded asset count of this pipeline.
    pub fn loaded_count(&self) -> usize {
        self.loaded_assets.len()
    }

    /// Get the cached asset count of this pipeline.
    pub fn cached_count(&self) -> usize {
        self.cached_assets.len()
    }

    /// Get the validated asset count of this pipeline.
    pub fn validated_count(&self) -> usize {
        self.validated_assets.len()
    }

    /// Get the pipeline hit rate of this pipeline.
    pub fn hit_rate(&self) -> f32 {
        if self.loaded_count() + self.cached_count() == 0 {
            0.0
        } else {
            self.validated_count() as f32 / (self.loaded_count() + self.cached_count()) as f32
        }
    }

    /// Get the pipeline miss rate of this pipeline.
    pub fn miss_rate(&self) -> f32 {
        if self.loaded_count() + self.cached_count() == 0 {
            0.0
        } else {
            (self.cached_count() - self.validated_count()) as f32 / (self.loaded_count() + self.cached_count()) as f32
        }
    }
}

impl Default for MaterialAssetPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Asset Loader
// ------------------------------------------------------------------

/// A loader for material assets.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let loader = MaterialAssetLoader::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! loader.load(&pipeline, asset)?;
//! ```
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
pub struct MaterialAssetLoader {
    /// The loaded material assets for this loader.
    pub loaded_assets: Vec<MaterialAsset>,
    /// The loader state for this loader.
    pub loader_state: MaterialAssetLoaderState,
    /// The engine for this loader.
    pub engine: Engine,
    /// The shader compilation cache for this loader.
    pub shader_compilation_cache: MaterialShaderCompilationCache,
}

impl MaterialAssetLoader {
    /// Create a new material asset loader.
    pub fn new() -> Self {
        Self {
            loaded_assets: Vec::new(),
            loader_state: MaterialAssetLoaderState::default(),
            engine: Engine::new().expect("failed to create engine"),
            shader_compilation_cache: MaterialShaderCompilationCache::new(),
        }
    }

    /// Load a material asset from this loader.
    pub fn load(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        self.loader_state = MaterialAssetLoaderState::Loading;
        self.loaded_assets.push(asset);
        self.loader_state = MaterialAssetLoaderState::Loaded;
        Ok(self)
    }

    /// Load multiple material assets from this loader.
    pub fn load_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.load(pipeline, asset)?;
        }
        self.loader_state = MaterialAssetLoaderState::Loaded;
        Ok(self)
    }

    /// Compile a shader from a material asset.
    pub fn compile_shader(&mut self, asset: &MaterialAsset) -> Result<crate::CompiledShaderArtifact> {
        let compile_desc = asset.shader_source.compile_desc();
        self.shader_compilation_cache.compile(&self, asset)?;
        Ok(self.shader_compilation_cache.get(&asset.name))
    }

    /// Get the compiled shader for a material asset.
    pub fn get_compiled_shader(&self, name: &str) -> Option<&crate::CompiledShaderArtifact> {
        self.shader_compilation_cache.get(name)
    }

    /// Get the loaded assets of this loader.
    pub fn loaded_assets(&self) -> &[MaterialAsset] {
        &self.loaded_assets
    }

    /// Get the loader state of this loader.
    pub fn loader_state(&self) -> MaterialAssetLoaderState {
        self.loader_state
    }

    /// Get the engine of this loader.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the shader compilation cache of this loader.
    pub fn shader_compilation_cache(&self) -> &MaterialShaderCompilationCache {
        &self.shader_compilation_cache
    }

    /// Get the loaded asset count of this loader.
    pub fn loaded_count(&self) -> usize {
        self.loaded_assets.len()
    }

    /// Get the shader compilation cache hit rate of this loader.
    pub fn shader_cache_hit_rate(&self) -> f32 {
        if self.loaded_count() + self.shader_compilation_cache.size() == 0 {
            0.0
        } else {
            self.shader_compilation_cache.size() as f32 / (self.loaded_count() + self.shader_compilation_cache.size()) as f32
        }
    }

    /// Get the shader compilation cache miss rate of this loader.
    pub fn shader_cache_miss_rate(&self) -> f32 {
        if self.loaded_count() + self.shader_compilation_cache.size() == 0 {
            0.0
        } else {
            (self.loaded_count() - self.shader_compilation_cache.size()) as f32 / (self.loaded_count() + self.shader_compilation_cache.size()) as f32
        }
    }
}

impl Default for MaterialAssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Asset Validator
// ------------------------------------------------------------------

/// A validator for material assets.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let validator = MaterialAssetValidator::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! validator.validate(&pipeline, asset)?;
//! ```
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
pub struct MaterialAssetValidator {
    /// The validated material assets for this validator.
    pub validated_assets: Vec<MaterialAsset>,
    /// The validator state for this validator.
    pub validator_state: MaterialAssetValidatorState,
    /// The engine for this validator.
    pub engine: Engine,
    /// The shader validation cache for this validator.
    pub shader_validation_cache: MaterialShaderValidationCache,
}

impl MaterialAssetValidator {
    /// Create a new material asset validator.
    pub fn new() -> Self {
        Self {
            validated_assets: Vec::new(),
            validator_state: MaterialAssetValidatorState::default(),
            engine: Engine::new().expect("failed to create engine"),
            shader_validation_cache: MaterialShaderValidationCache::new(),
        }
    }

    /// Validate a material asset from this validator.
    pub fn validate(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        self.validator_state = MaterialAssetValidatorState::Validating;
        let validation = self.validate_asset(&self, asset)?;
        if validation.is_valid {
            self.validated_assets.push(asset);
        }
        self.shader_validation_cache.cache(&self, asset)?;
        self.validator_state = MaterialAssetValidatorState::Validated;
        Ok(self)
    }

    /// Validate a material asset from this validator.
    pub fn validate_asset(&mut self, engine: &Engine, asset: &MaterialAsset) -> Result<MaterialAssetValidationResult> {
        let shader_validation = self.validate_shader(&self, asset)?;
        let binding_validation = self.validate_bindings(&self, asset)?;
        let format_validation = self.validate_format(&self, asset)?;
        let raytracing_validation = self.validate_raytracing(&self, asset)?;
        let path_tracing_validation = self.validate_path_tracing(&self, asset)?;
        Ok(MaterialAssetValidationResult {
            is_valid: shader_validation.is_valid && binding_validation.is_valid && format_validation.is_valid && raytracing_validation.is_valid && path_tracing_validation.is_valid,
            shader_validation: shader_validation,
            binding_validation: binding_validation,
            format_validation: format_validation,
            raytracing_validation: raytracing_validation,
            path_tracing_validation: path_tracing_validation,
        })
    }

    /// Validate the shader for a material asset.
    pub fn validate_shader(&mut self, engine: &Engine, asset: &MaterialAsset) -> Result<MaterialShaderValidationResult> {
        let compile_desc = asset.shader_source.compile_desc();
        let compilation = compile_and_reflect(engine, compile_desc)?;
        Ok(MaterialShaderValidationResult {
            is_valid: compilation.is_ok(),
            compilation_result: compilation,
            shader_source: asset.shader_source.clone(),
            shader_entry_point: asset.shader_entry_point.clone(),
            shader_stage: asset.shader_stage,
            shader_target: asset.shader_target,
        })
    }

    /// Validate the bindings for a material asset.
    pub fn validate_bindings(&mut self, engine: &Engine, asset: &MaterialAsset) -> Result<MaterialBindingValidationResult> {
        let layout = asset.create_pipeline_layout(engine)?;
        Ok(MaterialBindingValidationResult {
            is_valid: layout.is_ok(),
            layout_result: layout,
            binding_count: asset.push_constants.len() + asset.texture_bindings.len() + asset.buffer_bindings.len() + asset.acceleration_structure_bindings.len() + asset.raytraced_stages.len(),
            push_constant_count: asset.push_constants.len(),
            texture_binding_count: asset.texture_bindings.len(),
            buffer_binding_count: asset.buffer_bindings.len(),
            acceleration_structure_binding_count: asset.acceleration_structure_bindings.len(),
            raytraced_stage_count: asset.raytraced_stages.len(),
        })
    }

    /// Validate the format for a material asset.
    pub fn validate_format(&mut self, engine: &Engine, asset: &MaterialAsset) -> Result<MaterialFormatValidationResult> {
        let format_caps = engine.format_capabilities(asset.render_format());
        Ok(MaterialFormatValidationResult {
            is_valid: format_caps.sampled && format_caps.color_attachment,
            format_caps: format_caps,
            render_format: asset.render_format(),
            hdr_mode: asset.hdr_mode(),
            tone_mapping: asset.tone_mapping(),
            fp16_render: asset.format_capabilities.fp16_render,
            fp32_render: asset.format_capabilities.fp32_render,
            hdr_output: asset.format_capabilities.hdr_output,
            shader_fp16: asset.format_capabilities.shader_fp16,
            shader_fp64: asset.format_capabilities.shader_fp64,
            variable_rate_shading: asset.format_capabilities.variable_rate_shading,
            bindless: asset.format_capabilities.bindless,
        })
    }

    /// Validate the raytracing for a material asset.
    pub fn validate_raytracing(&mut self, engine: &Engine, asset: &MaterialAsset) -> Result<MaterialRaytracingValidationResult> {
        let raytracing_caps = asset.raytracing_caps();
        let raytracing_supported = engine.caps().supports_raytracing;
        Ok(MaterialRaytracingValidationResult {
            is_valid: raytracing_supported,
            raytracing_caps: raytracing_caps,
            raytracing_supported: raytracing_supported,
            raytracing_stage_count: asset.raytraced_stage_count(),
            raytraced_stages: asset.raytraced_stages.clone(),
            acceleration_structure_binding_count: asset.acceleration_structure_binding_count(),
            acceleration_structure_bindings: asset.acceleration_structure_bindings.clone(),
        })
    }

    /// Validate the path tracing for a material asset.
    pub fn validate_path_tracing(&mut self, engine: &Engine, asset: &MaterialAsset) -> Result<MaterialPathTracingValidationResult> {
        let path_tracing_caps = asset.path_tracing_caps();
        let path_tracing_supported = engine.caps().supports_raytracing;
        Ok(MaterialPathTracingValidationResult {
            is_valid: path_tracing_supported,
            path_tracing_caps: path_tracing_caps,
            path_tracing_supported: path_tracing_supported,
            path_traced_bounce_count: asset.path_traced_bounce_count(),
            path_traced_bounces: asset.path_traced_bounces.clone(),
            importance_sampling: asset.path_traced_bounces.importance_sampling(),
            termination_strategy: asset.path_traced_bounces.termination_strategy(),
        })
    }

    /// Get the validated assets of this validator.
    pub fn validated_assets(&self) -> &[MaterialAsset] {
        &self.validated_assets
    }

    /// Get the validator state of this validator.
    pub fn validator_state(&self) -> MaterialAssetValidatorState {
        self.validator_state
    }

    /// Get the engine of this validator.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the shader validation cache of this validator.
    pub fn shader_validation_cache(&self) -> &MaterialShaderValidationCache {
        &self.shader_validation_cache
    }

    /// Get the validated asset count of this validator.
    pub fn validated_count(&self) -> usize {
        self.validated_assets.len()
    }

    /// Get the shader validation cache hit rate of this validator.
    pub fn shader_cache_hit_rate(&self) -> f32 {
        if self.validated_count() + self.shader_validation_cache.size() == 0 {
            0.0
        } else {
            self.shader_validation_cache.size() as f32 / (self.validated_count() + self.shader_validation_cache.size()) as f32
        }
    }

    /// Get the shader validation cache miss rate of this validator.
    pub fn shader_cache_miss_rate(&self) -> f32 {
        if self.validated_count() + self.shader_validation_cache.size() == 0 {
            0.0
        } else {
            (self.validated_count() - self.shader_validation_cache.size()) as f32 / (self.validated_count() + self.shader_validation_cache.size()) as f32
        }
    }
}

impl Default for MaterialAssetValidator {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Asset Cache
// ------------------------------------------------------------------

/// A cache for material assets.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialAssetCache::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.cache(&pipeline, asset)?;
//! ```
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
pub struct MaterialAssetCache {
    /// The cached material assets for this cache.
    pub cached_assets: Vec<MaterialAsset>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The engine for this cache.
    pub engine: Engine,
    /// The cache hit count for this cache.
    pub hit_count: usize,
    /// The cache miss count for this cache.
    pub miss_count: usize,
}

impl MaterialAssetCache {
    /// Create a new material asset cache.
    pub fn new() -> Self {
        Self {
            cached_assets: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            engine: Engine::new().expect("failed to create engine"),
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Cache a material asset from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_assets.len() >= self.size_limit {
            self.evict();
        }
        self.cached_assets.push(asset);
        Ok(self)
    }

    /// Cache multiple material assets from this cache.
    pub fn cache_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.cache(pipeline, asset)?;
        }
        Ok(self)
    }

    /// Get a cached material asset from this cache by name.
    pub fn get(&self, name: &str) -> Option<&MaterialAsset> {
        self.cached_assets.iter().find(|a| a.name == name)
    }

    /// Evict material assets from this cache.
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

    /// Get the engine of this cache.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the hit count of this cache.
    pub fn hit_count(&self) -> usize {
        self.hit_count
    }

    /// Get the miss count of this cache.
    pub fn miss_count(&self) -> usize {
        self.miss_count
    }

    /// Get the cached asset count of this cache.
    pub fn cached_count(&self) -> usize {
        self.cached_assets.len()
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

    /// Get the cache eviction count of this cache.
    pub fn eviction_count(&self) -> usize {
        self.cached_count().saturating_sub(self.cached_assets.len())
    }
}

impl Default for MaterialAssetCache {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Cross-Language Compiler
// ------------------------------------------------------------------

/// A compiler for material assets across languages.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let compiler = MaterialCrossLanguageCompiler::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! compiler.compile(&pipeline, asset)?;
//! ```
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
pub struct MaterialCrossLanguageCompiler {
    /// The compiled material assets for this compiler.
    pub compiled_assets: Vec<MaterialAsset>,
    /// The compiler state for this compiler.
    pub compiler_state: MaterialCrossLanguageCompilerState,
    /// The engine for this compiler.
    pub engine: Engine,
    /// The shader compilation cache for this compiler.
    pub shader_compilation_cache: MaterialShaderCompilationCache,
    /// The cross-language compiler cache for this compiler.
    pub cross_language_cache: MaterialCrossLanguageCache,
}

impl MaterialCrossLanguageCompiler {
    /// Create a new material cross-language compiler.
    pub fn new() -> Self {
        Self {
            compiled_assets: Vec::new(),
            compiler_state: MaterialCrossLanguageCompilerState::default(),
            engine: Engine::new().expect("failed to create engine"),
            shader_compilation_cache: MaterialShaderCompilationCache::new(),
            cross_language_cache: MaterialCrossLanguageCache::new(),
        }
    }

    /// Compile a material asset from this compiler.
    pub fn compile(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        self.compiler_state = MaterialCrossLanguageCompilerState::Compiling;
        let compilation = self.compile_shader(&self, asset)?;
        if compilation.is_ok() {
            self.compiled_assets.push(asset);
        }
        self.shader_compilation_cache.cache(&self, asset)?;
        self.cross_language_cache.cache(&self, asset)?;
        self.compiler_state = MaterialCrossLanguageCompilerState::Compiled;
        Ok(self)
    }

    /// Compile multiple material assets from this compiler.
    pub fn compile_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.compile(pipeline, asset)?;
        }
        self.compiler_state = MaterialCrossLanguageCompilerState::Compiled;
        Ok(self)
    }

    /// Compile the shader for a material asset.
    pub fn compile_shader(&mut self, engine: &Engine, asset: &MaterialAsset) -> Result<crate::CompiledShaderArtifact> {
        let compile_desc = asset.shader_source.compile_desc();
        let compilation = compile_and_reflect(engine, compile_desc)?;
        Ok(self.shader_compilation_cache.get(&asset.name))
    }

    /// Get the compiled assets of this compiler.
    pub fn compiled_assets(&self) -> &[MaterialAsset] {
        &self.compiled_assets
    }

    /// Get the compiler state of this compiler.
    pub fn compiler_state(&self) -> MaterialCrossLanguageCompilerState {
        self.compiler_state
    }

    /// Get the engine of this compiler.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the shader compilation cache of this compiler.
    pub fn shader_compilation_cache(&self) -> &MaterialShaderCompilationCache {
        &self.shader_compilation_cache
    }

    /// Get the cross-language cache of this compiler.
    pub fn cross_language_cache(&self) -> &MaterialCrossLanguageCache {
        &self.cross_language_cache
    }

    /// Get the compiled asset count of this compiler.
    pub fn compiled_count(&self) -> usize {
        self.compiled_assets.len()
    }

    /// Get the shader compilation cache hit rate of this compiler.
    pub fn shader_cache_hit_rate(&self) -> f32 {
        if self.compiled_count() + self.shader_compilation_cache.size() == 0 {
            0.0
        } else {
            self.shader_compilation_cache.size() as f32 / (self.compiled_count() + self.shader_compilation_cache.size()) as f32
        }
    }

    /// Get the shader compilation cache miss rate of this compiler.
    pub fn shader_cache_miss_rate(&self) -> f32 {
        if self.compiled_count() + self.shader_compilation_cache.size() == 0 {
            0.0
        } else {
            (self.compiled_count() - self.shader_compilation_cache.size()) as f32 / (self.compiled_count() + self.shader_compilation_cache.size()) as f32
        }
    }

    /// Get the cross-language cache hit rate of this compiler.
    pub fn cross_language_cache_hit_rate(&self) -> f32 {
        if self.compiled_count() + self.cross_language_cache.size() == 0 {
            0.0
        } else {
            self.cross_language_cache.size() as f32 / (self.compiled_count() + self.cross_language_cache.size()) as f32
        }
    }

    /// Get the cross-language cache miss rate of this compiler.
    pub fn cross_language_cache_miss_rate(&self) -> f32 {
        if self.compiled_count() + self.cross_language_cache.size() == 0 {
            0.0
        } else {
            (self.compiled_count() - self.cross_language_cache.size()) as f32 / (self.compiled_count() + self.cross_language_cache.size()) as f32
        }
    }
}

impl Default for MaterialCrossLanguageCompiler {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Shader Compilation Cache
// ------------------------------------------------------------------

/// A cache for material shader compilations.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialShaderCompilationCache::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.cache(&pipeline, asset)?;
//! ```
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
pub struct MaterialShaderCompilationCache {
    /// The cached shader compilations for this cache.
    pub cached_compilations: Vec<crate::CompiledShaderArtifact>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The cache hit count for this cache.
    pub hit_count: usize,
    /// The cache miss count for this cache.
    pub miss_count: usize,
}

impl MaterialShaderCompilationCache {
    /// Create a new material shader compilation cache.
    pub fn new() -> Self {
        Self {
            cached_compilations: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Cache a shader compilation from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_compilations.len() >= self.size_limit {
            self.evict();
        }
        self.cached_compilations.push(asset.shader_source.compile_artifact());
        Ok(self)
    }

    /// Cache multiple shader compilations from this cache.
    pub fn cache_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.cache(pipeline, asset)?;
        }
        Ok(self)
    }

    /// Get a cached shader compilation from this cache by name.
    pub fn get(&self, name: &str) -> Option<&crate::CompiledShaderArtifact> {
        self.cached_compilations.iter().find(|c| c.target == crate::ShaderTarget::Spirv)
    }

    /// Evict shader compilations from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.cached_compilations.remove(0);
            }
            Self::SizeBased => {
                self.cached_compilations.remove(self.cached_compilations.len() - 1);
            }
            Self::Random => {
                let index = self.cached_compilations.len() / 2;
                self.cached_compilations.remove(index);
            }
        }
    }

    /// Get the cached compilations of this cache.
    pub fn cached_compilations(&self) -> &[crate::CompiledShaderArtifact] {
        &self.cached_compilations
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
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

    /// Get the cached compilation count of this cache.
    pub fn cached_count(&self) -> usize {
        self.cached_compilations.len()
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

impl Default for MaterialShaderCompilationCache {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Shader Validation Cache
// ------------------------------------------------------------------

/// A cache for material shader validations.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialShaderValidationCache::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.cache(&pipeline, asset)?;
//! ```
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
pub struct MaterialShaderValidationCache {
    /// The cached shader validations for this cache.
    pub cached_validations: Vec<MaterialShaderValidationResult>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The cache hit count for this cache.
    pub hit_count: usize,
    /// The cache miss count for this cache.
    pub miss_count: usize,
}

impl MaterialShaderValidationCache {
    /// Create a new material shader validation cache.
    pub fn new() -> Self {
        Self {
            cached_validations: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Cache a shader validation from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_validations.len() >= self.size_limit {
            self.evict();
        }
        self.cached_validations.push(asset.shader_source.validate_artifact());
        Ok(self)
    }

    /// Cache multiple shader validations from this cache.
    pub fn cache_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.cache(pipeline, asset)?;
        }
        Ok(self)
    }

    /// Get a cached shader validation from this cache by name.
    pub fn get(&self, name: &str) -> Option<&MaterialShaderValidationResult> {
        self.cached_validations.iter().find(|v| v.is_valid)
    }

    /// Evict shader validations from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.cached_validations.remove(0);
            }
            Self::SizeBased => {
                self.cached_validations.remove(self.cached_validations.len() - 1);
            }
            Self::Random => {
                let index = self.cached_validations.len() / 2;
                self.cached_validations.remove(index);
            }
        }
    }

    /// Get the cached validations of this cache.
    pub fn cached_validations(&self) -> &[MaterialShaderValidationResult] {
        &self.cached_validations
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
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

    /// Get the cached validation count of this cache.
    pub fn cached_count(&self) -> usize {
        self.cached_validations.len()
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

impl Default for MaterialShaderValidationCache {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Cross-Language Cache
// ------------------------------------------------------------------

/// A cache for material cross-language compilations.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialCrossLanguageCache::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.cache(&pipeline, asset)?;
//! ```
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
pub struct MaterialCrossLanguageCache {
    /// The cached cross-language compilations for this cache.
    pub cached_compilations: Vec<crate::CompiledShaderArtifact>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The cache hit count for this cache.
    pub hit_count: usize,
    /// The cache miss count for this cache.
    pub miss_count: usize,
}

impl MaterialCrossLanguageCache {
    /// Create a new material cross-language cache.
    pub fn new() -> Self {
        Self {
            cached_compilations: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Cache a cross-language compilation from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_compilations.len() >= self.size_limit {
            self.evict();
        }
        self.cached_compilations.push(asset.shader_source.compile_artifact());
        Ok(self)
    }

    /// Cache multiple cross-language compilations from this cache.
    pub fn cache_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.cache(pipeline, asset)?;
        }
        Ok(self)
    }

    /// Get a cached cross-language compilation from this cache by name.
    pub fn get(&self, name: &str) -> Option<&crate::CompiledShaderArtifact> {
        self.cached_compilations.iter().find(|c| c.target == crate::ShaderTarget::Spirv)
    }

    /// Evict cross-language compilations from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.cached_compilations.remove(0);
            }
            Self::SizeBased => {
                self.cached_compilations.remove(self.cached_compilations.len() - 1);
            }
            Self::Random => {
                let index = self.cached_compilations.len() / 2;
                self.cached_compilations.remove(index);
            }
        }
    }

    /// Get the cached compilations of this cache.
    pub fn cached_compilations(&self) -> &[crate::CompiledShaderArtifact] {
        &self.cached_compilations
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
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

    /// Get the cached compilation count of this cache.
    pub fn cached_count(&self) -> usize {
        self.cached_compilations.len()
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

impl Default for MaterialCrossLanguageCache {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Asset Metadata
// ------------------------------------------------------------------

/// Metadata for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let metadata = MaterialAssetMetadata::default();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! asset.metadata = metadata;
//! ```
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
    /// The asset format for this metadata.
    pub format: MaterialAssetFormat,
    /// The asset shader target for this metadata.
    pub shader_target: crate::ShaderTarget,
    /// The asset shader stage for this metadata.
    pub shader_stage: ShaderStage,
    /// The asset entry point for this metadata.
    pub entry_point: String,
    /// The asset push constants for this metadata.
    pub push_constants: Vec<PushConstantRegistration>,
    /// The asset texture bindings for this metadata.
    pub texture_bindings: Vec<TextureBindingRegistration>,
    /// The asset buffer bindings for this metadata.
    pub buffer_bindings: Vec<BufferBindingRegistration>,
    /// The asset acceleration structure bindings for this metadata.
    pub acceleration_structure_bindings: Vec<AccelerationStructureBindingRegistration>,
    /// The asset raytraced stages for this metadata.
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// The asset path traced bounces for this metadata.
    pub path_traced_bounces: PathTracedBounceConfig,
    /// The asset render state for this metadata.
    pub render_state: RenderState,
    /// The asset format capabilities for this metadata.
    pub format_capabilities: FormatCapabilities,
}

impl MaterialAssetMetadata {
    /// Create a new material asset metadata.
    pub fn new() -> Self {
        Self {
            version: 0,
            author: String::new(),
            date: String::new(),
            license: String::new(),
            description: String::new(),
            format: MaterialAssetFormat::default(),
            shader_target: crate::ShaderTarget::Spirv,
            shader_stage: ShaderStage::Fragment,
            entry_point: "main".to_owned(),
            push_constants: Vec::new(),
            texture_bindings: Vec::new(),
            buffer_bindings: Vec::new(),
            acceleration_structure_bindings: Vec::new(),
            raytraced_stages: Vec::new(),
            path_traced_bounces: PathTracedBounceConfig::default(),
            render_state: RenderState::default(),
            format_capabilities: FormatCapabilities::default(),
        }
    }

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

    /// Get the format of this metadata.
    pub fn format(&self) -> MaterialAssetFormat {
        self.format
    }

    /// Get the shader target of this metadata.
    pub fn shader_target(&self) -> crate::ShaderTarget {
        self.shader_target
    }

    /// Get the shader stage of this metadata.
    pub fn shader_stage(&self) -> ShaderStage {
        self.shader_stage
    }

    /// Get the entry point of this metadata.
    pub fn entry_point(&self) -> &str {
        &self.entry_point
    }

    /// Get the push constants of this metadata.
    pub fn push_constants(&self) -> &[PushConstantRegistration] {
        &self.push_constants
    }

    /// Get the texture bindings of this metadata.
    pub fn texture_bindings(&self) -> &[TextureBindingRegistration] {
        &self.texture_bindings
    }

    /// Get the buffer bindings of this metadata.
    pub fn buffer_bindings(&self) -> &[BufferBindingRegistration] {
        &self.buffer_bindings
    }

    /// Get the acceleration structure bindings of this metadata.
    pub fn acceleration_structure_bindings(&self) -> &[AccelerationStructureBindingRegistration] {
        &self.acceleration_structure_bindings
    }

    /// Get the raytraced stages of this metadata.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the path traced bounces of this metadata.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }

    /// Get the render state of this metadata.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the format capabilities of this metadata.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }
}

impl Default for MaterialAssetMetadata {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Asset Validation
// ------------------------------------------------------------------

/// Validation status for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let status = MaterialAssetValidationStatus::Unvalidated;
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! asset.validation_status = status;
//! ```
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialAssetValidationStatus {
    /// Unvalidated status.
    #[default]
    Unvalidated,
    /// Validating status.
    Validating,
    /// Validated status.
    Validated,
    /// Error status.
    Error,
}

impl MaterialAssetValidationStatus {
    /// Get the validation weight for this status.
    pub fn weight(self) -> f32 {
        match self {
            Self::Unvalidated => 1.0,
            Self::Validating => 0.5,
            Self::Validated => 0.25,
            Self::Error => 0.0,
        }
    }
}

/// Material asset validation result.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let result = MaterialAssetValidationResult::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! result.validate(&pipeline, asset)?;
//! ```
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
pub struct MaterialAssetValidationResult {
    /// The validation result for this material asset.
    pub is_valid: bool,
    /// The shader validation result for this material asset.
    pub shader_validation: MaterialShaderValidationResult,
    /// The binding validation result for this material asset.
    pub binding_validation: MaterialBindingValidationResult,
    /// The format validation result for this material asset.
    pub format_validation: MaterialFormatValidationResult,
    /// The raytracing validation result for this material asset.
    pub raytracing_validation: MaterialRaytracingValidationResult,
    /// The path tracing validation result for this material asset.
    pub path_tracing_validation: MaterialPathTracingValidationResult,
}

impl MaterialAssetValidationResult {
    /// Create a new material asset validation result.
    pub fn new() -> Self {
        Self {
            is_valid: false,
            shader_validation: MaterialShaderValidationResult::new(),
            binding_validation: MaterialBindingValidationResult::new(),
            format_validation: MaterialFormatValidationResult::new(),
            raytracing_validation: MaterialRaytracingValidationResult::new(),
            path_tracing_validation: MaterialPathTracingValidationResult::new(),
        }
    }

    /// Get the validation result of this result.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// Get the shader validation of this result.
    pub fn shader_validation(&self) -> &MaterialShaderValidationResult {
        &self.shader_validation
    }

    /// Get the binding validation of this result.
    pub fn binding_validation(&self) -> &MaterialBindingValidationResult {
        &self.binding_validation
    }

    /// Get the format validation of this result.
    pub fn format_validation(&self) -> &MaterialFormatValidationResult {
        &self.format_validation
    }

    /// Get the raytracing validation of this result.
    pub fn raytracing_validation(&self) -> &MaterialRaytracingValidationResult {
        &self.raytracing_validation
    }

    /// Get the path tracing validation of this result.
    pub fn path_tracing_validation(&self) -> &MaterialPathTracingValidationResult {
        &self.path_tracing_validation
    }
}

impl Default for MaterialAssetValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Shader Validation Result
// ------------------------------------------------------------------

/// Shader validation result for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let result = MaterialShaderValidationResult::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! result.validate(&pipeline, asset)?;
//! ```
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
pub struct MaterialShaderValidationResult {
    /// The validation result for this shader.
    pub is_valid: bool,
    /// The compilation result for this shader.
    pub compilation_result: Result<crate::CompiledShaderArtifact>,
    /// The shader source for this shader.
    pub shader_source: ShaderSource,
    /// The shader entry point for this shader.
    pub shader_entry_point: String,
    /// The shader stage for this shader.
    pub shader_stage: ShaderStage,
    /// The shader target for this shader.
    pub shader_target: crate::ShaderTarget,
}

impl MaterialShaderValidationResult {
    /// Create a new material shader validation result.
    pub fn new() -> Self {
        Self {
            is_valid: false,
            compilation_result: Err(crate::Error::Unsupported("shader validation not performed")),
            shader_source: ShaderSource::Inline(String::new()),
            shader_entry_point: "main".to_owned(),
            shader_stage: ShaderStage::Fragment,
            shader_target: crate::ShaderTarget::Spirv,
        }
    }

    /// Get the validation result of this result.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// Get the compilation result of this result.
    pub fn compilation_result(&self) -> &Result<crate::CompiledShaderArtifact> {
        &self.compilation_result
    }

    /// Get the shader source of this result.
    pub fn shader_source(&self) -> &ShaderSource {
        &self.shader_source
    }

    /// Get the shader entry point of this result.
    pub fn shader_entry_point(&self) -> &str {
        &self.shader_entry_point
    }

    /// Get the shader stage of this result.
    pub fn shader_stage(&self) -> ShaderStage {
        self.shader_stage
    }

    /// Get the shader target of this result.
    pub fn shader_target(&self) -> crate::ShaderTarget {
        self.shader_target
    }
}

impl Default for MaterialShaderValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Binding Validation Result
// ------------------------------------------------------------------

/// Binding validation result for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let result = MaterialBindingValidationResult::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! result.validate(&pipeline, asset)?;
//! ```
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
pub struct MaterialBindingValidationResult {
    /// The validation result for this binding.
    pub is_valid: bool,
    /// The layout result for this binding.
    pub layout_result: Result<PipelineLayout>,
    /// The binding count for this binding.
    pub binding_count: usize,
    /// The push constant count for this binding.
    pub push_constant_count: usize,
    /// The texture binding count for this binding.
    pub texture_binding_count: usize,
    /// The buffer binding count for this binding.
    pub buffer_binding_count: usize,
    /// The acceleration structure binding count for this binding.
    pub acceleration_structure_binding_count: usize,
    /// The raytraced stage count for this binding.
    pub raytraced_stage_count: usize,
}

impl MaterialBindingValidationResult {
    /// Create a new material binding validation result.
    pub fn new() -> Self {
        Self {
            is_valid: false,
            layout_result: Err(crate::Error::Unsupported("binding validation not performed")),
            binding_count: 0,
            push_constant_count: 0,
            texture_binding_count: 0,
            buffer_binding_count: 0,
            acceleration_structure_binding_count: 0,
            raytraced_stage_count: 0,
        }
    }

    /// Get the validation result of this result.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// Get the layout result of this result.
    pub fn layout_result(&self) -> &Result<PipelineLayout> {
        &self.layout_result
    }

    /// Get the binding count of this result.
    pub fn binding_count(&self) -> usize {
        self.binding_count
    }

    /// Get the push constant count of this result.
    pub fn push_constant_count(&self) -> usize {
        self.push_constant_count
    }

    /// Get the texture binding count of this result.
    pub fn texture_binding_count(&self) -> usize {
        self.texture_binding_count
    }

    /// Get the buffer binding count of this result.
    pub fn buffer_binding_count(&self) -> usize {
        self.buffer_binding_count
    }

    /// Get the acceleration structure binding count of this result.
    pub fn acceleration_structure_binding_count(&self) -> usize {
        self.acceleration_structure_binding_count
    }

    /// Get the raytraced stage count of this result.
    pub fn raytraced_stage_count(&self) -> usize {
        self.raytraced_stage_count
    }
}

impl Default for MaterialBindingValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Format Validation Result
// ------------------------------------------------------------------

/// Format validation result for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let result = MaterialFormatValidationResult::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! result.validate(&pipeline, asset)?;
//! ```
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
pub struct MaterialFormatValidationResult {
    /// The validation result for this format.
    pub is_valid: bool,
    /// The format capabilities for this format.
    pub format_caps: FormatCapabilities,
    /// The render format for this format.
    pub render_format: Format,
    /// The HDR mode for this format.
    pub hdr_mode: crate::HdrMode,
    /// The tone mapping for this format.
    pub tone_mapping: crate::ToneMappingOp,
    /// The FP16 render capability for this format.
    pub fp16_render: bool,
    /// The FP32 render capability for this format.
    pub fp32_render: bool,
    /// The HDR output capability for this format.
    pub hdr_output: bool,
    /// The shader FP16 capability for this format.
    pub shader_fp16: bool,
    /// The shader FP64 capability for this format.
    pub shader_fp64: bool,
    /// The variable rate shading capability for this format.
    pub variable_rate_shading: bool,
    /// The bindless capability for this format.
    pub bindless: bool,
}

impl MaterialFormatValidationResult {
    /// Create a new material format validation result.
    pub fn new() -> Self {
        Self {
            is_valid: false,
            format_caps: FormatCapabilities::default(),
            render_format: Format::Rgba8Unorm,
            hdr_mode: crate::HdrMode::Sdr,
            tone_mapping: crate::ToneMappingOp::Aces,
            fp16_render: false,
            fp32_render: false,
            hdr_output: false,
            shader_fp16: false,
            shader_fp64: false,
            variable_rate_shading: false,
            bindless: false,
        }
    }

    /// Get the validation result of this result.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// Get the format capabilities of this result.
    pub fn format_caps(&self) -> &FormatCapabilities {
        &self.format_caps
    }

    /// Get the render format of this result.
    pub fn render_format(&self) -> Format {
        self.render_format
    }

    /// Get the HDR mode of this result.
    pub fn hdr_mode(&self) -> crate::HdrMode {
        self.hdr_mode
    }

    /// Get the tone mapping of this result.
    pub fn tone_mapping(&self) -> crate::ToneMappingOp {
        self.tone_mapping
    }

    /// Get the FP16 render capability of this result.
    pub fn fp16_render(&self) -> bool {
        self.fp16_render
    }

    /// Get the FP32 render capability of this result.
    pub fn fp32_render(&self) -> bool {
        self.fp32_render
    }

    /// Get the HDR output capability of this result.
    pub fn hdr_output(&self) -> bool {
        self.hdr_output
    }

    /// Get the shader FP16 capability of this result.
    pub fn shader_fp16(&self) -> bool {
        self.shader_fp16
    }

    /// Get the shader FP64 capability of this result.
    pub fn shader_fp64(&self) -> bool {
        self.shader_fp64
    }

    /// Get the variable rate shading capability of this result.
    pub fn variable_rate_shading(&self) -> bool {
        self.variable_rate_shading
    }

    /// Get the bindless capability of this result.
    pub fn bindless(&self) -> bool {
        self.bindless
    }
}

impl Default for MaterialFormatValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Raytracing Validation Result
// ------------------------------------------------------------------

/// Raytracing validation result for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let result = MaterialRaytracingValidationResult::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! result.validate(&pipeline, asset)?;
//! ```
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
pub struct MaterialRaytracingValidationResult {
    /// The validation result for this raytracing.
    pub is_valid: bool,
    /// The raytracing capabilities for this raytracing.
    pub raytracing_caps: RaytracingCapabilities,
    /// The raytracing supported for this raytracing.
    pub raytracing_supported: bool,
    /// The raytraced stage count for this raytracing.
    pub raytraced_stage_count: usize,
    /// The raytraced stages for this raytracing.
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// The acceleration structure binding count for this raytracing.
    pub acceleration_structure_binding_count: usize,
    /// The acceleration structure bindings for this raytracing.
    pub acceleration_structure_bindings: Vec<AccelerationStructureBindingRegistration>,
}

impl MaterialRaytracingValidationResult {
    /// Create a new material raytracing validation result.
    pub fn new() -> Self {
        Self {
            is_valid: false,
            raytracing_caps: RaytracingCapabilities::None,
            raytracing_supported: false,
            raytraced_stage_count: 0,
            raytraced_stages: Vec::new(),
            acceleration_structure_binding_count: 0,
            acceleration_structure_bindings: Vec::new(),
        }
    }

    /// Get the validation result of this result.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// Get the raytracing capabilities of this result.
    pub fn raytracing_caps(&self) -> &RaytracingCapabilities {
        &self.raytracing_caps
    }

    /// Get the raytracing supported of this result.
    pub fn raytracing_supported(&self) -> bool {
        self.raytracing_supported
    }

    /// Get the raytraced stage count of this result.
    pub fn raytraced_stage_count(&self) -> usize {
        self.raytraced_stage_count
    }

    /// Get the raytraced stages of this result.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the acceleration structure binding count of this result.
    pub fn acceleration_structure_binding_count(&self) -> usize {
        self.acceleration_structure_binding_count
    }

    /// Get the acceleration structure bindings of this result.
    pub fn acceleration_structure_bindings(&self) -> &[AccelerationStructureBindingRegistration] {
        &self.acceleration_structure_bindings
    }
}

impl Default for MaterialRaytracingValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Path Tracing Validation Result
// ------------------------------------------------------------------

/// Path tracing validation result for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let result = MaterialPathTracingValidationResult::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! result.validate(&pipeline, asset)?;
//! ```
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
pub struct MaterialPathTracingValidationResult {
    /// The validation result for this path tracing.
    pub is_valid: bool,
    /// The path tracing capabilities for this path tracing.
    pub path_tracing_caps: PathTracingCapabilities,
    /// The path tracing supported for this path tracing.
    pub path_tracing_supported: bool,
    /// The path traced bounce count for this path tracing.
    pub path_traced_bounce_count: usize,
    /// The path traced bounces for this path tracing.
    pub path_traced_bounces: PathTracedBounceConfig,
    /// The importance sampling for this path tracing.
    pub importance_sampling: PathTracedImportanceSampling,
    /// The termination strategy for this path tracing.
    pub termination_strategy: PathTracedTerminationStrategy,
}

impl MaterialPathTracingValidationResult {
    /// Create a new material path tracing validation result.
    pub fn new() -> Self {
        Self {
            is_valid: false,
            path_tracing_caps: PathTracingCapabilities::None,
            path_tracing_supported: false,
            path_traced_bounce_count: 0,
            path_traced_bounces: PathTracedBounceConfig::default(),
            importance_sampling: PathTracedImportanceSampling::Uniform,
            termination_strategy: PathTracedTerminationStrategy::BounceCount,
        }
    }

    /// Get the validation result of this result.
    pub fn is_valid(&self) -> bool {
        self.is_valid
    }

    /// Get the path tracing capabilities of this result.
    pub fn path_tracing_caps(&self) -> &PathTracingCapabilities {
        &self.path_tracing_caps
    }

    /// Get the path tracing supported of this result.
    pub fn path_tracing_supported(&self) -> bool {
        self.path_tracing_supported
    }

    /// Get the path traced bounce count of this result.
    pub fn path_traced_bounce_count(&self) -> usize {
        self.path_traced_bounce_count
    }

    /// Get the path traced bounces of this result.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }

    /// Get the importance sampling of this result.
    pub fn importance_sampling(&self) -> PathTracedImportanceSampling {
        self.importance_sampling
    }

    /// Get the termination strategy of this result.
    pub fn termination_strategy(&self) -> PathTracedTerminationStrategy {
        self.termination_strategy
    }
}

impl Default for MaterialPathTracingValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Asset Pipeline State
// ------------------------------------------------------------------

/// Pipeline state for a material asset pipeline.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let state = MaterialAssetPipelineState::default();
//! let pipeline = MaterialAssetPipeline::new();
//! pipeline.pipeline_state = state;
//! ```
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
// Material Asset Loader State
// ------------------------------------------------------------------

/// Loader state for a material asset loader.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let state = MaterialAssetLoaderState::default();
//! let loader = MaterialAssetLoader::new();
//! loader.loader_state = state;
//! ```
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

// ------------------------------------------------------------------
// Material Asset Validator State
// ------------------------------------------------------------------

/// Validator state for a material asset validator.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let state = MaterialAssetValidatorState::default();
//! let validator = MaterialAssetValidator::new();
//! validator.validator_state = state;
//! ```
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

// ------------------------------------------------------------------
// Material Cross-Language Compiler State
// ------------------------------------------------------------------

/// Compiler state for a material cross-language compiler.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let state = MaterialCrossLanguageCompilerState::default();
//! let compiler = MaterialCrossLanguageCompiler::new();
//! compiler.compiler_state = state;
//! ```
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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialCrossLanguageCompilerState {
    /// Idle state.
    #[default]
    Idle,
    /// Compiling state.
    Compiling,
    /// Compiled state.
    Compiled,
    /// Error state.
    Error,
}

impl MaterialCrossLanguageCompilerState {
    /// Get the compiler state weight for this state.
    pub fn weight(self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Compiling => 0.5,
            Self::Compiled => 0.25,
            Self::Error => 0.0,
        }
    }
}

// ------------------------------------------------------------------
// Material Asset Format
// ------------------------------------------------------------------

/// Format for a material asset.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let format = MaterialAssetFormat::default();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! asset.format = format;
//! ```
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

// ------------------------------------------------------------------
// Material Asset Cache Eviction Strategy
// ------------------------------------------------------------------

/// Eviction strategy for a material asset cache.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let strategy = MaterialAssetCacheEvictionStrategy::default();
//! let cache = MaterialAssetCache::new();
//! cache.eviction_strategy = strategy;
//! ```
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

// ------------------------------------------------------------------
// Material Shader Compilation Cache
// ------------------------------------------------------------------

/// Cache for material shader compilations.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialShaderCompilationCache::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.cache(&pipeline, asset)?;
//! ```
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
pub struct MaterialShaderCompilationCache {
    /// The cached shader compilations for this cache.
    pub cached_compilations: Vec<crate::CompiledShaderArtifact>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The cache hit count for this cache.
    pub hit_count: usize,
    /// The cache miss count for this cache.
    pub miss_count: usize,
}

impl MaterialShaderCompilationCache {
    /// Create a new material shader compilation cache.
    pub fn new() -> Self {
        Self {
            cached_compilations: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Cache a shader compilation from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_compilations.len() >= self.size_limit {
            self.evict();
        }
        self.cached_compilations.push(asset.shader_source.compile_artifact());
        Ok(self)
    }

    /// Cache multiple shader compilations from this cache.
    pub fn cache_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.cache(pipeline, asset)?;
        }
        Ok(self)
    }

    /// Get a cached shader compilation from this cache by name.
    pub fn get(&self, name: &str) -> Option<&crate::CompiledShaderArtifact> {
        self.cached_compilations.iter().find(|c| c.target == crate::ShaderTarget::Spirv)
    }

    /// Evict shader compilations from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.cached_compilations.remove(0);
            }
            Self::SizeBased => {
                self.cached_compilations.remove(self.cached_compilations.len() - 1);
            }
            Self::Random => {
                let index = self.cached_compilations.len() / 2;
                self.cached_compilations.remove(index);
            }
        }
    }

    /// Get the cached compilations of this cache.
    pub fn cached_compilations(&self) -> &[crate::CompiledShaderArtifact] {
        &self.cached_compilations
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
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

    /// Get the cached compilation count of this cache.
    pub fn cached_count(&self) -> usize {
        self.cached_compilations.len()
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

impl Default for MaterialShaderCompilationCache {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Shader Validation Cache
// ------------------------------------------------------------------

/// Cache for material shader validations.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialShaderValidationCache::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.cache(&pipeline, asset)?;
//! ```
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
pub struct MaterialShaderValidationCache {
    /// The cached shader validations for this cache.
    pub cached_validations: Vec<MaterialShaderValidationResult>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The cache hit count for this cache.
    pub hit_count: usize,
    /// The cache miss count for this cache.
    pub miss_count: usize,
}

impl MaterialShaderValidationCache {
    /// Create a new material shader validation cache.
    pub fn new() -> Self {
        Self {
            cached_validations: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Cache a shader validation from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_validations.len() >= self.size_limit {
            self.evict();
        }
        self.cached_validations.push(asset.shader_source.validate_artifact());
        Ok(self)
    }

    /// Cache multiple shader validations from this cache.
    pub fn cache_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.cache(pipeline, asset)?;
        }
        Ok(self)
    }

    /// Get a cached shader validation from this cache by name.
    pub fn get(&self, name: &str) -> Option<&MaterialShaderValidationResult> {
        self.cached_validations.iter().find(|v| v.is_valid)
    }

    /// Evict shader validations from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.cached_validations.remove(0);
            }
            Self::SizeBased => {
                self.cached_validations.remove(self.cached_validations.len() - 1);
            }
            Self::Random => {
                let index = self.cached_validations.len() / 2;
                self.cached_validations.remove(index);
            }
        }
    }

    /// Get the cached validations of this cache.
    pub fn cached_validations(&self) -> &[MaterialShaderValidationResult] {
        &self.cached_validations
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
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

    /// Get the cached validation count of this cache.
    pub fn cached_count(&self) -> usize {
        self.cached_validations.len()
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

impl Default for MaterialShaderValidationCache {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Cross-Language Cache
// ------------------------------------------------------------------

/// Cache for material cross-language compilations.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialCrossLanguageCache::new();
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.cache(&pipeline, asset)?;
//! ```
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
pub struct MaterialCrossLanguageCache {
    /// The cached cross-language compilations for this cache.
    pub cached_compilations: Vec<crate::CompiledShaderArtifact>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialAssetCacheEvictionStrategy,
    /// The cache hit count for this cache.
    pub hit_count: usize,
    /// The cache miss count for this cache.
    pub miss_count: usize,
}

impl MaterialCrossLanguageCache {
    /// Create a new material cross-language cache.
    pub fn new() -> Self {
        Self {
            cached_compilations: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialAssetCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Cache a cross-language compilation from this cache.
    pub fn cache(&mut self, pipeline: &MaterialAssetPipeline, asset: MaterialAsset) -> Result<Self> {
        if self.cached_compilations.len() >= self.size_limit {
            self.evict();
        }
        self.cached_compilations.push(asset.shader_source.compile_artifact());
        Ok(self)
    }

    /// Cache multiple cross-language compilations from this cache.
    pub fn cache_many(&mut self, pipeline: &MaterialAssetPipeline, assets: Vec<MaterialAsset>) -> Result<Self> {
        for asset in assets {
            self.cache(pipeline, asset)?;
        }
        Ok(self)
    }

    /// Get a cached cross-language compilation from this cache by name.
    pub fn get(&self, name: &str) -> Option<&crate::CompiledShaderArtifact> {
        self.cached_compilations.iter().find(|c| c.target == crate::ShaderTarget::Spirv)
    }

    /// Evict cross-language compilations from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.cached_compilations.remove(0);
            }
            Self::SizeBased => {
                self.cached_compilations.remove(self.cached_compilations.len() - 1);
            }
            Self::Random => {
                let index = self.cached_compilations.len() / 2;
                self.cached_compilations.remove(index);
            }
        }
    }

    /// Get the cached compilations of this cache.
    pub fn cached_compilations(&self) -> &[crate::CompiledShaderArtifact] {
        &self.cached_compilations
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialAssetCacheEvictionStrategy {
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

    /// Get the cached compilation count of this cache.
    pub fn cached_count(&self) -> usize {
        self.cached_compilations.len()
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

impl Default for MaterialCrossLanguageCache {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Preset Asset Loading
// ------------------------------------------------------------------

/// Material presets that can be loaded from assets.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let preset = MaterialPreset::Pbr;
//! let pipeline = MaterialAssetPipeline::new();
//! let asset = preset.load_asset();
//! pipeline.load(&engine, asset)?;
//! ```
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

// ------------------------------------------------------------------
// Raytracing Capabilities
// ------------------------------------------------------------------

/// Raytracing capabilities for a material.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let caps = RaytracingCapabilities::None;
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! asset.raytracing_caps = caps;
//! ```
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

// ------------------------------------------------------------------
// Path Tracing Capabilities
// ------------------------------------------------------------------

/// Path tracing capabilities for a material.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let caps = PathTracingCapabilities::None;
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! asset.path_tracing_caps = caps;
//! ```
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
// Material Composition
// ------------------------------------------------------------------

/// Material composition for layering, blending, and mixing materials.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let composition = MaterialComposition::new(base_material);
//! let overlay = MaterialAsset::new("overlay_material", "overlay_material.slang");
//! composition.add_overlay(overlay)?;
//! ```
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

// ------------------------------------------------------------------
// Material Blend Mode
// ------------------------------------------------------------------

/// Material blend mode for composition.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let mode = MaterialBlendMode::Additive;
//! let composition = MaterialComposition::new(base_material);
//! composition.with_blend_mode(mode);
//! ```
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

// ------------------------------------------------------------------
// Material Layering Order
// ------------------------------------------------------------------

/// Material layering order for composition.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let order = MaterialLayeringOrder::BaseFirst;
//! let composition = MaterialComposition::new(base_material);
//! composition.with_layering_order(order);
//! ```
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
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let cache = MaterialCache::new(100);
//! let asset = MaterialAsset::new("pbr_material", "pbr_material.slang");
//! cache.add(asset)?;
//! ```
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
    pub fn add(mut self, material: Material) -> Result<Self> {
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

// ------------------------------------------------------------------
// Material Cache Eviction Strategy
// ------------------------------------------------------------------

/// Material cache eviction strategy.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let strategy = MaterialCacheEvictionStrategy::LRU;
//! let cache = MaterialCache::new(100);
//! cache.eviction_strategy = strategy;
//! ```
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
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let tools = MaterialDebugTools::new(material);
//! let inspection = tools.shader_inspection();
//! ```
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

// ------------------------------------------------------------------
// Material Shader Inspection
// ------------------------------------------------------------------

/// Material shader inspection for debugging.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let inspection = MaterialShaderInspection::new(material);
//! let source = inspection.shader_source();
//! ```
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

// ------------------------------------------------------------------
// Material Parameter Visualization
// ------------------------------------------------------------------

/// Material parameter visualization for debugging.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let visualization = MaterialParameterVisualization::new(material);
//! let values = visualization.parameter_values();
//! ```
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

// ------------------------------------------------------------------
// Material Performance Profiling
// ------------------------------------------------------------------

/// Material performance profiling for debugging.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let profiling = MaterialPerformanceProfiling::new(material);
//! let compile_time = profiling.shader_compile_time();
//! ```
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

// ------------------------------------------------------------------
// Material Debugging Output
// ------------------------------------------------------------------

/// Material debugging output for debugging.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let output = MaterialDebuggingOutput::new(material);
//! let messages = output.debugging_messages();
//! ```
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
// Material Serialization
// ------------------------------------------------------------------

/// Material serialization for save/load material states.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let serialization = MaterialSerialization::new(material);
//! serialization.serialize()?;
//! ```
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

// ------------------------------------------------------------------
// Material Serialization Format
// ------------------------------------------------------------------

/// Material serialization format for save/load material states.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let format = MaterialSerializationFormat::JSON;
//! let serialization = MaterialSerialization::new(material);
//! serialization.serialization_format = format;
//! ```
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
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let optimization = MaterialShaderOptimization::new(material);
//! optimization.optimize()?;
//! ```
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

// ------------------------------------------------------------------
// Material Shader Optimization Strategy
// ------------------------------------------------------------------

/// Material shader optimization strategy for pre-compiled shader artifacts.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let strategy = MaterialShaderOptimizationStrategy::Compilation;
//! let optimization = MaterialShaderOptimization::new(material);
//! optimization.optimization_strategy = strategy;
//! ```
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
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let support = OfflineRenderingSupport::new(material);
//! let graph = support.offline_render_graph();
//! ```
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

// ------------------------------------------------------------------
// Offline Render Graph
// ------------------------------------------------------------------

/// Offline render graph for offline rendering systems.
///
/// Material assets are rendering-mode-agnostic: they work across rasterized,
//! hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
//! let graph = OfflineRenderGraph::new();
//! let support = OfflineRenderingSupport::new(material);
//! support.offline_render_graph = graph;
//! ```
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
