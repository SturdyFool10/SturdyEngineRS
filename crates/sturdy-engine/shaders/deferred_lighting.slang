// Deferred GGX PBR lighting with cascaded PCF shadows and split-sum IBL.
//
// G-Buffer inputs:   gbuffer_albedo_metallic, gbuffer_normal_rough,
//                    gbuffer_emissive, gbuffer_world_pos
// Shadow inputs:     shadow_map_0..3  (one Texture2D per CSM cascade)
// IBL inputs:        brdf_lut, env_specular (Texture2DArray), sh9_irradiance
// Light inputs:      lights (StructuredBuffer<GpuLightData>)
// CSM data:          csm_data (StructuredBuffer<GpuCsmData>)

import brdf;

// ── G-Buffer ──────────────────────────────────────────────────────────────────

Texture2D<float4> gbuffer_albedo_metallic;
Texture2D<float4> gbuffer_normal_rough;
Texture2D<float4> gbuffer_emissive;
Texture2D<float4> gbuffer_world_pos;
SamplerState      gbuffer_sampler;

// ── Shadow maps (one per cascade, up to 4) ────────────────────────────────────

Texture2D<float>  shadow_map_0;
Texture2D<float>  shadow_map_1;
Texture2D<float>  shadow_map_2;
Texture2D<float>  shadow_map_3;
SamplerState      shadow_sampler;

// ── CSM data ──────────────────────────────────────────────────────────────────

struct GpuCsmData {
    float4x4 matrices[4];          // per-cascade light view-proj
    float4   split_distances;      // view-space linear distances (xyz = splits 0-2, w = far)
    uint     cascade_count;
    float    depth_bias;
    float    blend_range;
    float    _pad;
};
StructuredBuffer<GpuCsmData> csm_data;

// ── IBL ───────────────────────────────────────────────────────────────────────

Texture2D<float4>        brdf_lut;
Texture2DArray<float4>   env_specular;
StructuredBuffer<float4> sh9_irradiance;

// ── Analytic lights ───────────────────────────────────────────────────────────

struct GpuLightData {
    float4 color_intensity;
    float4 position_range;
    float4 direction_kind;
    float2 spot_angles;
    float2 _pad;
};
StructuredBuffer<GpuLightData> lights;

// ── Light BVH (StructuredBuffer<GpuBvhNode>) ─────────────────────────────────
//
// Flat AABB BVH over point and spot lights. Each fragment traverses the BVH
// in O(log N) to find the lights whose influence volumes contain it.
//
// Node layout (32 bytes, matches GpuBvhNode in light_bvh.rs):
//   float3  aabb_min
//   uint    left_or_light   — internal: left child index; leaf: light array idx
//   float3  aabb_max
//   uint    right_or_leaf   — internal: right child index; leaf: LEAF_FLAG

static const uint LEAF_FLAG  = 0x80000000u;
static const uint BVH_EMPTY  = 0xFFFFFFFFu;

struct GpuBvhNode {
    float3 aabb_min;
    uint   left_or_light;
    float3 aabb_max;
    uint   right_or_leaf;
};
StructuredBuffer<GpuBvhNode> light_bvh;

// ── Push constants ────────────────────────────────────────────────────────────

struct DeferredLightingConstants {
    float4 camera_world_pos;    // xyz = camera pos, w unused
    float3 ambient;             // flat ambient fallback when ibl_strength == 0
    uint   dir_light_count;     // number of directional lights (typically 1)
    float  ibl_strength;        // 0 = no IBL, 1 = full IBL
    float  ibl_max_layer;       // SPECULAR_LAYER_COUNT - 1
    uint   bvh_root;            // root index into light_bvh; BVH_EMPTY if no BVH
    float  _pad;
};
uniform DeferredLightingConstants push;

// ── Vertex ────────────────────────────────────────────────────────────────────

struct VSOut { float4 position : SV_POSITION; float2 uv : TEXCOORD0; };

// ── Shadow helpers ────────────────────────────────────────────────────────────

float pcf3x3(Texture2D<float> smap, float2 uv, float ref_depth) {
    uint w, h;
    smap.GetDimensions(w, h);
    float2 texel = 1.0 / float2(w, h);
    float  sum   = 0.0;
    [unroll] for (int dx = -1; dx <= 1; ++dx)
    [unroll] for (int dy = -1; dy <= 1; ++dy) {
        float d = smap.SampleLevel(shadow_sampler, uv + float2(dx,dy)*texel, 0);
        sum += (ref_depth <= d) ? 1.0 : 0.0;
    }
    return sum / 9.0;
}

/// Sample one cascade's PCF shadow factor.
float sample_cascade(uint cas, float3 world_pos, float bias) {
    float4x4 lm = csm_data[0].matrices[cas];
    float4   lc = mul(lm, float4(world_pos, 1.0));
    lc.xyz /= lc.w;
    float2 uv  = lc.xy * float2(0.5, -0.5) + 0.5;
    float  ref = lc.z - bias;
    if (any(uv < 0.0) || any(uv > 1.0) || ref > 1.0 || ref < 0.0) return 1.0;
    Texture2D<float> smap;
    if      (cas == 0) smap = shadow_map_0;
    else if (cas == 1) smap = shadow_map_1;
    else if (cas == 2) smap = shadow_map_2;
    else               smap = shadow_map_3;
    return pcf3x3(smap, uv, ref);
}

