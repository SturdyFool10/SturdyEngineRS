// Unified mesh loading — dispatches to the format-specific loader by file extension.
//
// Supported formats:
//   .gltf / .glb   — GLTF 2.0 (primary; richest material support)
//   .obj            — Wavefront OBJ + MTL (universal fallback; all tools export it)
//   .stl            — STereoLithography (binary + ASCII; engineering / photogrammetry)
//
// All formats produce `Vec<MeshPrimitive>` so downstream code is format-agnostic.
//
// Roadmap: Track 3 — Asset loading.

use std::path::Path;

use crate::{
    Engine, Mesh, Result,
    MaterialDomain, ShadingModel, UnifiedMaterial, UnifiedMaterialBuilder,
    scene::MaterialDescriptor,
};

// ── Common types ──────────────────────────────────────────────────────────────

/// One rasterizable surface from any mesh file format.
///
/// A single file may contain multiple primitives (GLTF multi-primitive meshes,
/// OBJ groups, STL solid bodies). `load_mesh` always returns a flat list —
/// one `MeshPrimitive` per draw-call-worthy piece of geometry.
///
/// `MeshPrimitive` is not `Clone` because `Mesh` owns its GPU buffers.
/// Move each primitive into the scene exactly once.
///
/// # Usage
/// ```ignore
/// for prim in engine.load_mesh("assets/helmet.glb")? {
///     let id = scene.add_mesh(prim.mesh, MeshProgram::lit(&engine)?);
///     scene.set_material(id, prim.material_params.to_material_descriptor());
/// }
/// ```
pub struct MeshPrimitive {
    /// GPU-resident vertex + index buffers with pre-computed bounding sphere.
    pub mesh: Mesh,
    /// Human-readable name from the source file.
    pub name: String,
    /// PBR material parameters extracted from the file (or sensible defaults).
    pub material_params: MeshMaterialParams,
}

/// PBR material parameters that work regardless of source format.
///
/// Fields map to GLTF 2.0 metallic-roughness semantics. Other formats are
/// approximated: OBJ diffuse → base_color, OBJ dissolve → opacity, etc.
#[derive(Clone, Debug)]
pub struct MeshMaterialParams {
    /// Base colour factor (linear RGBA). Default `[1, 1, 1, 1]`.
    pub base_color_factor: [f32; 4],
    /// Metallic factor `[0, 1]`. Default `0.0` (dielectric).
    pub metallic_factor: f32,
    /// Perceptual roughness factor `[0, 1]`. Default `0.5`.
    pub roughness_factor: f32,
    /// Emissive radiance factor (linear RGB). Default `[0, 0, 0]`.
    pub emissive_factor: [f32; 3],
    /// Emissive strength multiplier (KHR_materials_emissive_strength). Default `1.0`.
    pub emissive_strength: f32,
    /// Whether the surface is double-sided.
    pub double_sided: bool,
    /// Blending mode.
    pub alpha_mode: MeshAlphaMode,
    /// Alpha threshold for `Mask` mode. Default `0.5`.
    pub alpha_cutoff: f32,
    /// True when the material ignores all lighting (KHR_materials_unlit or vertex-only).
    pub unlit: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshAlphaMode {
    Opaque,
    Mask,
    Blend,
}

impl Default for MeshMaterialParams {
    fn default() -> Self {
        Self {
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            metallic_factor: 0.0,
            roughness_factor: 0.5,
            emissive_factor: [0.0, 0.0, 0.0],
            emissive_strength: 1.0,
            double_sided: false,
            alpha_mode: MeshAlphaMode::Opaque,
            alpha_cutoff: 0.5,
            unlit: false,
        }
    }
}

impl MeshMaterialParams {
    /// Convert to a `UnifiedMaterial` for use with `DeferredPass`.
    pub fn to_unified_material(&self, name: impl Into<String>) -> UnifiedMaterial {
        let shading = if self.unlit { ShadingModel::Unlit } else { ShadingModel::PbrMetallicRoughness };
        let domain = match self.alpha_mode {
            MeshAlphaMode::Opaque => MaterialDomain::Opaque,
            MeshAlphaMode::Mask => MaterialDomain::Masked,
            MeshAlphaMode::Blend => MaterialDomain::Translucent,
        };
        let e = self.emissive_factor;
        let s = self.emissive_strength;
        UnifiedMaterialBuilder::new(name)
            .shading_model(shading)
            .domain(domain)
            .base_color_constant(self.base_color_factor)
            .metallic_roughness_constants(self.metallic_factor, self.roughness_factor)
            .emissive_constant([e[0] * s, e[1] * s, e[2] * s])
            .build()
    }

    /// Convert to the `MaterialDescriptor` for `Scene::set_material` (forward path).
    pub fn to_material_descriptor(&self) -> MaterialDescriptor {
        let e = self.emissive_factor;
        let s = self.emissive_strength;
        MaterialDescriptor {
            albedo: glam::Vec3::new(
                self.base_color_factor[0],
                self.base_color_factor[1],
                self.base_color_factor[2],
            ),
            opacity: self.base_color_factor[3],
            emissive: glam::Vec3::new(e[0] * s, e[1] * s, e[2] * s),
            metallic: self.metallic_factor,
            roughness: self.roughness_factor,
        }
    }
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

/// Load a mesh file by dispatching on the file extension.
///
/// | Extension        | Format              | Loader     |
/// |-----------------|---------------------|------------|
/// | `.gltf` `.glb`  | GLTF 2.0            | gltf_loader|
/// | `.obj`          | Wavefront OBJ + MTL | obj_loader |
/// | `.stl`          | STereoLithography   | stl_loader |
pub(crate) fn load_mesh_from_path(engine: &Engine, path: &Path) -> Result<Vec<MeshPrimitive>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "gltf" | "glb" => crate::gltf_loader::load(engine, path),
        "obj"          => crate::obj_loader::load(engine, path),
        "stl"          => crate::stl_loader::load(engine, path),
        other => Err(crate::Error::Unknown(format!(
            "load_mesh: unsupported file extension '.{other}' in '{}'. \
             Supported: .gltf, .glb, .obj, .stl",
            path.display()
        ))),
    }
}
