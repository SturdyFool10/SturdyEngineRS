// SRD Radiance Post-Blur pass.
//
// Adaptive bilateral Gaussian applied as the final denoising step. Blur sigma
// is history-adaptive: sigma = max_blur_sigma / sqrt(history_length + 1), so
// pixels with long temporal history receive progressively less smoothing.
// This recurrent-style decay prevents over-blurring converged regions while
// still cleaning residual noise on disoccluded or newly-appeared surfaces.
//
// Reads:    srd_radiance_input         (Texture2D<float4>, RGB + history_length in A)
//           srd_guide_view_depth       (Texture2D<float>)
//           srd_guide_normal_roughness (Texture2D<float4>, SrdShaderContract default)
//           srd_radiance_surface_mask  (Texture2D<float>, optional)
// Writes:   srd_radiance_post_blur_out (RWTexture2D<float4>, RGB + history_length in A)
// Constants: SrdRadiancePostBlurConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;
import srd_packing;
import srd_filter;
import srd_history;

Texture2D<float4>   srd_radiance_input;
Texture2D<float>    srd_guide_view_depth;
Texture2D<float4>   srd_guide_normal_roughness;
Texture2D<float>    srd_radiance_surface_mask;
RWTexture2D<float4> srd_radiance_post_blur_out;

struct SrdRadiancePostBlurConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float depth_tolerance_relative;
    float depth_tolerance_absolute;
    float normal_sharpness;
    float roughness_tolerance;
    float max_blur_sigma;
    uint  blur_radius;
    uint  use_surface_mask;
    uint  tile_size;
};
uniform SrdRadiancePostBlurConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                srd_radiance_post_blur_out[tid.xy] =
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

    // Adaptive sigma: long history → small sigma → pass-through.
    float sigma = push.max_blur_sigma / max(sqrt(history_length + 1.0f), SRD_TINY);

    if (centre_depth <= 0.0f || sigma < 0.01f) {
        srd_radiance_post_blur_out[tid.xy] = float4(centre_radiance, history_length);
        return;
    }

    float depth_tolerance = srd_depth_tolerance(
        centre_depth,
        push.depth_tolerance_relative,
        push.depth_tolerance_absolute);
    float inv_two_sigma2 = 1.0f / max(2.0f * sigma * sigma, SRD_TINY);
    int   radius = int(max(push.blur_radius, 1u));

    float3 sum_radiance = float3(0.0f);
    float  sum_weight   = 0.0f;

    for (int dy = -radius; dy <= radius; ++dy) {
        for (int dx = -radius; dx <= radius; ++dx) {
            int2 tap_px = int2(tid.xy) + int2(dx, dy);
            if (any(tap_px < int2(0, 0)) || any(tap_px >= int2(push.rect_extent))) {
                continue;
            }

            float tap_depth = srd_guide_view_depth.Load(int3(tap_px, 0));
            if (tap_depth <= 0.0f) continue;

            SrdNormalRoughness tap_nr = srd_decode_normal_roughness(
                srd_guide_normal_roughness.Load(int3(tap_px, 0)));

            float3 tap_radiance = srd_sanitize_colour(
                srd_radiance_input.Load(int3(tap_px, 0)).rgb);

            float r2       = float(dx * dx + dy * dy);
            float w_gauss  = exp(-r2 * inv_two_sigma2);
            float w_depth  = srd_weight_depth(centre_depth, tap_depth, depth_tolerance);
            float w_normal = srd_weight_normal(
                centre_nr.normal, tap_nr.normal, push.normal_sharpness);
            float w_rough  = srd_weight_roughness(
                centre_nr.roughness, tap_nr.roughness, push.roughness_tolerance);
            float w = w_gauss * w_depth * w_normal * w_rough;
            if (w <= 1.0e-6f) continue;

            sum_radiance += tap_radiance * w;
            sum_weight   += w;
        }
    }

    float3 filtered = sum_weight > SRD_TINY
        ? sum_radiance / sum_weight
        : centre_radiance;
    srd_radiance_post_blur_out[tid.xy] = float4(
        srd_sanitize_colour(filtered),
        history_length);
}
