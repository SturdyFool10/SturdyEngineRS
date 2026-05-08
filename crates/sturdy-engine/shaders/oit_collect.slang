// OIT fragment collection pass (Per-Pixel Linked List) with forward PBR lighting.
//
// Renders Translucent-domain objects into a PPLL with GGX PBR lighting evaluated
// at each fragment, matching the quality of the deferred opaque pass.
//
// Resource bindings:
//   oit_head_buf, oit_frag_pool, oit_counter — OIT storage (set by OitPass)
//   material_desc                             — per-mesh material constants
//   normal_map                                — per-mesh tangent-space normal map
//   lights, light_bvh                         — pre-bound by DeferredPass
//   csm_data, shadow_map_0..3                 — pre-bound by DeferredPass
//   brdf_lut, env_specular, sh9_irradiance    — pre-bound by DeferredPass
//   forward_lighting                          — camera pos + IBL + BVH root
//
// Roadmap: Track 6b — ForwardLitVariant for OIT translucent tail.

import brdf;

// ── Constants ─────────────────────────────────────────────────────────────────

static const uint OIT_INVALID = 0xFFFFFFFFu;
static const uint LEAF_FLAG   = 0x80000000u;
static const uint BVH_EMPTY   = 0xFFFFFFFFu;

// ── OIT storage buffers ───────────────────────────────────────────────────────

struct OitFragment {
    uint  color_rg;  // f16 R (low) + f16 G (high), premultiplied by alpha
    uint  color_ba;  // f16 B (low) + f16 A (high)
    float depth;
    uint  next;
};

RWStructuredBuffer<uint>        oit_head_buf;
RWStructuredBuffer<OitFragment> oit_frag_pool;
RWStructuredBuffer<uint>        oit_counter;

// ── Material ──────────────────────────────────────────────────────────────────

struct MaterialConstants {
    float4 albedo;             // rgb = base color, a = alpha
    float4 emissive_metallic;  // rgb = emissive, a = metallic
    float4 roughness_flags;    // x = roughness
};
StructuredBuffer<MaterialConstants> material_desc;

Texture2D<float4> normal_map;
SamplerState      normal_sampler;

// ── Push constants ────────────────────────────────────────────────────────────

struct OitPushConstants {
    float4x4 view_proj;
    float4x4 previous_view_proj;
    float    time;
    float3   _pad_cam;
    uint     fb_width;
    uint     pool_size;
    float2   _pad_oit;
};
uniform OitPushConstants push;

// ── Light data structs ────────────────────────────────────────────────────────
// Mirrors GpuLightData in scene/scene.rs (96 bytes / 6 × float4).

struct GpuLightData {
    float4 color_intensity;  // rgb × intensity
    float4 position_range;   // xyz = world pos, w = range
    float4 direction_kind;   // xyz = dir, w = kind (0=dir,1=point,2=spot,…)
    float4 misc;             // [cos_inner, cos_outer, area_radius, two_sided]
    float4 right_half_w;
    float4 up_half_h;
};
StructuredBuffer<GpuLightData> lights;

// ── BVH ───────────────────────────────────────────────────────────────────────

struct GpuBvhNode {
    float3 aabb_min;
    uint   left_or_light;
    float3 aabb_max;
    uint   right_or_leaf;
};
StructuredBuffer<GpuBvhNode> light_bvh;

// ── CSM shadow data ───────────────────────────────────────────────────────────

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

// ── IBL ───────────────────────────────────────────────────────────────────────

Texture2D<float4>        brdf_lut;
Texture2DArray<float4>   env_specular;
StructuredBuffer<float4> sh9_irradiance;
StructuredBuffer<float>  e_avg_lut;
SamplerState             gbuffer_sampler;

// ── Forward lighting uniforms ─────────────────────────────────────────────────
// Bound as a 1-element StructuredBuffer by DeferredPass before the OIT phase.

struct ForwardLightingUniforms {
    float4 camera_world_pos;  // xyz = camera world position
    float3 ambient;           // flat ambient fallback (when no IBL)
    uint   dir_light_count;
    float  ibl_strength;      // 0 = no IBL, 1 = full IBL
    float  ibl_max_layer;
    uint   bvh_root;          // BVH root index; BVH_EMPTY if no BVH
    float  _pad;
};
StructuredBuffer<ForwardLightingUniforms> forward_lighting;

// ── VSOut (from mesh_vertex_3d.slang) ────────────────────────────────────────

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
    float4 tangent   : TANGENT;
};

// ── Helpers ───────────────────────────────────────────────────────────────────

uint pack_f16x2(float a, float b) {
    return (uint(f32tof16(a)) & 0xFFFFu) | (uint(f32tof16(b)) << 16u);
}

// ── Shadow helpers ────────────────────────────────────────────────────────────

