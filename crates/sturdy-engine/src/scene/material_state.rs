use std::sync::Arc;

use glam::Vec3;

use crate::{Buffer, Image};

/// Per-mesh material parameters for the built-in lit shader.
///
/// Set via [`Scene::set_material`](super::Scene::set_material) after calling
/// [`Scene::add_mesh`](super::Scene::add_mesh). The default is a white, fully
/// opaque, mid-roughness dielectric (no metallic, no emission).
///
/// # In the shader
///
/// `lit_fragment.slang` reads a `StructuredBuffer<MaterialConstants> material_desc`
/// buffer that `Scene::draw` binds automatically. Custom fragment shaders can
/// declare the same binding to receive these values.
#[derive(Clone, Debug)]
pub struct MaterialDescriptor {
    /// Base colour (linear RGB). Multiplied with the lighting result.
    pub albedo: Vec3,
    /// Opacity in [0, 1]. Values below 1.0 require an alpha-blend program.
    pub opacity: f32,
    /// Self-emission added after lighting (HDR values are valid).
    pub emissive: Vec3,
    /// Metallic factor in [0, 1]. 0 = dielectric, 1 = conductor.
    pub metallic: f32,
    /// Surface roughness in [0, 1]. 0 = mirror, 1 = fully diffuse.
    pub roughness: f32,
    /// Whether a tangent-space normal map is bound for this mesh.
    ///
    /// Set automatically by `Scene::set_normal_map`. When `true`,
    /// `gbuffer_fragment.slang` applies a TBN transform using the bound
    /// `"normal_map"` texture; when `false`, the geometric normal is used.
    pub has_normal_map: bool,
}

impl Default for MaterialDescriptor {
    fn default() -> Self {
        Self {
            albedo: Vec3::ONE,
            opacity: 1.0,
            emissive: Vec3::ZERO,
            metallic: 0.0,
            roughness: 0.5,
            has_normal_map: false,
        }
    }
}

/// GPU-layout mirror of `MaterialConstants` in the lit and G-Buffer shaders.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct MaterialConstants {
    /// xyz = albedo, w = opacity
    albedo: [f32; 4],
    /// xyz = emissive, w = metallic
    emissive_metallic: [f32; 4],
    /// x = roughness, y = has_normal_map (1.0 = yes), zw = unused
    roughness_flags: [f32; 4],
}

impl MaterialConstants {
    pub(super) fn from_descriptor(desc: &MaterialDescriptor) -> Self {
        Self {
            albedo: [desc.albedo.x, desc.albedo.y, desc.albedo.z, desc.opacity],
            emissive_metallic: [
                desc.emissive.x,
                desc.emissive.y,
                desc.emissive.z,
                desc.metallic,
            ],
            roughness_flags: [
                desc.roughness,
                if desc.has_normal_map { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ],
        }
    }
}

/// Per-mesh material state owned by the scene.
pub(super) struct MeshMaterial {
    pub(super) descriptor: MaterialDescriptor,
    pub(super) gpu_buffer: Option<Buffer>,
    pub(super) dirty: bool,
    /// Optional tangent-space normal map for the G-Buffer fill pass.
    ///
    /// Set via `Scene::set_normal_map`. When present, it is bound as
    /// `"normal_map"` before each G-Buffer draw call for this mesh.
    pub(super) normal_map: Option<Arc<Image>>,
    /// Full expression-based material for the deferred G-Buffer path.
    ///
    /// When set, `DeferredPass` compiles a unique G-Buffer fragment shader
    /// variant from this material's expression tree and uses it instead of the
    /// default G-Buffer program. The `descriptor` above is still used by the
    /// forward lit path and for `set_material` compatibility.
    pub(super) unified: Option<super::material::UnifiedMaterial>,
}
