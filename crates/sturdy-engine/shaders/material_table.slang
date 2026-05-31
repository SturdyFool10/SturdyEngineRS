// material_table.slang — engine-standard material table declaration.
//
// Include this file in any shader that reads per-material PBR parameters from
// the centrally-managed MaterialRegistry.  Pair with bindless.slang to access
// textures via their bindless indices.
//
// ## Usage
//
// ```slang
// #include "bindless.slang"
// #include "material_table.slang"
//
// struct DrawConstants {
//     uint material_id;
//     uint sampler_override; // 0 = use material's own sampler_idx
// };
//
// float4 fragment_main(VSOut v, uniform DrawConstants push) : SV_TARGET {
//     GpuMaterialEntry mat = material_table[push.material_id];
//
//     uint smp = push.sampler_override != 0 ? push.sampler_override : mat.sampler_idx;
//
//     float4 albedo = mat.albedo_idx != MATERIAL_NO_TEXTURE
//         ? bindless_sample(mat.albedo_idx, smp, v.uv)
//         : float4(mat.albedo_tint, mat.opacity);
//
//     float3 normal_ts = mat.normal_idx != MATERIAL_NO_TEXTURE
//         ? bindless_sample(mat.normal_idx, smp, v.uv).xyz * 2.0 - 1.0
//         : float3(0.0, 0.0, 1.0);
//
//     // ...
// }
// ```
//
// ## Binding
//
// Bind from Rust via:
//   frame.bind_material_table(&mat_registry);
//
// This registers the MaterialRegistry GPU buffer as "material_table" so the
// reflection-driven bind group builder picks it up automatically.
//
// ## Rust layout
//
// Matches `GpuMaterialEntry` in material_registry.rs (64 bytes per entry).

/// Sentinel: no texture bound for this slot.
static const uint MATERIAL_NO_TEXTURE = 0xFFFFFFFF;

/// Per-material PBR descriptor.  One entry per MaterialId in the registry.
struct GpuMaterialEntry {
    uint  albedo_idx;              ///< Bindless sampled-image index (or MATERIAL_NO_TEXTURE)
    uint  normal_idx;              ///< Tangent-space normal map (or MATERIAL_NO_TEXTURE)
    uint  roughness_metallic_idx;  ///< R=roughness, G=metallic (or MATERIAL_NO_TEXTURE)
    uint  emissive_idx;            ///< Emissive radiance texture (or MATERIAL_NO_TEXTURE)
    uint  sampler_idx;             ///< Bindless sampler for all slots in this material

    float3 albedo_tint;            ///< Linear-space RGB tint multiplied with albedo texture
    float  opacity;                ///< Opacity [0, 1] — values below 1.0 need alpha blending

    float3 emissive_color;         ///< Linear-space emissive radiance (HDR allowed)
    float  metallic;               ///< Metallic factor [0, 1]

    float  roughness;              ///< Roughness [0, 1]
    uint   flags;                  ///< Reserved for future per-material flags
    float2 _pad;
};

/// The engine's global material table.
///
/// Bound automatically when `frame.bind_material_table(&registry)` is called.
/// Access entries as `material_table[push.material_id]`.
StructuredBuffer<GpuMaterialEntry> material_table;

// ── Helper functions ──────────────────────────────────────────────────────────

/// Sample the albedo texture for a material, or return the tint as a solid
/// colour when no texture is bound.
float4 material_sample_albedo(GpuMaterialEntry mat, uint smp, float2 uv) {
    if (mat.albedo_idx != MATERIAL_NO_TEXTURE) {
        return bindless_sample(mat.albedo_idx, smp, uv)
               * float4(mat.albedo_tint, mat.opacity);
    }
    return float4(mat.albedo_tint, mat.opacity);
}

/// Sample the tangent-space normal for a material, or return the face normal
/// when no normal map is bound.
float3 material_sample_normal_ts(GpuMaterialEntry mat, uint smp, float2 uv) {
    if (mat.normal_idx != MATERIAL_NO_TEXTURE) {
        return bindless_sample(mat.normal_idx, smp, uv).xyz * 2.0 - 1.0;
    }
    return float3(0.0, 0.0, 1.0);
}

/// Sample roughness and metallic for a material.
/// Returns float2(roughness, metallic).
float2 material_sample_roughness_metallic(GpuMaterialEntry mat, uint smp, float2 uv) {
    if (mat.roughness_metallic_idx != MATERIAL_NO_TEXTURE) {
        float4 s = bindless_sample(mat.roughness_metallic_idx, smp, uv);
        return float2(s.r * mat.roughness, s.g * mat.metallic);
    }
    return float2(mat.roughness, mat.metallic);
}

/// Sample emissive radiance for a material, or return the solid colour.
float3 material_sample_emissive(GpuMaterialEntry mat, uint smp, float2 uv) {
    if (mat.emissive_idx != MATERIAL_NO_TEXTURE) {
        return bindless_sample(mat.emissive_idx, smp, uv).rgb * mat.emissive_color;
    }
    return mat.emissive_color;
}
