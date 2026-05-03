// G-Buffer fill fragment shader.
//
// Packs PBR material parameters into four render targets.
// The vertex shader is mesh_vertex_3d.slang (unchanged).
// No lighting is computed here — this pass only writes surface data.
//
// Render target layout (must match gbuffer::* constants in scene/material.rs):
//   RT0  RGBA8Unorm   albedo.rgb | metallic
//   RT1  RGBA16Float  oct-encoded normal.xy | roughness | 0
//   RT2  RGBA16Float  emissive.rgb | 0
//   RT3  RGBA16Float  world_pos.xyz | 0   (half-precision; upgrade to RGBA32Float for large worlds)
//
// Bindings written by Scene::draw_gbuffer via Scene::update_lighting_uniform
// and the per-batch bind_buffer calls:
//   "material_desc"  StructuredBuffer<MaterialConstants>  (one element per mesh)

struct MaterialConstants {
    float4 albedo;            // xyz = base colour (linear), w = opacity
    float4 emissive_metallic; // xyz = emissive (HDR), w = metallic [0, 1]
    float4 roughness_pad;     // x = roughness [0, 1], yzw = unused
};
StructuredBuffer<MaterialConstants> material_desc;

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
};

// ── Octahedral normal encoding ────────────────────────────────────────────────
// Maps a unit normal to two floats in [-1, 1]. Decodes exactly in the
// deferred lighting pass without error accumulation.
float2 oct_encode(float3 n) {
    float3 a = abs(n);
    n /= a.x + a.y + a.z;
    if (n.z < 0.0) {
        float2 s = float2(n.x >= 0.0 ? 1.0 : -1.0,
                          n.y >= 0.0 ? 1.0 : -1.0);
        n.xy = (1.0 - abs(n.yx)) * s;
    }
    return n.xy;
}

// ── G-Buffer output struct ────────────────────────────────────────────────────

struct GBufferOut {
    float4 g0 : SV_TARGET0; // albedo.rgb | metallic
    float4 g1 : SV_TARGET1; // oct-normal.xy | roughness | 0
    float4 g2 : SV_TARGET2; // emissive.rgb | 0
    float4 g3 : SV_TARGET3; // world_pos.xyz | 0
};

GBufferOut main(VSOut v) {
    MaterialConstants M = material_desc[0];

    float3 albedo    = M.albedo.xyz;
    float  metallic  = M.emissive_metallic.w;
    float  roughness = M.roughness_pad.x;
    float3 emissive  = M.emissive_metallic.xyz;

    float3 N = normalize(v.normal);
    float2 oct_n = oct_encode(N);

    GBufferOut o;
    o.g0 = float4(albedo, metallic);
    o.g1 = float4(oct_n, roughness, 0.0);
    o.g2 = float4(emissive, 0.0);
    o.g3 = float4(v.world_pos, 0.0);
    return o;
}
