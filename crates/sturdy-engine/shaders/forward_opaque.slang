// Forward-lit opaque fragment shader — full GGX PBR in one pass.
//
// Used by DeferredPass when render_path == ForwardOnly. Evaluates the same
// lighting model as oit_collect.slang but writes directly to the HDR color
// target instead of inserting into the OIT linked list.
//
// All light bindings (lights, light_bvh, csm_data, shadow maps, IBL) are
// pre-bound by DeferredPass::draw() before the forward draw loop runs.
//
// Roadmap: Track 6d — RenderPath::ForwardOnly fallback.

import brdf;

static const uint LEAF_FLAG = 0x80000000u;
static const uint BVH_EMPTY = 0xFFFFFFFFu;

// ── Material ──────────────────────────────────────────────────────────────────

struct MaterialConstants {
    float4 albedo;
    float4 emissive_metallic;
    float4 roughness_flags;
};
StructuredBuffer<MaterialConstants> material_desc;

Texture2D<float4> normal_map;
SamplerState      normal_sampler;

// ── Lighting (pre-bound by DeferredPass) ─────────────────────────────────────

struct GpuLightData {
    float4 color_intensity;
    float4 position_range;
    float4 direction_kind;
    float4 misc;
    float4 right_half_w;
    float4 up_half_h;
};
StructuredBuffer<GpuLightData> lights;

struct GpuBvhNode {
    float3 aabb_min; uint left_or_light;
    float3 aabb_max; uint right_or_leaf;
};
StructuredBuffer<GpuBvhNode> light_bvh;

struct GpuCsmData {
    float4x4 matrices[4];
    float4   split_distances;
    uint     cascade_count;
    float    depth_bias;
    float    blend_range;
    float    _pad;
};
StructuredBuffer<GpuCsmData> csm_data;

Texture2D<float>  shadow_map_0;
Texture2D<float>  shadow_map_1;
Texture2D<float>  shadow_map_2;
Texture2D<float>  shadow_map_3;
SamplerState      shadow_sampler;

Texture2D<float4>        brdf_lut;
Texture2DArray<float4>   env_specular;
StructuredBuffer<float4> sh9_irradiance;
StructuredBuffer<float>  e_avg_lut;
SamplerState             gbuffer_sampler;

struct ForwardLightingUniforms {
    float4 camera_world_pos;
    float3 ambient;
    uint   dir_light_count;
    float  ibl_strength;
    float  ibl_max_layer;
    uint   bvh_root;
    float  _pad;
};
StructuredBuffer<ForwardLightingUniforms> forward_lighting;

// ── VSOut ────────────────────────────────────────────────────────────────────

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
    float4 tangent   : TANGENT;
};

// ── Shadow helpers ────────────────────────────────────────────────────────────

float pcf3x3_fo(Texture2D<float> smap, float2 uv, float ref_d) {
    uint w, h;
    smap.GetDimensions(w, h);
    float2 tx = 1.0 / float2(w, h);
    float  s  = 0.0;
    [unroll] for (int dx = -1; dx <= 1; ++dx)
    [unroll] for (int dy = -1; dy <= 1; ++dy) {
        float d = smap.SampleLevel(shadow_sampler, uv + float2(dx, dy) * tx, 0);
        s += (ref_d <= d) ? 1.0 : 0.0;
    }
    return s / 9.0;
}

float sample_csm_cascade_fo(uint cas, float3 wp, float bias) {
    float4x4 lm = csm_data[0].matrices[cas];
    float4   lc = mul(lm, float4(wp, 1.0));
    lc.xyz /= lc.w;
    float2 uv = lc.xy * float2(0.5, -0.5) + 0.5;
    float  ref = lc.z - bias;
    if (any(uv < 0.0) || any(uv > 1.0) || ref > 1.0 || ref < 0.0) return 1.0;
    Texture2D<float> smap;
    if      (cas == 0) smap = shadow_map_0;
    else if (cas == 1) smap = shadow_map_1;
    else if (cas == 2) smap = shadow_map_2;
    else               smap = shadow_map_3;
    return pcf3x3_fo(smap, uv, ref);
}

float eval_csm_fo(float3 wp, float3 cam) {
    GpuCsmData csm = csm_data[0];
    float dist = length(wp - cam);
    uint cas = csm.cascade_count - 1;
    for (uint i = 0; i < csm.cascade_count - 1; ++i)
        if (dist < csm.split_distances[i + 1]) { cas = i; break; }
    float s = sample_csm_cascade_fo(cas, wp, csm.depth_bias);
    if (cas + 1 < csm.cascade_count) {
        float sf = csm.split_distances[cas + 1];
        float sb = sf * (1.0 - csm.blend_range);
        if (dist > sb) s = lerp(s, sample_csm_cascade_fo(cas + 1, wp, csm.depth_bias),
                                 saturate((dist - sb) / (sf - sb)));
    }
    return s;
}

// ── IBL helpers ───────────────────────────────────────────────────────────────

float2 dir_to_ll(float3 d) {
    d = normalize(d);
    return float2(atan2(d.z, d.x) / (2.0 * 3.14159265) + 0.5,
                  acos(clamp(d.y, -1.0, 1.0)) / 3.14159265);
}

