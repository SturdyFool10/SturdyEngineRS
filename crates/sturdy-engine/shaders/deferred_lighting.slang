// Deferred GGX PBR lighting with cascaded PCF shadows and split-sum IBL.
//
// G-Buffer inputs:   gbuffer_albedo_metallic, gbuffer_normal_rough, gbuffer_emissive,
//                    gbuffer_depth (world pos reconstructed from depth + inv_view_proj)
// Shadow inputs:     shadow_map_0..3  (one Texture2D per CSM cascade)
// IBL inputs:        brdf_lut, env_specular (Texture2DArray), sh9_irradiance
// Light inputs:      lights (StructuredBuffer<GpuLightData>)
// CSM data:          csm_data (StructuredBuffer<GpuCsmData>)

import brdf;
import material_surface;

// ── G-Buffer ──────────────────────────────────────────────────────────────────
// G3 (world_pos) removed — world position reconstructed from gbuffer_depth.

Texture2D<float4> gbuffer_albedo_metallic;
Texture2D<float4> gbuffer_normal_rough;
Texture2D<float4> gbuffer_emissive;
Texture2D<float>  gbuffer_depth;    // hardware depth [0,1] — used for world pos reconstruction
SamplerState      gbuffer_sampler;

// ── Shadow atlas ──────────────────────────────────────────────────────────────
// All CSM cascades are packed into a single Depth32Float atlas (2×2 tile grid).
// The legacy per-cascade names (shadow_map_0..3) are also registered as aliases
// so forward/OIT shaders continue to work unmodified.

Texture2D<float>  shadow_atlas;   // packed cascade atlas (replaces shadow_map_0..3)
// Legacy aliases — bound to the same atlas image for compatibility.
Texture2D<float>  shadow_map_0;
Texture2D<float>  shadow_map_1;
Texture2D<float>  shadow_map_2;
Texture2D<float>  shadow_map_3;
SamplerState      shadow_sampler;

// ── CSM data ──────────────────────────────────────────────────────────────────

struct GpuCsmData {
    float4x4 matrices[4];          // per-cascade light view-proj        → 256 bytes
    float4   split_distances;      // view-space linear distances (x=cas0…w=cas3) → 16 bytes
    uint     cascade_count;        //  4
    float    depth_bias;           //  4
    float    blend_range;          //  4
    uint     pcss_enabled;         //  4 bytes → 288
    float4   atlas_tiles[4];       // 64 bytes → 352
    float4   pcss_light_size_uv;   // 16 bytes → 368  (x=cas0…w=cas3)
};
StructuredBuffer<GpuCsmData> csm_data;

// ── IBL ───────────────────────────────────────────────────────────────────────

Texture2D<float4>        brdf_lut;
Texture2DArray<float4>   env_specular;
StructuredBuffer<float4> sh9_irradiance;
/// E_avg(roughness) LUT: 128 float32 values.
/// Hemispherical average of E_s_white(NdotV, roughness) — used for multi-scatter.
StructuredBuffer<float>  e_avg_lut;

// ── Spot light shadow maps ────────────────────────────────────────────────────
// Up to MAX_SPOT_SHADOWS (4) spot lights cast shadows via individual depth maps.

static const uint MAX_SPOT_SHADOWS_SHADER = 4u;
static const uint SPOT_SHADOW_INVALID    = 0xFFFFFFFFu;

struct GpuSpotShadowData {
    float4x4 matrices[4];      // light view-proj per caster
    uint     light_indices[4]; // index into `lights[]`; SPOT_SHADOW_INVALID = unused
    uint     count;
    float    depth_bias;
    uint     _pad0;
    uint     _pad1;
};
StructuredBuffer<GpuSpotShadowData> spot_shadow_data;

Texture2D<float> spot_shadow_map_0;
Texture2D<float> spot_shadow_map_1;
Texture2D<float> spot_shadow_map_2;
Texture2D<float> spot_shadow_map_3;

