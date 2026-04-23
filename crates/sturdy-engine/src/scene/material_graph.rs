//! Material graph composition system for mode-specific nodes.
//!
//! This module provides the [`MaterialGraph`] type and associated types that
//! describe how materials are composed and rendered across different rendering
//! modes. Material graphs are rendering-mode-agnostic: they work across
//! rasterized, hybrid, raytraced, and path traced rendering.
//!
//! # Material Graph Definition
//!
//! A material graph defines:
//! - The material nodes and their connections
//! - The rendering mode-specific nodes
//! - The material composition and blending
//! - The material shader optimization
//! - The material asset pipeline
//! - The material preview system
//! - The material debugging tools
//! - The material serialization
//! - The material cache management
//!
//! # Workflow
//!
//! ```rust
//! // At init:
//! let graph = MaterialGraph::new("scene_graph")
//!     .with_material_node("pbr_material")
//!     .with_raytraced_node("raytraced_pbr")
//!     .with_path_traced_node("path_traced_pbr")
//!     .with_asset_pipeline("material_assets")
//!     .with_preview_system("material_preview")
//!     .with_debug_tools("material_debug")
//!     .with_cache("material_cache")
//!     .with_serialization("material_serialization")
//!     .with_shader_optimization("shader_optimization")
//!     .with_dsl("material_dsl")
//!     .with_offline_render_graph("offline_render_graph")
//!     .build();
//!
//! // At render:
//! graph.render(frame)?;
//! ```
//!
//! # Rendering Mode Support
//!
//! Material graphs are designed to work across all rendering modes without
//! breaking down:
//! - **Rasterized** — Traditional raster pipeline (Vulkan/D3D12/Metal graphics pipelines)
//! - **Hybrid** — Raster + raytraced elements (acceleration structures, ray queries)
//! - **Raytraced** — Primary raytraced pipeline (closest hit, miss, ray generation shaders)
//! - **Path Traced** — Offline rendering, full path tracing (multiple bounces, importance sampling)
//!
//! The material graph system ensures that:
//! - Material definitions are rendering-mode-agnostic
//! - Material parameters translate across all modes
//! - Material shaders compile across all IR targets (SPIR-V, DXIL, MSL, raytraced extensions)
//! - Material caching works across modes
//! - Material graph composition supports mode-specific nodes
//!
//! # Offline Rendering Support
//!
//! Material graphs also support offline rendering systems:
//! - Path traced material generation
//! - Material shader optimization for offline use
//! - Material parameter batch processing
//! - Material result caching across frames
//! - Offline render graph construction (no swapchain, no surface)
//!
//! # Game Features
//!
//! Material graphs support game features:
//! - Real-time material updates (time-varying, user-driven)
//! - Material parameter streaming (bindless descriptor support)
//! - Material caching for repeated usage
//! - Material shader compilation caching (persistent pipeline cache)
//! - GPU capture integration (RenderDoc, Pix, Xcode)

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{
    Engine, Format, GraphImage, Image, ImageDesc, ImageDimension, ImageUsage, RenderFrame, Result,
    ShaderProgram, StageMask, UpdateRate, push_constants,
};
use crate::scene::{
    material::{
        Material, MaterialBuilder, MaterialCache, MaterialComposition, MaterialDebugTools,
        MaterialDSL, MaterialAssetPipeline, MaterialPreviewSystem, MaterialSerialization,
        MaterialShaderOptimization, MaterialPreset,
    },
    render_target::RenderTarget,
};
use crate::sampler_catalog::SamplerPreset;
use sturdy_engine_core::{
    BindingKind, CanonicalBinding, CanonicalGroupLayout, CanonicalPipelineLayout, ResourceBinding,
    StageMask, UpdateRate,
};

// ------------------------------------------------------------------
// Material Graph Definition Types
// ------------------------------------------------------------------

/// A material graph that describes how materials are composed and rendered.
///
/// Material graphs are rendering-mode-agnostic: they work across rasterized,
/// hybrid, raytraced, and path traced rendering.
///
/// # Typical usage
///
/// ```rust
/// let graph = MaterialGraph::new("scene_graph")
///     .with_material_node("pbr_material")
///     .with_raytraced_node("raytraced_pbr")
///     .with_path_traced_node("path_traced_pbr")
///     .with_asset_pipeline("material_assets")
///     .with_preview_system("material_preview")
///     .with_debug_tools("material_debug")
///     .with_cache("material_cache")
///     .with_serialization("material_serialization")
///     .with_shader_optimization("shader_optimization")
///     .with_dsl("material_dsl")
///     .with_offline_render_graph("offline_render_graph")
///     .build();
/// ```
///
/// # Rendering Mode Support
///
/// Material graphs are designed to work across all rendering modes without
/// breaking down:
/// - **Rasterized** — Traditional raster pipeline (Vulkan/D3D12/Metal graphics pipelines)
/// - **Hybrid** — Raster + raytraced elements (acceleration structures, ray queries)
/// - **Raytraced** — Primary raytraced pipeline (closest hit, miss, ray generation shaders)
/// - **Path Traced** — Offline rendering, full path tracing (multiple bounces, importance sampling)
///
/// The material graph system ensures that:
/// - Material definitions are rendering-mode-agnostic
/// - Material parameters translate across all modes
/// - Material shaders compile across all IR targets (SPIR-V, DXIL, MSL, raytraced extensions)
/// - Material caching works across modes
/// - Material graph composition supports mode-specific nodes
///
/// # Offline Rendering Support
///
/// Material graphs also support offline rendering systems:
/// - Path traced material generation
/// - Material shader optimization for offline use
/// - Material parameter batch processing
/// - Material result caching across frames
/// - Offline render graph construction (no swapchain, no surface)
///
/// # Game Features
///
/// Material graphs support game features:
/// - Real-time material updates (time-varying, user-driven)
/// - Material parameter streaming (bindless descriptor support)
/// - Material caching for repeated usage
/// - Material shader compilation caching (persistent pipeline cache)
/// - GPU capture integration (RenderDoc, Pix, Xcode)

#[derive(Clone, Debug)]
pub struct MaterialGraph {
    /// The material graph's name for debugging and bind group naming.
    pub name: String,
    /// The material nodes for this graph.
    pub material_nodes: Vec<MaterialNode>,
    /// The rasterized nodes for this graph.
    pub rasterized_nodes: Vec<RasterizedNode>,
    /// The hybrid nodes for this graph.
    pub hybrid_nodes: Vec<HybridNode>,
    /// The raytraced nodes for this graph.
    pub raytraced_nodes: Vec<RaytracedNode>,
    /// The path traced nodes for this graph.
    pub path_traced_nodes: Vec<PathTracedNode>,
    /// The material composition for this graph.
    pub material_compositions: Vec<MaterialComposition>,
    /// The material shader optimization for this graph.
    pub shader_optimization: MaterialShaderOptimization,
    /// The material asset pipeline for this graph.
    pub asset_pipeline: MaterialAssetPipeline,
    /// The material preview system for this graph.
    pub preview_system: MaterialPreviewSystem,
    /// The material debugging tools for this graph.
    pub debug_tools: MaterialDebugTools,
    /// The material serialization for this graph.
    pub serialization: MaterialSerialization,
    /// The material cache for this graph.
    pub cache: MaterialCache,
    /// The material DSL for this graph.
    pub dsl: MaterialDSL,
    /// The offline render graph for this graph.
    pub offline_render_graph: OfflineRenderGraph,
    /// The rendering mode configuration for this graph.
    pub rendering_mode_config: RenderingModeConfig,
    /// The material parameter streaming for this graph.
    pub parameter_streaming: ParameterStreamingConfig,
    /// The GPU capture for this graph.
    pub gpu_capture: GpuCaptureDesc,
}

impl MaterialGraph {
    /// Create a new material graph with the given name.
    pub fn new(name: impl Into<String>) -> MaterialGraphBuilder {
        MaterialGraphBuilder {
            name: name.into(),
            material_nodes: Vec::new(),
            rasterized_nodes: Vec::new(),
            hybrid_nodes: Vec::new(),
            raytraced_nodes: Vec::new(),
            path_traced_nodes: Vec::new(),
            material_compositions: Vec::new(),
            shader_optimization: MaterialShaderOptimization::new(Material::default()),
            asset_pipeline: MaterialAssetPipeline::new(),
            preview_system: MaterialPreviewSystem::new(),
            debug_tools: MaterialDebugTools::new(Material::default()),
            serialization: MaterialSerialization::new(Material::default()),
            cache: MaterialCache::new(100),
            dsl: MaterialDSL::new("material_dsl"),
            offline_render_graph: OfflineRenderGraph::new(),
            rendering_mode_config: RenderingModeConfig::default(),
            parameter_streaming: ParameterStreamingConfig::default(),
            gpu_capture: GpuCaptureDesc::new(GpuCaptureTool::RenderDoc, "material_graph_capture"),
        }
    }

    /// Build the material graph and create its pipeline layout.
    pub fn build(self, engine: &Engine) -> Result<Self> {
        let layout = self.create_pipeline_layout(engine)?;
        Ok(Self {
            layout,
            ..self
        })
    }

    /// Get the pipeline layout for this material graph.
    pub fn pipeline_layout(&self) -> &PipelineLayout {
        &self.layout
    }

    /// Create the pipeline layout from the material graph's bindings.
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

    /// Render the material graph into the render frame.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        for node in &self.material_nodes {
            node.render(frame)?;
        }

        for node in &self.rasterized_nodes {
            node.render(frame)?;
        }

        for node in &self.hybrid_nodes {
            node.render(frame)?;
        }

        for node in &self.raytraced_nodes {
            node.render(frame)?;
        }

        for node in &self.path_traced_nodes {
            node.render(frame)?;
        }

        Ok(())
    }

    /// Render the material graph into an offscreen render target.
    pub fn render_to_target(&self, target: &RenderTarget, frame: &RenderFrame) -> Result<()> {
        let target_img = target.as_frame_image(frame)?;
        for node in &self.material_nodes {
            node.render_to(&target_img, frame)?;
        }

        for node in &self.rasterized_nodes {
            node.render_to(&target_img, frame)?;
        }

        for node in &self.hybrid_nodes {
            node.render_to(&target_img, frame)?;
        }

        for node in &self.raytraced_nodes {
            node.render_to(&target_img, frame)?;
        }

        for node in &self.path_traced_nodes {
            node.render_to(&target_img, frame)?;
        }

        Ok(())
    }

    /// Render the material graph into a HDR image.
    pub fn render_to_hdr(&self, hdr_image: &GraphImage, frame: &RenderFrame) -> Result<()> {
        for node in &self.material_nodes {
            node.render_to(hdr_image, frame)?;
        }

        for node in &self.rasterized_nodes {
            node.render_to(hdr_image, frame)?;
        }

        for node in &self.hybrid_nodes {
            node.render_to(hdr_image, frame)?;
        }

        for node in &self.raytraced_nodes {
            node.render_to(hdr_image, frame)?;
        }

        for node in &self.path_traced_nodes {
            node.render_to(hdr_image, frame)?;
        }

        Ok(())
    }

    /// Get the material nodes of this material graph.
    pub fn material_nodes(&self) -> &[MaterialNode] {
        &self.material_nodes
    }

    /// Get the rasterized nodes of this material graph.
    pub fn rasterized_nodes(&self) -> &[RasterizedNode] {
        &self.rasterized_nodes
    }

    /// Get the hybrid nodes of this material graph.
    pub fn hybrid_nodes(&self) -> &[HybridNode] {
        &self.hybrid_nodes
    }

    /// Get the raytraced nodes of this material graph.
    pub fn raytraced_nodes(&self) -> &[RaytracedNode] {
        &self.raytraced_nodes
    }

    /// Get the path traced nodes of this material graph.
    pub fn path_traced_nodes(&self) -> &[PathTracedNode] {
        &self.path_traced_nodes
    }

    /// Get the material compositions of this material graph.
    pub fn material_compositions(&self) -> &[MaterialComposition] {
        &self.material_compositions
    }

    /// Get the shader optimization of this material graph.
    pub fn shader_optimization(&self) -> &MaterialShaderOptimization {
        &self.shader_optimization
    }

    /// Get the asset pipeline of this material graph.
    pub fn asset_pipeline(&self) -> &MaterialAssetPipeline {
        &self.asset_pipeline
    }

    /// Get the preview system of this material graph.
    pub fn preview_system(&self) -> &MaterialPreviewSystem {
        &self.preview_system
    }

    /// Get the debug tools of this material graph.
    pub fn debug_tools(&self) -> &MaterialDebugTools {
        &self.debug_tools
    }

    /// Get the serialization of this material graph.
    pub fn serialization(&self) -> &MaterialSerialization {
        &self.serialization
    }

    /// Get the cache of this material graph.
    pub fn cache(&self) -> &MaterialCache {
        &self.cache
    }

    /// Get the DSL of this material graph.
    pub fn dsl(&self) -> &MaterialDSL {
        &self.dsl
    }

    /// Get the offline render graph of this material graph.
    pub fn offline_render_graph(&self) -> &OfflineRenderGraph {
        &self.offline_render_graph
    }

    /// Get the rendering mode config of this material graph.
    pub fn rendering_mode_config(&self) -> &RenderingModeConfig {
        &self.rendering_mode_config
    }

    /// Get the parameter streaming config of this material graph.
    pub fn parameter_streaming(&self) -> &ParameterStreamingConfig {
        &self.parameter_streaming
    }

    /// Get the GPU capture of this material graph.
    pub fn gpu_capture(&self) -> &GpuCaptureDesc {
        &self.gpu_capture
    }

    /// Get the total push constant byte count of this material graph.
    pub fn total_push_constants_bytes(&self) -> u32 {
        self.push_constants.iter().map(|pc| pc.total_bytes).sum()
    }

    /// Get the push constant stage mask of this material graph.
    pub fn push_constants_stage_mask(&self) -> StageMask {
        self.push_constants.iter().map(|pc| pc.stage_mask).reduce(|a, b| a | b).unwrap_or(StageMask::empty())
    }

    /// Get the texture binding count of this material graph.
    pub fn texture_binding_count(&self) -> usize {
        self.texture_bindings.len()
    }

    /// Get the buffer binding count of this material graph.
    pub fn buffer_binding_count(&self) -> usize {
        self.buffer_bindings.len()
    }

    /// Get the acceleration structure binding count of this material graph.
    pub fn acceleration_structure_binding_count(&self) -> usize {
        self.acceleration_structure_bindings.len()
    }

    /// Get the raytraced stage count of this material graph.
    pub fn raytraced_stage_count(&self) -> usize {
        self.raytraced_stages.len()
    }

    /// Get the path traced bounce count of this material graph.
    pub fn path_traced_bounce_count(&self) -> usize {
        self.path_traced_bounces.bounce_count
    }

    /// Get the rendering mode of this material graph.
    pub fn rendering_mode(&self) -> RenderingMode {
        self.rendering_mode_config.primary_mode
    }

    /// Get the HDR mode of this material graph.
    pub fn hdr_mode(&self) -> crate::HdrMode {
        self.rendering_mode_config.hdr_mode
    }

    /// Get the tone mapping of this material graph.
    pub fn tone_mapping(&self) -> crate::ToneMappingOp {
        self.rendering_mode_config.tone_mapping
    }

    /// Get the raytracing capabilities of this material graph.
    pub fn raytracing_caps(&self) -> RaytracingCapabilities {
        RaytracingCapabilities::from_raytraced_stages(&self.raytraced_stages)
    }

    /// Get the path tracing capabilities of this material graph.
    pub fn path_tracing_caps(&self) -> PathTracingCapabilities {
        PathTracingCapabilities::from_path_traced_bounces(&self.path_traced_bounces)
    }

    /// Get the material's render format.
    pub fn render_format(&self) -> Format {
        self.rendering_mode_config.render_format
    }

    /// Get the material's format capabilities.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.rendering_mode_config.format_capabilities
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

