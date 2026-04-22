//! Scene system module for the sturdy-engine.
//!
//! This module provides the scene, camera, material, and object types that
//! work above the backend abstraction layer. All components are backend-neutral
//! and work across Vulkan, D3D12, Metal, and other graphics APIs.
//!
//! # Scene System
//!
//! The [`Scene`] type manages meshes, object instances, and cameras. It provides
//! automatic instance buffer management with static/dynamic split for efficient
//! per-frame updates.
//!
//! # Camera System
//!
//! The [`SceneCamera`] type provides view/projection matrices and output targets.
//! Cameras can render to persistent [`RenderTarget`] images or to frame-managed
//! images.
//!
//! # Material System
//!
//! The [`Material`] type defines how objects are rendered with shader programs,
//! parameter bindings, rendering state, and format capabilities. Materials are
//! rendering-mode-agnostic and work across rasterized, hybrid, raytraced, and
//! path traced rendering.
//!
//! # Workflow
//!
//! ```rust
//! // At init:
//! let scene = Scene::new();
//! let mat = Material::new("pbr_material").build(engine)?;
//!
//! // At render:
//! scene.add_mesh(mesh, program);
//! scene.add_object(mesh_id, Mat4::IDENTITY, ObjectKind::Static);
//! scene.add_camera(SceneCamera::offscreen(view, proj, crt_target));
//! scene.prepare(engine)?;
//! scene.render(frame)?;
//! ```
//!
//! # Rendering Mode Support
//!
//! All scene components are designed to work across all rendering modes without
//! breaking down:
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
//!
//! # Text Rendering
//!
//! Text rendering is managed via the crate root `text_engine` module.
//! Scene components access text via `crate::text_engine` and `crate::text_draw` exports.

mod batch;
mod camera;
mod material;
mod material_asset;
mod material_graph;
mod object;
mod procedural_material;
mod render_target;
mod scene;

pub use camera::{CameraId, CameraOutput, SceneCamera};
pub use object::{InstanceData, MeshId, ObjectId, ObjectKind, SceneObject};
pub use render_target::RenderTarget;
pub use scene::{CameraConstants, Scene};

// ------------------------------------------------------------------
// Material System Exports
// ------------------------------------------------------------------

pub use material::{
    AccelerationStructureBindingRegistration, BufferBindingRegistration, FormatCapabilities,
    Material, MaterialBlendMode, MaterialBuilder, MaterialCache, MaterialComposition,
    MaterialLayeringOrder, MaterialPreset, PathTracedBounceConfig, PathTracedImportanceSampling,
    PathTracedProgram, PathTracedTerminationStrategy, PathTracingCapabilities,
    PushConstantRegistration, RaytracedProgram, RaytracedShaderStage, RaytracedStageRegistration,
    RaytracingCapabilities, RenderState, TextureBindingRegistration,
};

// ------------------------------------------------------------------
// Material Asset Pipeline Exports
// ------------------------------------------------------------------

pub use material_asset::{
    MaterialAsset, MaterialAssetCache, MaterialAssetFormat, MaterialAssetLoader,
    MaterialAssetMetadata, MaterialAssetPipeline, MaterialAssetPipelineState,
    MaterialAssetValidator,
};

// ------------------------------------------------------------------
// Material Graph Exports
// ------------------------------------------------------------------

pub use material_graph::{
    MaterialGraph, MaterialGraphBlend, MaterialGraphComposition, MaterialGraphEdge,
    MaterialGraphLayer, MaterialGraphMix, MaterialGraphNode,
};

// ------------------------------------------------------------------
// Procedural Material Exports
// ------------------------------------------------------------------

pub use procedural_material::ProceduralMaterial;

// ------------------------------------------------------------------
// Text Rendering System Exports
// ------------------------------------------------------------------

// Text rendering is managed via the crate root `text_engine` module.
// Scene components access text via `crate::text_engine` and `crate::text_draw` exports.
use crate::text_draw::{TextGlyphQuad, TextLayoutOutput, TextRenderer};
use crate::text_engine::{TextAtlasPage, TextDrawDesc, TextEngine};
