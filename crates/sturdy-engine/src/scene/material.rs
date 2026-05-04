// Unified material system for the deferred PBR pipeline.
//
// A `UnifiedMaterial` describes every channel of a material surface using a
// composable expression tree. The expression tree is compiled to a Slang
// G-Buffer fill shader variant at pipeline creation time by `DeferredPass`.
//
// Expression tree highlights:
//   - Constants baked into the shader
//   - Texture samples with full UV control (default, tiled, scrolled, custom)
//   - Texture2DArray image sequences animated by `cam.time`
//   - Procedural Slang expressions (access `v.uv`, `v.world_pos`, `cam.time`)
//   - Compose with Multiply, Mix (lerp), Add, Clamp, and Swizzle
//
// Roadmap: Track 6.

use std::collections::HashSet;

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

// ── UV source ─────────────────────────────────────────────────────────────────

/// How UV coordinates are computed when sampling a texture.
#[derive(Clone, Debug)]
pub enum UvSource {
    /// Standard UV0 from the mesh vertex — no transform.
    MeshUv0,
    /// UV0 scaled by `(u, v)` — repeats the texture that many times across the surface.
    ///
    /// Equivalent to Blender's "Mapping" node with Scale.
    Tiled { u: f32, v: f32 },
    /// UV0 scrolled over time at `(speed_u, speed_v)` units per second.
    ///
    /// Uses `cam.time`. Equivalent to Blender's "Mapping" node animated via drivers.
    Scrolled { speed_u: f32, speed_v: f32 },
    /// Tiled and scrolled simultaneously.
    TiledScrolled { u: f32, v: f32, speed_u: f32, speed_v: f32 },
    /// Arbitrary Slang expression returning `float2`.
    ///
    /// `v` is the `VSOut` struct (position, normal, uv, world_pos, tangent).
    /// `cam.time` is available for animation.
    Custom(String),
}

impl Default for UvSource {
    fn default() -> Self {
        Self::MeshUv0
    }
}

impl UvSource {
    /// Repeat the texture `u` times horizontally and `v` times vertically.
    pub fn tiled(u: f32, v: f32) -> Self {
        Self::Tiled { u, v }
    }

    /// Scroll the texture at (speed_u, speed_v) units/second, driven by `cam.time`.
    pub fn scrolled(speed_u: f32, speed_v: f32) -> Self {
        Self::Scrolled { speed_u, speed_v }
    }

    /// Tile and scroll simultaneously.
    pub fn tiled_scrolled(u: f32, v: f32, speed_u: f32, speed_v: f32) -> Self {
        Self::TiledScrolled { u, v, speed_u, speed_v }
    }

    /// Custom Slang expression for the UV. `v.uv`, `v.world_pos`, `cam.time` are in scope.
    pub fn custom(expr: impl Into<String>) -> Self {
        Self::Custom(expr.into())
    }

    /// Generate the Slang UV expression (returns a `float2`).
    pub(crate) fn to_slang(&self) -> String {
        match self {
            Self::MeshUv0 => "v.uv".to_owned(),
            Self::Tiled { u, v } => format!("(v.uv * float2({u:.6}, {v:.6}))"),
            Self::Scrolled { speed_u, speed_v } => {
                format!("(v.uv + float2({speed_u:.6}, {speed_v:.6}) * cam.time)")
            }
            Self::TiledScrolled { u, v, speed_u, speed_v } => {
                format!(
                    "(v.uv * float2({u:.6}, {v:.6}) + float2({speed_u:.6}, {speed_v:.6}) * cam.time)"
                )
            }
            Self::Custom(expr) => format!("({expr})"),
        }
    }
}

// ── MaterialExpr ──────────────────────────────────────────────────────────────

