// Unified material system for the deferred PBR pipeline.
//
// A `UnifiedMaterial` defines a `MaterialSurface` evaluation function once;
// the engine derives all required shader variants automatically:
//
//   UnifiedMaterial
//     ├─ GBufferFill    — writes base_color/metallic/normal/roughness/occlusion/emissive
//     ├─ ForwardLit     — one-pass lit (transparent objects, forward fallback)
//     ├─ Shadow         — depth-only (directional CSM, point, spot)
//     ├─ RtAnyHit       — opacity test for masked geometry in RT traversal
//     ├─ RtClosestHit   — full surface for RT shadows, reflections, GI
//     └─ PathTraced     — full BSDF for offline reference renders
//
// All backends (raster deferred, raster forward, RT, path-traced) share the
// same `UnifiedMaterial` definition; no re-authoring is needed when switching.
//
// Roadmap: Track 6.

// ── Rendering state ───────────────────────────────────────────────────────────

/// Per-material rasterization configuration.
#[derive(Clone, Debug)]
pub struct RenderState {
    pub cull_mode: crate::CullMode,
    pub front_face: crate::FrontFace,
    pub depth_write: bool,
    pub depth_compare: Option<crate::CompareOp>,
    pub topology: crate::PrimitiveTopology,
    pub raster: crate::RasterState,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            cull_mode: crate::CullMode::Back,
            front_face: crate::FrontFace::CounterClockwise,
            depth_write: true,
            depth_compare: Some(crate::CompareOp::Less),
            topology: crate::PrimitiveTopology::TriangleList,
            raster: crate::RasterState::default(),
        }
    }
}

impl RenderState {
    pub fn with_cull_mode(mut self, cull: crate::CullMode) -> Self {
        self.cull_mode = cull;
        self
    }

    pub fn with_front_face(mut self, face: crate::FrontFace) -> Self {
        self.front_face = face;
        self
    }

    pub fn with_depth_write(mut self, write: bool) -> Self {
        self.depth_write = write;
        self
    }

    pub fn with_depth_compare(mut self, compare: crate::CompareOp) -> Self {
        self.depth_compare = Some(compare);
        self
    }

    pub fn with_topology(mut self, topology: crate::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }
}

// ── Material domain and shading model ─────────────────────────────────────────

/// Blending and depth-write behaviour of a material surface.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum MaterialDomain {
    /// Depth-tested, depth-written; rendered via the deferred G-Buffer fill path.
    #[default]
    Opaque,
    /// Like `Opaque` but discards pixels whose `opacity < UnifiedMaterial::ALPHA_CUTOFF`
    /// in the shadow pass and RT any-hit shader.
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
    /// Lambertian diffuse — legacy fallback for non-PBR assets.
    Lambert,
    /// GGX metallic-roughness BRDF — standard GLTF 2.0 workflow.
    /// Energy-compensated via a precomputed BRDF integration LUT.
    #[default]
    PbrMetallicRoughness,
    /// GGX metallic-roughness with a clear-coat layer (GLTF `KHR_materials_clearcoat`).
    PbrClearcoat,
    /// Screen-space subsurface scattering for skin and organic materials.
    PbrSubsurface,
    /// Transmission and refraction for glass and liquids
    /// (GLTF `KHR_materials_transmission` + `KHR_materials_volume`).
    PbrTransmission,
}

// ── Material inputs ───────────────────────────────────────────────────────────

/// One input channel of a [`UnifiedMaterial`].
///
/// Each `MaterialSurface` field may come from a constant, a texture sampled
/// at UV0, or a texture modulated by a constant factor (the GLTF convention
/// for e.g. `baseColorFactor * baseColorTexture`).
#[derive(Clone, Debug)]
pub enum MaterialInput<T: Clone> {
    Constant(T),
    Texture(String),
    TextureTimesConstant { texture: String, factor: T },
}

impl<T: Clone + Default> Default for MaterialInput<T> {
    fn default() -> Self {
        Self::Constant(T::default())
    }
}

// ── UnifiedMaterial ───────────────────────────────────────────────────────────

/// A rendering-path-agnostic material definition.
///
/// The user either fills in the standard PBR parameter inputs (asset-driven
/// workflow) or supplies a Slang snippet implementing `evaluate_material`.
/// The [`MaterialVariantCompiler`] (Track 6b) derives every shader variant from
/// that single definition — no re-authoring when switching rendering backends.
///
/// # Standard PBR workflow (GLTF)
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
    pub name: String,
    pub domain: MaterialDomain,
    pub shading_model: ShadingModel,
    pub render_state: RenderState,

    // Standard PBR inputs — each may be a constant, texture, or texture×constant.
    pub base_color: MaterialInput<[f32; 4]>,
    pub metallic: MaterialInput<f32>,
    pub roughness: MaterialInput<f32>,
    /// Tangent-space normal map binding name. `None` uses the geometric normal.
    pub normal_map: Option<String>,
    pub occlusion: MaterialInput<f32>,
    /// Linear HDR emissive radiance (unclamped — drives bloom).
    pub emissive: MaterialInput<[f32; 3]>,

    // Clear-coat layer (only evaluated when shading_model == PbrClearcoat).
    pub clearcoat: MaterialInput<f32>,
    pub clearcoat_roughness: MaterialInput<f32>,

    /// Optional Slang function body for `evaluate_material(VertexData)`.
    /// When `Some`, all structured inputs above are ignored.
    pub evaluate_material_snippet: Option<String>,
}