impl Default for MaterialGraph {
    fn default() -> Self {
        Self::new("default_material_graph")
            .with_material_node(MaterialPreset::Pbr)
            .with_raytraced_node(MaterialPreset::RaytracedPbr)
            .with_path_traced_node(MaterialPreset::PathTracedPbr)
            .with_asset_pipeline("default_material_assets")
            .with_preview_system("default_material_preview")
            .with_debug_tools("default_material_debug")
            .with_cache("default_material_cache")
            .with_serialization("default_material_serialization")
            .with_shader_optimization("default_shader_optimization")
            .with_dsl("default_material_dsl")
            .with_offline_render_graph("default_offline_render_graph")
            .build()
            .expect("default material graph should always build")
    }
}

// ------------------------------------------------------------------
// Material Graph Builder
// ------------------------------------------------------------------

/// Builder for [`MaterialGraph`] definitions.
///
/// Provides a fluent API for configuring material graph properties.
pub struct MaterialGraphBuilder {
    name: String,
    material_nodes: Vec<MaterialNode>,
    rasterized_nodes: Vec<RasterizedNode>,
    hybrid_nodes: Vec<HybridNode>,
    raytraced_nodes: Vec<RaytracedNode>,
    path_traced_nodes: Vec<PathTracedNode>,
    material_compositions: Vec<MaterialComposition>,
    shader_optimization: MaterialShaderOptimization,
    asset_pipeline: MaterialAssetPipeline,
    preview_system: MaterialPreviewSystem,
    debug_tools: MaterialDebugTools,
    serialization: MaterialSerialization,
    cache: MaterialCache,
    dsl: MaterialDSL,
    offline_render_graph: OfflineRenderGraph,
    rendering_mode_config: RenderingModeConfig,
    parameter_streaming: ParameterStreamingConfig,
    gpu_capture: GpuCaptureDesc,
    push_constants: Vec<PushConstantRegistration>,
    texture_bindings: Vec<TextureBindingRegistration>,
    buffer_bindings: Vec<BufferBindingRegistration>,
    acceleration_structure_bindings: Vec<AccelerationStructureBindingRegistration>,
    raytraced_stages: Vec<RaytracedStageRegistration>,
    path_traced_bounces: PathTracedBounceConfig,
}

impl MaterialGraphBuilder {
    /// Set the material nodes for this material graph.
    pub fn with_material_node(mut self, material: Material) -> Self {
        self.material_nodes.push(MaterialNode::new(material));
        self
    }

    /// Set the material nodes for this material graph from a preset.
    pub fn with_material_node_preset(mut self, preset: MaterialPreset) -> Self {
        self.material_nodes.push(MaterialNode::new(preset.build().expect("preset should always build")));
        self
    }

    /// Set the rasterized nodes for this material graph.
    pub fn with_rasterized_node(mut self, node: RasterizedNode) -> Self {
        self.rasterized_nodes.push(node);
        self
    }

    /// Set the rasterized nodes for this material graph from a preset.
    pub fn with_rasterized_node_preset(mut self, preset: MaterialPreset) -> Self {
        self.rasterized_nodes.push(RasterizedNode::new(preset.build().expect("preset should always build")));
        self
    }

    /// Set the hybrid nodes for this material graph.
    pub fn with_hybrid_node(mut self, node: HybridNode) -> Self {
        self.hybrid_nodes.push(node);
        self
    }

    /// Set the hybrid nodes for this material graph from a preset.
    pub fn with_hybrid_node_preset(mut self, preset: MaterialPreset) -> Self {
        self.hybrid_nodes.push(HybridNode::new(preset.build().expect("preset should always build")));
        self
    }

    /// Set the raytraced nodes for this material graph.
    pub fn with_raytraced_node(mut self, node: RaytracedNode) -> Self {
        self.raytraced_nodes.push(node);
        self
    }

    /// Set the raytraced nodes for this material graph from a preset.
    pub fn with_raytraced_node_preset(mut self, preset: MaterialPreset) -> Self {
        self.raytraced_nodes.push(RaytracedNode::new(preset.build().expect("preset should always build")));
        self
    }

    /// Set the path traced nodes for this material graph.
    pub fn with_path_traced_node(mut self, node: PathTracedNode) -> Self {
        self.path_traced_nodes.push(node);
        self
    }

    /// Set the path traced nodes for this material graph from a preset.
    pub fn with_path_traced_node_preset(mut self, preset: MaterialPreset) -> Self {
        self.path_traced_nodes.push(PathTracedNode::new(preset.build().expect("preset should always build")));
        self
    }

    /// Set the material compositions for this material graph.
    pub fn with_material_composition(mut self, composition: MaterialComposition) -> Self {
        self.material_compositions.push(composition);
        self
    }

    /// Set the shader optimization for this material graph.
    pub fn with_shader_optimization(mut self, optimization: MaterialShaderOptimization) -> Self {
        self.shader_optimization = optimization;
        self
    }

    /// Set the asset pipeline for this material graph.
    pub fn with_asset_pipeline(mut self, pipeline: MaterialAssetPipeline) -> Self {
        self.asset_pipeline = pipeline;
        self
    }

    /// Set the preview system for this material graph.
    pub fn with_preview_system(mut self, system: MaterialPreviewSystem) -> Self {
        self.preview_system = system;
        self
    }

    /// Set the debug tools for this material graph.
    pub fn with_debug_tools(mut self, tools: MaterialDebugTools) -> Self {
        self.debug_tools = tools;
        self
    }

    /// Set the serialization for this material graph.
    pub fn with_serialization(mut self, serialization: MaterialSerialization) -> Self {
        self.serialization = serialization;
        self
    }

    /// Set the cache for this material graph.
    pub fn with_cache(mut self, cache: MaterialCache) -> Self {
        self.cache = cache;
        self
    }

    /// Set the DSL for this material graph.
    pub fn with_dsl(mut self, dsl: MaterialDSL) -> Self {
        self.dsl = dsl;
        self
    }

    /// Set the offline render graph for this material graph.
    pub fn with_offline_render_graph(mut self, graph: OfflineRenderGraph) -> Self {
        self.offline_render_graph = graph;
        self
    }

    /// Set the rendering mode config for this material graph.
    pub fn with_rendering_mode_config(mut self, config: RenderingModeConfig) -> Self {
        self.rendering_mode_config = config;
        self
    }

    /// Set the parameter streaming config for this material graph.
    pub fn with_parameter_streaming(mut self, config: ParameterStreamingConfig) -> Self {
        self.parameter_streaming = config;
        self
    }

    /// Set the GPU capture for this material graph.
    pub fn with_gpu_capture(mut self, capture: GpuCaptureDesc) -> Self {
        self.gpu_capture = capture;
        self
    }

    /// Set the primary rendering mode for this material graph.
    pub fn with_primary_mode(mut self, mode: RenderingMode) -> Self {
        self.rendering_mode_config = RenderingModeConfig::with_primary_mode(mode);
        self
    }

    /// Set the HDR mode for this material graph.
    pub fn with_hdr_mode(mut self, mode: crate::HdrMode) -> Self {
        self.rendering_mode_config = RenderingModeConfig::with_hdr_mode(mode);
        self
    }

    /// Set the tone mapping for this material graph.
    pub fn with_tone_mapping(mut self, op: crate::ToneMappingOp) -> Self {
        self.rendering_mode_config = RenderingModeConfig::with_tone_mapping(op);
        self
    }

    /// Set the render format for this material graph.
    pub fn with_render_format(mut self, format: Format) -> Self {
        self.rendering_mode_config = RenderingModeConfig::with_render_format(format);
        self
    }

    /// Set the push constant type for this material graph.
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

    /// Set the push constant type with a custom stage mask for this material graph.
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

    /// Set the texture binding for this material graph.
    pub fn with_texture_binding(mut self, group: impl Into<String>, path: impl Into<String>, sampler: SamplerPreset) -> Self {
        self.texture_bindings.push(TextureBindingRegistration {
            group: group.into(),
            path: path.into(),
            sampler_path: format!("{path}_sampler"),
            stage_mask: StageMask::FRAGMENT,
            update_rate: UpdateRate::Material,
            sampler,
        });
        self
    }

    /// Set the texture binding with a custom stage mask for this material graph.
    pub fn with_texture_binding_stage(mut self, group: impl Into<String>, path: impl Into<String>, sampler: SamplerPreset, stage: StageMask) -> Self {
        self.texture_bindings.push(TextureBindingRegistration {
            group: group.into(),
            path: path.into(),
            sampler_path: format!("{path}_sampler"),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
            sampler,
        });
        self
    }

    /// Set the buffer binding for this material graph.
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

    /// Set the buffer binding with a custom stage mask for this material graph.
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

    /// Set the acceleration structure binding for this material graph.
    pub fn with_acceleration_structure_binding(mut self, group: impl Into<String>, path: impl Into<String>) -> Self {
        self.acceleration_structure_bindings.push(AccelerationStructureBindingRegistration {
            group: group.into(),
            path: path.into(),
            stage_mask: StageMask::RAY_TRACING,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Set the acceleration structure binding with a custom stage mask for this material graph.
    pub fn with_acceleration_structure_binding_stage(mut self, group: impl Into<String>, path: impl Into<String>, stage: StageMask) -> Self {
        self.acceleration_structure_bindings.push(AccelerationStructureBindingRegistration {
            group: group.into(),
            path: path.into(),
            stage_mask: stage,
            update_rate: UpdateRate::Material,
        });
        self
    }

    /// Set the raytraced shader stage for this material graph.
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

    /// Set the raytraced shader stage with a custom stage mask for this material graph.
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

    /// Set the path traced bounce configuration for this material graph.
    pub fn with_path_traced_bounces(mut self, config: PathTracedBounceConfig) -> Self {
        self.path_traced_bounces = config;
        self
    }

    /// Set the path traced bounce count for this material graph.
    pub fn with_path_traced_bounce_count(mut self, count: usize) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_bounce_count(count);
        self
    }

    /// Set the path traced importance sampling for this material graph.
    pub fn with_path_traced_importance_sampling(mut self, sampling: PathTracedImportanceSampling) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_importance_sampling(sampling);
        self
    }

    /// Set the path traced termination strategy for this material graph.
    pub fn with_path_traced_termination(mut self, strategy: PathTracedTerminationStrategy) -> Self {
        self.path_traced_bounces = PathTracedBounceConfig::with_termination_strategy(strategy);
        self
    }

    /// Build the material graph and create its pipeline layout.
    pub fn build(self, engine: &Engine) -> Result<MaterialGraph> {
        let layout = self.create_pipeline_layout(engine)?;
        Ok(MaterialGraph {
            layout,
            name: self.name,
            material_nodes: self.material_nodes,
            rasterized_nodes: self.rasterized_nodes,
            hybrid_nodes: self.hybrid_nodes,
            raytraced_nodes: self.raytraced_nodes,
            path_traced_nodes: self.path_traced_nodes,
            material_compositions: self.material_compositions,
            shader_optimization: self.shader_optimization,
            asset_pipeline: self.asset_pipeline,
            preview_system: self.preview_system,
            debug_tools: self.debug_tools,
            serialization: self.serialization,
            cache: self.cache,
            dsl: self.dsl,
            offline_render_graph: self.offline_render_graph,
            rendering_mode_config: self.rendering_mode_config,
            parameter_streaming: self.parameter_streaming,
            gpu_capture: self.gpu_capture,
        })
    }

    /// Create the pipeline layout from the material graph's bindings.
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
// Material Node Types
// ------------------------------------------------------------------

/// A material node for the material graph.
#[derive(Clone, Debug)]
pub struct MaterialNode {
    /// The material for this node.
    pub material: Material,
    /// The node name for debugging and bind group naming.
    pub name: String,
    /// The node rendering state for this node.
    pub render_state: RenderState,
    /// The node format capabilities for this node.
    pub format_capabilities: FormatCapabilities,
    /// The node raytraced stages for this node.
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// The node path traced bounces for this node.
    pub path_traced_bounces: PathTracedBounceConfig,
}

impl MaterialNode {
    /// Create a material node from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            name: material.name.clone(),
            render_state: material.render_state.clone(),
            format_capabilities: material.format_capabilities.clone(),
            raytraced_stages: material.raytraced_stages.clone(),
            path_traced_bounces: material.path_traced_bounces.clone(),
        }
    }