/// A composable expression for one material channel.
///
/// Expressions are a tree structure that gets compiled to Slang at pipeline
/// creation time. Constants are baked into the shader; textures become named
/// bindings that you set via [`RenderFrame::bind_image`] before drawing.
///
/// # Type parameters
/// - `MaterialExpr<[f32; 4]>` — colour/RGBA channel
/// - `MaterialExpr<[f32; 3]>` — RGB or vec3 channel
/// - `MaterialExpr<f32>` — scalar channel (roughness, metallic, occlusion…)
///
/// # Examples
/// ```ignore
/// // Tiled rock texture × a warm tint
/// let base = MaterialExpr::texture_uv("rock_albedo", UvSource::tiled(4.0, 4.0));
/// let tint = MaterialExpr::constant([1.1f32, 1.0, 0.9, 1.0]);
/// let col  = MaterialExpr::multiply(base, tint);
///
/// // Lava glow driven by procedural noise
/// let glow = MaterialExpr::procedural(
///     "float3(sin(v.world_pos.x * 3.0 + cam.time * 2.0) * 0.5 + 0.5, 0.05, 0.0) * 4.0"
/// );
///
/// // Animated emissive image sequence (Texture2DArray, 24 fps, 8 frames)
/// let fire = MaterialExpr::image_sequence("fire_frames", 8, 24.0);
/// ```
#[derive(Clone, Debug)]
pub enum MaterialExpr<T: Clone> {
    /// A fixed value baked as a literal into the generated shader.
    Constant(T),

    /// Sample a 2D texture at the given UV.
    ///
    /// Bind the image each frame: `frame.bind_image("name", &texture)`.
    Texture {
        /// Shader binding name — matches a `Texture2D` declaration.
        name: String,
        uv: UvSource,
    },

    /// Sample a 2D texture and multiply the result by a constant factor.
    ///
    /// Equivalent to Blender's "Multiply" blend between a texture and a constant —
    /// and the standard GLTF `baseColorTexture × baseColorFactor` convention.
    TextureFactor {
        name: String,
        uv: UvSource,
        factor: T,
    },

    /// Sample a frame from an animated image sequence (Texture2DArray).
    ///
    /// The frame index is driven by `cam.time * fps` modulo `frame_count`.
    /// Bind the array texture: `frame.bind_image("name", &my_texture_array)`.
    ///
    /// To create a Texture2DArray with N layers, use `ImageDesc { layers: N, .. }`
    /// and upload each frame to the corresponding layer via `copy_buffer_to_image`.
    ImageSequence {
        /// Shader binding name — matches a `Texture2DArray` declaration.
        name: String,
        /// Number of frames in the sequence.
        frame_count: u32,
        /// Playback rate in frames per second.
        fps: f32,
        uv: UvSource,
    },

    /// An arbitrary Slang expression returning type `T`.
    ///
    /// In scope: `v` (VSOut — position, normal, uv, world_pos, tangent),
    /// `cam.time` (elapsed seconds). Use for noise, wave functions, etc.
    ///
    /// # Example
    /// ```ignore
    /// // Procedural roughness based on world height
    /// MaterialExpr::procedural("saturate(v.world_pos.y * 0.1 + 0.5)")
    /// ```
    Procedural(String),

    /// Element-wise multiply of two sub-expressions.
    Multiply(Box<MaterialExpr<T>>, Box<MaterialExpr<T>>),

    /// Element-wise addition of two sub-expressions.
    Add(Box<MaterialExpr<T>>, Box<MaterialExpr<T>>),

    /// Linear interpolation: `lerp(a, b, t)`.
    ///
    /// `t` is a scalar expression in `[0, 1]` — use a texture's R channel,
    /// a procedural value, or a constant.
    Mix {
        a: Box<MaterialExpr<T>>,
        b: Box<MaterialExpr<T>>,
        /// Blend factor — scalar expression returning `float`.
        t: Box<MaterialExpr<f32>>,
    },

    /// Clamp to `[min, max]`.
    Clamp {
        value: Box<MaterialExpr<T>>,
        min: T,
        max: T,
    },

    /// Raise to a power: `pow(base, exp)`.
    Pow(Box<MaterialExpr<T>>, f32),
}

impl<T: Clone> MaterialExpr<T> {
    /// Sample a 2D texture at the default UV (UV0).
    pub fn texture(name: impl Into<String>) -> Self {
        Self::Texture { name: name.into(), uv: UvSource::MeshUv0 }
    }

