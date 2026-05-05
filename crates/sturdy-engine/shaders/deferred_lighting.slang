// Deferred GGX PBR lighting pass with PCF shadows and split-sum IBL.
//
// Reads the G-Buffer written by gbuffer_fragment.slang and evaluates
// physically based lighting for all active lights plus image-based lighting.
//
// Inputs (resolved by name from the frame image/buffer registry):
//   gbuffer_albedo_metallic  Texture2D RGBA8Unorm
//   gbuffer_normal_rough     Texture2D RGBA16Float
//   gbuffer_emissive         Texture2D RGBA16Float
//   gbuffer_world_pos        Texture2D RGBA16Float
//   shadow_map               Texture2D Depth32Float
//   lights                   StructuredBuffer<GpuLightData>
//   brdf_lut                 Texture2D RGBA16Float (128×128, R=scale G=bias)
//   env_specular             Texture2DArray RGBA16Float (SPECULAR_LAYER_COUNT layers)
//   sh9_irradiance           StructuredBuffer<float4> (9 elements, xyz=coefficients)
//
// When IBL is unavailable, ibl_strength == 0.0 and the IBL terms are skipped;
// the push.ambient constant is used as a simple flat ambient instead.

import brdf;

// ── G-Buffer textures ─────────────────────────────────────────────────────────

Texture2D<float4> gbuffer_albedo_metallic;
Texture2D<float4> gbuffer_normal_rough;
Texture2D<float4> gbuffer_emissive;
Texture2D<float4> gbuffer_world_pos;
SamplerState      gbuffer_sampler;

// ── Shadow map ────────────────────────────────────────────────────────────────

Texture2D<float>  shadow_map;
SamplerState      shadow_sampler;

// ── IBL resources (optional — bound by DeferredPass when env map is set) ──────

Texture2D<float4>      brdf_lut;        // R=F_scale, G=F_bias  (N·V × roughness)
Texture2DArray<float4> env_specular;    // prefiltered specular env, layers = roughness levels
StructuredBuffer<float4> sh9_irradiance; // SH2 coefficients, 9 elements, xyz used

// ── Light data ────────────────────────────────────────────────────────────────

struct GpuLightData {
    float4 color_intensity;  // rgb × intensity
    float4 position_range;   // pos or toward-light dir + range
    float4 direction_kind;   // spot dir xyz + kind (0=dir,1=point,2=spot)
    float2 spot_angles;      // cos(inner), cos(outer)
    float2 _pad;
};
StructuredBuffer<GpuLightData> lights;

// ── Push constants ────────────────────────────────────────────────────────────

struct DeferredLightingConstants {
    float4   camera_world_pos;    // xyz = camera pos, w unused
    float3   ambient;             // flat ambient fallback (used when ibl_strength == 0)
    uint     light_count;
    float4x4 light_view_proj;
    float    shadow_bias;
    float    ibl_strength;        // 0 = no IBL, 1 = full IBL
    float    ibl_max_layer;       // SPECULAR_LAYER_COUNT - 1 (e.g. 7.0)
    float    _pad;
};
uniform DeferredLightingConstants push;

// ── Vertex input ──────────────────────────────────────────────────────────────

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

// ── IBL helpers ───────────────────────────────────────────────────────────────

/// Direction → equirectangular UV.
float2 dir_to_latlong(float3 d) {
    d = normalize(d);
    float u = atan2(d.z, d.x) / (2.0 * 3.14159265) + 0.5;
    float v = acos(clamp(d.y, -1.0, 1.0)) / 3.14159265;
    return float2(u, v);
}

/// Evaluate 9 SH2 (L0+L1+L2) basis functions at unit direction N and return irradiance.
float3 eval_sh9(float3 N, StructuredBuffer<float4> coeffs) {
    float3 result = float3(0.0, 0.0, 0.0);
    float  basis[9];
    basis[0] =  0.282095;
    basis[1] =  0.488603 * N.y;
    basis[2] =  0.488603 * N.z;
    basis[3] =  0.488603 * N.x;
    basis[4] =  1.092548 * N.x * N.y;
    basis[5] =  1.092548 * N.y * N.z;
    basis[6] =  0.315392 * (3.0 * N.z * N.z - 1.0);
    basis[7] =  1.092548 * N.x * N.z;
    basis[8] =  0.546274 * (N.x * N.x - N.y * N.y);
    for (int i = 0; i < 9; ++i) {
        result += coeffs[i].xyz * basis[i];
    }
    return max(result, float3(0.0, 0.0, 0.0));
}