/// Select cascade and evaluate shadow with optional blend at boundaries.
float evaluate_csm_shadow(float3 world_pos) {
    GpuCsmData csm = csm_data[0];
    float dist = length(world_pos - push.camera_world_pos.xyz);

    // Select cascade based on view-space distance.
    uint cas = csm.cascade_count - 1;
    for (uint i = 0; i < csm.cascade_count - 1; ++i) {
        if (dist < csm.split_distances[i + 1]) { cas = i; break; }
    }

    float shadow = sample_cascade(cas, world_pos, csm.depth_bias);

    // Blend into the next cascade near the far edge of this cascade.
    if (cas + 1 < csm.cascade_count) {
        float split_far  = csm.split_distances[cas + 1];
        float blend_start = split_far * (1.0 - csm.blend_range);
        if (dist > blend_start) {
            float t = (dist - blend_start) / (split_far - blend_start);
            float next_shadow = sample_cascade(cas + 1, world_pos, csm.depth_bias);
            shadow = lerp(shadow, next_shadow, saturate(t));
        }
    }
    return shadow;
}

// ── IBL helpers ───────────────────────────────────────────────────────────────

float2 dir_to_latlong(float3 d) {
    d = normalize(d);
    return float2(atan2(d.z, d.x) / (2.0 * 3.14159265) + 0.5,
                  acos(clamp(d.y, -1.0, 1.0)) / 3.14159265);
}

float3 eval_sh9(float3 N) {
    float basis[9];
    basis[0]=0.282095; basis[1]=0.488603*N.y; basis[2]=0.488603*N.z;
    basis[3]=0.488603*N.x; basis[4]=1.092548*N.x*N.y; basis[5]=1.092548*N.y*N.z;
    basis[6]=0.315392*(3.0*N.z*N.z-1.0); basis[7]=1.092548*N.x*N.z;
    basis[8]=0.546274*(N.x*N.x-N.y*N.y);
    float3 r = float3(0,0,0);
    for (int i=0; i<9; ++i) r += sh9_irradiance[i].xyz * basis[i];
    return max(r, 0.0);
}

float3 sample_specular_env(float3 refl, float roughness) {
    float2 uv    = dir_to_latlong(refl);
    float  layer = roughness * push.ibl_max_layer;
    int    l0    = int(layer), l1 = min(l0+1, int(push.ibl_max_layer));
    return lerp(env_specular.Sample(gbuffer_sampler, float3(uv, float(l0))).rgb,
                env_specular.Sample(gbuffer_sampler, float3(uv, float(l1))).rgb,
                frac(layer));
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
    float3 V         = normalize(push.camera_world_pos.xyz - world_pos);

    float3 Lo = float3(0,0,0);

    // ── Directional lights (index 0..dir_light_count, always evaluated) ───────
    for (uint i = 0; i < push.dir_light_count; ++i) {
        GpuLightData light = lights[i];
        float3 L   = normalize(light.position_range.xyz);
        float  att = (i == 0) ? evaluate_csm_shadow(world_pos) : 1.0;
        if (att <= 0.0) continue;
        Lo += brdf_eval(N, V, L, albedo, metallic, roughness,
                        light.color_intensity.xyz * att);
    }

    // ── Point + spot lights via BVH traversal — O(log N) ─────────────────────
    //
    // Each fragment walks the BVH from the root, discarding subtrees whose
    // AABB does not contain world_pos. Leaf nodes evaluate one light.
    // Stack depth of 32 handles up to 2^32 lights.
    if (push.bvh_root != BVH_EMPTY) {
        uint stack[32];
        uint sp = 0u;
        stack[sp++] = push.bvh_root;

        while (sp > 0u) {
            GpuBvhNode node = light_bvh[stack[--sp]];

            // Point-in-AABB test — skip entire subtree if fragment is outside.
            if (any(world_pos < node.aabb_min) || any(world_pos > node.aabb_max))
                continue;

            if ((node.right_or_leaf & LEAF_FLAG) != 0u) {
                // Leaf: evaluate this light.
                GpuLightData light = lights[node.left_or_light];
                float3 to_l = light.position_range.xyz - world_pos;
                float  dist = length(to_l);
                float3 L    = to_l / max(dist, 1e-5);
                float  att  = brdf_point_attenuation(dist, light.position_range.w);
                int kind = int(light.direction_kind.w + 0.5);
                if (kind == 2)
                    att *= brdf_spot_attenuation(L, normalize(light.direction_kind.xyz),
                                                 light.spot_angles.x, light.spot_angles.y);
                if (att > 0.0)
                    Lo += brdf_eval(N, V, L, albedo, metallic, roughness,
                                    light.color_intensity.xyz * att);
            } else {
                // Internal: push both children (right first so left is processed first).
                if (sp + 1u < 32u) {
                    stack[sp++] = node.right_or_leaf;
                    stack[sp++] = node.left_or_light;
                }
            }
        }
    }

    // ── IBL ───────────────────────────────────────────────────────────────────
    float3 ambient;
    if (push.ibl_strength > 0.001) {
        float  ndv   = max(dot(N, V), 0.0);
        float3 F0    = lerp(float3(0.04,0.04,0.04), albedo, metallic);
        float3 F     = F0 + (1.0 - F0) * pow(1.0 - ndv, 5.0);
        float3 R     = reflect(-V, N);
        float3 spec  = sample_specular_env(R, roughness);
        float2 lut   = brdf_lut.SampleLevel(gbuffer_sampler, float2(ndv, roughness), 0).rg;
        float3 ibl_s = spec * (F * lut.x + lut.y);
        float3 irrad = eval_sh9(N);
        float3 kd    = (1.0 - F) * (1.0 - metallic);
        float3 ibl_d = irrad * albedo * kd;
        ambient = (ibl_d + ibl_s) * push.ibl_strength;
    } else {
        ambient = push.ambient * albedo;
    }

    return float4(ambient + Lo + emissive, 1.0);
}