    /// Sample a 2D texture with an explicit UV source.
    pub fn texture_uv(name: impl Into<String>, uv: UvSource) -> Self {
        Self::Texture { name: name.into(), uv }
    }

    /// Sample a 2D texture and multiply by a constant factor.
    pub fn texture_factor(name: impl Into<String>, factor: T) -> Self {
        Self::TextureFactor { name: name.into(), uv: UvSource::MeshUv0, factor }
    }

    /// Animated Texture2DArray sequence cycling at `fps` frames/second.
    pub fn image_sequence(name: impl Into<String>, frame_count: u32, fps: f32) -> Self {
        Self::ImageSequence { name: name.into(), frame_count, fps, uv: UvSource::MeshUv0 }
    }

    /// Animated sequence with explicit UV source.
    pub fn image_sequence_uv(
        name: impl Into<String>,
        frame_count: u32,
        fps: f32,
        uv: UvSource,
    ) -> Self {
        Self::ImageSequence { name: name.into(), frame_count, fps, uv }
    }

    /// Arbitrary Slang expression string (returns type T).
    pub fn procedural(expr: impl Into<String>) -> Self {
        Self::Procedural(expr.into())
    }

    /// Element-wise multiply.
    pub fn multiply(a: MaterialExpr<T>, b: MaterialExpr<T>) -> Self {
        Self::Multiply(Box::new(a), Box::new(b))
    }

    /// Element-wise addition.
    pub fn add(a: MaterialExpr<T>, b: MaterialExpr<T>) -> Self {
        Self::Add(Box::new(a), Box::new(b))
    }

    /// Linear interpolation.
    pub fn mix(a: MaterialExpr<T>, b: MaterialExpr<T>, t: MaterialExpr<f32>) -> Self {
        Self::Mix { a: Box::new(a), b: Box::new(b), t: Box::new(t) }
    }

    /// Clamp to [min, max].
    pub fn clamp(value: MaterialExpr<T>, min: T, max: T) -> Self {
        Self::Clamp { value: Box::new(value), min, max }
    }
}

impl<T: Clone> MaterialExpr<T> {
    /// Collect all texture binding names referenced by this expression tree.
    pub(crate) fn collect_textures(&self, out: &mut HashSet<String>) {
        match self {
            Self::Constant(_) | Self::Procedural(_) => {}
            Self::Texture { name, .. } | Self::TextureFactor { name, .. } => {
                out.insert(name.clone());
            }
            Self::ImageSequence { name, .. } => {
                out.insert(name.clone());
            }
            Self::Multiply(a, b) | Self::Add(a, b) => {
                a.collect_textures(out);
                b.collect_textures(out);
            }
            Self::Mix { a, b, t } => {
                a.collect_textures(out);
                b.collect_textures(out);
                t.collect_textures(out);
            }
            Self::Clamp { value, .. } | Self::Pow(value, _) => {
                value.collect_textures(out);
            }
        }
    }

    /// Whether any node in the tree uses `cam.time`.
    pub(crate) fn uses_time(&self) -> bool {
        match self {
            Self::Procedural(s) => s.contains("cam.time"),
            Self::Texture { uv, .. } | Self::TextureFactor { uv, .. } => uv_uses_time(uv),
            Self::ImageSequence { .. } => true, // frame index driven by time
            Self::Multiply(a, b) | Self::Add(a, b) => a.uses_time() || b.uses_time(),
            Self::Mix { a, b, t } => a.uses_time() || b.uses_time() || t.uses_time(),
            Self::Clamp { value, .. } | Self::Pow(value, _) => value.uses_time(),
            Self::Constant(_) => false,
        }
    }
}

fn uv_uses_time(uv: &UvSource) -> bool {
    matches!(uv, UvSource::Scrolled { .. } | UvSource::TiledScrolled { .. })
        || matches!(uv, UvSource::Custom(s) if s.contains("cam.time"))
}

// ── Slang codegen ─────────────────────────────────────────────────────────────