/// Sample the prefiltered specular env at the given reflection direction and roughness.
/// Lerps between adjacent layers for smooth roughness transitions.
float3 sample_env_specular(float3 refl, float roughness) {
    float2 uv    = dir_to_latlong(refl);
    float  layer = roughness * push.ibl_max_layer;
    int    l0    = int(layer);
    int    l1    = min(l0 + 1, int(push.ibl_max_layer));
    float  t     = frac(layer);
    float3 s0    = env_specular.Sample(gbuffer_sampler, float3(uv, float(l0))).rgb;
    float3 s1    = env_specular.Sample(gbuffer_sampler, float3(uv, float(l1))).rgb;
    return lerp(s0, s1, t);
}

// ── Shadow (3×3 PCF) ──────────────────────────────────────────────────────────

float sample_shadow_pcf(float3 world_pos) {
    float4 lc = mul(push.light_view_proj, float4(world_pos, 1.0));
    lc.xyz /= lc.w;
    float2 uv  = lc.xy * float2(0.5, -0.5) + 0.5;
    float  ref = lc.z - push.shadow_bias;
    if (any(uv < 0.0) || any(uv > 1.0) || ref > 1.0 || ref < 0.0) return 1.0;
    uint w, h;
    shadow_map.GetDimensions(w, h);
    float2 texel = 1.0 / float2(w, h);
    float  sum   = 0.0;
    [unroll]
    for (int dx = -1; dx <= 1; ++dx) {
        [unroll]
        for (int dy = -1; dy <= 1; ++dy) {
            float depth = shadow_map.SampleLevel(shadow_sampler, uv + float2(dx,dy)*texel, 0);
            sum += (ref <= depth) ? 1.0 : 0.0;
        }
    }
    return sum / 9.0;
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
    float  roughness = clamp(g1.z, 0.04, 1.0);
    float3 emissive  = g2.rgb;
    float3 world_pos = g3.xyz;

    float3 V  = normalize(push.camera_world_pos.xyz - world_pos);
    float3 Lo = float3(0.0, 0.0, 0.0);

    // ── Analytic lights ───────────────────────────────────────────────────────
    for (uint i = 0; i < push.light_count; ++i) {
        GpuLightData light = lights[i];
        float3 light_color = light.color_intensity.xyz;
        int    kind        = int(light.direction_kind.w + 0.5);

        float3 L;
        float  attenuation = 1.0;

        if (kind == 0) {
            L = normalize(light.position_range.xyz);
            if (i == 0) attenuation = sample_shadow_pcf(world_pos);
        } else {
            float3 to_light = light.position_range.xyz - world_pos;
            float  dist     = length(to_light);
            float  range    = light.position_range.w;
            L           = to_light / max(dist, 1e-5);
            attenuation = brdf_point_attenuation(dist, range);
            if (kind == 2) {
                attenuation *= brdf_spot_attenuation(
                    L, normalize(light.direction_kind.xyz),
                    light.spot_angles.x, light.spot_angles.y
                );
            }
        }

        if (attenuation <= 0.0) continue;
        Lo += brdf_eval(N, V, L, albedo, metallic, roughness, light_color * attenuation);
    }

    // ── IBL (split-sum specular + SH9 diffuse) ────────────────────────────────
    float3 ambient;
    if (push.ibl_strength > 0.001) {
        float  n_dot_v  = max(dot(N, V), 0.0);
        float3 F0       = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
        float3 F        = F0 + (1.0 - F0) * pow(1.0 - n_dot_v, 5.0);

        // Specular — sample prefiltered env + BRDF LUT.
        float3 R             = reflect(-V, N);
        float3 prefiltered   = sample_env_specular(R, roughness);
        float2 env_brdf      = brdf_lut.SampleLevel(gbuffer_sampler,
                                   float2(n_dot_v, roughness), 0).rg;
        float3 ibl_specular  = prefiltered * (F * env_brdf.x + env_brdf.y);

        // Diffuse — SH9 irradiance.
        float3 irradiance   = eval_sh9(N, sh9_irradiance);
        float3 kd           = (1.0 - F) * (1.0 - metallic);
        float3 ibl_diffuse  = irradiance * albedo * kd;

        ambient = (ibl_diffuse + ibl_specular) * push.ibl_strength;
    } else {
        ambient = push.ambient * albedo;
    }

    float3 color = ambient + Lo + emissive;
    return float4(color, 1.0);
}