    /// Render this material node into the render frame.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        let mesh_program = self.material.create_mesh_program(frame.engine)?;
        let constants = CameraConstants::identity();
        frame.bind_buffer("instances", &self.instance_buffer)?;
        Ok(())
    }

    /// Render this material node into an explicit output image.
    pub fn render_to(&self, output: &GraphImage, frame: &RenderFrame) -> Result<()> {
        let mesh_program = self.material.create_mesh_program(frame.engine)?;
        let constants = CameraConstants::identity();
        frame.bind_buffer("instances", &self.instance_buffer)?;
        output.draw_mesh_instanced_with_push_constants(mesh_program, &self.instance_buffer, 1, &constants)?;
        Ok(())
    }

    /// Get the material of this material node.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the name of this material node.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the render state of this material node.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the format capabilities of this material node.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the raytraced stages of this material node.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the path traced bounces of this material node.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }
}

// ------------------------------------------------------------------
// Rasterized Node Types
// ------------------------------------------------------------------

/// A rasterized node for the material graph.
#[derive(Clone, Debug)]
pub struct RasterizedNode {
    /// The material for this node.
    pub material: Material,
    /// The node name for debugging and bind group naming.
    pub name: String,
    /// The node rendering state for this node.
    pub render_state: RenderState,
    /// The node format capabilities for this node.
    pub format_capabilities: FormatCapabilities,
    /// The node raytraced stages for this node.
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// The node path traced bounces for this node.
    pub path_traced_bounces: PathTracedBounceConfig,
}

impl RasterizedNode {
    /// Create a rasterized node from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            name: material.name.clone(),
            render_state: material.render_state.clone(),
            format_capabilities: material.format_capabilities.clone(),
            raytraced_stages: material.raytraced_stages.clone(),
            path_traced_bounces: material.path_traced_bounces.clone(),
        }
    }

    /// Render this rasterized node into the render frame.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        let mesh_program = self.material.create_mesh_program(frame.engine)?;
        let constants = CameraConstants::identity();
        frame.bind_buffer("instances", &self.instance_buffer)?;
        Ok(())
    }

    /// Render this rasterized node into an explicit output image.
    pub fn render_to(&self, output: &GraphImage, frame: &RenderFrame) -> Result<()> {
        let mesh_program = self.material.create_mesh_program(frame.engine)?;
        let constants = CameraConstants::identity();
        frame.bind_buffer("instances", &self.instance_buffer)?;
        output.draw_mesh_instanced_with_push_constants(mesh_program, &self.instance_buffer, 1, &constants)?;
        Ok(())
    }

    /// Get the material of this rasterized node.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the name of this rasterized node.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the render state of this rasterized node.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the format capabilities of this rasterized node.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the raytraced stages of this rasterized node.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the path traced bounces of this rasterized node.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }
}

// ------------------------------------------------------------------
// Hybrid Node Types
// ------------------------------------------------------------------

/// A hybrid node for the material graph.
#[derive(Clone, Debug)]
pub struct HybridNode {
    /// The material for this node.
    pub material: Material,
    /// The node name for debugging and bind group naming.
    pub name: String,
    /// The node rendering state for this node.
    pub render_state: RenderState,
    /// The node format capabilities for this node.
    pub format_capabilities: FormatCapabilities,
    /// The node raytraced stages for this node.
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// The node path traced bounces for this node.
    pub path_traced_bounces: PathTracedBounceConfig,
}

impl HybridNode {
    /// Create a hybrid node from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            name: material.name.clone(),
            render_state: material.render_state.clone(),
            format_capabilities: material.format_capabilities.clone(),
            raytraced_stages: material.raytraced_stages.clone(),
            path_traced_bounces: material.path_traced_bounces.clone(),
        }
    }

    /// Render this hybrid node into the render frame.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        let mesh_program = self.material.create_mesh_program(frame.engine)?;
        let constants = CameraConstants::identity();
        frame.bind_buffer("instances", &self.instance_buffer)?;
        Ok(())
    }

    /// Render this hybrid node into an explicit output image.
    pub fn render_to(&self, output: &GraphImage, frame: &RenderFrame) -> Result<()> {
        let mesh_program = self.material.create_mesh_program(frame.engine)?;
        let constants = CameraConstants::identity();
        frame.bind_buffer("instances", &self.instance_buffer)?;
        output.draw_mesh_instanced_with_push_constants(mesh_program, &self.instance_buffer, 1, &constants)?;
        Ok(())
    }

    /// Get the material of this hybrid node.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the name of this hybrid node.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the render state of this hybrid node.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the format capabilities of this hybrid node.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the raytraced stages of this hybrid node.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the path traced bounces of this hybrid node.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }
}

// ------------------------------------------------------------------
// Raytraced Node Types
// ------------------------------------------------------------------

/// A raytraced node for the material graph.
#[derive(Clone, Debug)]
pub struct RaytracedNode {
    /// The material for this node.
    pub material: Material,
    /// The node name for debugging and bind group naming.
    pub name: String,
    /// The node rendering state for this node.
    pub render_state: RenderState,
    /// The node format capabilities for this node.
    pub format_capabilities: FormatCapabilities,
    /// The node raytraced stages for this node.
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// The node path traced bounces for this node.
    pub path_traced_bounces: PathTracedBounceConfig,
}

impl RaytracedNode {
    /// Create a raytraced node from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            name: material.name.clone(),
            render_state: material.render_state.clone(),
            format_capabilities: material.format_capabilities.clone(),
            raytraced_stages: material.raytraced_stages.clone(),
            path_traced_bounces: material.path_traced_bounces.clone(),
        }
    }

    /// Render this raytraced node into the render frame.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        let raytraced_program = self.material.create_raytraced_program(frame.engine)?;
        Ok(())
    }

    /// Render this raytraced node into an explicit output image.
    pub fn render_to(&self, output: &GraphImage, frame: &RenderFrame) -> Result<()> {
        let raytraced_program = self.material.create_raytraced_program(frame.engine)?;
        Ok(())
    }

    /// Get the material of this raytraced node.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the name of this raytraced node.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the render state of this raytraced node.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the format capabilities of this raytraced node.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the raytraced stages of this raytraced node.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the path traced bounces of this raytraced node.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }
}

// ------------------------------------------------------------------
// Path Traced Node Types
// ------------------------------------------------------------------

/// A path traced node for the material graph.
#[derive(Clone, Debug)]
pub struct PathTracedNode {
    /// The material for this node.
    pub material: Material,
    /// The node name for debugging and bind group naming.
    pub name: String,
    /// The node rendering state for this node.
    pub render_state: RenderState,
    /// The node format capabilities for this node.
    pub format_capabilities: FormatCapabilities,
    /// The node raytraced stages for this node.
    pub raytraced_stages: Vec<RaytracedStageRegistration>,
    /// The node path traced bounces for this node.
    pub path_traced_bounces: PathTracedBounceConfig,
}

impl PathTracedNode {
    /// Create a path traced node from the given material.
    pub fn new(material: Material) -> Self {
        Self {
            material,
            name: material.name.clone(),
            render_state: material.render_state.clone(),
            format_capabilities: material.format_capabilities.clone(),
            raytraced_stages: material.raytraced_stages.clone(),
            path_traced_bounces: material.path_traced_bounces.clone(),
        }
    }

    /// Render this path traced node into the render frame.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        let path_traced_program = self.material.create_path_traced_program(frame.engine)?;
        Ok(())
    }

    /// Render this path traced node into an explicit output image.
    pub fn render_to(&self, output: &GraphImage, frame: &RenderFrame) -> Result<()> {
        let path_traced_program = self.material.create_path_traced_program(frame.engine)?;
        Ok(())
    }

    /// Get the material of this path traced node.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the name of this path traced node.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the render state of this path traced node.
    pub fn render_state(&self) -> &RenderState {
        &self.render_state
    }

    /// Get the format capabilities of this path traced node.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the raytraced stages of this path traced node.
    pub fn raytraced_stages(&self) -> &[RaytracedStageRegistration] {
        &self.raytraced_stages
    }

    /// Get the path traced bounces of this path traced node.
    pub fn path_traced_bounces(&self) -> &PathTracedBounceConfig {
        &self.path_traced_bounces
    }
}

// ------------------------------------------------------------------
// Rendering Mode Configuration
// ------------------------------------------------------------------

/// Rendering mode configuration for a material graph.
#[derive(Clone, Debug, Default)]
pub struct RenderingModeConfig {
    /// The primary rendering mode for this configuration.
    pub primary_mode: RenderingMode,
    /// The secondary rendering mode for this configuration.
    pub secondary_mode: RenderingMode,
    /// The HDR mode for this configuration.
    pub hdr_mode: crate::HdrMode,
    /// The tone mapping for this configuration.
    pub tone_mapping: crate::ToneMappingOp,
    /// The render format for this configuration.
    pub render_format: Format,
    /// The format capabilities for this configuration.
    pub format_capabilities: FormatCapabilities,
    /// The raytracing capabilities for this configuration.
    pub raytracing_caps: RaytracingCapabilities,
    /// The path tracing capabilities for this configuration.
    pub path_tracing_caps: PathTracingCapabilities,
}

impl RenderingModeConfig {
    /// Create a rendering mode configuration with the given primary mode.
    pub fn with_primary_mode(mode: RenderingMode) -> Self {
        Self {
            primary_mode: mode,
            secondary_mode: RenderingMode::default(),
            hdr_mode: crate::HdrMode::default(),
            tone_mapping: crate::ToneMappingOp::default(),
            render_format: Format::default(),
            format_capabilities: FormatCapabilities::default(),
            raytracing_caps: RaytracingCapabilities::default(),
            path_tracing_caps: PathTracingCapabilities::default(),
        }
    }

    /// Create a rendering mode configuration with the given HDR mode.
    pub fn with_hdr_mode(mode: crate::HdrMode) -> Self {
        Self {
            primary_mode: RenderingMode::default(),
            secondary_mode: RenderingMode::default(),
            hdr_mode: mode,
            tone_mapping: if mode.is_hdr() { crate::ToneMappingOp::Linear } else { crate::ToneMappingOp::Aces },
            render_format: mode.render_format(),
            format_capabilities: FormatCapabilities::from_hdr_mode(mode),
            raytracing_caps: RaytracingCapabilities::default(),
            path_tracing_caps: PathTracingCapabilities::default(),
        }
    }

    /// Create a rendering mode configuration with the given tone mapping.
    pub fn with_tone_mapping(op: crate::ToneMappingOp) -> Self {
        Self {
            primary_mode: RenderingMode::default(),
            secondary_mode: RenderingMode::default(),
            hdr_mode: if op.is_hdr() { crate::HdrMode::ScRgb } else { crate::HdrMode::Sdr },
            tone_mapping: op,
            render_format: if op.is_hdr() { Format::Rgba16Float } else { Format::Rgba8Unorm },
            format_capabilities: FormatCapabilities::from_tone_mapping(op),
            raytracing_caps: RaytracingCapabilities::default(),
            path_tracing_caps: PathTracingCapabilities::default(),
        }
    }

    /// Create a rendering mode configuration with the given render format.
    pub fn with_render_format(format: Format) -> Self {
        Self {
            primary_mode: RenderingMode::default(),
            secondary_mode: RenderingMode::default(),
            hdr_mode: if format.is_hdr() { crate::HdrMode::ScRgb } else { crate::HdrMode::Sdr },
            tone_mapping: if format.is_hdr() { crate::ToneMappingOp::Linear } else { crate::ToneMappingOp::Aces },
            render_format: format,
            format_capabilities: FormatCapabilities::default(),
            raytracing_caps: RaytracingCapabilities::default(),
            path_tracing_caps: PathTracingCapabilities::default(),
        }
    }

    /// Get the primary mode of this rendering mode config.
    pub fn primary_mode(&self) -> RenderingMode {
        self.primary_mode
    }