/// Marker trait implemented for types that can be a `MaterialExpr` element.
pub trait SlangType {
    fn constant_expr(val: &Self) -> String;
    fn slang_type_name() -> &'static str;
}

impl SlangType for f32 {
    fn constant_expr(v: &Self) -> String { format!("{v:.6}") }
    fn slang_type_name() -> &'static str { "float" }
}

impl SlangType for [f32; 2] {
    fn constant_expr(v: &Self) -> String { format!("float2({:.6}, {:.6})", v[0], v[1]) }
    fn slang_type_name() -> &'static str { "float2" }
}

impl SlangType for [f32; 3] {
    fn constant_expr(v: &Self) -> String {
        format!("float3({:.6}, {:.6}, {:.6})", v[0], v[1], v[2])
    }
    fn slang_type_name() -> &'static str { "float3" }
}

impl SlangType for [f32; 4] {
    fn constant_expr(v: &Self) -> String {
        format!("float4({:.6}, {:.6}, {:.6}, {:.6})", v[0], v[1], v[2], v[3])
    }
    fn slang_type_name() -> &'static str { "float4" }
}

impl<T: Clone + SlangType> MaterialExpr<T> {
    /// Generate a Slang expression string that evaluates to type `T`.
    ///
    /// Texture reads use the named binding and `material_sampler`.
    /// `v` refers to `VSOut`; `cam` refers to the `CameraConstants` uniform.
    pub fn to_slang_expr(&self) -> String {
        match self {
            Self::Constant(v) => T::constant_expr(v),

            Self::Texture { name, uv } => {
                let uv_expr = uv.to_slang();
                texture_sample_expr(name, &uv_expr, T::slang_type_name())
            }

            Self::TextureFactor { name, uv, factor } => {
                let uv_expr = uv.to_slang();
                let sample = texture_sample_expr(name, &uv_expr, T::slang_type_name());
                let fact = T::constant_expr(factor);
                format!("({sample} * {fact})")
            }

            Self::ImageSequence { name, frame_count, fps, uv } => {
                let uv_expr = uv.to_slang();
                let fc = frame_count;
                let fps_val = fps;
                format!(
                    "{name}.Sample(material_sampler, float3({uv_expr}, \
                     float(int(cam.time * {fps_val:.6}) % {fc})))"
                )
            }

            Self::Procedural(code) => format!("({code})"),

            Self::Multiply(a, b) => {
                format!("(({}) * ({}))", a.to_slang_expr(), b.to_slang_expr())
            }

            Self::Add(a, b) => {
                format!("(({}) + ({}))", a.to_slang_expr(), b.to_slang_expr())
            }

            Self::Mix { a, b, t } => {
                format!(
                    "lerp(({a_e}), ({b_e}), ({t_e}))",
                    a_e = a.to_slang_expr(),
                    b_e = b.to_slang_expr(),
                    t_e = t.to_slang_expr(),
                )
            }

            Self::Clamp { value, min, max } => {
                format!(
                    "clamp(({v}), {mn}, {mx})",
                    v  = value.to_slang_expr(),
                    mn = T::constant_expr(min),
                    mx = T::constant_expr(max),
                )
            }

            Self::Pow(base, exp) => {
                format!("pow(({b}), {exp:.6})", b = base.to_slang_expr())
            }
        }
    }

    /// Collect all `Texture2DArray` binding names (from `ImageSequence` nodes).
    pub(crate) fn collect_array_textures(&self, out: &mut HashSet<String>) {
        match self {
            Self::ImageSequence { name, .. } => { out.insert(name.clone()); }
            Self::Multiply(a, b) | Self::Add(a, b) => {
                a.collect_array_textures(out);
                b.collect_array_textures(out);
            }
            Self::Mix { a, b, t } => {
                a.collect_array_textures(out);
                b.collect_array_textures(out);
                t.collect_array_textures(out);
            }
            Self::Clamp { value, .. } | Self::Pow(value, _) => value.collect_array_textures(out),
            _ => {}
        }
    }
}