float pcf3x3_fwd(Texture2D<float> smap, float2 uv, float ref_depth) {
    uint w, h;
    smap.GetDimensions(w, h);
    float2 texel = 1.0 / float2(w, h);
    float  sum   = 0.0;
    [unroll] for (int dx = -1; dx <= 1; ++dx)
    [unroll] for (int dy = -1; dy <= 1; ++dy) {
        float d = smap.SampleLevel(shadow_sampler, uv + float2(dx, dy) * texel, 0);
        sum += (ref_depth <= d) ? 1.0 : 0.0;
    }
    return sum / 9.0;
}

float sample_cascade_fwd(uint cas, float3 world_pos, float bias) {
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
    return pcf3x3_fwd(smap, uv, ref);
}

float evaluate_csm_shadow_fwd(float3 world_pos, float3 camera_pos) {
    GpuCsmData csm = csm_data[0];
    float dist = length(world_pos - camera_pos);

    uint cas = csm.cascade_count - 1;
    for (uint i = 0; i < csm.cascade_count - 1; ++i) {
        if (dist < csm.split_distances[i + 1]) { cas = i; break; }
    }

    float shadow = sample_cascade_fwd(cas, world_pos, csm.depth_bias);

    if (cas + 1 < csm.cascade_count) {
        float split_far   = csm.split_distances[cas + 1];
        float blend_start = split_far * (1.0 - csm.blend_range);
        if (dist > blend_start) {
            float t = (dist - blend_start) / (split_far - blend_start);
            shadow = lerp(shadow, sample_cascade_fwd(cas + 1, world_pos, csm.depth_bias),
                          saturate(t));
        }
    }
    return shadow;
}

// ── IBL helpers ───────────────────────────────────────────────────────────────

float2 dir_to_latlong_fwd(float3 d) {
    d = normalize(d);
    return float2(atan2(d.z, d.x) / (2.0 * 3.14159265) + 0.5,
                  acos(clamp(d.y, -1.0, 1.0)) / 3.14159265);
}

float3 eval_sh9_fwd(float3 N) {
    float basis[9];
    basis[0] = 0.282095;
    basis[1] = 0.488603 * N.y;
    basis[2] = 0.488603 * N.z;
    basis[3] = 0.488603 * N.x;
    basis[4] = 1.092548 * N.x * N.y;
    basis[5] = 1.092548 * N.y * N.z;
    basis[6] = 0.315392 * (3.0 * N.z * N.z - 1.0);
    basis[7] = 1.092548 * N.x * N.z;
    basis[8] = 0.546274 * (N.x * N.x - N.y * N.y);
    float3 r = float3(0, 0, 0);
    for (int i = 0; i < 9; ++i) r += sh9_irradiance[i].xyz * basis[i];
    return max(r, 0.0);
}

float3 sample_specular_env_fwd(float3 refl, float roughness, float ibl_max_layer) {
    float2 uv    = dir_to_latlong_fwd(refl);
    float  layer = roughness * ibl_max_layer;
    int    l0    = int(layer);
    int    l1    = min(l0 + 1, int(ibl_max_layer));
    return lerp(env_specular.Sample(gbuffer_sampler, float3(uv, float(l0))).rgb,
                env_specular.Sample(gbuffer_sampler, float3(uv, float(l1))).rgb,
                frac(layer));
}

// ── BVH light evaluation ──────────────────────────────────────────────────────

float3 eval_bvh_light_fwd(float3 N, float3 V, float3 P,
                           float3 albedo, float metallic, float roughness,
                           GpuLightData light, int kind)
{
    float3 radiance = light.color_intensity.xyz;

    if (kind == 1) {
        // Point light
        float3 to_l = light.position_range.xyz - P;
        float  dist = length(to_l);
        float3 L    = to_l / max(dist, 1e-5);
        float  att  = brdf_point_attenuation(dist, light.position_range.w);
        if (att <= 0.0) return float3(0, 0, 0);
        return brdf_eval(N, V, L, albedo, metallic, roughness, radiance * att);
    }

    if (kind == 2) {
        // Spot light
        float3 to_l = light.position_range.xyz - P;
        float  dist = length(to_l);
        float3 L    = to_l / max(dist, 1e-5);
        float  att  = brdf_point_attenuation(dist, light.position_range.w);
        att *= brdf_spot_attenuation(L, normalize(light.direction_kind.xyz),
                                     light.misc.x, light.misc.y);
        if (att <= 0.0) return float3(0, 0, 0);
        return brdf_eval(N, V, L, albedo, metallic, roughness, radiance * att);
    }

    // Area lights (rect/sphere/disk) — approximate as point source.
    if (kind == 3 || kind == 4 || kind == 5) {
        float3 to_l = light.position_range.xyz - P;
        float  dist = length(to_l);
        float3 L    = to_l / max(dist, 1e-5);
        float  att  = brdf_point_attenuation(dist, light.position_range.w);
        if (att <= 0.0) return float3(0, 0, 0);
        return brdf_eval(N, V, L, albedo, metallic, roughness, radiance * att);
    }

    return float3(0, 0, 0);
}

// ── Main ─────────────────────────────────────────────────────────────────────