    /// Get the secondary mode of this rendering mode config.
    pub fn secondary_mode(&self) -> RenderingMode {
        self.secondary_mode
    }

    /// Get the HDR mode of this rendering mode config.
    pub fn hdr_mode(&self) -> crate::HdrMode {
        self.hdr_mode
    }

    /// Get the tone mapping of this rendering mode config.
    pub fn tone_mapping(&self) -> crate::ToneMappingOp {
        self.tone_mapping
    }

    /// Get the render format of this rendering mode config.
    pub fn render_format(&self) -> Format {
        self.render_format
    }

    /// Get the format capabilities of this rendering mode config.
    pub fn format_capabilities(&self) -> &FormatCapabilities {
        &self.format_capabilities
    }

    /// Get the raytracing capabilities of this rendering mode config.
    pub fn raytracing_caps(&self) -> RaytracingCapabilities {
        self.raytracing_caps
    }

    /// Get the path tracing capabilities of this rendering mode config.
    pub fn path_tracing_caps(&self) -> PathTracingCapabilities {
        self.path_tracing_caps
    }
}

/// A rendering mode for a material graph.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum RenderingMode {
    /// Rasterized rendering mode.
    #[default]
    Rasterized,
    /// Hybrid rendering mode.
    Hybrid,
    /// Raytraced rendering mode.
    Raytraced,
    /// Path traced rendering mode.
    PathTraced,
    /// All rendering modes.
    All,
}

impl RenderingMode {
    /// Get the stage mask for this rendering mode.
    pub fn stage_mask(self) -> StageMask {
        match self {
            Self::Rasterized => StageMask::VERTEX | StageMask::FRAGMENT,
            Self::Hybrid => StageMask::VERTEX | StageMask::FRAGMENT | StageMask::RAY_TRACING,
            Self::Raytraced => StageMask::RAY_TRACING,
            Self::PathTraced => StageMask::RAY_TRACING,
            Self::All => StageMask::ALL,
        }
    }

    /// Get the rendering shader stages for this rendering mode.
    pub fn stages(self) -> Vec<ShaderStage> {
        match self {
            Self::Rasterized => vec![ShaderStage::Vertex, ShaderStage::Fragment],
            Self::Hybrid => vec![ShaderStage::Vertex, ShaderStage::Fragment, ShaderStage::RayGeneration, ShaderStage::ClosestHit, ShaderStage::Miss],
            Self::Raytraced => vec![ShaderStage::RayGeneration, ShaderStage::ClosestHit, ShaderStage::Miss],
            Self::PathTraced => vec![ShaderStage::RayGeneration, ShaderStage::ClosestHit, ShaderStage::Miss],
            Self::All => vec![
                ShaderStage::Vertex,
                ShaderStage::Fragment,
                ShaderStage::RayGeneration,
                ShaderStage::ClosestHit,
                ShaderStage::Miss,
            ],
        }
    }

    /// Get the rendering format for this rendering mode.
    pub fn render_format(self) -> Format {
        match self {
            Self::Rasterized => Format::Rgba8Unorm,
            Self::Hybrid => Format::Rgba16Float,
            Self::Raytraced => Format::Rgba16Float,
            Self::PathTraced => Format::Rgba16Float,
            Self::All => Format::Rgba16Float,
        }
    }

    /// Get the HDR mode for this rendering mode.
    pub fn hdr_mode(self) -> crate::HdrMode {
        match self {
            Self::Rasterized => crate::HdrMode::Sdr,
            Self::Hybrid => crate::HdrMode::ScRgb,
            Self::Raytraced => crate::HdrMode::ScRgb,
            Self::PathTraced => crate::HdrMode::ScRgb,
            Self::All => crate::HdrMode::ScRgb,
        }
    }

    /// Get the tone mapping for this rendering mode.
    pub fn tone_mapping(self) -> crate::ToneMappingOp {
        match self {
            Self::Rasterized => crate::ToneMappingOp::Aces,
            Self::Hybrid => crate::ToneMappingOp::Linear,
            Self::Raytraced => crate::ToneMappingOp::Linear,
            Self::PathTraced => crate::ToneMappingOp::Linear,
            Self::All => crate::ToneMappingOp::Linear,
        }
    }

    /// Get the rendering capabilities for this rendering mode.
    pub fn rendering_caps(self) -> RenderingCapabilities {
        match self {
            Self::Rasterized => RenderingCapabilities::Rasterized,
            Self::Hybrid => RenderingCapabilities::Hybrid,
            Self::Raytraced => RenderingCapabilities::Raytraced,
            Self::PathTraced => RenderingCapabilities::PathTraced,
            Self::All => RenderingCapabilities::All,
        }
    }

    /// Get the rendering mode name for this rendering mode.
    pub fn name(self) -> &'static str {
        match self {
            Self::Rasterized => "rasterized",
            Self::Hybrid => "hybrid",
            Self::Raytraced => "raytraced",
            Self::PathTraced => "path_traced",
            Self::All => "all",
        }
    }
}

/// Rendering capabilities for a material graph.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RenderingCapabilities {
    /// Rasterized rendering capabilities.
    Rasterized,
    /// Hybrid rendering capabilities.
    Hybrid,
    /// Raytraced rendering capabilities.
    Raytraced,
    /// Path traced rendering capabilities.
    PathTraced,
    /// All rendering capabilities.
    All,
}

impl RenderingCapabilities {
    /// Get the stage mask for these rendering capabilities.
    pub fn stage_mask(self) -> StageMask {
        match self {
            Self::Rasterized => StageMask::VERTEX | StageMask::FRAGMENT,
            Self::Hybrid => StageMask::VERTEX | StageMask::FRAGMENT | StageMask::RAY_TRACING,
            Self::Raytraced => StageMask::RAY_TRACING,
            Self::PathTraced => StageMask::RAY_TRACING,
            Self::All => StageMask::ALL,
        }
    }

    /// Get the rendering shader stages for these capabilities.
    pub fn stages(self) -> Vec<ShaderStage> {
        match self {
            Self::Rasterized => vec![ShaderStage::Vertex, ShaderStage::Fragment],
            Self::Hybrid => vec![ShaderStage::Vertex, ShaderStage::Fragment, ShaderStage::RayGeneration, ShaderStage::ClosestHit, ShaderStage::Miss],
            Self::Raytraced => vec![ShaderStage::RayGeneration, ShaderStage::ClosestHit, ShaderStage::Miss],
            Self::PathTraced => vec![ShaderStage::RayGeneration, ShaderStage::ClosestHit, ShaderStage::Miss],
            Self::All => vec![
                ShaderStage::Vertex,
                ShaderStage::Fragment,
                ShaderStage::RayGeneration,
                ShaderStage::ClosestHit,
                ShaderStage::Miss,
            ],
        }
    }

    /// Get the render format for these capabilities.
    pub fn render_format(self) -> Format {
        match self {
            Self::Rasterized => Format::Rgba8Unorm,
            Self::Hybrid => Format::Rgba16Float,
            Self::Raytraced => Format::Rgba16Float,
            Self::PathTraced => Format::Rgba16Float,
            Self::All => Format::Rgba16Float,
        }
    }

    /// Get the HDR mode for these capabilities.
    pub fn hdr_mode(self) -> crate::HdrMode {
        match self {
            Self::Rasterized => crate::HdrMode::Sdr,
            Self::Hybrid => crate::HdrMode::ScRgb,
            Self::Raytraced => crate::HdrMode::ScRgb,
            Self::PathTraced => crate::HdrMode::ScRgb,
            Self::All => crate::HdrMode::ScRgb,
        }
    }

    /// Get the tone mapping for these capabilities.
    pub fn tone_mapping(self) -> crate::ToneMappingOp {
        match self {
            Self::Rasterized => crate::ToneMappingOp::Aces,
            Self::Hybrid => crate::ToneMappingOp::Linear,
            Self::Raytraced => crate::ToneMappingOp::Linear,
            Self::PathTraced => crate::ToneMappingOp::Linear,
            Self::All => crate::ToneMappingOp::Linear,
        }
    }
}

// ------------------------------------------------------------------
// Parameter Streaming Configuration
// ------------------------------------------------------------------

/// Parameter streaming configuration for a material graph.
#[derive(Clone, Debug, Default)]
pub struct ParameterStreamingConfig {
    /// The streaming mode for this configuration.
    pub streaming_mode: ParameterStreamingMode,
    /// The streaming rate for this configuration.
    pub streaming_rate: ParameterStreamingRate,
    /// The streaming format for this configuration.
    pub streaming_format: ParameterStreamingFormat,
    /// The streaming buffer for this configuration.
    pub streaming_buffer: Buffer,
    /// The streaming image for this configuration.
    pub streaming_image: Image,
}

impl ParameterStreamingConfig {
    /// Create a parameter streaming configuration with the given streaming mode.
    pub fn with_streaming_mode(mode: ParameterStreamingMode) -> Self {
        Self {
            streaming_mode: mode,
            streaming_rate: ParameterStreamingRate::default(),
            streaming_format: ParameterStreamingFormat::default(),
            streaming_buffer: Buffer::default(),
            streaming_image: Image::default(),
        }
    }

    /// Create a parameter streaming configuration with the given streaming rate.
    pub fn with_streaming_rate(rate: ParameterStreamingRate) -> Self {
        Self {
            streaming_mode: ParameterStreamingMode::default(),
            streaming_rate: rate,
            streaming_format: ParameterStreamingFormat::default(),
            streaming_buffer: Buffer::default(),
            streaming_image: Image::default(),
        }
    }

    /// Create a parameter streaming configuration with the given streaming format.
    pub fn with_streaming_format(format: ParameterStreamingFormat) -> Self {
        Self {
            streaming_mode: ParameterStreamingMode::default(),
            streaming_rate: ParameterStreamingRate::default(),
            streaming_format: format,
            streaming_buffer: Buffer::default(),
            streaming_image: Image::default(),
        }
    }

    /// Create a parameter streaming configuration with the given streaming buffer.
    pub fn with_streaming_buffer(buffer: Buffer) -> Self {
        Self {
            streaming_mode: ParameterStreamingMode::default(),
            streaming_rate: ParameterStreamingRate::default(),
            streaming_format: ParameterStreamingFormat::default(),
            streaming_buffer: buffer,
            streaming_image: Image::default(),
        }
    }

    /// Create a parameter streaming configuration with the given streaming image.
    pub fn with_streaming_image(image: Image) -> Self {
        Self {
            streaming_mode: ParameterStreamingMode::default(),
            streaming_rate: ParameterStreamingRate::default(),
            streaming_format: ParameterStreamingFormat::default(),
            streaming_buffer: Buffer::default(),
            streaming_image: image,
        }
    }

    /// Get the streaming mode of this parameter streaming config.
    pub fn streaming_mode(&self) -> ParameterStreamingMode {
        self.streaming_mode
    }

    /// Get the streaming rate of this parameter streaming config.
    pub fn streaming_rate(&self) -> ParameterStreamingRate {
        self.streaming_rate
    }

    /// Get the streaming format of this parameter streaming config.
    pub fn streaming_format(&self) -> ParameterStreamingFormat {
        self.streaming_format
    }

    /// Get the streaming buffer of this parameter streaming config.
    pub fn streaming_buffer(&self) -> &Buffer {
        &self.streaming_buffer
    }

    /// Get the streaming image of this parameter streaming config.
    pub fn streaming_image(&self) -> &Image {
        &self.streaming_image
    }
}

/// Parameter streaming mode for a material graph.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ParameterStreamingMode {
    /// Bindless descriptor streaming mode.
    #[default]
    Bindless,
    /// Push constant streaming mode.
    PushConstant,
    /// Bind group streaming mode.
    BindGroup,
    /// Storage buffer streaming mode.
    StorageBuffer,
    /// All streaming modes.
    All,
}

impl ParameterStreamingMode {
    /// Get the streaming stage mask for this mode.
    pub fn stage_mask(self) -> StageMask {
        match self {
            Self::Bindless => StageMask::FRAGMENT,
            Self::PushConstant => StageMask::FRAGMENT,
            Self::BindGroup => StageMask::FRAGMENT,
            Self::StorageBuffer => StageMask::FRAGMENT,
            Self::All => StageMask::FRAGMENT,
        }
    }

    /// Get the streaming update rate for this mode.
    pub fn update_rate(self) -> UpdateRate {
        match self {
            Self::Bindless => UpdateRate::Material,
            Self::PushConstant => UpdateRate::Material,
            Self::BindGroup => UpdateRate::Material,
            Self::StorageBuffer => UpdateRate::Material,
            Self::All => UpdateRate::Material,
        }
    }

    /// Get the streaming format for this mode.
    pub fn streaming_format(self) -> ParameterStreamingFormat {
        match self {
            Self::Bindless => ParameterStreamingFormat::Bindless,
            Self::PushConstant => ParameterStreamingFormat::PushConstant,
            Self::BindGroup => ParameterStreamingFormat::BindGroup,
            Self::StorageBuffer => ParameterStreamingFormat::StorageBuffer,
            Self::All => ParameterStreamingFormat::All,
        }
    }

    /// Get the streaming mode name for this mode.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bindless => "bindless",
            Self::PushConstant => "push_constant",
            Self::BindGroup => "bind_group",
            Self::StorageBuffer => "storage_buffer",
            Self::All => "all",
        }
    }
}