/// Generate a texture sample expression, handling the return type swizzle.
fn texture_sample_expr(name: &str, uv_expr: &str, ty: &str) -> String {
    // Texture2D.Sample returns float4; swizzle to target type.
    let sample = format!("{name}.Sample(material_sampler, {uv_expr})");
    match ty {
        "float"  => format!("({sample}.r)"),
        "float2" => format!("({sample}.rg)"),
        "float3" => format!("({sample}.rgb)"),
        _        => format!("({sample})"),  // float4 — no swizzle
    }
}

// ── UnifiedMaterial ───────────────────────────────────────────────────────────

/// A rendering-path-agnostic material definition.
///
/// Each channel is driven by a [`MaterialExpr`] expression tree. Expressions
/// can be constants, texture samples, procedural Slang code, image sequences,
/// or compositions of the above via Multiply, Mix, Add, and Clamp nodes.
///
/// `DeferredPass` compiles a unique G-Buffer fragment shader for each
/// `UnifiedMaterial` (cached by content hash). Expressions that reference named
/// textures (e.g. `MaterialExpr::texture("rock_albedo")`) require those textures
/// to be registered in the render frame before the G-Buffer pass:
/// ```ignore
/// frame.bind_image("rock_albedo", &rock_albedo_texture);
/// ```
///
/// # Quick start
/// ```ignore
/// let mat = UnifiedMaterial::pbr_metallic_roughness("rock")
///     .base_color(MaterialExpr::texture_uv("rock_albedo", UvSource::tiled(4.0, 4.0)))
///     .roughness(MaterialExpr::texture("rock_roughness"))
///     .normal(MaterialExpr::texture_uv("rock_normal", UvSource::tiled(4.0, 4.0)))
///     .metallic(MaterialExpr::constant(0.0))
///     .build();
/// scene.set_unified_material(mesh_id, mat);
///
/// // Each frame, bind textures before DeferredPass::draw():
/// frame.bind_image("rock_albedo",    &rock_albedo);
/// frame.bind_image("rock_roughness", &rock_roughness);
/// frame.bind_image("rock_normal",    &rock_normal);
/// ```
///
/// # Animated emissive (image sequence)
/// ```ignore
/// // Upload frames as a Texture2DArray with 8 layers.
/// let fire_mat = UnifiedMaterial::pbr_metallic_roughness("fire")
///     .emissive(MaterialExpr::image_sequence("fire_frames", 8, 24.0).rgb3())
///     .base_color(MaterialExpr::constant([0.02f32, 0.02, 0.02, 1.0]))
///     .build();
/// // Each frame:
/// frame.bind_image("fire_frames", &fire_texture_array);
/// ```
///
/// # Procedural lava
/// ```ignore
/// let lava = UnifiedMaterial::pbr_metallic_roughness("lava")
///     .emissive(MaterialExpr::procedural(
///         "float3(pow(sin(v.world_pos.x*3.0+cam.time*1.5)*0.5+0.5, 2.0)*4.0, 0.05, 0.0)"
///     ))
///     .roughness(MaterialExpr::procedural(
///         "lerp(0.3, 0.95, sin(v.world_pos.z*5.0+cam.time)*0.5+0.5)"
///     ))
///     .build();
/// ```
#[derive(Clone, Debug)]
pub struct UnifiedMaterial {
    pub name: String,
    pub domain: MaterialDomain,
    pub shading_model: ShadingModel,
    pub render_state: RenderState,

    pub base_color: MaterialExpr<[f32; 4]>,
    pub metallic:   MaterialExpr<f32>,
    pub roughness:  MaterialExpr<f32>,
    /// Tangent-space normal. Use `MaterialExpr::texture("normal_map")` here;
    /// the G-Buffer shader applies the TBN transform automatically.
    /// Default is `float3(0,0,1)` — geometric normal, no perturbation.
    pub normal:     MaterialExpr<[f32; 3]>,
    pub occlusion:  MaterialExpr<f32>,
    /// Linear-HDR emissive radiance. Values > 1.0 drive bloom.
    pub emissive:   MaterialExpr<[f32; 3]>,