float sample_spot_shadow(uint slot, float3 world_pos) {
    float4x4 lm = spot_shadow_data[0].matrices[slot];
    float4   lc = mul(lm, float4(world_pos, 1.0));
    lc.xyz /= lc.w;
    float2 uv  = lc.xy * float2(0.5, -0.5) + 0.5;
    float  ref = lc.z - spot_shadow_data[0].depth_bias;
    if (any(uv < 0.0) || any(uv > 1.0) || ref > 1.0 || ref < 0.0) return 1.0;
    Texture2D<float> smap;
    if      (slot == 0) smap = spot_shadow_map_0;
    else if (slot == 1) smap = spot_shadow_map_1;
    else if (slot == 2) smap = spot_shadow_map_2;
    else                smap = spot_shadow_map_3;
    return pcf3x3(smap, uv, ref);
}

/// Return PCF shadow factor for a spot light identified by its global light index.
/// Returns 1.0 (no shadow) if the light has no shadow map assigned.
float spot_light_shadow_factor(uint light_idx, float3 world_pos) {
    GpuSpotShadowData sd = spot_shadow_data[0];
    for (uint s = 0; s < sd.count && s < MAX_SPOT_SHADOWS_SHADER; ++s) {
        if (sd.light_indices[s] == light_idx)
            return sample_spot_shadow(s, world_pos);
    }
    return 1.0;
}

// ── Point light shadow maps (dual-paraboloid) ─────────────────────────────────

static const uint MAX_POINT_SHADOWS_SHADER = 4u;
static const uint POINT_SHADOW_INVALID     = 0xFFFFFFFFu;

struct GpuPointShadowData {
    float4x4 world_to_light[4]; // world → light-local (translation only)
    float2   near_far[4];       // [near, far] per caster
    uint     light_indices[4];  // global index into lights[]; INVALID = unused
    uint     count;
    float    depth_bias;
    uint     _pad0;
    uint     _pad1;
};
StructuredBuffer<GpuPointShadowData> point_shadow_data;

Texture2D<float> point_shadow_front_0;
Texture2D<float> point_shadow_front_1;
Texture2D<float> point_shadow_front_2;
Texture2D<float> point_shadow_front_3;
Texture2D<float> point_shadow_back_0;
Texture2D<float> point_shadow_back_1;
Texture2D<float> point_shadow_back_2;
Texture2D<float> point_shadow_back_3;

/// Sample a dual-paraboloid shadow map for one shadow caster slot.
/// Returns 1.0 (lit) or 0.0 (in shadow).
float sample_point_shadow(uint slot, float3 world_pos) {
    GpuPointShadowData psd = point_shadow_data[0];
    float4x4 wtl  = psd.world_to_light[slot];
    float    near = psd.near_far[slot].x;
    float    far  = psd.near_far[slot].y;

    float3 lp   = mul(wtl, float4(world_pos, 1.0)).xyz;
    float  dist = length(lp);
    if (dist < near || dist > far) return 1.0;

    float lin_depth = (dist - near) / (far - near);

    // Determine hemisphere: back if lp.z < 0; flip z so formula is uniform.
    bool   is_back = lp.z < 0.0;
    float3 lp_n    = lp / max(dist, 1e-5);
    if (is_back) lp_n.z = -lp_n.z;

    // Paraboloid UV in [-1, 1].
    float2 dp_uv = lp_n.xy / (1.0 + lp_n.z);
    if (any(abs(dp_uv) > 1.0)) return 1.0;  // outside paraboloid coverage

    // Remap to [0, 1] matching the vertex shader's NDC output convention:
    //   NDC.x = dp_uv.x   → UV.x = (dp_uv.x + 1) / 2
    //   NDC.y = -dp_uv.y  → UV.y = (1 - dp_uv.y) / 2
    float2 uv = float2((dp_uv.x + 1.0) * 0.5, (1.0 - dp_uv.y) * 0.5);

    float ref = lin_depth - psd.depth_bias;

    Texture2D<float> smap;
    if (!is_back) {
        if      (slot == 0) smap = point_shadow_front_0;
        else if (slot == 1) smap = point_shadow_front_1;
        else if (slot == 2) smap = point_shadow_front_2;
        else                smap = point_shadow_front_3;
    } else {
        if      (slot == 0) smap = point_shadow_back_0;
        else if (slot == 1) smap = point_shadow_back_1;
        else if (slot == 2) smap = point_shadow_back_2;
        else                smap = point_shadow_back_3;
    }
    float stored = smap.SampleLevel(gbuffer_sampler, uv, 0);
    return (stored < ref) ? 0.0 : 1.0;
}

