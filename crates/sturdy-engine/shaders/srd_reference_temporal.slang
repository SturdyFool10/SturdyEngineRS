// SRD Reference Temporal Stabilizer — compute accumulation pass.
//
// Temporally accumulates per-pixel radiance into a persistent history ring using
// a running average capped at `max_frames`. This is the compute replacement for
// the fullscreen-fragment `srd_temporal_accumulate` path; it uses `Load` instead
// of `SampleLevel` so no sampler is required, and dispatches 8×8×1 thread groups.
//
// Alpha stores the second luminance moment for downstream variance estimation.
//
// Reads:    srd_current_signal   (Texture2D<float4>)
//           srd_history_signal   (Texture2D<float4>, RGB = accumulated, A = moment2)
// Writes:   srd_history_write    (RWTexture2D<float4>)
// Constants: SrdTemporalConstants
// Dispatch:  8x8x1 thread groups covering rect_extent

import srd_common;

Texture2D<float4>   srd_current_signal;
Texture2D<float4>   srd_history_signal;
RWTexture2D<float4> srd_history_write;

struct SrdTemporalConstants {
    uint frame_index;
    uint has_history;
    uint max_frames;
    uint mode;
};
uniform SrdTemporalConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    uint2 extent;
    srd_history_write.GetDimensions(extent.x, extent.y);
    if (any(tid.xy >= extent)) return;

    float3 current    = srd_sanitize_colour(srd_current_signal.Load(int3(tid.xy, 0)).rgb);
    float  cur_lum    = srd_luminance(current);
    float  cur_mom2   = cur_lum * cur_lum;

    if (push.has_history == 0u || push.frame_index == 0u) {
        srd_history_write[tid.xy] = float4(current, cur_mom2);
        return;
    }

    float4 hist_val   = srd_history_signal.Load(int3(tid.xy, 0));
    float3 history    = srd_sanitize_colour(hist_val.rgb);
    float  hist_mom2  = max(hist_val.a, 0.0f);

    float  eff_frame  = min(float(push.frame_index), max(float(push.max_frames), 1.0f) - 1.0f);
    float  blend      = 1.0f / (eff_frame + 1.0f);
    float3 blended    = srd_sanitize_colour(lerp(history, current, blend));
    srd_history_write[tid.xy] = float4(blended, lerp(hist_mom2, cur_mom2, blend));
}