    /// Clear-coat intensity (only used when `shading_model == PbrClearcoat`).
    pub clearcoat:           MaterialExpr<f32>,
    pub clearcoat_roughness: MaterialExpr<f32>,
}

impl UnifiedMaterial {
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

    /// A stable content hash of this material (hashes the generated shader source).
    /// Used by `DeferredPass` to cache compiled variants.
    pub fn content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        let mut h = DefaultHasher::new();
        self.generate_gbuffer_source().hash(&mut h);
        h.finish()
    }

    /// Generate the complete Slang source for the G-Buffer fill fragment shader.
    ///
    /// Called by `DeferredPass` when compiling a material variant. The result
    /// is passed as `ShaderSource::MemoryUtf8` to the shader compiler.
    pub fn generate_gbuffer_source(&self) -> String {
        let mut tex2d:  HashSet<String> = HashSet::new();
        let mut array2d: HashSet<String> = HashSet::new();

        // Collect all texture binding names from every channel.
        self.base_color.collect_textures(&mut tex2d);
        self.metallic.collect_textures(&mut tex2d);
        self.roughness.collect_textures(&mut tex2d);
        self.normal.collect_textures(&mut tex2d);
        self.occlusion.collect_textures(&mut tex2d);
        self.emissive.collect_textures(&mut tex2d);

        self.base_color.collect_array_textures(&mut array2d);
        self.metallic.collect_array_textures(&mut array2d);
        self.roughness.collect_array_textures(&mut array2d);
        self.normal.collect_array_textures(&mut array2d);
        self.occlusion.collect_array_textures(&mut array2d);
        self.emissive.collect_array_textures(&mut array2d);

        // Array textures are NOT in tex2d (they're Texture2DArray).
        tex2d.retain(|n| !array2d.contains(n));

        let mut decls = String::new();

        // SamplerState (single shared sampler for all texture reads).
        decls.push_str("SamplerState material_sampler;\n");

        // Texture2D declarations.
        for name in &tex2d {
            decls.push_str(&format!("Texture2D<float4> {name};\n"));
        }
        // Texture2DArray declarations (for image sequences).
        for name in &array2d {
            decls.push_str(&format!("Texture2DArray<float4> {name};\n"));
        }

        let base_color_e = self.base_color.to_slang_expr();
        let metallic_e   = self.metallic.to_slang_expr();
        let roughness_e  = self.roughness.to_slang_expr();
        let normal_e     = self.normal.to_slang_expr();
        let emissive_e   = self.emissive.to_slang_expr();

        // The normal channel's expression returns float3 in tangent space.
        // We always apply TBN (even when normal is [0,0,1] → geometric normal).
        format!(r#"
// ── Material: {name} ──────────────────────────────────────────────────────────
// Auto-generated by UnifiedMaterial::generate_gbuffer_source().
// DO NOT EDIT — regenerated when the material is modified.

// ── Vertex data ──────────────────────────────────────────────────────────────
struct InstanceData {{ float4x4 model; }};
StructuredBuffer<InstanceData> instances;

struct CameraConstants {{
    float4x4 view_proj;
    float4x4 previous_view_proj;
    float    time;
    float3   _pad;
}};
uniform CameraConstants cam;

struct VSOut {{
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
    float4 tangent   : TANGENT;
}};

// ── Texture and sampler bindings ─────────────────────────────────────────────
{decls}
// ── Octahedral normal encoding ────────────────────────────────────────────────
float2 oct_encode(float3 n) {{
    float3 a = abs(n);
    n /= a.x + a.y + a.z;
    if (n.z < 0.0) {{
        float2 s = float2(n.x >= 0.0 ? 1.0 : -1.0,
                          n.y >= 0.0 ? 1.0 : -1.0);
        n.xy = (1.0 - abs(n.yx)) * s;
    }}
    return n.xy;
}}

// ── G-Buffer output ──────────────────────────────────────────────────────────
struct GBufferOut {{
    float4 g0 : SV_TARGET0; // base_color.rgb | metallic
    float4 g1 : SV_TARGET1; // oct-normal.xy | roughness | 0
    float4 g2 : SV_TARGET2; // emissive.rgb | 0
    float4 g3 : SV_TARGET3; // world_pos.xyz | 0
}};

// ── Material evaluation ──────────────────────────────────────────────────────
GBufferOut main(VSOut v) {{
    // Evaluate all material channels.
    float4 base_color = {base_color_e};
    float  metallic   = {metallic_e};
    float  roughness  = {roughness_e};
    float3 normal_ts  = {normal_e};  // tangent-space normal
    float3 emissive   = {emissive_e};

    // Apply TBN to produce world-space normal.
    // When normal_ts == float3(0,0,1) (no normal map), this returns the geometric normal.
    float3 N = normalize(v.normal);
    float3 T = normalize(v.tangent.xyz);
    T = normalize(T - N * dot(N, T));       // Gram-Schmidt re-orthogonalise
    float3 B = cross(N, T) * v.tangent.w;
    float3x3 tbn = float3x3(T, B, N);
    N = normalize(mul(normal_ts, tbn));

    float2 oct_n = oct_encode(N);

    GBufferOut o;
    o.g0 = float4(base_color.rgb, metallic);
    o.g1 = float4(oct_n, roughness, 0.0);
    o.g2 = float4(emissive, 0.0);
    o.g3 = float4(v.world_pos, 0.0);
    return o;
}}
"#,
            name       = self.name,
            decls      = decls,
            base_color_e = base_color_e,
            metallic_e   = metallic_e,
            roughness_e  = roughness_e,
            normal_e     = normal_e,
            emissive_e   = emissive_e,
        )
    }
}