float3 eval_sh9_fo(float3 N) {
    float b[9];
    b[0]=0.282095; b[1]=0.488603*N.y; b[2]=0.488603*N.z;
    b[3]=0.488603*N.x; b[4]=1.092548*N.x*N.y; b[5]=1.092548*N.y*N.z;
    b[6]=0.315392*(3.0*N.z*N.z-1.0); b[7]=1.092548*N.x*N.z;
    b[8]=0.546274*(N.x*N.x-N.y*N.y);
    float3 r = float3(0,0,0);
    for (int i=0;i<9;++i) r += sh9_irradiance[i].xyz * b[i];
    return max(r, 0.0);
}

float3 sample_env_fo(float3 R, float roughness, float ibl_max) {
    float2 uv = dir_to_ll(R);
    float  l  = roughness * ibl_max;
    int l0 = int(l), l1 = min(l0+1, int(ibl_max));
    return lerp(env_specular.Sample(gbuffer_sampler, float3(uv, float(l0))).rgb,
                env_specular.Sample(gbuffer_sampler, float3(uv, float(l1))).rgb,
                frac(l));
}

// ── Main ─────────────────────────────────────────────────────────────────────

float4 main(VSOut v) : SV_TARGET {
    MaterialConstants M = material_desc[0];
    float3 albedo    = M.albedo.rgb;
    float  alpha     = M.albedo.a;
    float3 emissive  = M.emissive_metallic.xyz;
    float  metallic  = M.emissive_metallic.w;
    float  roughness = clamp(M.roughness_flags.x, 0.04, 1.0);

    // Discard masked fragments (MaterialDomain::Masked alpha cutoff).
    if (alpha < 0.5) discard;

    // TBN normal mapping.
    float3 geom_N = normalize(v.normal);
    float3 T      = normalize(v.tangent.xyz);
    float3 B      = cross(geom_N, T) * v.tangent.w;
    float3 tbn_n  = normal_map.Sample(normal_sampler, v.uv).xyz * 2.0 - 1.0;
    float3 N      = normalize(T * tbn_n.x + B * tbn_n.y + geom_N * tbn_n.z);

    ForwardLightingUniforms fl = forward_lighting[0];
    float3 V  = normalize(fl.camera_world_pos.xyz - v.world_pos);
    float3 Lo = float3(0, 0, 0);

    // Directional lights + CSM shadows.
    for (uint i = 0; i < fl.dir_light_count; ++i) {
        GpuLightData L = lights[i];
        float3 Ldir = normalize(L.position_range.xyz);
        float  att  = (i == 0) ? eval_csm_fo(v.world_pos, fl.camera_world_pos.xyz) : 1.0;
        if (att > 0.0)
            Lo += brdf_eval(N, V, Ldir, albedo, metallic, roughness,
                            L.color_intensity.xyz * att);
    }

    // BVH point/spot/area lights.
    if (fl.bvh_root != BVH_EMPTY) {
        uint stack[32]; uint sp = 0u;
        stack[sp++] = fl.bvh_root;
        while (sp > 0u) {
            GpuBvhNode node = light_bvh[stack[--sp]];
            if (any(v.world_pos < node.aabb_min) || any(v.world_pos > node.aabb_max)) continue;
            if ((node.right_or_leaf & LEAF_FLAG) != 0u) {
                GpuLightData L = lights[node.left_or_light];
                int kind = int(L.direction_kind.w + 0.5);
                float3 to_l = L.position_range.xyz - v.world_pos;
                float  dist = length(to_l);
                float3 Ld   = to_l / max(dist, 1e-5);
                float  att  = brdf_point_attenuation(dist, L.position_range.w);
                if (kind == 2) att *= brdf_spot_attenuation(Ld, normalize(L.direction_kind.xyz), L.misc.x, L.misc.y);
                if (att > 0.0)
                    Lo += brdf_eval(N, V, Ld, albedo, metallic, roughness,
                                    L.color_intensity.xyz * att);
            } else {
                if (sp + 1u < 32u) { stack[sp++] = node.right_or_leaf; stack[sp++] = node.left_or_light; }
            }
        }
    }

    // IBL + multi-scatter.
    float3 ambient;
    if (fl.ibl_strength > 0.001) {
        float  ndv  = max(dot(N, V), 0.0);
        float3 F0   = lerp(float3(0.04,0.04,0.04), albedo, metallic);
        float3 F    = F0 + (1.0 - F0) * pow(1.0 - ndv, 5.0);
        float4 lut4 = brdf_lut.SampleLevel(gbuffer_sampler, float2(ndv, roughness), 0);
        float3 spec = sample_env_fo(reflect(-V, N), roughness, fl.ibl_max_layer);
        float3 ibl_s= spec * (F * lut4.x + lut4.y);
        float3 irrad= eval_sh9_fo(N);
        float3 ibl_d= irrad * albedo * (1.0 - F) * (1.0 - metallic);
        float  e_sw = lut4.z;
        float  e_avg= e_avg_lut[uint(roughness * 127.5)];
        float3 F_avg= F0 + (1.0 - F0) / 21.0;
        float3 ms   = irrad * F_avg * e_avg / max(1.0 - F_avg * (1.0 - e_avg), 1e-5) * (1.0 - e_sw);
        ambient = (ibl_d + ibl_s + ms) * fl.ibl_strength;
    } else {
        ambient = fl.ambient * albedo;
    }

    return float4(ambient + Lo + emissive, 1.0);
}
