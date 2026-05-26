// SRD Radiance Stabilizer — Outlier Suppress pass.
//
// Optional pass that clamps isolated bright pixels which survive the spatial
// reconstruct. The first-cut implementation computes a local 3x3 luminance
// mean and standard deviation per pixel and rescales the centre RGB so its
// luminance never exceeds `mean + sigma * stddev`, with a hard absolute
// ceiling at `mean * max_relative_luminance`. Hue is preserved.
//
// When the moments pass lands the same logic will switch to consuming the
// per-pixel running mean/variance instead of computing the neighbourhood
// inline; the pass's public input/output contract will not change.
//
// Reads:    srd_radiance_input          (Texture2D<float4>, RGB radiance + history length alpha)
//           srd_radiance_surface_mask   (Texture2D<float>, optional)
// Writes:   srd_radiance_clamped_out    (RWTexture2D<float4>, RGB radiance + history length alpha)
// Constants: SrdRadianceOutlierSuppressConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;

Texture2D<float4>   srd_radiance_input;
Texture2D<float>    srd_radiance_surface_mask;
RWTexture2D<float4> srd_radiance_clamped_out;

struct SrdRadianceOutlierSuppressConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float luminance_sigma;
    float max_relative_luminance;
    uint  use_surface_mask;
    uint  tile_size;
};
uniform SrdRadianceOutlierSuppressConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                srd_radiance_clamped_out[tid.xy] = float4(0.0f, 0.0f, 0.0f, 0.0f);
                return;
            }
        }
    }

    float4 centre = srd_radiance_input.Load(int3(tid.xy, 0));
    float3 centre_rgb = srd_sanitize_colour(centre.rgb);
    float  history_length = srd_sanitize_scalar(centre.a);
    float  centre_lum = srd_luminance(centre_rgb);

    float sum_lum = 0.0f;
    float sum_lum_sq = 0.0f;
    float count = 0.0f;

    [unroll]
    for (int dy = -1; dy <= 1; ++dy) {
        [unroll]
        for (int dx = -1; dx <= 1; ++dx) {
            int2 tap_px = int2(tid.xy) + int2(dx, dy);
            if (any(tap_px < int2(0, 0)) || any(tap_px >= int2(push.rect_extent))) {
                continue;
            }
            float3 tap_rgb = srd_sanitize_colour(
                srd_radiance_input.Load(int3(tap_px, 0)).rgb);
            float tap_lum = srd_luminance(tap_rgb);
            sum_lum += tap_lum;
            sum_lum_sq += tap_lum * tap_lum;
            count += 1.0f;
        }
    }

    if (count < 1.0f) {
        srd_radiance_clamped_out[tid.xy] = float4(centre_rgb, history_length);
        return;
    }

    float mean = sum_lum / count;
    float variance = max(sum_lum_sq / count - mean * mean, 0.0f);
    float stddev = sqrt(variance);

    float sigma_threshold = mean + max(push.luminance_sigma, 0.0f) * stddev;
    float absolute_ceiling = mean * max(push.max_relative_luminance, 1.0f);
    float threshold = min(sigma_threshold, absolute_ceiling);
    threshold = max(threshold, SRD_TINY);

    float3 out_rgb = centre_rgb;
    if (centre_lum > threshold) {
        float scale = threshold / max(centre_lum, SRD_TINY);
        out_rgb *= scale;
    }

    srd_radiance_clamped_out[tid.xy] = float4(srd_sanitize_colour(out_rgb), history_length);
}