impl Default for UnifiedMaterial {
    fn default() -> Self {
        Self {
            name: "default_pbr".into(),
            domain: MaterialDomain::Opaque,
            shading_model: ShadingModel::PbrMetallicRoughness,
            render_state: RenderState::default(),
            base_color: MaterialExpr::Constant([1.0, 1.0, 1.0, 1.0]),
            metallic:   MaterialExpr::Constant(0.0),
            roughness:  MaterialExpr::Constant(0.5),
            normal:     MaterialExpr::Constant([0.0, 0.0, 1.0]),
            occlusion:  MaterialExpr::Constant(1.0),
            emissive:   MaterialExpr::Constant([0.0, 0.0, 0.0]),
            clearcoat:           MaterialExpr::Constant(0.0),
            clearcoat_roughness: MaterialExpr::Constant(0.0),
        }
    }
}

// ── UnifiedMaterialBuilder ────────────────────────────────────────────────────

/// Fluent builder for [`UnifiedMaterial`].
///
/// Obtain one via [`UnifiedMaterial::pbr_metallic_roughness`],
/// [`UnifiedMaterial::unlit`], or [`UnifiedMaterial::procedural`].
#[derive(Clone, Debug)]
pub struct UnifiedMaterialBuilder {
    inner: UnifiedMaterial,
}

