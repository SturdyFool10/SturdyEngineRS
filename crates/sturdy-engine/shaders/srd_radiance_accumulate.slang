// SRD Radiance Stabilizer — Accumulate pass.
//
// Consumes the per-pixel reprojection records from the Reproject pass and
// blends the current frame's radiance against the corresponding history
// sample. History stores accumulated colour in RGB and an integer-valued
// frame count in alpha; the running mean is `lerp(history, current, weight)`
// where weight is the standard online-average weight, clamped so it never
// drops below `1 / fast_history_budget`.
//
// Reads:    srd_radiance_input            (Texture2D<float4>, current combined radiance)
//           srd_radiance_reproject        (Texture2D<float4>, (prev_x, prev_y, validity, fallback))
//           srd_radiance_surface_mask     (Texture2D<float>, tile-resolution, optional)
//           srd_radiance_history_read     (Texture2D<float4>, RGB radiance + A history length)
//           srd_radiance_history_sampler  (SamplerState, linear/clamp)
// Writes:   srd_radiance_history_write    (RWTexture2D<float4>)
// Constants: SrdRadianceAccumulateConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;

Texture2D<float4>   srd_radiance_input;
Texture2D<float4>   srd_radiance_reproject;
Texture2D<float>    srd_radiance_surface_mask;
Texture2D<float4>   srd_radiance_history_read;
SamplerState        srd_radiance_history_sampler;
RWTexture2D<float4> srd_radiance_history_write;

struct SrdRadianceAccumulateConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float fast_history_budget;
    float history_frame_budget;
    uint  use_surface_mask;
    uint  tile_size;
};
uniform SrdRadianceAccumulateConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                // Sky/out-of-range pixels carry no useful history. Write a
                // sanitised zero so a later pass that misreads this tile sees
                // a defined value rather than stale memory.
                srd_radiance_history_write[tid.xy] = float4(0.0f, 0.0f, 0.0f, 0.0f);
                return;
            }
        }
    }

    float3 current = srd_sanitize_colour(srd_radiance_input.Load(int3(tid.xy, 0)).rgb);

    float4 reproject = srd_radiance_reproject.Load(int3(tid.xy, 0));
    float2 prev_pixel = reproject.xy;
    float  validity = reproject.z;

    float3 history_colour = float3(0.0f, 0.0f, 0.0f);
    float  history_length = 0.0f;

    if (validity > 0.0f) {
        float2 uv = (prev_pixel + 0.5f) / float2(max(push.rect_extent, uint2(1, 1)));
        float4 sample = srd_radiance_history_read.SampleLevel(
            srd_radiance_history_sampler, uv, 0.0f);
        history_colour = srd_sanitize_colour(sample.rgb);
        history_length = srd_sanitize_scalar(sample.a);
    }

    float fast = max(push.fast_history_budget, 1.0f);
    float weight = saturate(1.0f / max(history_length + 1.0f, 1.0f));
    weight = max(weight, 1.0f / fast);

    float3 accumulated = lerp(history_colour, current, weight);
    float  next_length = min(history_length + 1.0f, max(push.history_frame_budget, 1.0f));

    srd_radiance_history_write[tid.xy] = float4(
        srd_sanitize_colour(accumulated),
        next_length);
}