/// Return shadow factor [0, 1] for a point light identified by its global light index.
/// Returns 1.0 (fully lit) when the light has no shadow map assigned.
float point_light_shadow_factor(uint light_idx, float3 world_pos) {
    GpuPointShadowData psd = point_shadow_data[0];
    for (uint s = 0u; s < psd.count && s < MAX_POINT_SHADOWS_SHADER; ++s) {
        if (psd.light_indices[s] == light_idx)
            return sample_point_shadow(s, world_pos);
    }
    return 1.0;
}

// ── Analytic lights ───────────────────────────────────────────────────────────
// GpuLightData — 96 bytes (6 × float4), matches scene/scene.rs GpuLightData.
//
//   color_intensity  rgb × intensity
//   position_range   world pos + influence range (0 for directional)
//   direction_kind   dir/normal.xyz + kind (0=dir,1=point,2=spot,3=rect,4=sphere,5=disk)
//   misc             [cos_inner, cos_outer, area_radius, two_sided (0/1)]
//   right_half_w     rect: right.xyz + half_width;  others: zero
//   up_half_h        rect: up.xyz    + half_height; others: zero

struct GpuLightData {
    float4 color_intensity;
    float4 position_range;
    float4 direction_kind;  // .w = kind
    float4 misc;            // [cos_inner, cos_outer, area_radius, two_sided]
    float4 right_half_w;
    float4 up_half_h;
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
    float4   camera_world_pos;   // xyz = camera pos, w unused
    float3   ambient;            // flat ambient fallback when ibl_strength == 0
    uint     dir_light_count;
    float    ibl_strength;       // 0 = no IBL, 1 = full IBL
    float    ibl_max_layer;      // SPECULAR_LAYER_COUNT - 1
    uint     bvh_root;           // root index into light_bvh; BVH_EMPTY if no BVH
    uint     sky_enabled;        // 1 = render procedural sky for background pixels
    float4x4 inv_view_proj;      // inverse(proj × view) — for world pos reconstruction
    float    sky_turbidity;      // atmospheric turbidity [1, 10]
    float    sky_exposure;       // sky exposure multiplier
    float    sky_sun_size;       // cos(sun_disc_radius) — dot threshold for sun disc
    float    _pad;
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

// ── PCSS — Percentage-Closer Soft Shadows ─────────────────────────────────────
//
// Two-pass algorithm (Fernando 2005):
//   1. Blocker search — find average depth of occluders within a search disc.
//   2. Penumbra estimation — penumbra_radius = (d_receiver - d_avg_blocker)
//                                              / d_avg_blocker * light_size_uv
//   3. Variable-kernel PCF — PCF using the penumbra radius.
//
// Both passes use the same 32-point Poisson disc. Blocker search uses 16 taps,
// PCF uses all 32. The atlas tile bounds are respected so lookups never bleed
// into a neighbouring cascade.

static const float2 POISSON_DISK[32] = {
    float2(-0.975402, -0.091116), float2( 0.220162, -0.938053),
    float2( 0.987766,  0.159899), float2( 0.717278,  0.655820),
    float2(-0.171612,  0.994272), float2(-0.778744,  0.626903),
    float2(-0.390991,  0.921700), float2( 0.474516, -0.880023),
    float2(-0.498705, -0.849979), float2(-0.980862,  0.194701),
    float2( 0.534441,  0.845017), float2( 0.218253,  0.975985),
    float2(-0.683882, -0.726618), float2( 0.950821, -0.309053),
    float2( 0.128643,  0.406538), float2(-0.358577, -0.433583),
    float2(-0.773359,  0.193378), float2( 0.441798,  0.133218),
    float2( 0.765778, -0.640960), float2( 0.175520, -0.459618),
    float2(-0.234099,  0.228696), float2(-0.041654, -0.757143),
    float2( 0.589093,  0.435432), float2(-0.647628,  0.012411),
    float2(-0.284987, -0.034764), float2( 0.756773,  0.268025),
    float2(-0.437107,  0.561498), float2( 0.338858, -0.234398),
    float2(-0.798551, -0.370025), float2( 0.023987,  0.638500),
    float2(-0.527001,  0.812997), float2( 0.327498,  0.726093),
};

/// Step 1: Average depth of blockers within a search disc.
/// Returns -1 if no blockers found (receiver is fully lit).
float find_avg_blocker_depth(float2 atlas_uv, float ref_depth,
                              float search_radius, float4 tile_bounds) {
    float sum   = 0.0;
    int   count = 0;
    for (int i = 0; i < 16; ++i) {
        float2 uv = clamp(atlas_uv + POISSON_DISK[i] * search_radius,
                          tile_bounds.xy, tile_bounds.zw);
        float d = shadow_atlas.SampleLevel(shadow_sampler, uv, 0);
        if (ref_depth > d) { sum += d; count++; }
    }
    return count > 0 ? sum / float(count) : -1.0;
}

/// Step 3: PCF over a variable-radius disc.
float pcf_disk(float2 atlas_uv, float ref_depth,
               float radius, float4 tile_bounds) {
    float sum = 0.0;
    for (int i = 0; i < 32; ++i) {
        float2 uv = clamp(atlas_uv + POISSON_DISK[i] * radius,
                          tile_bounds.xy, tile_bounds.zw);
        float d = shadow_atlas.SampleLevel(shadow_sampler, uv, 0);
        sum += (ref_depth <= d) ? 1.0 : 0.0;
    }
    return sum / 32.0;
}

/// Full PCSS shadow factor for one cascade (atlas tile already selected).
float sample_cascade_pcss(float2 atlas_uv, float ref_depth,
                           float light_size_uv, float4 tile_bounds) {
    // Step 1: blocker search.
    float avg_blocker = find_avg_blocker_depth(atlas_uv, ref_depth,
                                               light_size_uv, tile_bounds);
    if (avg_blocker < 0.0) return 1.0; // no occluder → fully lit

    // Step 2: penumbra radius (clamped so we don't over-blur near-contact shadows).
    float penumbra = (ref_depth - avg_blocker) / avg_blocker * light_size_uv;
    // Minimum of 1.5 texels in atlas space to avoid aliasing at near contact.
    float2 atlas_size;
    shadow_atlas.GetDimensions(atlas_size.x, atlas_size.y);
    float min_radius = 1.5 / atlas_size.x;
    penumbra = max(penumbra, min_radius);

    // Step 3: PCF with penumbra-sized disc.
    return pcf_disk(atlas_uv, ref_depth, penumbra, tile_bounds);
}

/// Sample one cascade's shadow factor from the shadow atlas.
/// Dispatches to PCSS or fixed 3×3 PCF based on csm_data[0].pcss_enabled.
float sample_cascade(uint cas, float3 world_pos, float bias) {
    float4x4 lm = csm_data[0].matrices[cas];
    float4   lc = mul(lm, float4(world_pos, 1.0));
    lc.xyz /= lc.w;
    float2 tile_uv = lc.xy * float2(0.5, -0.5) + 0.5;
    float  ref     = lc.z - bias;
    if (any(tile_uv < 0.0) || any(tile_uv > 1.0) || ref > 1.0 || ref < 0.0) return 1.0;

    float4 tile      = csm_data[0].atlas_tiles[cas];
    float2 tile_size = tile.zw - tile.xy;
    float2 atlas_uv  = tile.xy + tile_uv * tile_size;

    if (csm_data[0].pcss_enabled != 0u) {
        float light_size = csm_data[0].pcss_light_size_uv[cas];
        return sample_cascade_pcss(atlas_uv, ref, light_size, tile);
    }
    return pcf3x3(shadow_atlas, atlas_uv, ref);
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

// ── Area light evaluation ─────────────────────────────────────────────────────
//
// Diffuse: exact polygon solid-angle integral (Arvo 1995 / Ureña 2013 formula).
// Specular: representative-point approximation (Karis 2013) — find the closest
//           point on the area shape to the reflection ray, use it as a virtual
//           point light with a modified roughness for energy conservation.

/// Solid-angle integral of a polygon projected onto the unit hemisphere above N.
/// verts[] must be in world space; already normalized to (P → vertex).
float polygon_irradiance(float3 N, float3 verts[4], uint n) {
    float sum = 0.0;
    for (uint i = 0; i < n; ++i) {
        float3 a = verts[i];
        float3 b = verts[(i + 1) % n];
        float3 c = cross(a, b);
        float  len_c = length(c);
        if (len_c < 1e-6) continue;
        sum += acos(clamp(dot(a, b), -1.0, 1.0)) * dot(c / len_c, N);
    }
    return max(0.0, sum) * (1.0 / (2.0 * 3.14159265));
}

/// Rect light: clamp a world-space point to the rectangle surface.
float3 rect_closest_point(float3 hit, float3 center, float3 right_n, float3 up_n, float hw, float hh) {
    float3 rel = hit - center;
    float u = clamp(dot(rel, right_n), -hw, hw);
    float v = clamp(dot(rel, up_n),   -hh, hh);
    return center + right_n * u + up_n * v;
}

/// Evaluate one BVH-leaf light (point, spot, rect, sphere, disk).
/// `light_idx` is the global index into `lights[]`, used for spot shadow lookup.
float3 eval_bvh_light(float3 N, float3 V, float3 P,
                      float3 albedo, float metallic, float roughness,
                      GpuLightData light, int kind, uint light_idx)
{
    float3 radiance = light.color_intensity.xyz;

    if (kind == 1) {
        // ── Point light ──────────────────────────────────────────────────────
        float3 to_l = light.position_range.xyz - P;
        float  dist = length(to_l);
        float3 L    = to_l / max(dist, 1e-5);
        float  att  = brdf_point_attenuation(dist, light.position_range.w);
        if (att <= 0.0) return 0.0;
        att *= point_light_shadow_factor(light_idx, P);
        if (att <= 0.0) return 0.0;
        return brdf_eval(N, V, L, albedo, metallic, roughness, radiance * att);
    }

    if (kind == 2) {
        // ── Spot light ───────────────────────────────────────────────────────
        float3 to_l = light.position_range.xyz - P;
        float  dist = length(to_l);
        float3 L    = to_l / max(dist, 1e-5);
        float  att  = brdf_point_attenuation(dist, light.position_range.w);
        att *= brdf_spot_attenuation(L, normalize(light.direction_kind.xyz),
                                     light.misc.x, light.misc.y);
        if (att <= 0.0) return 0.0;
        // Apply spot shadow (1.0 when no shadow map is assigned).
        att *= spot_light_shadow_factor(light_idx, P);
        if (att <= 0.0) return 0.0;
        return brdf_eval(N, V, L, albedo, metallic, roughness, radiance * att);
    }

    if (kind == 3) {
        // ── Rect light ───────────────────────────────────────────────────────
        float3 center  = light.position_range.xyz;
        float3 normal  = normalize(light.direction_kind.xyz);
        float3 right_n = normalize(light.right_half_w.xyz);
        float3 up_n    = normalize(light.up_half_h.xyz);
        float  hw      = light.right_half_w.w;
        float  hh      = light.up_half_h.w;
        bool   two_sided = light.misc.w > 0.5;

        // Back-face cull (unless two-sided).
        float3 to_center = center - P;
        if (!two_sided && dot(normal, to_center) < 0.0) return 0.0;

        // Diffuse: polygon solid angle of the 4 corners.
        float3 corners[4];
        corners[0] = normalize(center - right_n * hw - up_n * hh - P);
        corners[1] = normalize(center + right_n * hw - up_n * hh - P);
        corners[2] = normalize(center + right_n * hw + up_n * hh - P);
        corners[3] = normalize(center - right_n * hw + up_n * hh - P);
        float diff_irr = polygon_irradiance(N, corners, 4);

        // Specular: representative point (closest on rect to reflection ray).
        float3 R   = reflect(-V, N);
        float3 hit = P + R * max(dot(normal, center - P) / (dot(normal, R) + 1e-6), 0.0);
        float3 rep = rect_closest_point(hit, center, right_n, up_n, hw, hh);
        float3 L_s = normalize(rep - P);
        float  dist_s = length(rep - P);
        float  att_s  = brdf_point_attenuation(dist_s, light.position_range.w);

        // Modified roughness for energy normalization (Karis 2013).
        float area = 4.0 * hw * hh;
        float alpha = max(roughness, 0.04);
        float alpha_prime = clamp(alpha + sqrt(area) / (2.0 * max(dist_s, 0.1)), 0.0, 1.0);

        float3 Lo = float3(0.0, 0.0, 0.0);
        float3 F0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);

        // Diffuse contribution using polygon irradiance.
        float3 kd  = (1.0 - F0) * (1.0 - metallic);
        Lo += kd * albedo * diff_irr * radiance;

        // Specular via representative point.
        if (att_s > 0.0)
            Lo += brdf_eval(N, V, L_s, albedo, metallic, alpha_prime, radiance * att_s);

        return Lo;
    }

    if (kind == 4) {
        // ── Sphere area light ─────────────────────────────────────────────────
        float3 to_l    = light.position_range.xyz - P;
        float  dist    = length(to_l);
        float  src_r   = light.misc.z;  // source radius
        float  range   = light.position_range.w;
        float  att     = brdf_point_attenuation(dist, range);
        if (att <= 0.0) return 0.0;

        // Diffuse: clamp to horizon (the sphere subtends a solid angle).
        float3 L_diff = to_l / max(dist, 1e-5);

        // Specular representative point: move the reflection ray so it intersects the sphere.
        float3 R    = reflect(-V, N);
        float3 L0   = to_l / max(dist, 1e-5);
        float3 perp = to_l - R * dot(R, to_l);
        float3 L_s  = normalize(to_l - perp * clamp(src_r / length(perp), 0.0, 1.0));

        // Energy normalization.
        float alpha_prime = clamp(roughness + src_r / (2.0 * max(dist, 0.1)), 0.0, 1.0);

        float3 Lo = float3(0.0, 0.0, 0.0);
        Lo += brdf_eval(N, V, L_diff, albedo, metallic, roughness, radiance * att * 0.5); // diffuse portion
        Lo += brdf_eval(N, V, L_s,   albedo, metallic, alpha_prime, radiance * att * 0.5); // specular
        return Lo;
    }

    if (kind == 5) {
        // ── Disk light ────────────────────────────────────────────────────────
        float3 center   = light.position_range.xyz;
        float3 normal   = normalize(light.direction_kind.xyz);
        float  radius   = light.misc.z;
        float  range    = light.position_range.w;
        bool   two_sided = light.misc.w > 0.5;

        float3 to_center = center - P;
        if (!two_sided && dot(normal, to_center) < 0.0) return 0.0;

        float dist = length(to_center);
        float att  = brdf_point_attenuation(dist, range);
        if (att <= 0.0) return 0.0;

        // Approximate disk as 4-vertex polygon for irradiance.
        // Build a local frame on the disk.
        float3 t1 = normalize(abs(normal.x) < 0.9 ? cross(normal, float3(1,0,0)) : cross(normal, float3(0,1,0)));
        float3 t2 = cross(normal, t1);
        float3 corners[4];
        corners[0] = normalize(center + (t1 *  radius)              - P);
        corners[1] = normalize(center + (t2 *  radius)              - P);
        corners[2] = normalize(center + (t1 * -radius)              - P);
        corners[3] = normalize(center + (t2 * -radius)              - P);
        float diff_irr = polygon_irradiance(N, corners, 4);

        float3 L = normalize(to_center);
        float3 Lo = float3(0.0, 0.0, 0.0);
        float3 F0 = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
        float3 kd = (1.0 - F0) * (1.0 - metallic);
        Lo += kd * albedo * diff_irr * radiance;
        Lo += brdf_eval(N, V, L, albedo, metallic, roughness, radiance * att);
        return Lo;
    }

    return float3(0.0, 0.0, 0.0);
}

// ── Procedural sky ────────────────────────────────────────────────────────────
//
// Physically-inspired Rayleigh+Mie sky evaluated per background pixel.
// Sun direction is taken from the first directional light (lights[0]).
// Output is linear HDR — feeds the same bloom/AA/tonemap chain as geometry.

float3 sky_rayleigh(float cos_theta) {
    // Rayleigh phase: bluer overhead, warmer at horizon.
    return lerp(
        float3(0.12, 0.28, 0.75),   // zenith (deep blue)
        float3(0.65, 0.78, 1.00),   // mid sky
        pow(saturate(cos_theta), 0.4)
    );
}

float3 sky_mie(float cos_gamma, float turbidity) {
    // Mie scattering: forward-peaked glow around the sun.
    float g    = 0.76;  // anisotropy (0 = isotropic, 1 = strong forward peak)
    float mie  = (1.0 - g * g) / pow(1.0 + g * g - 2.0 * g * cos_gamma, 1.5);
    float haze = mie * turbidity * 0.04;
    return float3(haze * 1.2, haze * 0.9, haze * 0.7);
}

float3 sky_horizon_haze(float cos_theta, float turbidity) {
    // Warm horizon band (dust/humidity).
    float band = pow(1.0 - abs(cos_theta), 6.0);
    return float3(0.8, 0.55, 0.3) * band * turbidity * 0.12;
}

float3 compute_sky(float3 view_dir) {
    // Recover sun toward-light direction from the directional light.
    float3 sun_toward = (push.dir_light_count > 0)
        ? normalize(lights[0].position_range.xyz)
        : float3(0.0, 1.0, 0.0);

    float cos_theta = dot(view_dir, float3(0.0, 1.0, 0.0));  // elevation
    float cos_gamma = dot(view_dir, sun_toward);              // angle to sun

    float3 sky = sky_rayleigh(cos_theta)
               + sky_mie(cos_gamma, push.sky_turbidity)
               + sky_horizon_haze(cos_theta, push.sky_turbidity);

    // Below-horizon: dark ground fill.
    sky = lerp(float3(0.04, 0.03, 0.025) * push.sky_turbidity * 0.15,
               sky,
               saturate(cos_theta * 8.0 + 0.5));

    // Sun disc — physical solid-angle threshold.
    if (cos_gamma >= push.sky_sun_size) {
        float3 sun_color = lights[0].color_intensity.xyz;
        // Limb darkening: slightly dimmer at the edge of the disc.
        float limb = (cos_gamma - push.sky_sun_size) / (1.0 - push.sky_sun_size + 1e-6);
        sky += sun_color * pow(limb, 0.2) * 8.0;
    }

    // Solar corona/glow around the disc.
    float corona = pow(saturate(cos_gamma * 0.5 + 0.5), 16.0) * 0.4;
    if (push.dir_light_count > 0) {
        sky += lights[0].color_intensity.xyz * corona;
    }

    return sky * push.sky_exposure;
}

// ── Main ─────────────────────────────────────────────────────────────────────

float4 main(VSOut v) : SV_TARGET {
    float  d  = gbuffer_depth.SampleLevel(gbuffer_sampler, v.uv, 0);

    // Background pixel (depth == 1.0 → no geometry, just sky).
    if (push.sky_enabled != 0u && d >= 0.9999) {
        // Reconstruct view direction from UV.
        float4 ndc     = float4(v.uv.x * 2.0 - 1.0, v.uv.y * 2.0 - 1.0, 0.5, 1.0);
        float4 world_h = mul(push.inv_view_proj, ndc);
        float3 view_dir = normalize(world_h.xyz / world_h.w - push.camera_world_pos.xyz);
        return float4(compute_sky(view_dir), 1.0);
    }

    float4 g0 = gbuffer_albedo_metallic.SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g1 = gbuffer_normal_rough   .SampleLevel(gbuffer_sampler, v.uv, 0);
    float4 g2 = gbuffer_emissive       .SampleLevel(gbuffer_sampler, v.uv, 0);

    float3 albedo    = g0.rgb;
    float  metallic  = g0.a;
    float3 N         = oct_decode(g1.xy);
    float  roughness = clamp(g1.z, 0.04, 1.0);
    float3 emissive  = g2.rgb;

    // Reconstruct world position from hardware depth (Vulkan NDC: x,y in [-1,1], z in [0,1]).
    float4 ndc      = float4(v.uv.x * 2.0 - 1.0, v.uv.y * 2.0 - 1.0, d, 1.0);
    float4 world_h  = mul(push.inv_view_proj, ndc);
    float3 world_pos = world_h.xyz / world_h.w;

    float3 V = normalize(push.camera_world_pos.xyz - world_pos);

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
                // Leaf: evaluate this light based on its kind.
                uint light_idx = node.left_or_light;
                GpuLightData light = lights[light_idx];
                int kind = int(light.direction_kind.w + 0.5);
                Lo += eval_bvh_light(N, V, world_pos, albedo, metallic, roughness, light, kind, light_idx);
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
        float4 lut4  = brdf_lut.SampleLevel(gbuffer_sampler, float2(ndv, roughness), 0);
        float2 lut   = lut4.rg;
        float3 ibl_s = spec * (F * lut.x + lut.y);
        float3 irrad = eval_sh9(N);
        float3 kd    = (1.0 - F) * (1.0 - metallic);

        // Multi-scatter energy compensation (Turquin 2019 / Kulla-Conty 2017).
        // Single-scatter reflectance for a "white" (F0=1) surface:
        float  e_s_white = lut4.b;   // stored: scale + bias (from B channel)
        // Hemispherical average — index the 128-element LUT by roughness.
        uint   e_avg_idx = uint(roughness * 127.5);
        float  e_avg     = e_avg_lut[e_avg_idx];
        // Average Fresnel (Lagarde 2015 approximation: F_avg ≈ F0 + (1-F0)/21).
        float3 F_avg     = F0 + (1.0 - F0) / 21.0;
        // Multi-scatter radiance from the diffuse irradiance environment:
        float3 ms_color  = F_avg * e_avg / max(1.0 - F_avg * (1.0 - e_avg), 1e-5);
        float3 ibl_ms    = irrad * ms_color * (1.0 - e_s_white);
        float3 ibl_d = irrad * albedo * kd;
        ambient = (ibl_d + ibl_s + ibl_ms) * push.ibl_strength;
    } else {
        ambient = push.ambient * albedo;
    }

    return float4(ambient + Lo + emissive, 1.0);
}
