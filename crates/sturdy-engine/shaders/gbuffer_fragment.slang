// G-Buffer fill fragment shader.
//
// Packs PBR material parameters into three render targets using the shared
// MaterialSurface layout defined in material_surface.slang.
//
// Render target layout (must match gbuffer::* constants in scene/material.rs):
//   RT0  RGBA8Unorm   albedo.rgb | metallic
//   RT1  RGBA16Float  oct-encoded normal.xy | roughness | 0
//   RT2  RGBA16Float  emissive.rgb | 0
//
// Bindings:
//   "material_desc"  StructuredBuffer<MaterialConstants>  (one element per mesh)
//   "normal_map"     Texture2D<float4>   tangent-space normal map (or flat 1×1 fallback)
//   "normal_sampler" SamplerState        linear clamp sampler
//
// Roadmap: Track 6a — MaterialSurface shared module now imported here.

import material_surface;

struct MaterialConstants {
    float4 albedo;              // xyz = base colour (linear), w = opacity
    float4 emissive_metallic;   // xyz = emissive (HDR), w = metallic [0, 1]
    float4 roughness_flags;     // x = roughness [0, 1], y = has_normal_map (1.0 = yes)
};
StructuredBuffer<MaterialConstants> material_desc;

Texture2D<float4> normal_map;
SamplerState      normal_sampler;

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
    float4 tangent   : TANGENT;
    float4 curr_clip : TEXCOORD5;
    float4 prev_clip : TEXCOORD6;
};

GBufferOut main(VSOut v) {
    MaterialConstants M = material_desc[0];

    float3 albedo    = M.albedo.xyz;
    float  metallic  = M.emissive_metallic.w;
    float  roughness = M.roughness_flags.x;
    float3 emissive  = M.emissive_metallic.xyz;
    bool   has_nmap  = M.roughness_flags.y > 0.5;

    float3 N = normalize(v.normal);
    if (has_nmap) {
        float3 ts = normal_map.Sample(normal_sampler, v.uv).xyz;
        N = apply_normal_map(N, v.tangent, ts);
    }

    MaterialSurface surface;
    surface.albedo    = albedo;
    surface.normal    = N;
    surface.roughness = roughness;
    surface.metallic  = metallic;
    surface.emissive  = emissive;
    surface.opacity   = M.albedo.w;

    // Per-object motion velocity: clip-space positions interpolated from vertex shader.
    float2 curr_ndc = v.curr_clip.xy / v.curr_clip.w;
    float2 prev_ndc = v.prev_clip.xy / v.prev_clip.w;
    float2 motion_velocity = (prev_ndc - curr_ndc) * 0.5; // NDC→UV scale: *0.5

    return pack_gbuffer(surface, motion_velocity);
}