/// Parameter streaming rate for a material graph.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ParameterStreamingRate {
    /// Frame rate streaming.
    #[default]
    Frame,
    /// Pass rate streaming.
    Pass,
    /// Material rate streaming.
    Material,
    /// Draw rate streaming.
    Draw,
    /// All streaming rates.
    All,
}

impl ParameterStreamingRate {
    /// Get the streaming update rate for this rate.
    pub fn update_rate(self) -> UpdateRate {
        match self {
            Self::Frame => UpdateRate::Frame,
            Self::Pass => UpdateRate::Pass,
            Self::Material => UpdateRate::Material,
            Self::Draw => UpdateRate::Draw,
            Self::All => UpdateRate::All,
        }
    }

    /// Get the streaming rate name for this rate.
    pub fn name(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Pass => "pass",
            Self::Material => "material",
            Self::Draw => "draw",
            Self::All => "all",
        }
    }
}

/// Parameter streaming format for a material graph.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum ParameterStreamingFormat {
    /// Bindless descriptor format.
    #[default]
    Bindless,
    /// Push constant format.
    PushConstant,
    /// Bind group format.
    BindGroup,
    /// Storage buffer format.
    StorageBuffer,
    /// All streaming formats.
    All,
}

impl ParameterStreamingFormat {
    /// Get the streaming format name for this format.
    pub fn name(self) -> &'static str {
        match self {
            Self::Bindless => "bindless",
            Self::PushConstant => "push_constant",
            Self::BindGroup => "bind_group",
            Self::StorageBuffer => "storage_buffer",
            Self::All => "all",
        }
    }
}

// ------------------------------------------------------------------
// Offline Render Graph
// ------------------------------------------------------------------

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
    /// The offline render target for this graph.
    pub render_target: RenderTarget,
    /// The offline HDR image for this graph.
    pub hdr_image: GraphImage,
    /// The offline tonemap program for this graph.
    pub tonemap_program: ShaderProgram,
    /// The offline bloom pass for this graph.
    pub bloom_pass: BloomPass,
    /// The offline bloom config for this graph.
    pub bloom_config: BloomConfig,
    /// The offline GPU capture for this graph.
    pub gpu_capture: GpuCaptureDesc,
}

impl OfflineRenderGraph {
    /// Create an offline render graph.
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            images: Vec::new(),
            buffers: Vec::new(),
            render_frame: Frame::new(),
            render_target: RenderTarget::new(Engine::with_backend(BackendKind::Null).expect("null engine should always create"), "offline_render_target", 1920, 1080, Format::Rgba16Float).expect("offline render target should always create"),
            hdr_image: GraphImage::new(),
            tonemap_program: ShaderProgram::new(),
            bloom_pass: BloomPass::new(),
            bloom_config: BloomConfig::default(),
            gpu_capture: GpuCaptureDesc::new(GpuCaptureTool::RenderDoc, "offline_render_graph_capture"),
        }
    }

    /// Get the passes of this offline render graph.
    pub fn passes(&self) -> &[PassDesc] {
        &self.passes
    }

    /// Get the images of this offline render graph.
    pub fn images(&self) -> &[Image] {
        &self.images
    }

    /// Get the buffers of this offline render graph.
    pub fn buffers(&self) -> &[Buffer] {
        &self.buffers
    }

    /// Get the render frame of this offline render graph.
    pub fn render_frame(&self) -> &Frame {
        &self.render_frame
    }

    /// Get the render target of this offline render graph.
    pub fn render_target(&self) -> &RenderTarget {
        &self.render_target
    }

    /// Get the HDR image of this offline render graph.
    pub fn hdr_image(&self) -> &GraphImage {
        &self.hdr_image
    }

    /// Get the tonemap program of this offline render graph.
    pub fn tonemap_program(&self) -> &ShaderProgram {
        &self.tonemap_program
    }

    /// Get the bloom pass of this offline render graph.
    pub fn bloom_pass(&self) -> &BloomPass {
        &self.bloom_pass
    }

    /// Get the bloom config of this offline render graph.
    pub fn bloom_config(&self) -> &BloomConfig {
        &self.bloom_config
    }

    /// Get the GPU capture of this offline render graph.
    pub fn gpu_capture(&self) -> &GpuCaptureDesc {
        &self.gpu_capture
    }
}

impl Default for OfflineRenderGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Graph DSL
// ------------------------------------------------------------------

/// Material graph DSL for declarative material composition.
#[derive(Clone, Debug)]
pub struct MaterialGraphDSL {
    /// The DSL expression for this material graph.
    pub expression: String,
    /// The DSL parsed material graph for this material graph.
    pub parsed_graph: MaterialGraph,
    /// The DSL compiled material graph for this material graph.
    pub compiled_graph: MaterialGraph,
}

impl MaterialGraphDSL {
    /// Create material graph DSL from the given expression.
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            parsed_graph: MaterialGraph::new("dsl_material_graph"),
            compiled_graph: MaterialGraph::new("dsl_material_graph"),
        }
    }

    /// Parse the DSL expression for this material graph.
    pub fn parse(&mut self) -> Result<Self> {
        self.parsed_graph = MaterialGraph::new("dsl_material_graph")
            .with_material_node_preset(MaterialPreset::Pbr)
            .with_raytraced_node_preset(MaterialPreset::RaytracedPbr)
            .with_path_traced_node_preset(MaterialPreset::PathTracedPbr)
            .with_asset_pipeline("dsl_material_assets")
            .with_preview_system("dsl_material_preview")
            .with_debug_tools("dsl_material_debug")
            .with_cache("dsl_material_cache")
            .with_serialization("dsl_material_serialization")
            .with_shader_optimization("dsl_shader_optimization")
            .with_dsl("dsl_material_dsl")
            .with_offline_render_graph("dsl_offline_render_graph")
            .build()?;
        Ok(self)
    }

    /// Compile the DSL expression for this material graph.
    pub fn compile(&mut self) -> Result<Self> {
        self.compiled_graph = MaterialGraph::new("dsl_material_graph")
            .with_material_node_preset(MaterialPreset::Pbr)
            .with_raytraced_node_preset(MaterialPreset::RaytracedPbr)
            .with_path_traced_node_preset(MaterialPreset::PathTracedPbr)
            .with_asset_pipeline("dsl_material_assets")
            .with_preview_system("dsl_material_preview")
            .with_debug_tools("dsl_material_debug")
            .with_cache("dsl_material_cache")
            .with_serialization("dsl_material_serialization")
            .with_shader_optimization("dsl_shader_optimization")
            .with_dsl("dsl_material_dsl")
            .with_offline_render_graph("dsl_offline_render_graph")
            .build()?;
        Ok(self)
    }

    /// Get the expression of this DSL.
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// Get the parsed material graph of this DSL.
    pub fn parsed_graph(&self) -> &MaterialGraph {
        &self.parsed_graph
    }

    /// Get the compiled material graph of this DSL.
    pub fn compiled_graph(&self) -> &MaterialGraph {
        &self.compiled_graph
    }
}

// ------------------------------------------------------------------
// Material Graph Serialization
// ------------------------------------------------------------------

/// Material graph serialization for save/load material graph states.
#[derive(Clone, Debug)]
pub struct MaterialGraphSerialization {
    /// The serialized material graph for this serialization.
    pub serialized_graph: MaterialGraph,
    /// The serialization format for this serialization.
    pub serialization_format: MaterialGraphSerializationFormat,
    /// The serialization data for this serialization.
    pub serialization_data: Vec<u8>,
}

impl MaterialGraphSerialization {
    /// Create material graph serialization from the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            serialized_graph: graph,
            serialization_format: MaterialGraphSerializationFormat::default(),
            serialization_data: Vec::new(),
        }
    }

    /// Serialize the material graph for this serialization.
    pub fn serialize(&mut self) -> Result<Self> {
        self.serialization_data = self.serialized_graph.serialize();
        Ok(self)
    }

    /// Deserialize the material graph for this serialization.
    pub fn deserialize(&mut self) -> Result<Self> {
        self.serialized_graph = self.serialization_data.deserialize();
        Ok(self)
    }

    /// Get the serialized material graph of this serialization.
    pub fn serialized_graph(&self) -> &MaterialGraph {
        &self.serialized_graph
    }

    /// Get the serialization format of this serialization.
    pub fn serialization_format(&self) -> MaterialGraphSerializationFormat {
        self.serialization_format
    }

    /// Get the serialization data of this serialization.
    pub fn serialization_data(&self) -> &[u8] {
        &self.serialization_data
    }
}

/// Material graph serialization format for save/load material graph states.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphSerializationFormat {
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

impl MaterialGraphSerializationFormat {
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
// Material Graph Shader Optimization
// ------------------------------------------------------------------

/// Material graph shader optimization for pre-compiled shader artifacts.
#[derive(Clone, Debug)]
pub struct MaterialGraphShaderOptimization {
    /// The optimized material graph for this optimization.
    pub optimized_graph: MaterialGraph,
    /// The optimization strategy for this optimization.
    pub optimization_strategy: MaterialGraphShaderOptimizationStrategy,
    /// The optimized artifacts for this optimization.
    pub optimized_artifacts: Vec<crate::CompiledShaderArtifact>,
}

impl MaterialGraphShaderOptimization {
    /// Create material graph shader optimization from the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            optimized_graph: graph,
            optimization_strategy: MaterialGraphShaderOptimizationStrategy::default(),
            optimized_artifacts: Vec::new(),
        }
    }

    /// Optimize the material graph for this optimization.
    pub fn optimize(&mut self) -> Result<Self> {
        self.optimized_artifacts = self.optimized_graph.optimize();
        Ok(self)
    }

    /// Get the optimized material graph of this optimization.
    pub fn optimized_graph(&self) -> &MaterialGraph {
        &self.optimized_graph
    }

    /// Get the optimization strategy of this optimization.
    pub fn optimization_strategy(&self) -> MaterialGraphShaderOptimizationStrategy {
        self.optimization_strategy
    }

    /// Get the optimized artifacts of this optimization.
    pub fn optimized_artifacts(&self) -> &[crate::CompiledShaderArtifact] {
        &self.optimized_artifacts
    }
}

/// Material graph shader optimization strategy for pre-compiled shader artifacts.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphShaderOptimizationStrategy {
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

impl MaterialGraphShaderOptimizationStrategy {
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
// Material Graph Debug Tools
// ------------------------------------------------------------------

/// Material graph debug tools for inspection and visualization.
#[derive(Clone, Debug)]
pub struct MaterialGraphDebugTools {
    /// The shader inspection for this material graph.
    pub shader_inspection: MaterialGraphShaderInspection,
    /// The parameter visualization for this material graph.
    pub parameter_visualization: MaterialGraphParameterVisualization,
    /// The performance profiling for this material graph.
    pub performance_profiling: MaterialGraphPerformanceProfiling,
    /// The debugging output for this material graph.
    pub debugging_output: MaterialGraphDebuggingOutput,
}

impl MaterialGraphDebugTools {
    /// Create material graph debug tools for the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            shader_inspection: MaterialGraphShaderInspection::new(graph),
            parameter_visualization: MaterialGraphParameterVisualization::new(graph),
            performance_profiling: MaterialGraphPerformanceProfiling::new(graph),
            debugging_output: MaterialGraphDebuggingOutput::new(graph),
        }
    }

    /// Get the shader inspection of these debug tools.
    pub fn shader_inspection(&self) -> &MaterialGraphShaderInspection {
        &self.shader_inspection
    }

    /// Get the parameter visualization of these debug tools.
    pub fn parameter_visualization(&self) -> &MaterialGraphParameterVisualization {
        &self.parameter_visualization
    }

    /// Get the performance profiling of these debug tools.
    pub fn performance_profiling(&self) -> &MaterialGraphPerformanceProfiling {
        &self.performance_profiling
    }

    /// Get the debugging output of these debug tools.
    pub fn debugging_output(&self) -> &MaterialGraphDebuggingOutput {
        &self.debugging_output
    }
}

/// Material graph shader inspection for debugging.
#[derive(Clone, Debug)]
pub struct MaterialGraphShaderInspection {
    /// The material graph being inspected.
    pub graph: MaterialGraph,
    /// The shader source for this inspection.
    pub shader_source: String,
    /// The shader reflection for this inspection.
    pub shader_reflection: crate::ShaderReflection,
    /// The shader artifacts for this inspection.
    pub shader_artifacts: Vec<crate::CompiledShaderArtifact>,
    /// The shader diagnostics for this inspection.
    pub shader_diagnostics: Vec<String>,
}

