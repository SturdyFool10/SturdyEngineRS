// Deferred lighting pass.
//
// Reads the G-Buffer written by gbuffer_fragment.slang and evaluates
// physically based lighting for each lit pixel.
//
// BRDF: GGX specular (Trowbridge-Reitz NDF, Smith height-correlated G2,
//       Schlick Fresnel) + Lambertian diffuse complement. One directional
//       light from the existing LightingUniforms storage buffer.
//
// Inputs (resolved by name from the frame image/buffer registry):
//   gbuffer_albedo_metallic  Texture2D RGBA8Unorm  (RT0 of the fill pass)
//   gbuffer_normal_rough     Texture2D RGBA16Float (RT1)
//   gbuffer_emissive         Texture2D RGBA16Float (RT2)
//   gbuffer_world_pos        Texture2D RGBA16Float (RT3)
//   lighting                 StructuredBuffer<LightingUniforms>
//
// Push constants:
//   camera_world_pos.xyz     camera position in world space (for Fresnel / specular V)
//
// Output:
//   SV_TARGET0  Linear HDR RGBA — fed directly into the existing post-process chain.

Texture2D<float4> gbuffer_albedo_metallic;
Texture2D<float4> gbuffer_normal_rough;
Texture2D<float4> gbuffer_emissive;
Texture2D<float4> gbuffer_world_pos;
SamplerState      gbuffer_sampler;   // point / no filter — pixel-exact readback

struct LightingUniforms {
    float4 dir_direction;    // xyz = toward-light (normalised), w = intensity
    float4 dir_color;        // xyz = light colour, w = unused
    float4 ambient;          // xyz = ambient colour, w = unused
    float4 camera_world_pos; // xyz = camera world position, w = unused
};
StructuredBuffer<LightingUniforms> lighting;

struct DeferredLightingConstants {
    float4 camera_world_pos; // xyz = camera position, w = unused
};
uniform DeferredLightingConstants push;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

// ── Octahedral normal decode ──────────────────────────────────────────────────

float3 oct_decode(float2 f) {
    float3 n = float3(f.x, f.y, 1.0 - abs(f.x) - abs(f.y));
    if (n.z < 0.0) {
        float2 s = float2(n.x >= 0.0 ? 1.0 : -1.0,
                          n.y >= 0.0 ? 1.0 : -1.0);
        n.xy = (1.0 - abs(n.yx)) * s;
    }
    return normalize(n);
}

// ── GGX BRDF ─────────────────────────────────────────────────────────────────
// All vectors assumed normalised. NdotL clamped before this is called.

// Trowbridge-Reitz / GGX normal distribution function.
float ggx_ndf(float NdotH, float alpha2) {
    float d = NdotH * NdotH * (alpha2 - 1.0) + 1.0;
    return alpha2 / (3.14159265 * d * d);
}

// Smith height-correlated masking-shadowing (exact GGX form).
// Returns G2 / (4 * NdotL * NdotV) — the combined geometry + denominator term,
// so the caller multiplies by D * F only.
float ggx_vis(float NdotL, float NdotV, float alpha2) {
    float denom_l = NdotV * sqrt(alpha2 + NdotL * NdotL * (1.0 - alpha2));
    float denom_v = NdotL * sqrt(alpha2 + NdotV * NdotV * (1.0 - alpha2));
    return 0.5 / max(denom_l + denom_v, 1e-5);
}

// Schlick Fresnel.
float3 fresnel_schlick(float VdotH, float3 F0) {
    float t = 1.0 - saturate(VdotH);
    float t2 = t * t;
    return F0 + (1.0 - F0) * (t2 * t2 * t);
}

// ── Main ─────────────────────────────────────────────────────────────────────

float4 main(VSOut v) : SV_TARGET {
    // Sample G-Buffer.
    float4 g0 = gbuffer_albedo_metallic.SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g1 = gbuffer_normal_rough   .SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g2 = gbuffer_emissive       .SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g3 = gbuffer_world_pos      .SampleLevel(gbuffer_sampler, v.uv, 0);

    float3 albedo    = g0.rgb;
    float  metallic  = g0.a;
    float3 N         = oct_decode(g1.xy);
    float  roughness = g1.z;
    float3 emissive  = g2.rgb;
    float3 world_pos = g3.xyz;

    // Derived surface properties.
    float  alpha     = roughness * roughness;   // perceptual → linear
    float  alpha2    = alpha * alpha;
    float3 F0        = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
    float3 kd_coeff  = (1.0 - F0) * (1.0 - metallic); // no diffuse for conductors

    LightingUniforms L = lighting[0];

    float3 light_dir   = normalize(L.dir_direction.xyz);  // toward-light
    float3 light_color = L.dir_color.xyz * L.dir_direction.w;
    float3 cam_pos     = push.camera_world_pos.xyz;

    float3 V     = normalize(cam_pos - world_pos);
    float3 H     = normalize(light_dir + V);
    float  NdotL = saturate(dot(N, light_dir));
    float  NdotV = saturate(dot(N, V)) + 1e-5;
    float  NdotH = saturate(dot(N, H));
    float  VdotH = saturate(dot(V, H));

    // Specular (GGX).
    float  D   = ggx_ndf(NdotH, alpha2);
    float  Vis = ggx_vis(NdotL, NdotV, alpha2);
    float3 F   = fresnel_schlick(VdotH, F0);
    float3 specular = D * Vis * F * NdotL;

    // Diffuse (Lambertian, energy-conserving complement of specular).
    float3 diffuse = kd_coeff * albedo * (1.0 / 3.14159265) * NdotL;

    // Direct lighting.
    float3 direct = (diffuse + specular) * light_color;

    // Ambient (constant approximation — replaced by IBL in Track 6c).
    float3 ambient = L.ambient.xyz * albedo;

    float3 color = ambient + direct + emissive;
    return float4(color, 1.0);
}