impl UnifiedMaterial {
    /// Pixels with `opacity < ALPHA_CUTOFF` are discarded in `Masked` domain passes.
    pub const ALPHA_CUTOFF: f32 = 0.5;

    pub fn pbr_metallic_roughness(name: impl Into<String>) -> UnifiedMaterialBuilder {
        UnifiedMaterialBuilder::new(name).shading_model(ShadingModel::PbrMetallicRoughness)
    }

    pub fn unlit(name: impl Into<String>) -> UnifiedMaterialBuilder {
        UnifiedMaterialBuilder::new(name).shading_model(ShadingModel::Unlit)
    }

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

// ── UnifiedMaterialBuilder ────────────────────────────────────────────────────

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

    pub fn base_color_constant(mut self, rgba: [f32; 4]) -> Self {
        self.inner.base_color = MaterialInput::Constant(rgba);
        self
    }

    pub fn base_color_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.base_color = MaterialInput::Texture(binding.into());
        self
    }

    pub fn base_color_texture_factor(mut self, binding: impl Into<String>, factor: [f32; 4]) -> Self {
        self.inner.base_color = MaterialInput::TextureTimesConstant { texture: binding.into(), factor };
        self
    }

    pub fn metallic_constant(mut self, v: f32) -> Self {
        self.inner.metallic = MaterialInput::Constant(v);
        self
    }

    pub fn roughness_constant(mut self, v: f32) -> Self {
        self.inner.roughness = MaterialInput::Constant(v);
        self
    }

    /// GLTF convention: B channel = metallic, G channel = roughness.
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

    pub fn normal_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.normal_map = Some(binding.into());
        self
    }

    pub fn occlusion_constant(mut self, v: f32) -> Self {
        self.inner.occlusion = MaterialInput::Constant(v);
        self
    }

    pub fn occlusion_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.occlusion = MaterialInput::Texture(binding.into());
        self
    }

    pub fn emissive_constant(mut self, rgb: [f32; 3]) -> Self {
        self.inner.emissive = MaterialInput::Constant(rgb);
        self
    }

    pub fn emissive_texture(mut self, binding: impl Into<String>) -> Self {
        self.inner.emissive = MaterialInput::Texture(binding.into());
        self
    }

    pub fn emissive_texture_factor(mut self, binding: impl Into<String>, factor: [f32; 3]) -> Self {
        self.inner.emissive = MaterialInput::TextureTimesConstant { texture: binding.into(), factor };
        self
    }

    pub fn clearcoat(mut self, intensity: f32, roughness: f32) -> Self {
        self.inner.shading_model = ShadingModel::PbrClearcoat;
        self.inner.clearcoat = MaterialInput::Constant(intensity);
        self.inner.clearcoat_roughness = MaterialInput::Constant(roughness);
        self
    }

    pub fn evaluate_material_fn(mut self, snippet: impl Into<String>) -> Self {
        self.inner.evaluate_material_snippet = Some(snippet.into());
        self
    }

    pub fn build(self) -> UnifiedMaterial {
        self.inner
    }
}

// ── G-Buffer layout constants ─────────────────────────────────────────────────

/// Standard G-Buffer attachment slots and formats for the deferred PBR pipeline.
///
/// All passes that read or write G-Buffer data must use these constants so
/// attachment indices and formats are consistent across the frame graph.
///
/// ```text
/// G0  RGBA8Unorm   base_color.rgb (linear) | metallic
/// G1  RGBA16Float  world-normal.xy (oct-encoded) | roughness | occlusion
/// G2  RGBA16Float  emissive.rgb (linear HDR, unclamped) | shading_model_id
/// D   Depth32Float hardware depth
/// ```
pub mod gbuffer {
    use crate::Format;

    pub const SLOT_BASE_COLOR_METALLIC: u32 = 0;
    pub const SLOT_NORMAL_ROUGHNESS_OCCLUSION: u32 = 1;
    pub const SLOT_EMISSIVE_SHADING: u32 = 2;
    pub const SLOT_DEPTH: u32 = 3;

    pub const FORMAT_BASE_COLOR_METALLIC: Format = Format::Rgba8Unorm;
    pub const FORMAT_NORMAL_ROUGHNESS_OCCLUSION: Format = Format::Rgba16Float;
    pub const FORMAT_EMISSIVE_SHADING: Format = Format::Rgba16Float;
    pub const FORMAT_DEPTH: Format = Format::Depth32Float;

    pub const COLOR_ATTACHMENT_COUNT: u32 = 3;
}
