// SRD Translucent Shadow Stabilizer — Accumulate pass.
//
// Temporally accumulates per-pixel translucent-shadow color values. The
// reprojection result from the Reproject pass gates blending: high validity
// → large blend weight; zero validity → accept current sample only, resetting
// history length.
//
// History format: float4 where RGB = translucency color, A = history_length.
//
// Reads:    srd_translucency_input          (Texture2D<float4>, current translucency RGBA)
//           srd_translucency_reproject      (Texture2D<float4>, packed reproject record)
//           srd_radiance_surface_mask       (Texture2D<float>,  tile-resolution mask)
//           srd_translucency_history_read   (Texture2D<float4>, previous frame history)
// Writes:   srd_translucency_history_write  (RWTexture2D<float4>)
// Constants: SrdTranslucentShadowAccumulateConstants
// Dispatch: 8x8x1 thread groups covering rect_extent

import srd_common;
import srd_history;

Texture2D<float4>   srd_translucency_input;
Texture2D<float4>   srd_translucency_reproject;
Texture2D<float>    srd_radiance_surface_mask;
Texture2D<float4>   srd_translucency_history_read;
RWTexture2D<float4> srd_translucency_history_write;

struct SrdTranslucentShadowAccumulateConstants {
    uint2 rect_extent;
    uint2 mask_extent;
    float history_frame_budget;
    uint  use_surface_mask;
    uint  tile_size;
    uint  _pad;
};
uniform SrdTranslucentShadowAccumulateConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.rect_extent)) return;

    if (push.use_surface_mask != 0u) {
        uint2 tile = tid.xy / max(push.tile_size, 1u);
        if (all(tile < push.mask_extent)) {
            float mask = srd_radiance_surface_mask.Load(int3(tile, 0));
            if (mask <= 0.0f) {
                srd_translucency_history_write[tid.xy] = float4(1.0f, 1.0f, 1.0f, 0.0f);
                return;
            }
        }
    }

    float4 current_color = srd_translucency_input.Load(int3(tid.xy, 0));
    float4 reproject     = srd_translucency_reproject.Load(int3(tid.xy, 0));
    float  validity      = saturate(reproject.z);

    float4 history     = float4(1.0f, 1.0f, 1.0f, 0.0f);
    float  history_len = 0.0f;

    if (validity > 0.0f) {
        int2 prev_px      = int2(reproject.xy);
        int2 prev_clamped = clamp(prev_px, int2(0, 0), int2(push.rect_extent) - int2(1, 1));
        history     = srd_translucency_history_read.Load(int3(prev_clamped, 0));
        history_len = srd_sanitize_scalar(history.a);
    }

    float  new_len = min(history_len * validity + 1.0f, push.history_frame_budget);
    float  blend   = validity / max(new_len, 1.0f);
    float3 blended = lerp(history.rgb, current_color.rgb, blend);

    srd_translucency_history_write[tid.xy] = float4(saturate(blended), new_len);
}