void main(VSOut v) {
    MaterialConstants M = material_desc[0];
    float3 base_color = M.albedo.rgb;
    float  alpha      = M.albedo.a;
    float3 emissive   = M.emissive_metallic.xyz;
    float  metallic   = M.emissive_metallic.w;
    float  roughness  = clamp(M.roughness_flags.x, 0.04, 1.0);

    if (alpha < 1.0 / 255.0) return;

    // Apply tangent-space normal mapping via TBN.
    float3 geom_N = normalize(v.normal);
    float3 T      = normalize(v.tangent.xyz);
    float3 B      = cross(geom_N, T) * v.tangent.w;
    float3 tbn_n  = normal_map.Sample(normal_sampler, v.uv).xyz * 2.0 - 1.0;
    float3 N      = normalize(T * tbn_n.x + B * tbn_n.y + geom_N * tbn_n.z);

    ForwardLightingUniforms fl = forward_lighting[0];
    float3 V  = normalize(fl.camera_world_pos.xyz - v.world_pos);
    float3 Lo = float3(0, 0, 0);

    // ── Directional lights ────────────────────────────────────────────────────
    for (uint i = 0; i < fl.dir_light_count; ++i) {
        GpuLightData light = lights[i];
        float3 L   = normalize(light.position_range.xyz);
        float  att = (i == 0) ? evaluate_csm_shadow_fwd(v.world_pos, fl.camera_world_pos.xyz) : 1.0;
        if (att <= 0.0) continue;
        Lo += brdf_eval(N, V, L, base_color, metallic, roughness,
                        light.color_intensity.xyz * att);
    }

    // ── Point / spot / area lights via BVH ───────────────────────────────────
    if (fl.bvh_root != BVH_EMPTY) {
        uint stack[32];
        uint sp = 0u;
        stack[sp++] = fl.bvh_root;

        while (sp > 0u) {
            GpuBvhNode node = light_bvh[stack[--sp]];
            if (any(v.world_pos < node.aabb_min) || any(v.world_pos > node.aabb_max))
                continue;
            if ((node.right_or_leaf & LEAF_FLAG) != 0u) {
                GpuLightData light = lights[node.left_or_light];
                int kind = int(light.direction_kind.w + 0.5);
                Lo += eval_bvh_light_fwd(N, V, v.world_pos, base_color, metallic, roughness,
                                         light, kind);
            } else {
                if (sp + 1u < 32u) {
                    stack[sp++] = node.right_or_leaf;
                    stack[sp++] = node.left_or_light;
                }
            }
        }
    }

    // ── IBL ───────────────────────────────────────────────────────────────────
    float3 ambient;
    if (fl.ibl_strength > 0.001) {
        float  ndv   = max(dot(N, V), 0.0);
        float3 F0    = lerp(float3(0.04, 0.04, 0.04), base_color, metallic);
        float3 F     = F0 + (1.0 - F0) * pow(1.0 - ndv, 5.0);
        float3 R     = reflect(-V, N);
        float3 spec  = sample_specular_env_fwd(R, roughness, fl.ibl_max_layer);
        float4 lut4  = brdf_lut.SampleLevel(gbuffer_sampler, float2(ndv, roughness), 0);
        float2 lut   = lut4.rg;
        float3 ibl_s = spec * (F * lut.x + lut.y);
        float3 irrad = eval_sh9_fwd(N);
        float3 kd    = (1.0 - F) * (1.0 - metallic);

        // Multi-scatter energy compensation.
        float  e_s_white = lut4.b;
        uint   e_avg_idx = uint(roughness * 127.5);
        float  e_avg     = e_avg_lut[e_avg_idx];
        float3 F_avg     = F0 + (1.0 - F0) / 21.0;
        float3 ms_color  = F_avg * e_avg / max(1.0 - F_avg * (1.0 - e_avg), 1e-5);
        float3 ibl_ms    = irrad * ms_color * (1.0 - e_s_white);

        ambient = (irrad * base_color * kd + ibl_s + ibl_ms) * fl.ibl_strength;
    } else {
        ambient = fl.ambient * base_color;
    }

    float3 lit_color = ambient + Lo + emissive;

    // Allocate a slot in the OIT fragment pool (lock-free atomic).
    uint slot;
    InterlockedAdd(oit_counter[0], 1u, slot);
    if (slot >= push.pool_size) return;

    // Pack premultiplied color + alpha into f16 pairs.
    float3 premul = lit_color * alpha;
    OitFragment frag;
    frag.color_rg = pack_f16x2(premul.r, premul.g);
    frag.color_ba = pack_f16x2(premul.b, alpha);
    frag.depth    = v.position.z;
    frag.next     = OIT_INVALID;
    oit_frag_pool[slot] = frag;

    // Link into the per-pixel list (atomic exchange).
    uint head_idx = uint(v.position.y) * push.fb_width + uint(v.position.x);
    uint old_head;
    InterlockedExchange(oit_head_buf[head_idx], slot, old_head);
    oit_frag_pool[slot].next = old_head;
}
