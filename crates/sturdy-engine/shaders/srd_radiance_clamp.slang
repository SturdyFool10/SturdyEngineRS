// SRD Radiance History Clamp pass.
//
// Clamps the temporally-accumulated radiance to the colour-space AABB of the
// current frame's 3x3 neighbourhood. Working in YCoCg space allows a tighter
// luma clip without introducing the hue shifts a straight RGB clip causes.
//
// This pass runs between Accumulate and Reconstruct. Its output is the
// de-ghosted accumulated radiance that the spatial filter then refines.

import srd_common;
import srd_packing;

// Push constants.
struct SrdRadianceClampConstants {
    uint2  rect_extent;
    uint2  mask_extent;
    float  sigma_scale;       // k in mean ± k·sigma (typ. 1.0 – 2.0)
    uint   use_surface_mask;
    uint   tile_size;
    uint   _pad;
};

[[vk::push_constant]]
SrdRadianceClampConstants g_const;

// Accumulated history (output of the Accumulate pass, RGB = radiance, A = history length).
[[vk::binding(0)]] Sampler2D<float4> srd_radiance_accumulated;
// Current-frame noisy radiance (same slot as the Accumulate input).
[[vk::binding(1)]] Sampler2D<float4> srd_radiance_input;
// Optional surface-mask (R8, 0 = sky/out-of-range, 1 = valid surface).
[[vk::binding(2)]] Sampler2D<float>  srd_radiance_surface_mask;
// Clamped output (RGBA16F).
[[vk::binding(3)]] RWTexture2D<float4> srd_radiance_clamped_out;

// --- YCoCg helpers ---

float3 rgb_to_ycocg(float3 c) {
    return float3(
         0.25f * c.r + 0.5f * c.g + 0.25f * c.b,
         0.5f  * c.r               - 0.5f  * c.b,
        -0.25f * c.r + 0.5f * c.g - 0.25f * c.b
    );
}

float3 ycocg_to_rgb(float3 y) {
    float t = y.x - y.z;
    return clamp(float3(t + y.y, y.x + y.z, t - y.y),
                 float3(0.0f), float3(SRD_LUMA_MAX));
}

// --- Main ---

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    uint2 pixel = tid.xy;
    if (any(pixel >= g_const.rect_extent))
        return;

    // Early-out for sky / out-of-range pixels.
    if (g_const.use_surface_mask != 0u) {
        uint2 mask_pixel = pixel / g_const.tile_size;
        mask_pixel = min(mask_pixel, g_const.mask_extent - 1u);
        float2 mask_uv = srd_pixel_centre_uv(mask_pixel, g_const.mask_extent);
        float mask = srd_radiance_surface_mask.SampleLevel(mask_uv, 0).r;
        if (srd_unpack_unorm_byte(mask) == 0u) {
            // Sky: pass history straight through.
            float2 uv = srd_pixel_centre_uv(pixel, g_const.rect_extent);
            srd_radiance_clamped_out[pixel] = srd_radiance_accumulated.SampleLevel(uv, 0);
            return;
        }
    }

    float2 uv = srd_pixel_centre_uv(pixel, g_const.rect_extent);
    float4 history = srd_radiance_accumulated.SampleLevel(uv, 0);
    float  history_len = history.a;

    // If we have only one frame of history, clamping can't improve anything.
    if (history_len < 2.0f) {
        srd_radiance_clamped_out[pixel] = history;
        return;
    }

    // Compute 3×3 neighbourhood AABB in YCoCg space from the CURRENT frame.
    float3 c_min =  float3(1e30f);
    float3 c_max = -float3(1e30f);
    float3 c_mean  = float3(0.0f);
    float3 c_mean2 = float3(0.0f);
    float  n_taps  = 0.0f;

    for (int dy = -1; dy <= 1; ++dy) {
        for (int dx = -1; dx <= 1; ++dx) {
            int2 tap = int2(pixel) + int2(dx, dy);
            if (any(tap < 0) || any(uint2(tap) >= g_const.rect_extent))
                continue;
            float2 tap_uv = srd_pixel_centre_uv(uint2(tap), g_const.rect_extent);
            float3 tap_rgb = srd_sanitize_colour(srd_radiance_input.SampleLevel(tap_uv, 0).rgb);
            float3 tap_ycc = rgb_to_ycocg(tap_rgb);
            c_min   = min(c_min,   tap_ycc);
            c_max   = max(c_max,   tap_ycc);
            c_mean  += tap_ycc;
            c_mean2 += tap_ycc * tap_ycc;
            n_taps  += 1.0f;
        }
    }

    if (n_taps < 1.0f) {
        srd_radiance_clamped_out[pixel] = history;
        return;
    }

    c_mean  /= n_taps;
    c_mean2 /= n_taps;

    // Variance-expanded AABB: expand each side by sigma_scale * sigma.
    float3 variance = max(c_mean2 - c_mean * c_mean, float3(0.0f));
    float3 sigma    = sqrt(variance);
    float3 sigma_ex = g_const.sigma_scale * sigma;
    float3 box_min  = min(c_min, c_mean - sigma_ex);
    float3 box_max  = max(c_max, c_mean + sigma_ex);

    // Clamp history in YCoCg space.
    float3 hist_ycc  = rgb_to_ycocg(srd_sanitize_colour(history.rgb));
    float3 clamped   = clamp(hist_ycc, box_min, box_max);
    float3 result    = ycocg_to_rgb(clamped);

    srd_radiance_clamped_out[pixel] = float4(result, history_len);
}