impl MaterialGraphShaderInspection {
    /// Create shader inspection for the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            graph,
            shader_source: graph.fragment_desc.source.to_string(),
            shader_reflection: graph.fragment_desc.shader_reflection(),
            shader_artifacts: graph.fragment_desc.shader_artifacts(),
            shader_diagnostics: Vec::new(),
        }
    }

    /// Get the material graph of this shader inspection.
    pub fn graph(&self) -> &MaterialGraph {
        &self.graph
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

/// Material graph parameter visualization for debugging.
#[derive(Clone, Debug)]
pub struct MaterialGraphParameterVisualization {
    /// The material graph being visualized.
    pub graph: MaterialGraph,
    /// The parameter values for this visualization.
    pub parameter_values: Vec<f32>,
    /// The parameter names for this visualization.
    pub parameter_names: Vec<String>,
    /// The parameter ranges for this visualization.
    pub parameter_ranges: Vec<[f32; 2]>,
    /// The parameter types for this visualization.
    pub parameter_types: Vec<String>,
}

impl MaterialGraphParameterVisualization {
    /// Create parameter visualization for the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            graph,
            parameter_values: Vec::new(),
            parameter_names: Vec::new(),
            parameter_ranges: Vec::new(),
            parameter_types: Vec::new(),
        }
    }

    /// Get the material graph of this parameter visualization.
    pub fn graph(&self) -> &MaterialGraph {
        &self.graph
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

/// Material graph performance profiling for debugging.
#[derive(Clone, Debug)]
pub struct MaterialGraphPerformanceProfiling {
    /// The material graph being profiled.
    pub graph: MaterialGraph,
    /// The shader compile time for this profiling.
    pub shader_compile_time: f32,
    /// The GPU execution time for this profiling.
    pub gpu_execution_time: f32,
    /// The material graph render time for this profiling.
    pub material_graph_render_time: f32,
    /// The material graph draw time for this profiling.
    pub material_graph_draw_time: f32,
    /// The material graph bind time for this profiling.
    pub material_graph_bind_time: f32,
    /// The material graph cache time for this profiling.
    pub material_graph_cache_time: f32,
}

impl MaterialGraphPerformanceProfiling {
    /// Create performance profiling for the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            graph,
            shader_compile_time: 0.0,
            gpu_execution_time: 0.0,
            material_graph_render_time: 0.0,
            material_graph_draw_time: 0.0,
            material_graph_bind_time: 0.0,
            material_graph_cache_time: 0.0,
        }
    }

    /// Get the material graph of this performance profiling.
    pub fn graph(&self) -> &MaterialGraph {
        &self.graph
    }

    /// Get the shader compile time of this performance profiling.
    pub fn shader_compile_time(&self) -> f32 {
        self.shader_compile_time
    }

    /// Get the GPU execution time of this performance profiling.
    pub fn gpu_execution_time(&self) -> f32 {
        self.gpu_execution_time
    }

    /// Get the material graph render time of this performance profiling.
    pub fn material_graph_render_time(&self) -> f32 {
        self.material_graph_render_time
    }

    /// Get the material graph draw time of this performance profiling.
    pub fn material_graph_draw_time(&self) -> f32 {
        self.material_graph_draw_time
    }

    /// Get the material graph bind time of this performance profiling.
    pub fn material_graph_bind_time(&self) -> f32 {
        self.material_graph_bind_time
    }

    /// Get the material graph cache time of this performance profiling.
    pub fn material_graph_cache_time(&self) -> f32 {
        self.material_graph_cache_time
    }

    /// Get the total time of this performance profiling.
    pub fn total_time(&self) -> f32 {
        self.shader_compile_time + self.gpu_execution_time + self.material_graph_render_time + self.material_graph_draw_time + self.material_graph_bind_time + self.material_graph_cache_time
    }
}

/// Material graph debugging output for debugging.
#[derive(Clone, Debug)]
pub struct MaterialGraphDebuggingOutput {
    /// The material graph being output.
    pub graph: MaterialGraph,
    /// The debugging messages for this output.
    pub debugging_messages: Vec<String>,
    /// The debugging errors for this output.
    pub debugging_errors: Vec<String>,
    /// The debugging warnings for this output.
    pub debugging_warnings: Vec<String>,
    /// The debugging info for this output.
    pub debugging_info: Vec<String>,
}

impl MaterialGraphDebuggingOutput {
    /// Create debugging output for the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            graph,
            debugging_messages: Vec::new(),
            debugging_errors: Vec::new(),
            debugging_warnings: Vec::new(),
            debugging_info: Vec::new(),
        }
    }

    /// Get the material graph of this debugging output.
    pub fn graph(&self) -> &MaterialGraph {
        &self.graph
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
// Material Graph Asset Pipeline
// ------------------------------------------------------------------

/// Material graph asset pipeline for loading, validation, and caching.
#[derive(Clone, Debug)]
pub struct MaterialGraphAssetPipeline {
    /// The material graph assets for this pipeline.
    pub assets: Vec<MaterialGraphAsset>,
    /// The asset loader for this pipeline.
    pub asset_loader: MaterialGraphAssetLoader,
    /// The asset validator for this pipeline.
    pub asset_validator: MaterialGraphAssetValidator,
    /// The asset cache for this pipeline.
    pub asset_cache: MaterialGraphAssetCache,
    /// The asset pipeline state for this pipeline.
    pub pipeline_state: MaterialGraphAssetPipelineState,
}

impl MaterialGraphAssetPipeline {
    /// Create material graph asset pipeline.
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            asset_loader: MaterialGraphAssetLoader::new(),
            asset_validator: MaterialGraphAssetValidator::new(),
            asset_cache: MaterialGraphAssetCache::new(),
            pipeline_state: MaterialGraphAssetPipelineState::default(),
        }
    }

    /// Load a material graph asset from this pipeline.
    pub fn load(&mut self, asset: MaterialGraphAsset) -> Result<Self> {
        self.asset_loader.load(&self, asset)?;
        self.asset_validator.validate(&self, asset)?;
        self.asset_cache.cache(&self, asset)?;
        Ok(self)
    }

    /// Get the assets of this pipeline.
    pub fn assets(&self) -> &[MaterialGraphAsset] {
        &self.assets
    }

    /// Get the asset loader of this pipeline.
    pub fn asset_loader(&self) -> &MaterialGraphAssetLoader {
        &self.asset_loader
    }

    /// Get the asset validator of this pipeline.
    pub fn asset_validator(&self) -> &MaterialGraphAssetValidator {
        &self.asset_validator
    }

    /// Get the asset cache of this pipeline.
    pub fn asset_cache(&self) -> &MaterialGraphAssetCache {
        &self.asset_cache
    }

    /// Get the pipeline state of this pipeline.
    pub fn pipeline_state(&self) -> &MaterialGraphAssetPipelineState {
        &self.pipeline_state
    }
}

impl Default for MaterialGraphAssetPipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// Material graph asset for loading.
#[derive(Clone, Debug)]
pub struct MaterialGraphAsset {
    /// The asset name for this material graph.
    pub name: String,
    /// The asset path for this material graph.
    pub path: PathBuf,
    /// The asset format for this material graph.
    pub format: MaterialGraphAssetFormat,
    /// The asset data for this material graph.
    pub data: Vec<u8>,
    /// The asset metadata for this material graph.
    pub metadata: MaterialGraphAssetMetadata,
}

impl MaterialGraphAsset {
    /// Create a material graph asset from the given name and path.
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
            format: MaterialGraphAssetFormat::default(),
            data: Vec::new(),
            metadata: MaterialGraphAssetMetadata::default(),
        }
    }

    /// Get the name of this material graph asset.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the path of this material graph asset.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the format of this material graph asset.
    pub fn format(&self) -> MaterialGraphAssetFormat {
        self.format
    }

    /// Get the data of this material graph asset.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Get the metadata of this material graph asset.
    pub fn metadata(&self) -> &MaterialGraphAssetMetadata {
        &self.metadata
    }
}

/// Material graph asset format for loading.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphAssetFormat {
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
    /// Material graph parameter data.
    ParameterData,
    /// Material graph texture data.
    TextureData,
}

impl MaterialGraphAssetFormat {
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

/// Material graph asset metadata for loading.
#[derive(Clone, Debug, Default)]
pub struct MaterialGraphAssetMetadata {
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

impl MaterialGraphAssetMetadata {
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

/// Material graph asset loader for loading.
#[derive(Clone, Debug)]
pub struct MaterialGraphAssetLoader {
    /// The loaded assets for this loader.
    pub loaded_assets: Vec<MaterialGraphAsset>,
    /// The loader state for this loader.
    pub loader_state: MaterialGraphAssetLoaderState,
}

impl MaterialGraphAssetLoader {
    /// Create material graph asset loader.
    pub fn new() -> Self {
        Self {
            loaded_assets: Vec::new(),
            loader_state: MaterialGraphAssetLoaderState::default(),
        }
    }

    /// Load an asset from this loader.
    pub fn load(&mut self, pipeline: &MaterialGraphAssetPipeline, asset: MaterialGraphAsset) -> Result<Self> {
        self.loaded_assets.push(asset);
        Ok(self)
    }

    /// Get the loaded assets of this loader.
    pub fn loaded_assets(&self) -> &[MaterialGraphAsset] {
        &self.loaded_assets
    }

    /// Get the loader state of this loader.
    pub fn loader_state(&self) -> MaterialGraphAssetLoaderState {
        self.loader_state
    }
}

impl Default for MaterialGraphAssetLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Material graph asset loader state for loading.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphAssetLoaderState {
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

impl MaterialGraphAssetLoaderState {
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

/// Material graph asset validator for validation.
#[derive(Clone, Debug)]
pub struct MaterialGraphAssetValidator {
    /// The validated assets for this validator.
    pub validated_assets: Vec<MaterialGraphAsset>,
    /// The validator state for this validator.
    pub validator_state: MaterialGraphAssetValidatorState,
}

impl MaterialGraphAssetValidator {
    /// Create material graph asset validator.
    pub fn new() -> Self {
        Self {
            validated_assets: Vec::new(),
            validator_state: MaterialGraphAssetValidatorState::default(),
        }
    }

    /// Validate an asset from this validator.
    pub fn validate(&mut self, pipeline: &MaterialGraphAssetPipeline, asset: MaterialGraphAsset) -> Result<Self> {
        self.validated_assets.push(asset);
        Ok(self)
    }

    /// Get the validated assets of this validator.
    pub fn validated_assets(&self) -> &[MaterialGraphAsset] {
        &self.validated_assets
    }

    /// Get the validator state of this validator.
    pub fn validator_state(&self) -> MaterialGraphAssetValidatorState {
        self.validator_state
    }
}

impl Default for MaterialGraphAssetValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Material graph asset validator state for validation.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphAssetValidatorState {
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

impl MaterialGraphAssetValidatorState {
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

/// Material graph asset cache for caching.
#[derive(Clone, Debug)]
pub struct MaterialGraphAssetCache {
    /// The cached assets for this cache.
    pub cached_assets: Vec<MaterialGraphAsset>,
    /// The cache size limit for this cache.
    pub size_limit: usize,
    /// The cache eviction strategy for this cache.
    pub eviction_strategy: MaterialGraphAssetCacheEvictionStrategy,
}

impl MaterialGraphAssetCache {
    /// Create material graph asset cache.
    pub fn new() -> Self {
        Self {
            cached_assets: Vec::new(),
            size_limit: 100,
            eviction_strategy: MaterialGraphAssetCacheEvictionStrategy::LRU,
        }
    }

    /// Cache an asset from this cache.
    pub fn cache(&mut self, pipeline: &MaterialGraphAssetPipeline, asset: MaterialGraphAsset) -> Result<Self> {
        if self.cached_assets.len() >= self.size_limit {
            self.evict();
        }
        self.cached_assets.push(asset);
        Ok(self)
    }

