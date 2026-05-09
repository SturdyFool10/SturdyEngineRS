// Shared material surface interface.
//
// `MaterialSurface` is the canonical intermediate between material evaluation
// (G-Buffer fill, forward lit, shadow, RT any-hit) and the lighting pass.
// Every lit shader variant produces one `MaterialSurface`; the backend decides
// how to consume it (pack into G-Buffer, evaluate forward lighting, etc.).
//
// Import in any lit shader with:
//   import material_surface;
//
// Roadmap: Track 6a — MaterialSurface interface and UnifiedMaterial.

// ── MaterialSurface ───────────────────────────────────────────────────────────

/// Evaluated PBR surface parameters at one shader invocation.
///
/// All channels are in linear light space. Normal is in world space.
struct MaterialSurface {
    /// Linear-space base colour (RGB).
    float3 albedo;
    /// World-space shading normal (unit vector).
    float3 normal;
    /// Perceptual roughness [0, 1].
    float  roughness;
    /// Metallic factor [0, 1].
    float  metallic;
    /// Linear-space emissive radiance (HDR).
    float3 emissive;
    /// Opacity [0, 1]. 1.0 = fully opaque.
    float  opacity;
};

// ── G-Buffer output layout ────────────────────────────────────────────────────
// Must match gbuffer::* format constants in scene/material.rs.
//
//   g0  RGBA8Unorm   albedo.rgb | metallic
//   g1  RGBA16Float  oct-encoded normal.xy | roughness | 0
//   g2  RGBA16Float  emissive.rgb | 0

struct GBufferOut {
    float4 g0 : SV_TARGET0; // albedo.rgb | metallic
    float4 g1 : SV_TARGET1; // oct-normal.xy | roughness | 0
    float4 g2 : SV_TARGET2; // emissive.rgb | 0
};

// ── Octahedral normal encoding / decoding ─────────────────────────────────────

/// Encode a unit normal into two floats in [-1, 1].
/// Decoded exactly in the deferred lighting pass without error accumulation.
float2 oct_encode(float3 n) {
    float3 a = abs(n);
    n /= a.x + a.y + a.z;
    if (n.z < 0.0f) {
        float2 s = float2(n.x >= 0.0f ? 1.0f : -1.0f,
                          n.y >= 0.0f ? 1.0f : -1.0f);
        n.xy = (1.0f - abs(n.yx)) * s;
    }
    return n.xy;
}

/// Decode an oct-encoded normal back to a unit vector.
float3 oct_decode(float2 f) {
    float3 n = float3(f.x, f.y, 1.0f - abs(f.x) - abs(f.y));
    if (n.z < 0.0f) {
        float2 s = float2(n.x >= 0.0f ? 1.0f : -1.0f,
                          n.y >= 0.0f ? 1.0f : -1.0f);
        n.xy = (1.0f - abs(n.yx)) * s;
    }
    return normalize(n);
}

// ── G-Buffer packing ──────────────────────────────────────────────────────────

/// Pack a `MaterialSurface` into the standard G-Buffer layout.
GBufferOut pack_gbuffer(MaterialSurface s) {
    GBufferOut o;
    o.g0 = float4(s.albedo, s.metallic);
    o.g1 = float4(oct_encode(s.normal), clamp(s.roughness, 0.04f, 1.0f), 0.0f);
    o.g2 = float4(s.emissive, 0.0f);
    return o;
}

// ── TBN helpers ───────────────────────────────────────────────────────────────

/// Build a TBN matrix and apply a sampled tangent-space normal.
/// Returns the world-space shading normal.
float3 apply_normal_map(float3 geom_n, float4 tangent, float3 tangent_sample) {
    float3 T  = normalize(tangent.xyz);
    float3 B  = cross(geom_n, T) * tangent.w;
    float3 ts = tangent_sample * 2.0f - 1.0f;
    // Gram-Schmidt re-orthogonalise T against N.
    T = normalize(T - geom_n * dot(geom_n, T));
    return normalize(T * ts.x + B * ts.y + geom_n * ts.z);
}
