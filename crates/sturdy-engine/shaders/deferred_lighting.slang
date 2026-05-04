// Deferred lighting pass with PCF shadow mapping.
//
// Reads the G-Buffer written by gbuffer_fragment.slang and evaluates
// physically based lighting for all active lights. The directional light
// is shadowed via a 3×3 PCF kernel sampled from the shadow map.
//
// Inputs (resolved by name from the frame image/buffer registry):
//   gbuffer_albedo_metallic  Texture2D RGBA8Unorm
//   gbuffer_normal_rough     Texture2D RGBA16Float
//   gbuffer_emissive         Texture2D RGBA16Float
//   gbuffer_world_pos        Texture2D RGBA16Float
//   shadow_map               Texture2D Depth32Float
//   lights                   StructuredBuffer<GpuLightData>
//
// Push constants: DeferredLightingConstants (camera_world_pos, ambient, light_count,
//                 light_view_proj, shadow_bias)
//
// Light kinds in GpuLightData.direction_kind.w:
//   0 = directional,  1 = point,  2 = spot

import brdf;

// ── G-Buffer textures ─────────────────────────────────────────────────────────

Texture2D<float4> gbuffer_albedo_metallic;
Texture2D<float4> gbuffer_normal_rough;
Texture2D<float4> gbuffer_emissive;
Texture2D<float4> gbuffer_world_pos;
SamplerState      gbuffer_sampler;

// ── Shadow map ────────────────────────────────────────────────────────────────

Texture2D<float>  shadow_map;
SamplerState      shadow_sampler;  // point/nearest — comparison done manually for portability

// ── Light data ────────────────────────────────────────────────────────────────

struct GpuLightData {
    float4 color_intensity;  // rgb intensity packed; multiply for radiance
    float4 position_range;   // pos or toward-light dir + range
    float4 direction_kind;   // spot dir xyz + kind (0=dir,1=point,2=spot)
    float2 spot_angles;      // cos(inner), cos(outer)
    float2 _pad;
};
StructuredBuffer<GpuLightData> lights;

// ── Push constants ────────────────────────────────────────────────────────────

struct DeferredLightingConstants {
    float4   camera_world_pos;    // xyz = camera pos, w unused
    float3   ambient;             // constant ambient (replaced by IBL in Track 6c)
    uint     light_count;
    float4x4 light_view_proj;     // directional light MVP (for shadow coordinate)
    float    shadow_bias;         // constant depth bias against self-shadowing
    float3   _pad;
};
uniform DeferredLightingConstants push;

// ── Vertex input ──────────────────────────────────────────────────────────────

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

// ── Shadow sampling (3×3 PCF) ─────────────────────────────────────────────────

float sample_shadow_pcf(float3 world_pos) {
    // Transform to light clip space.
    float4 lc = mul(push.light_view_proj, float4(world_pos, 1.0f));
    lc.xyz /= lc.w;

    // Remap [-1,1] NDC to [0,1] UV; Vulkan Y is already top-down in NDC.
    float2 uv  = lc.xy * float2(0.5f, -0.5f) + 0.5f;
    float  ref = lc.z - push.shadow_bias;

    // Early-out if outside the shadow frustum.
    if (any(uv < 0.0f) || any(uv > 1.0f) || ref > 1.0f || ref < 0.0f)
        return 1.0f;

    // 3×3 PCF — average 9 samples.
    uint w, h;
    shadow_map.GetDimensions(w, h);
    float2 texel = 1.0f / float2(w, h);
    float  sum   = 0.0f;
    [unroll]
    for (int dx = -1; dx <= 1; ++dx) {
        [unroll]
        for (int dy = -1; dy <= 1; ++dy) {
            float2 offset = float2(dx, dy) * texel;
            float  depth  = shadow_map.SampleLevel(shadow_sampler, uv + offset, 0);
            sum += (ref <= depth) ? 1.0f : 0.0f;
        }
    }
    return sum / 9.0f;
}

// ── Main ─────────────────────────────────────────────────────────────────────

float4 main(VSOut v) : SV_TARGET {
    float4 g0 = gbuffer_albedo_metallic.SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g1 = gbuffer_normal_rough   .SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g2 = gbuffer_emissive       .SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g3 = gbuffer_world_pos      .SampleLevel(gbuffer_sampler, v.uv, 0);

    float3 albedo    = g0.rgb;
    float  metallic  = g0.a;
    float3 N         = oct_decode(g1.xy);
    float  roughness = clamp(g1.z, 0.04f, 1.0f);
    float3 emissive  = g2.rgb;
    float3 world_pos = g3.xyz;

    float3 V  = normalize(push.camera_world_pos.xyz - world_pos);
    float3 Lo = float3(0.0f, 0.0f, 0.0f);

    for (uint i = 0; i < push.light_count; ++i) {
        GpuLightData light = lights[i];
        float3 light_color = light.color_intensity.xyz; // already multiplied by intensity
        int    kind        = int(light.direction_kind.w + 0.5f);

        float3 L;
        float  attenuation = 1.0f;

        if (kind == 0) {
            // Directional — position_range.xyz is precomputed toward-light direction.
            L = normalize(light.position_range.xyz);
            // Shadow only for the first directional light (i == 0).
            if (i == 0) attenuation = sample_shadow_pcf(world_pos);
        } else {
            float3 to_light = light.position_range.xyz - world_pos;
            float  dist     = length(to_light);
            float  range    = light.position_range.w;
            L           = to_light / max(dist, 1e-5f);
            attenuation = brdf_point_attenuation(dist, range);

            if (kind == 2) {
                float3 spot_dir = normalize(light.direction_kind.xyz);
                attenuation *= brdf_spot_attenuation(L, spot_dir,
                                                     light.spot_angles.x,
                                                     light.spot_angles.y);
            }
        }

        if (attenuation <= 0.0f) continue;
        Lo += brdf_eval(N, V, L, albedo, metallic, roughness, light_color * attenuation);
    }

    float3 ambient = push.ambient * albedo;
    float3 color   = ambient + Lo + emissive;
    return float4(color, 1.0f);
}
