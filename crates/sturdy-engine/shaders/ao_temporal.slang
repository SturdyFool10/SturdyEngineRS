// AO temporal accumulation shader.
//
// Reprojects the previous frame's accumulated AO using the motion vector
// buffer and blends with the current frame's filtered result.
//
// Temporal accumulation amortises the per-frame cost of GTAO by preserving
// history across frames. With a blend factor of 0.1 the effective sample
// count grows to ~10× with minimal ghosting on fast-moving objects.

Texture2D<float>   ao_current;          // current frame, spatially filtered
Texture2D<float>   ao_history;          // previous frame's accumulated AO
Texture2D<float2>  motion_vectors;      // screen-space UV motion (for reprojection)
Texture2D<float>   gbuffer_depth;       // depth for validity check
RWTexture2D<float> ao_accumulated;      // output: blended accumulation

struct TemporalConstants {
    float2 inv_resolution;
    float  temporal_blend;    // lerp weight toward current (default 0.1)
    float  _pad;
};
uniform TemporalConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    uint2 dims;
    ao_accumulated.GetDimensions(dims.x, dims.y);
    if (any(tid.xy >= dims)) return;

    float  current_ao   = ao_current.Load(int3(tid.xy, 0)).r;
    float  current_depth = gbuffer_depth.Load(int3(tid.xy, 0)).r;

    if (current_depth >= 0.9999) {
        ao_accumulated[tid.xy] = 1.0;
        return;
    }

    // Reproject using motion vectors.
    float2 motion     = motion_vectors.Load(int3(tid.xy, 0)).xy;
    float2 history_uv = (float2(tid.xy) + 0.5) * push.inv_resolution - motion;

    float accumulated;
    if (all(history_uv >= 0.0) && all(history_uv <= 1.0)) {
        // Bilinear sample of previous frame's AO.
        float2 history_px = history_uv / push.inv_resolution;
        int2   px0        = int2(floor(history_px - 0.5));
        float2 frac_px    = frac(history_px - 0.5);

        float h00 = ao_history.Load(int3(clamp(px0 + int2(0,0), 0, int2(dims)-1), 0)).r;
        float h10 = ao_history.Load(int3(clamp(px0 + int2(1,0), 0, int2(dims)-1), 0)).r;
        float h01 = ao_history.Load(int3(clamp(px0 + int2(0,1), 0, int2(dims)-1), 0)).r;
        float h11 = ao_history.Load(int3(clamp(px0 + int2(1,1), 0, int2(dims)-1), 0)).r;
        float history_ao = lerp(lerp(h00, h10, frac_px.x), lerp(h01, h11, frac_px.x), frac_px.y);

        // Blend toward current frame.
        accumulated = lerp(history_ao, current_ao, push.temporal_blend);
    } else {
        // Outside screen bounds — no history, use current frame.
        accumulated = current_ao;
    }

    ao_accumulated[tid.xy] = saturate(accumulated);
}