impl UnifiedMaterialBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
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

    // ── Channel setters ───────────────────────────────────────────────────────

    /// Set the base color / albedo channel to the given expression.
    pub fn base_color(mut self, expr: MaterialExpr<[f32; 4]>) -> Self {
        self.inner.base_color = expr;
        self
    }

    /// Constant base color `[r, g, b, a]` (linear).
    pub fn base_color_constant(mut self, rgba: [f32; 4]) -> Self {
        self.inner.base_color = MaterialExpr::Constant(rgba);
        self
    }

    /// Sample a texture for the base color, optionally multiplied by a factor.
    pub fn base_color_texture(mut self, name: impl Into<String>) -> Self {
        self.inner.base_color = MaterialExpr::texture(name);
        self
    }

    pub fn base_color_texture_factor(
        mut self,
        name: impl Into<String>,
        factor: [f32; 4],
    ) -> Self {
        self.inner.base_color = MaterialExpr::texture_factor(name, factor);
        self
    }

    /// Set the metallic channel.
    pub fn metallic(mut self, expr: MaterialExpr<f32>) -> Self {
        self.inner.metallic = expr;
        self
    }

    pub fn metallic_constant(mut self, v: f32) -> Self {
        self.inner.metallic = MaterialExpr::Constant(v);
        self
    }

    /// Set the roughness channel.
    pub fn roughness(mut self, expr: MaterialExpr<f32>) -> Self {
        self.inner.roughness = expr;
        self
    }

    pub fn roughness_constant(mut self, v: f32) -> Self {
        self.inner.roughness = MaterialExpr::Constant(v);
        self
    }

    /// Convenience: set both metallic and roughness constants (GLTF convention).
    pub fn metallic_roughness_constants(mut self, metallic: f32, roughness: f32) -> Self {
        self.inner.metallic = MaterialExpr::Constant(metallic);
        self.inner.roughness = MaterialExpr::Constant(roughness);
        self
    }

    /// Sample a GLTF-style metallic-roughness texture (B=metallic, G=roughness).
    pub fn metallic_roughness_texture(mut self, name: impl Into<String>) -> Self {
        let n = name.into();
        self.inner.metallic  = MaterialExpr::Texture { name: format!("{n}__met"),  uv: UvSource::MeshUv0 };
        self.inner.roughness = MaterialExpr::Texture { name: format!("{n}__rou"), uv: UvSource::MeshUv0 };
        self
    }

    /// Set the tangent-space normal channel.
    ///
    /// The shader automatically applies the TBN transform. Use
    /// `MaterialExpr::texture("my_normal_map")` for a standard normal map, or
    /// a procedural expression to generate normals procedurally.
    pub fn normal(mut self, expr: MaterialExpr<[f32; 3]>) -> Self {
        self.inner.normal = expr;
        self
    }

    pub fn normal_texture(mut self, name: impl Into<String>) -> Self {
        self.inner.normal = MaterialExpr::texture(name);
        self
    }

    /// Set the occlusion channel.
    pub fn occlusion(mut self, expr: MaterialExpr<f32>) -> Self {
        self.inner.occlusion = expr;
        self
    }

    pub fn occlusion_texture(mut self, name: impl Into<String>) -> Self {
        self.inner.occlusion = MaterialExpr::Texture {
            name: name.into(),
            uv: UvSource::MeshUv0,
        };
        self
    }

    /// Set the emissive channel (linear HDR, values > 1 drive bloom).
    pub fn emissive(mut self, expr: MaterialExpr<[f32; 3]>) -> Self {
        self.inner.emissive = expr;
        self
    }

    pub fn emissive_constant(mut self, rgb: [f32; 3]) -> Self {
        self.inner.emissive = MaterialExpr::Constant(rgb);
        self
    }

    pub fn emissive_texture(mut self, name: impl Into<String>) -> Self {
        self.inner.emissive = MaterialExpr::texture(name);
        self
    }

    /// Clear-coat intensity and roughness (activates `PbrClearcoat` shading model).
    pub fn clearcoat(mut self, intensity: f32, roughness: f32) -> Self {
        self.inner.shading_model = ShadingModel::PbrClearcoat;
        self.inner.clearcoat = MaterialExpr::Constant(intensity);
        self.inner.clearcoat_roughness = MaterialExpr::Constant(roughness);
        self
    }

    pub fn build(self) -> UnifiedMaterial {
        self.inner
    }
}

// ── Backward-compatibility alias ──────────────────────────────────────────────

/// Legacy name — use [`MaterialExpr`] directly in new code.
pub type MaterialInput<T> = MaterialExpr<T>;

// ── G-Buffer layout constants ─────────────────────────────────────────────────

/// Standard G-Buffer attachment slots and formats for the deferred PBR pipeline.
///
/// All passes that read or write G-Buffer data must use these constants so
/// attachment indices and formats are consistent across the frame graph.
///
/// ```text
/// G0  RGBA8Unorm   base_color.rgb (linear) | metallic
/// G1  RGBA16Float  world-normal.xy (oct-encoded) | roughness | 0
/// G2  RGBA16Float  emissive.rgb (linear HDR, unclamped) | 0
/// G3  RGBA16Float  world_pos.xyz | 0
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
