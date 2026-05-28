// SRD Radiance Spatial Filter pass.
//
// Final edge-aware bilateral filter applied to the radiance produced by the
// Reconstruct (and optional Outlier Suppress) passes. Where Reconstruct widens
// its footprint for short-history pixels, this pass applies a fixed-stride
// cross-5 kernel with full bilateral edge-stopping weights — depth, normal,
// roughness, and luminance — to clean up residual noise everywhere.
//
// Reads:    srd_radiance_input         (Texture2D<float4>, RGB + history_length in A)
//           srd_guide_view_depth       (Texture2D<float>)
//           srd_guide_normal_roughness (Texture2D<float4>, SrdShaderContract default)
//           srd_radiance_surface_mask  (Texture2D<float>, optional)
// Writes:   srd_radiance_filtered_out  (RWTexture2D<float4>, RGB + history_length in A)
// Constants: SrdRadianceSpatialFilterConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;
import srd_packing;
import srd_filter;
import srd_history;

Texture2D<float4>   srd_radiance_input;
Texture2D<float>    srd_guide_view_depth;
Texture2D<float4>   srd_guide_normal_roughness;
Texture2D<float>    srd_radiance_surface_mask;
RWTexture2D<float4> srd_radiance_filtered_out;

struct SrdRadianceSpatialFilterConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float depth_tolerance_relative;
    float depth_tolerance_absolute;
    float normal_sharpness;
    float roughness_tolerance;
    float luminance_sigma;
    uint  stride;
    uint  use_surface_mask;
    uint  tile_size;
    uint  _pad0;
    uint  _pad1;
};
uniform SrdRadianceSpatialFilterConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                srd_radiance_filtered_out[tid.xy] =
                    srd_radiance_input.Load(int3(tid.xy, 0));
                return;
            }
        }
    }

    float4 centre = srd_radiance_input.Load(int3(tid.xy, 0));
    float3 centre_radiance = srd_sanitize_colour(centre.rgb);
    float  history_length  = srd_sanitize_scalar(centre.a);

    float centre_depth = srd_guide_view_depth.Load(int3(tid.xy, 0));
    SrdNormalRoughness centre_nr = srd_decode_normal_roughness(
        srd_guide_normal_roughness.Load(int3(tid.xy, 0)));

    // No depth guide → can't compute a meaningful neighbourhood. Pass-through.
    if (centre_depth <= 0.0f) {
        srd_radiance_filtered_out[tid.xy] = float4(centre_radiance, history_length);
        return;
    }

    float centre_luma = srd_luminance(centre_radiance);
    float depth_tolerance = srd_depth_tolerance(
        centre_depth,
        push.depth_tolerance_relative,
        push.depth_tolerance_absolute);
    uint stride = max(push.stride, 1u);

    float3 sum_radiance = centre_radiance;
    float  sum_weight   = 1.0f;

    // SRD_CROSS5_OFFSETS[0] is the centre — start at 1.
    [unroll]
    for (int i = 1; i < 9; ++i) {
        int2 offset = srd_scaled_offset(SRD_CROSS5_OFFSETS[i], stride);
        int2 tap_px = int2(tid.xy) + offset;
        if (any(tap_px < int2(0, 0)) || any(tap_px >= int2(push.rect_extent))) {
            continue;
        }

        float tap_depth = srd_guide_view_depth.Load(int3(tap_px, 0));
        if (tap_depth <= 0.0f) continue;

        SrdNormalRoughness tap_nr = srd_decode_normal_roughness(
            srd_guide_normal_roughness.Load(int3(tap_px, 0)));

        float3 tap_radiance = srd_sanitize_colour(
            srd_radiance_input.Load(int3(tap_px, 0)).rgb);
        float  tap_luma = srd_luminance(tap_radiance);

        float w_depth     = srd_weight_depth(centre_depth, tap_depth, depth_tolerance);
        float w_normal    = srd_weight_normal(centre_nr.normal, tap_nr.normal, push.normal_sharpness);
        float w_roughness = srd_weight_roughness(
            centre_nr.roughness, tap_nr.roughness, push.roughness_tolerance);
        float w_luma      = srd_weight_luminance(centre_luma, tap_luma, push.luminance_sigma);
        float w = w_depth * w_normal * w_roughness * w_luma;
        if (w <= 1.0e-4f) continue;

        sum_radiance += tap_radiance * w;
        sum_weight   += w;
    }

    float3 filtered = sum_radiance / max(sum_weight, SRD_TINY);
    srd_radiance_filtered_out[tid.xy] = float4(
        srd_sanitize_colour(filtered),
        history_length);
}
