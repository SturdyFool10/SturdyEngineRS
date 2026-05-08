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

use std::sync::Arc;

use crate::{
    Engine, Image, Mesh, Result,
    MaterialDomain, ShadingModel, UnifiedMaterial, UnifiedMaterialBuilder,
    scene::MaterialDescriptor,
};

// ── Common types ──────────────────────────────────────────────────────────────

/// GPU textures extracted from a mesh file's material.
///
/// All fields are `Option<Arc<Image>>` — a `None` means the source material
/// had no texture for that channel; the caller should supply a default or
/// use a constant factor from [`MeshMaterialParams`] instead.
///
/// Textures are wrapped in `Arc` so multiple primitives sharing the same
/// source image (common in GLTF files) can hold a reference without
/// duplicating GPU memory.
#[derive(Default)]
pub struct MeshTextures {
    /// sRGB base color / albedo map. Multiply by `base_color_factor`.
    pub base_color: Option<Arc<Image>>,
    /// ORM or metallic-roughness map. GLTF convention: B = metallic, G = roughness.
    pub metallic_roughness: Option<Arc<Image>>,
    /// Tangent-space normal map (OpenGL convention: +Y = up).
    pub normal: Option<Arc<Image>>,
    /// Ambient occlusion map (R channel).
    pub occlusion: Option<Arc<Image>>,
    /// Linear-HDR emissive map. Multiply by `emissive_factor * emissive_strength`.
    pub emissive: Option<Arc<Image>>,
    // ── KHR_materials_clearcoat ───────────────────────────────────────────────
    /// Clearcoat intensity map (R channel). Multiply by `clearcoat_factor`.
    pub clearcoat: Option<Arc<Image>>,
    /// Clearcoat roughness map (G channel). Multiply by `clearcoat_roughness_factor`.
    pub clearcoat_roughness: Option<Arc<Image>>,
    /// Clearcoat tangent-space normal map.
    pub clearcoat_normal: Option<Arc<Image>>,
    // ── KHR_materials_transmission ────────────────────────────────────────────
    /// Transmission weight map (R channel). Multiply by `transmission_factor`.
    pub transmission: Option<Arc<Image>>,
    // ── KHR_materials_sheen ───────────────────────────────────────────────────
    /// Sheen color map (RGB). Multiply by `sheen_color_factor`.
    pub sheen_color: Option<Arc<Image>>,
    /// Sheen roughness map (A channel). Multiply by `sheen_roughness_factor`.
    pub sheen_roughness: Option<Arc<Image>>,
}

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
    /// GPU textures extracted from the file's material (may all be `None` for
    /// formats without texture support or when images failed to load).
    pub textures: MeshTextures,
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

    // ── KHR_materials_clearcoat ───────────────────────────────────────────────
    /// Clearcoat layer intensity `[0, 1]`. Non-zero → `ShadingModel::PbrClearcoat`.
    pub clearcoat_factor: f32,
    /// Clearcoat layer roughness `[0, 1]`.
    pub clearcoat_roughness_factor: f32,

    // ── KHR_materials_transmission ────────────────────────────────────────────
    /// Fraction of light transmitted through the surface `[0, 1]`.
    /// Non-zero → `MaterialDomain::Translucent` / `ShadingModel::PbrTransmission`.
    pub transmission_factor: f32,

    // ── KHR_materials_ior ─────────────────────────────────────────────────────
    /// Index of refraction. Default `1.5` (glass). Affects F0: `((ior-1)/(ior+1))²`.
    pub ior: f32,

    // ── KHR_materials_sheen ───────────────────────────────────────────────────
    /// Sheen color (linear RGB). Non-zero → `ShadingModel::PbrSubsurface` (sheen mode).
    pub sheen_color_factor: [f32; 3],
    /// Sheen roughness `[0, 1]`.
    pub sheen_roughness_factor: f32,
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
            clearcoat_factor: 0.0,
            clearcoat_roughness_factor: 0.0,
            transmission_factor: 0.0,
            ior: 1.5,
            sheen_color_factor: [0.0, 0.0, 0.0],
            sheen_roughness_factor: 0.0,
        }
    }
}

impl MeshMaterialParams {
    /// Convert to a `UnifiedMaterial` for use with `DeferredPass`.
    ///
    /// Shading model selection:
    /// - `unlit = true` → `ShadingModel::Unlit`
    /// - `clearcoat_factor > 0` → `ShadingModel::PbrClearcoat`
    /// - `transmission_factor > 0` → `ShadingModel::PbrTransmission`
    /// - otherwise → `ShadingModel::PbrMetallicRoughness`
    ///
    /// Material domain:
    /// - Transmission with factor > 0 → `Translucent` (overrides alpha_mode)
    /// - Otherwise follows `alpha_mode` as before
    pub fn to_unified_material(&self, name: impl Into<String>) -> UnifiedMaterial {
        let shading = if self.unlit {
            ShadingModel::Unlit
        } else if self.clearcoat_factor > 0.0 {
            ShadingModel::PbrClearcoat
        } else if self.transmission_factor > 0.0 {
            ShadingModel::PbrTransmission
        } else {
            ShadingModel::PbrMetallicRoughness
        };

        let domain = if self.transmission_factor > 0.0 {
            // Transmissive materials go through the translucent (OIT) path.
            MaterialDomain::Translucent
        } else {
            match self.alpha_mode {
                MeshAlphaMode::Opaque => MaterialDomain::Opaque,
                MeshAlphaMode::Mask   => MaterialDomain::Masked,
                MeshAlphaMode::Blend  => MaterialDomain::Translucent,
            }
        };

        // Compute F0 from IOR: F0 = ((ior - 1) / (ior + 1))²
        // Standard dielectric: IOR 1.5 → F0 ≈ 0.04 (matches the shader default).
        let _f0 = ((self.ior - 1.0) / (self.ior + 1.0)).powi(2);

        let e = self.emissive_factor;
        let s = self.emissive_strength;

        let mut builder = UnifiedMaterialBuilder::new(name)
            .shading_model(shading)
            .domain(domain)
            .base_color_constant(self.base_color_factor)
            .metallic_roughness_constants(self.metallic_factor, self.roughness_factor)
            .emissive_constant([e[0] * s, e[1] * s, e[2] * s]);

        if self.clearcoat_factor > 0.0 {
            builder = builder.clearcoat(self.clearcoat_factor, self.clearcoat_roughness_factor);
        }

        builder.build()
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
            has_normal_map: false,
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
