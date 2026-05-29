// SRD Translucent Shadow Stabilizer — Spatial Filter pass.
//
// Edge-aware bilateral filter applied to the temporally-accumulated translucent
// shadow color. Uses depth and normal edge-stopping weights to suppress
// cross-edge bleeding while smoothing noise within planar surfaces.
//
// Reads:    srd_translucency_accumulated  (Texture2D<float4>, RGB = translucency, A = history_length)
//           srd_guide_view_depth          (Texture2D<float>)
//           srd_guide_normal_roughness    (Texture2D<float4>, SrdShaderContract default)
//           srd_radiance_surface_mask     (Texture2D<float>, optional)
// Writes:   srd_translucency_out          (RWTexture2D<float4>, RGB = filtered translucency, A = history_length)
// Constants: SrdTranslucentShadowFilterConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;
import srd_packing;
import srd_filter;

Texture2D<float4>   srd_translucency_accumulated;
Texture2D<float>    srd_guide_view_depth;
Texture2D<float4>   srd_guide_normal_roughness;
Texture2D<float>    srd_radiance_surface_mask;
RWTexture2D<float4> srd_translucency_out;

struct SrdTranslucentShadowFilterConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float normal_weight_power;
    float spatial_radius;
    uint  use_surface_mask;
    uint  tile_size;
};
uniform SrdTranslucentShadowFilterConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                float4 centre_val = srd_translucency_accumulated.Load(int3(tid.xy, 0));
                srd_translucency_out[tid.xy] = float4(saturate(centre_val.rgb), centre_val.a);
                return;
            }
        }
    }

    float4 centre      = srd_translucency_accumulated.Load(int3(tid.xy, 0));
    float3 centre_col  = saturate(centre.rgb);
    float  history_len = srd_sanitize_scalar(centre.a);

    float centre_depth = srd_guide_view_depth.Load(int3(tid.xy, 0));
    if (centre_depth <= 0.0f) {
        srd_translucency_out[tid.xy] = float4(centre_col, history_len);
        return;
    }

    SrdNormalRoughness centre_nr = srd_decode_normal_roughness(
        srd_guide_normal_roughness.Load(int3(tid.xy, 0)));

    int   radius       = max(int(ceil(push.spatial_radius)), 1);
    float inv_two_sig2 = 1.0f / max(2.0f * push.spatial_radius * push.spatial_radius, SRD_TINY);
    float3 sum_col     = centre_col;
    float  sum_weight  = 1.0f;

    for (int dy = -radius; dy <= radius; ++dy) {
        for (int dx = -radius; dx <= radius; ++dx) {
            if (dx == 0 && dy == 0) continue;
            int2 tap_px = int2(tid.xy) + int2(dx, dy);
            if (any(tap_px < int2(0, 0)) || any(tap_px >= int2(push.rect_extent))) continue;

            float tap_depth = srd_guide_view_depth.Load(int3(tap_px, 0));
            if (tap_depth <= 0.0f) continue;

            SrdNormalRoughness tap_nr = srd_decode_normal_roughness(
                srd_guide_normal_roughness.Load(int3(tap_px, 0)));

            float r2       = float(dx * dx + dy * dy);
            float w_gauss  = exp(-r2 * inv_two_sig2);
            float w_depth  = srd_weight_depth(
                centre_depth, tap_depth,
                max(centre_depth * 0.02f, 0.01f));
            float w_normal = pow(max(dot(centre_nr.normal, tap_nr.normal), 0.0f),
                                 push.normal_weight_power);
            float w = w_gauss * w_depth * w_normal;
            if (w <= 1.0e-5f) continue;

            float3 tap_col = saturate(srd_translucency_accumulated.Load(int3(tap_px, 0)).rgb);
            sum_col    += tap_col * w;
            sum_weight += w;
        }
    }

    float3 filtered = sum_col / max(sum_weight, SRD_TINY);
    srd_translucency_out[tid.xy] = float4(saturate(filtered), history_len);
}
