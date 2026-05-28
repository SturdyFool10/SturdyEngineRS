// SRD Occlusion Stabilizer — Spatial Filter pass.
//
// Edge-aware bilateral filter applied to the temporally-accumulated occlusion
// signal. Uses depth and normal edge-stopping weights to suppress cross-edge
// bleeding while smoothing noise within planar surfaces. The spatial radius is
// taken from SrdOcclusionSettings::spatial_radius and is applied as a disc.
//
// Reads:    srd_occlusion_accumulated  (Texture2D<float4>, R = AO, A = history_length)
//           srd_guide_view_depth       (Texture2D<float>)
//           srd_guide_normal_roughness (Texture2D<float4>, SrdShaderContract default)
//           srd_radiance_surface_mask  (Texture2D<float>, optional)
// Writes:   srd_occlusion_out          (RWTexture2D<float4>, R = filtered AO, A = history_length)
// Constants: SrdOcclusionFilterConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;
import srd_packing;
import srd_filter;

Texture2D<float4>   srd_occlusion_accumulated;
Texture2D<float>    srd_guide_view_depth;
Texture2D<float4>   srd_guide_normal_roughness;
Texture2D<float>    srd_radiance_surface_mask;
RWTexture2D<float4> srd_occlusion_out;

struct SrdOcclusionFilterConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float normal_weight_power;
    float spatial_radius;
    uint  use_surface_mask;
    uint  tile_size;
};
uniform SrdOcclusionFilterConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                float4 centre_val = srd_occlusion_accumulated.Load(int3(tid.xy, 0));
                srd_occlusion_out[tid.xy] = float4(saturate(centre_val.r), 0.0f, 0.0f, centre_val.a);
                return;
            }
        }
    }

    float4 centre     = srd_occlusion_accumulated.Load(int3(tid.xy, 0));
    float  centre_ao  = saturate(centre.r);
    float  history_len = srd_sanitize_scalar(centre.a);

    float centre_depth = srd_guide_view_depth.Load(int3(tid.xy, 0));
    if (centre_depth <= 0.0f) {
        srd_occlusion_out[tid.xy] = float4(centre_ao, 0.0f, 0.0f, history_len);
        return;
    }

    SrdNormalRoughness centre_nr = srd_decode_normal_roughness(
        srd_guide_normal_roughness.Load(int3(tid.xy, 0)));

    int radius = max(int(ceil(push.spatial_radius)), 1);
    float inv_two_sigma2 = 1.0f / max(2.0f * push.spatial_radius * push.spatial_radius, SRD_TINY);
    float sum_ao     = centre_ao;
    float sum_weight = 1.0f;

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
            float w_gauss  = exp(-r2 * inv_two_sigma2);
            float w_depth  = srd_weight_depth(
                centre_depth, tap_depth,
                max(centre_depth * 0.02f, 0.01f));
            float w_normal = pow(max(dot(centre_nr.normal, tap_nr.normal), 0.0f),
                                 push.normal_weight_power);
            float w = w_gauss * w_depth * w_normal;
            if (w <= 1.0e-5f) continue;

            float tap_ao = saturate(srd_occlusion_accumulated.Load(int3(tap_px, 0)).r);
            sum_ao     += tap_ao * w;
            sum_weight += w;
        }
    }

    float filtered_ao = sum_ao / max(sum_weight, SRD_TINY);
    srd_occlusion_out[tid.xy] = float4(saturate(filtered_ao), 0.0f, 0.0f, history_len);
}