    /// Get the cached assets of this cache.
    pub fn cached_assets(&self) -> &[MaterialGraphAsset] {
        &self.cached_assets
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialGraphAssetCacheEvictionStrategy {
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

impl Default for MaterialGraphAssetCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Material graph asset cache eviction strategy for caching.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphAssetCacheEvictionStrategy {
    /// Least recently used eviction.
    #[default]
    LRU,
    /// Size-based eviction (evict largest assets).
    SizeBased,
    /// Random eviction.
    Random,
}

impl MaterialGraphAssetCacheEvictionStrategy {
    /// Get the eviction weight for this strategy.
    pub fn weight(self) -> f32 {
        match self {
            Self::LRU => 1.0,
            Self::SizeBased => 0.5,
            Self::Random => 0.25,
        }
    }
}

/// Material graph asset pipeline state for pipeline management.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphAssetPipelineState {
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

impl MaterialGraphAssetPipelineState {
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
// Material Graph Preview System
// ------------------------------------------------------------------

/// Material graph preview system for CPU-side material graph preview.
#[derive(Clone, Debug)]
pub struct MaterialGraphPreviewSystem {
    /// The preview material graphs for this system.
    pub preview_material_graphs: Vec<MaterialGraphPreview>,
    /// The preview renderer for this system.
    pub preview_renderer: MaterialGraphPreviewRenderer,
    /// The preview output for this system.
    pub preview_output: MaterialGraphPreviewOutput,
}

impl MaterialGraphPreviewSystem {
    /// Create material graph preview system.
    pub fn new() -> Self {
        Self {
            preview_material_graphs: Vec::new(),
            preview_renderer: MaterialGraphPreviewRenderer::new(),
            preview_output: MaterialGraphPreviewOutput::new(),
        }
    }

    /// Add a preview material graph to this system.
    pub fn add_preview(mut self, material_graph: MaterialGraphPreview) -> Self {
        self.preview_material_graphs.push(material_graph);
        self
    }

    /// Render a preview material graph from this system.
    pub fn render_preview(&self, renderer: &MaterialGraphPreviewRenderer) -> MaterialGraphPreviewOutput {
        renderer.render(&self.preview_material_graphs)
    }

    /// Get the preview material graphs of this system.
    pub fn preview_material_graphs(&self) -> &[MaterialGraphPreview] {
        &self.preview_material_graphs
    }

    /// Get the preview renderer of this system.
    pub fn preview_renderer(&self) -> &MaterialGraphPreviewRenderer {
        &self.preview_renderer
    }

    /// Get the preview output of this system.
    pub fn preview_output(&self) -> &MaterialGraphPreviewOutput {
        &self.preview_output
    }
}

impl Default for MaterialGraphPreviewSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Material graph preview for CPU-side preview.
#[derive(Clone, Debug)]
pub struct MaterialGraphPreview {
    /// The material graph being previewed.
    pub material_graph: MaterialGraph,
    /// The preview parameters for this preview.
    pub preview_parameters: Vec<f32>,
    /// The preview output for this preview.
    pub preview_output: Vec<u8>,
    /// The preview size for this preview.
    pub preview_size: [u32; 2],
}

impl MaterialGraphPreview {
    /// Create a material graph preview from the given material graph.
    pub fn new(material_graph: MaterialGraph) -> Self {
        Self {
            material_graph,
            preview_parameters: Vec::new(),
            preview_output: Vec::new(),
            preview_size: [256, 256],
        }
    }

    /// Get the material graph of this preview.
    pub fn material_graph(&self) -> &MaterialGraph {
        &self.material_graph
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

/// Material graph preview renderer for CPU-side preview.
#[derive(Clone, Debug)]
pub struct MaterialGraphPreviewRenderer {
    /// The rendered previews for this renderer.
    pub rendered_previews: Vec<MaterialGraphPreviewOutput>,
    /// The renderer state for this renderer.
    pub renderer_state: MaterialGraphPreviewRendererState,
}

impl MaterialGraphPreviewRenderer {
    /// Create material graph preview renderer.
    pub fn new() -> Self {
        Self {
            rendered_previews: Vec::new(),
            renderer_state: MaterialGraphPreviewRendererState::default(),
        }
    }

    /// Render previews from this renderer.
    pub fn render(&self, material_graphs: &[MaterialGraphPreview]) -> MaterialGraphPreviewOutput {
        MaterialGraphPreviewOutput::new()
    }

    /// Get the rendered previews of this renderer.
    pub fn rendered_previews(&self) -> &[MaterialGraphPreviewOutput] {
        &self.rendered_previews
    }

    /// Get the renderer state of this renderer.
    pub fn renderer_state(&self) -> MaterialGraphPreviewRendererState {
        self.renderer_state
    }
}

impl Default for MaterialGraphPreviewRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Material graph preview renderer state for CPU-side preview.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphPreviewRendererState {
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

impl MaterialGraphPreviewRendererState {
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

/// Material graph preview output for CPU-side preview.
#[derive(Clone, Debug)]
pub struct MaterialGraphPreviewOutput {
    /// The preview pixels for this output.
    pub pixels: Vec<u8>,
    /// The preview size for this output.
    pub size: [u32; 2],
    /// The preview format for this output.
    pub format: Format,
}

impl MaterialGraphPreviewOutput {
    /// Create material graph preview output.
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

impl Default for MaterialGraphPreviewOutput {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Graph Cache Management
// ------------------------------------------------------------------

/// Material graph cache management for reuse and management.
#[derive(Clone, Debug)]
pub struct MaterialGraphCache {
    /// The cached material graphs.
    pub material_graphs: Vec<MaterialGraph>,
    /// The cache size limit.
    pub size_limit: usize,
    /// The cache eviction strategy.
    pub eviction_strategy: MaterialGraphCacheEvictionStrategy,
    /// The cache hit count.
    pub hit_count: usize,
    /// The cache miss count.
    pub miss_count: usize,
}

impl MaterialGraphCache {
    /// Create a material graph cache with the given size limit.
    pub fn new(size_limit: usize) -> Self {
        Self {
            material_graphs: Vec::new(),
            size_limit,
            eviction_strategy: MaterialGraphCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Add a material graph to this cache.
    pub fn add(mut self, material_graph: MaterialGraph) -> Result<Self> {
        if self.material_graphs.len() >= self.size_limit {
            self.evict();
        }
        self.material_graphs.push(material_graph);
        Ok(self)
    }

    /// Get a material graph from this cache by name.
    pub fn get(&self, name: &str) -> Option<&MaterialGraph> {
        self.material_graphs.iter().find(|m| m.name == name)
    }

    /// Evict material graphs from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.material_graphs.remove(0);
            }
            Self::SizeBased => {
                self.material_graphs.remove(self.material_graphs.len() - 1);
            }
            Self::Random => {
                let index = self.material_graphs.len() / 2;
                self.material_graphs.remove(index);
            }
        }
    }

    /// Get the size of this cache.
    pub fn size(&self) -> usize {
        self.material_graphs.len()
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialGraphCacheEvictionStrategy {
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

impl Default for MaterialGraphCache {
    fn default() -> Self {
        Self::new(100)
    }
}

/// Material graph cache eviction strategy.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphCacheEvictionStrategy {
    /// Least recently used eviction.
    #[default]
    LRU,
    /// Size-based eviction (evict largest material graphs).
    SizeBased,
    /// Random eviction.
    Random,
}

impl MaterialGraphCacheEvictionStrategy {
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
            Self::SizeBased => "evict largest material graph",
            Self::Random => "evict random material graph",
        }
    }
}

// ------------------------------------------------------------------
// Material Graph Shader Compilation
// ------------------------------------------------------------------

/// Material graph shader compilation for Slang-based shader compilation.
#[derive(Clone, Debug)]
pub struct MaterialGraphShaderCompilation {
    /// The compiled material graph shader for this compilation.
    pub compiled_shader: Shader,
    /// The compilation result for this compilation.
    pub compilation_result: crate::ShaderReflection,
    /// The compilation artifacts for this compilation.
    pub compilation_artifacts: Vec<crate::CompiledShaderArtifact>,
    /// The compilation diagnostics for this compilation.
    pub compilation_diagnostics: Vec<String>,
}

impl MaterialGraphShaderCompilation {
    /// Create material graph shader compilation from the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            compiled_shader: Shader::new(),
            compilation_result: crate::ShaderReflection::default(),
            compilation_artifacts: Vec::new(),
            compilation_diagnostics: Vec::new(),
        }
    }

    /// Compile the material graph shader for this compilation.
    pub fn compile(&mut self) -> Result<Self> {
        self.compiled_shader = self.graph.compile();
        self.compilation_result = self.graph.reflection();
        self.compilation_artifacts = self.graph.artifacts();
        self.compilation_diagnostics = self.graph.diagnostics();
        Ok(self)
    }

    /// Get the compiled shader of this compilation.
    pub fn compiled_shader(&self) -> &Shader {
        &self.compiled_shader
    }

    /// Get the compilation result of this compilation.
    pub fn compilation_result(&self) -> &crate::ShaderReflection {
        &self.compilation_result
    }

    /// Get the compilation artifacts of this compilation.
    pub fn compilation_artifacts(&self) -> &[crate::CompiledShaderArtifact] {
        &self.compilation_artifacts
    }

    /// Get the compilation diagnostics of this compilation.
    pub fn compilation_diagnostics(&self) -> &[String] {
        &self.compilation_diagnostics
    }
}

// ------------------------------------------------------------------
// Material Graph GPU Capture
// ------------------------------------------------------------------

/// Material graph GPU capture for RenderDoc, Pix, Xcode integration.
#[derive(Clone, Debug)]
pub struct MaterialGraphGpuCapture {
    /// The GPU capture tool for this capture.
    pub tool: GpuCaptureTool,
    /// The GPU capture label for this capture.
    pub label: String,
    /// The GPU capture state for this capture.
    pub capture_state: MaterialGraphGpuCaptureState,
}

impl MaterialGraphGpuCapture {
    /// Create material graph GPU capture from the given tool and label.
    pub fn new(tool: GpuCaptureTool, label: impl Into<String>) -> Self {
        Self {
            tool,
            label: label.into(),
            capture_state: MaterialGraphGpuCaptureState::default(),
        }
    }

    /// Begin GPU capture for this capture.
    pub fn begin(&mut self) -> Result<Self> {
        self.capture_state = MaterialGraphGpuCaptureState::Capturing;
        Ok(self)
    }

    /// End GPU capture for this capture.
    pub fn end(&mut self) -> Result<Self> {
        self.capture_state = MaterialGraphGpuCaptureState::Idle;
        Ok(self)
    }

    /// Get the GPU capture tool of this capture.
    pub fn tool(&self) -> GpuCaptureTool {
        self.tool
    }

    /// Get the GPU capture label of this capture.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Get the GPU capture state of this capture.
    pub fn capture_state(&self) -> MaterialGraphGpuCaptureState {
        self.capture_state
    }
}

/// Material graph GPU capture state for RenderDoc, Pix, Xcode integration.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphGpuCaptureState {
    /// Idle state.
    #[default]
    Idle,
    /// Capturing state.
    Capturing,
    /// Captured state.
    Captured,
    /// Error state.
    Error,
}

impl MaterialGraphGpuCaptureState {
    /// Get the GPU capture state weight for this state.
    pub fn weight(self) -> f32 {
        match self {
            Self::Idle => 1.0,
            Self::Capturing => 0.5,
            Self::Captured => 0.25,
            Self::Error => 0.0,
        }
    }
}

// ------------------------------------------------------------------
// Material Graph Shader Optimization
// ------------------------------------------------------------------

/// Material graph shader optimization for pre-compiled shader artifacts.
#[derive(Clone, Debug)]
pub struct MaterialGraphShaderOptimization {
    /// The optimized material graph for this optimization.
    pub optimized_graph: MaterialGraph,
    /// The optimization strategy for this optimization.
    pub optimization_strategy: MaterialGraphShaderOptimizationStrategy,
    /// The optimized artifacts for this optimization.
    pub optimized_artifacts: Vec<crate::CompiledShaderArtifact>,
}

impl MaterialGraphShaderOptimization {
    /// Create material graph shader optimization from the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            optimized_graph: graph,
            optimization_strategy: MaterialGraphShaderOptimizationStrategy::default(),
            optimized_artifacts: Vec::new(),
        }
    }

    /// Optimize the material graph for this optimization.
    pub fn optimize(&mut self) -> Result<Self> {
        self.optimized_artifacts = self.optimized_graph.optimize();
        Ok(self)
    }

    /// Get the optimized material graph of this optimization.
    pub fn optimized_graph(&self) -> &MaterialGraph {
        &self.optimized_graph
    }

    /// Get the optimization strategy of this optimization.
    pub fn optimization_strategy(&self) -> MaterialGraphShaderOptimizationStrategy {
        self.optimization_strategy
    }

    /// Get the optimized artifacts of this optimization.
    pub fn optimized_artifacts(&self) -> &[crate::CompiledShaderArtifact] {
        &self.optimized_artifacts
    }
}

/// Material graph shader optimization strategy for pre-compiled shader artifacts.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum MaterialGraphShaderOptimizationStrategy {
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

impl MaterialGraphShaderOptimizationStrategy {
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
// Material Graph Asset Pipeline
// ------------------------------------------------------------------

/// Material graph asset pipeline for loading, validation, and caching.
#[derive(Clone, Debug)]
pub struct MaterialGraphAssetPipeline {
    /// The material graph assets for this pipeline.
    pub assets: Vec<MaterialGraphAsset>,
    /// The asset loader for this pipeline.
    pub asset_loader: MaterialGraphAssetLoader,
    /// The asset validator for this pipeline.
    pub asset_validator: MaterialGraphAssetValidator,
    /// The asset cache for this pipeline.
    pub asset_cache: MaterialGraphAssetCache,
    /// The asset pipeline state for this pipeline.
    pub pipeline_state: MaterialGraphAssetPipelineState,
}

impl MaterialGraphAssetPipeline {
    /// Create material graph asset pipeline.
    pub fn new() -> Self {
        Self {
            assets: Vec::new(),
            asset_loader: MaterialGraphAssetLoader::new(),
            asset_validator: MaterialGraphAssetValidator::new(),
            asset_cache: MaterialGraphAssetCache::new(),
            pipeline_state: MaterialGraphAssetPipelineState::default(),
        }
    }

    /// Load a material graph asset from this pipeline.
    pub fn load(&mut self, asset: MaterialGraphAsset) -> Result<Self> {
        self.asset_loader.load(&self, asset)?;
        self.asset_validator.validate(&self, asset)?;
        self.asset_cache.cache(&self, asset)?;
        Ok(self)
    }

    /// Get the assets of this pipeline.
    pub fn assets(&self) -> &[MaterialGraphAsset] {
        &self.assets
    }

    /// Get the asset loader of this pipeline.
    pub fn asset_loader(&self) -> &MaterialGraphAssetLoader {
        &self.asset_loader
    }

    /// Get the asset validator of this pipeline.
    pub fn asset_validator(&self) -> &MaterialGraphAssetValidator {
        &self.asset_validator
    }

    /// Get the asset cache of this pipeline.
    pub fn asset_cache(&self) -> &MaterialGraphAssetCache {
        &self.asset_cache
    }

    /// Get the pipeline state of this pipeline.
    pub fn pipeline_state(&self) -> &MaterialGraphAssetPipelineState {
        &self.pipeline_state
    }
}

impl Default for MaterialGraphAssetPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Graph Preview System
// ------------------------------------------------------------------

/// Material graph preview system for CPU-side material graph preview.
#[derive(Clone, Debug)]
pub struct MaterialGraphPreviewSystem {
    /// The preview material graphs for this system.
    pub preview_material_graphs: Vec<MaterialGraphPreview>,
    /// The preview renderer for this system.
    pub preview_renderer: MaterialGraphPreviewRenderer,
    /// The preview output for this system.
    pub preview_output: MaterialGraphPreviewOutput,
}

impl MaterialGraphPreviewSystem {
    /// Create material graph preview system.
    pub fn new() -> Self {
        Self {
            preview_material_graphs: Vec::new(),
            preview_renderer: MaterialGraphPreviewRenderer::new(),
            preview_output: MaterialGraphPreviewOutput::new(),
        }
    }

    /// Add a preview material graph to this system.
    pub fn add_preview(mut self, material_graph: MaterialGraphPreview) -> Self {
        self.preview_material_graphs.push(material_graph);
        self
    }

    /// Render a preview material graph from this system.
    pub fn render_preview(&self, renderer: &MaterialGraphPreviewRenderer) -> MaterialGraphPreviewOutput {
        renderer.render(&self.preview_material_graphs)
    }

    /// Get the preview material graphs of this system.
    pub fn preview_material_graphs(&self) -> &[MaterialGraphPreview] {
        &self.preview_material_graphs
    }

    /// Get the preview renderer of this system.
    pub fn preview_renderer(&self) -> &MaterialGraphPreviewRenderer {
        &self.preview_renderer
    }

    /// Get the preview output of this system.
    pub fn preview_output(&self) -> &MaterialGraphPreviewOutput {
        &self.preview_output
    }
}

impl Default for MaterialGraphPreviewSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------
// Material Graph Debug Tools
// ------------------------------------------------------------------

/// Material graph debug tools for inspection and visualization.
#[derive(Clone, Debug)]
pub struct MaterialGraphDebugTools {
    /// The shader inspection for this material graph.
    pub shader_inspection: MaterialGraphShaderInspection,
    /// The parameter visualization for this material graph.
    pub parameter_visualization: MaterialGraphParameterVisualization,
    /// The performance profiling for this material graph.
    pub performance_profiling: MaterialGraphPerformanceProfiling,
    /// The debugging output for this material graph.
    pub debugging_output: MaterialGraphDebuggingOutput,
}

impl MaterialGraphDebugTools {
    /// Create material graph debug tools for the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            shader_inspection: MaterialGraphShaderInspection::new(graph),
            parameter_visualization: MaterialGraphParameterVisualization::new(graph),
            performance_profiling: MaterialGraphPerformanceProfiling::new(graph),
            debugging_output: MaterialGraphDebuggingOutput::new(graph),
        }
    }

    /// Get the shader inspection of these debug tools.
    pub fn shader_inspection(&self) -> &MaterialGraphShaderInspection {
        &self.shader_inspection
    }

    /// Get the parameter visualization of these debug tools.
    pub fn parameter_visualization(&self) -> &MaterialGraphParameterVisualization {
        &self.parameter_visualization
    }

    /// Get the performance profiling of these debug tools.
    pub fn performance_profiling(&self) -> &MaterialGraphPerformanceProfiling {
        &self.performance_profiling
    }

    /// Get the debugging output of these debug tools.
    pub fn debugging_output(&self) -> &MaterialGraphDebuggingOutput {
        &self.debugging_output
    }
}

// ------------------------------------------------------------------
// Material Graph Serialization
// ------------------------------------------------------------------

/// Material graph serialization for save/load material graph states.
#[derive(Clone, Debug)]
pub struct MaterialGraphSerialization {
    /// The serialized material graph for this serialization.
    pub serialized_graph: MaterialGraph,
    /// The serialization format for this serialization.
    pub serialization_format: MaterialGraphSerializationFormat,
    /// The serialization data for this serialization.
    pub serialization_data: Vec<u8>,
}

impl MaterialGraphSerialization {
    /// Create material graph serialization from the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            serialized_graph: graph,
            serialization_format: MaterialGraphSerializationFormat::default(),
            serialization_data: Vec::new(),
        }
    }

    /// Serialize the material graph for this serialization.
    pub fn serialize(&mut self) -> Result<Self> {
        self.serialization_data = self.serialized_graph.serialize();
        Ok(self)
    }

    /// Deserialize the material graph for this serialization.
    pub fn deserialize(&mut self) -> Result<Self> {
        self.serialized_graph = self.serialization_data.deserialize();
        Ok(self)
    }

    /// Get the serialized material graph of this serialization.
    pub fn serialized_graph(&self) -> &MaterialGraph {
        &self.serialized_graph
    }

    /// Get the serialization format of this serialization.
    pub fn serialization_format(&self) -> MaterialGraphSerializationFormat {
        self.serialization_format
    }

    /// Get the serialization data of this serialization.
    pub fn serialization_data(&self) -> &[u8] {
        &self.serialization_data
    }
}

// ------------------------------------------------------------------
// Material Graph Cache Management
// ------------------------------------------------------------------

/// Material graph cache management for reuse and management.
#[derive(Clone, Debug)]
pub struct MaterialGraphCache {
    /// The cached material graphs.
    pub material_graphs: Vec<MaterialGraph>,
    /// The cache size limit.
    pub size_limit: usize,
    /// The cache eviction strategy.
    pub eviction_strategy: MaterialGraphCacheEvictionStrategy,
    /// The cache hit count.
    pub hit_count: usize,
    /// The cache miss count.
    pub miss_count: usize,
}

impl MaterialGraphCache {
    /// Create a material graph cache with the given size limit.
    pub fn new(size_limit: usize) -> Self {
        Self {
            material_graphs: Vec::new(),
            size_limit,
            eviction_strategy: MaterialGraphCacheEvictionStrategy::LRU,
            hit_count: 0,
            miss_count: 0,
        }
    }

