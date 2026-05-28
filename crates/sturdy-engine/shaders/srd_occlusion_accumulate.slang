// SRD Occlusion Stabilizer — Accumulate pass.
//
// Temporally accumulates per-pixel ambient occlusion (AO) or directional
// occlusion (DO) values. The reprojection result from the Reproject pass
// gates how much history to blend in: high validity → large blend weight;
// zero validity → accept current sample only, resetting history length.
//
// History format: float4 where R = occlusion value (0..1), A = history_length.
//
// Reads:    srd_occlusion_input        (Texture2D<float>,  current AO/DO value)
//           srd_occlusion_reproject    (Texture2D<float4>, packed reproject record)
//           srd_radiance_surface_mask  (Texture2D<float>,  tile-resolution mask)
//           srd_occlusion_history_read (Texture2D<float4>, previous frame history)
// Writes:   srd_occlusion_history_write (RWTexture2D<float4>)
// Constants: SrdOcclusionAccumulateConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;
import srd_history;

Texture2D<float>    srd_occlusion_input;
Texture2D<float4>   srd_occlusion_reproject;
Texture2D<float>    srd_radiance_surface_mask;
Texture2D<float4>   srd_occlusion_history_read;
RWTexture2D<float4> srd_occlusion_history_write;

struct SrdOcclusionAccumulateConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float history_frame_budget;
    uint  use_surface_mask;
    uint  tile_size;
    uint  _pad;
};
uniform SrdOcclusionAccumulateConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                srd_occlusion_history_write[tid.xy] = float4(1.0f, 0.0f, 0.0f, 0.0f);
                return;
            }
        }
    }

    float current_ao = saturate(srd_occlusion_input.Load(int3(tid.xy, 0)));
    float4 reproject = srd_occlusion_reproject.Load(int3(tid.xy, 0));
    float validity = saturate(reproject.z);

    float4 history      = float4(1.0f, 0.0f, 0.0f, 0.0f);
    float  history_ao   = 1.0f;
    float  history_len  = 0.0f;

    if (validity > 0.0f) {
        int2 prev_px  = int2(reproject.xy);
        int2 prev_clamped = clamp(prev_px, int2(0, 0), int2(push.rect_extent) - int2(1, 1));
        history     = srd_occlusion_history_read.Load(int3(prev_clamped, 0));
        history_ao  = saturate(history.r);
        history_len = srd_sanitize_scalar(history.a);
    }

    float new_len    = min(history_len * validity + 1.0f, push.history_frame_budget);
    float blend      = validity / max(new_len, 1.0f);
    float blended_ao = lerp(history_ao, current_ao, blend);

    srd_occlusion_history_write[tid.xy] = float4(saturate(blended_ao), 0.0f, 0.0f, new_len);
}