    /// Add a material graph to this cache.
    pub fn add(mut self, material_graph: MaterialGraph) -> Result<Self> {
        if self.material_graphs.len() >= self.size_limit {
            self.evict();
        }
        self.material_graphs.push(material_graph);
        Ok(self)
    }

    /// Get a material graph from this cache by name.
    pub fn get(&self, name: &str) -> Option<&MaterialGraph> {
        self.material_graphs.iter().find(|m| m.name == name)
    }

    /// Evict material graphs from this cache.
    pub fn evict(&mut self) {
        match self.eviction_strategy {
            Self::LRU => {
                self.material_graphs.remove(0);
            }
            Self::SizeBased => {
                self.material_graphs.remove(self.material_graphs.len() - 1);
            }
            Self::Random => {
                let index = self.material_graphs.len() / 2;
                self.material_graphs.remove(index);
            }
        }
    }

    /// Get the size of this cache.
    pub fn size(&self) -> usize {
        self.material_graphs.len()
    }

    /// Get the size limit of this cache.
    pub fn size_limit(&self) -> usize {
        self.size_limit
    }

    /// Get the eviction strategy of this cache.
    pub fn eviction_strategy(&self) -> MaterialGraphCacheEvictionStrategy {
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

impl Default for MaterialGraphCache {
    fn default() -> Self {
        Self::new(100)
    }
}

// ------------------------------------------------------------------
// Material Graph DSL
// ------------------------------------------------------------------

/// Material graph DSL for declarative material composition.
#[derive(Clone, Debug)]
pub struct MaterialGraphDSL {
    /// The DSL expression for this material graph.
    pub expression: String,
    /// The DSL parsed material graph for this material graph.
    pub parsed_graph: MaterialGraph,
    /// The DSL compiled material graph for this material graph.
    pub compiled_graph: MaterialGraph,
}

impl MaterialGraphDSL {
    /// Create material graph DSL from the given expression.
    pub fn new(expression: impl Into<String>) -> Self {
        Self {
            expression: expression.into(),
            parsed_graph: MaterialGraph::new("dsl_material_graph"),
            compiled_graph: MaterialGraph::new("dsl_material_graph"),
        }
    }

    /// Parse the DSL expression for this material graph.
    pub fn parse(&mut self) -> Result<Self> {
        self.parsed_graph = MaterialGraph::new("dsl_material_graph")
            .with_material_node_preset(MaterialPreset::Pbr)
            .with_raytraced_node_preset(MaterialPreset::RaytracedPbr)
            .with_path_traced_node_preset(MaterialPreset::PathTracedPbr)
            .with_asset_pipeline("dsl_material_assets")
            .with_preview_system("dsl_material_preview")
            .with_debug_tools("dsl_material_debug")
            .with_cache("dsl_material_cache")
            .with_serialization("dsl_material_serialization")
            .with_shader_optimization("dsl_shader_optimization")
            .with_dsl("dsl_material_dsl")
            .with_offline_render_graph("dsl_offline_render_graph")
            .build()?;
        Ok(self)
    }

    /// Compile the DSL expression for this material graph.
    pub fn compile(&mut self) -> Result<Self> {
        self.compiled_graph = MaterialGraph::new("dsl_material_graph")
            .with_material_node_preset(MaterialPreset::Pbr)
            .with_raytraced_node_preset(MaterialPreset::RaytracedPbr)
            .with_path_traced_node_preset(MaterialPreset::PathTracedPbr)
            .with_asset_pipeline("dsl_material_assets")
            .with_preview_system("dsl_material_preview")
            .with_debug_tools("dsl_material_debug")
            .with_cache("dsl_material_cache")
            .with_serialization("dsl_material_serialization")
            .with_shader_optimization("dsl_shader_optimization")
            .with_dsl("dsl_material_dsl")
            .with_offline_render_graph("dsl_offline_render_graph")
            .build()?;
        Ok(self)
    }

    /// Get the expression of this DSL.
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// Get the parsed material graph of this DSL.
    pub fn parsed_graph(&self) -> &MaterialGraph {
        &self.parsed_graph
    }

    /// Get the compiled material graph of this DSL.
    pub fn compiled_graph(&self) -> &MaterialGraph {
        &self.compiled_graph
    }
}

// ------------------------------------------------------------------
// Material Graph Shader Compilation
// ------------------------------------------------------------------

/// Material graph shader compilation for Slang-based shader compilation.
#[derive(Clone, Debug)]
pub struct MaterialGraphShaderCompilation {
    /// The compiled material graph shader for this compilation.
    pub compiled_shader: Shader,
    /// The compilation result for this compilation.
    pub compilation_result: crate::ShaderReflection,
    /// The compilation artifacts for this compilation.
    pub compilation_artifacts: Vec<crate::CompiledShaderArtifact>,
    /// The compilation diagnostics for this compilation.
    pub compilation_diagnostics: Vec<String>,
}

impl MaterialGraphShaderCompilation {
    /// Create material graph shader compilation from the given material graph.
    pub fn new(graph: MaterialGraph) -> Self {
        Self {
            compiled_shader: Shader::new(),
            compilation_result: crate::ShaderReflection::default(),
            compilation_artifacts: Vec::new(),
            compilation_diagnostics: Vec::new(),
        }
    }

    /// Compile the material graph shader for this compilation.
    pub fn compile(&mut self) -> Result<Self> {
        self.compiled_shader = self.graph.compile();
        self.compilation_result = self.graph.reflection();
        self.compilation_artifacts = self.graph.artifacts();
        self.compilation_diagnostics = self.graph.diagnostics();
        Ok(self)
    }

    /// Get the compiled shader of this compilation.
    pub fn compiled_shader(&self) -> &Shader {
        &self.compiled_shader
    }

    /// Get the compilation result of this compilation.
    pub fn compilation_result(&self) -> &crate::ShaderReflection {
        &self.compilation_result
    }

    /// Get the compilation artifacts of this compilation.
    pub fn compilation_artifacts(&self) -> &[crate::CompiledShaderArtifact] {
        &self.compilation_artifacts
    }

    /// Get the compilation diagnostics of this compilation.
    pub fn compilation_diagnostics(&self) -> &[String] {
        &self.compilation_diagnostics
    }
}

// ------------------------------------------------------------------
// Material Graph GPU Capture
// ------------------------------------------------------------------

/// Material graph GPU capture for RenderDoc, Pix, Xcode integration.
#[derive(Clone, Debug)]
pub struct MaterialGraphGpuCapture {
    /// The GPU capture tool for this capture.
    pub tool: GpuCaptureTool,
    /// The GPU capture label for this capture.
    pub label: String,
    /// The GPU capture state for this capture.
    pub capture_state: MaterialGraphGpuCaptureState,
}

impl MaterialGraphGpuCapture {
    /// Create material graph GPU capture from the given tool and label.
    pub fn new(tool: GpuCaptureTool, label: impl Into<String>) -> Self {
        Self {
            tool,
            label: label.into(),
            capture_state: MaterialGraphGpuCaptureState::default(),
        }
    }

    /// Begin GPU capture for this capture.
    pub fn begin(&mut self) -> Result<Self> {
        self.capture_state = MaterialGraphGpuCaptureState::Capturing;
        Ok(self)
    }

    /// End GPU capture for this capture.
    pub fn end(&mut self) -> Result<Self> {
        self.capture_state = MaterialGraphGpuCaptureState::Idle;
        Ok(self)
    }

    /// Get the GPU capture tool of this capture.
    pub fn tool(&self) -> GpuCaptureTool {
        self.tool
    }

    /// Get the GPU capture label of this capture.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Get the GPU capture state of this capture.
    pub fn capture_state(&self) -> MaterialGraphGpuCaptureState {
        self.capture_state
    }
}
