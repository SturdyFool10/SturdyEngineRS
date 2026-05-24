// Luminance histogram compute pass for auto-exposure.
//
// Reads the pre-exposure HDR scene image and writes per-pixel log-luminance
// counts into a 256-bin histogram buffer.
//
// Bin layout:
//   bin 0    — pixels with luminance below `min_log_luma` (or essentially black)
//   bin 1..254 — uniformly spaced in log2(luminance) between
//                `min_log_luma` and `max_log_luma`
//   bin 255  — pixels with luminance above `max_log_luma`
//
// The histogram is incremental: the adapt pass clears it after reading.
// Workgroup is 16x16; each thread builds a per-group histogram in shared
// memory before atomically accumulating into the global buffer.

Texture2D<float4>        scene_color;
SamplerState             auto_exposure_sampler;
RWStructuredBuffer<uint> histogram;

struct HistogramConstants {
    float2 inv_resolution;
    float  min_log_luma;
    float  inv_log_luma_range;
    uint2  resolution;
    uint2  _pad;
};
uniform HistogramConstants push;

groupshared uint local_histogram[256];

uint bin_for_luminance(float luma) {
    if (luma < 1e-5) {
        return 0;
    }
    float log_luma   = log2(luma);
    float normalized = saturate((log_luma - push.min_log_luma) * push.inv_log_luma_range);
    return uint(normalized * 254.0 + 1.0);
}

[numthreads(16, 16, 1)]
void main(uint3 dt_id : SV_DispatchThreadID, uint group_index : SV_GroupIndex) {
    local_histogram[group_index] = 0u;
    GroupMemoryBarrierWithGroupSync();

    if (dt_id.x < push.resolution.x && dt_id.y < push.resolution.y) {
        float2 uv    = (float2(dt_id.xy) + 0.5) * push.inv_resolution;
        float3 color = scene_color.SampleLevel(auto_exposure_sampler, uv, 0.0).rgb;
        float  luma  = dot(color, float3(0.2126, 0.7152, 0.0722));
        uint   bin   = bin_for_luminance(luma);
        InterlockedAdd(local_histogram[bin], 1u);
    }

    GroupMemoryBarrierWithGroupSync();
    InterlockedAdd(histogram[group_index], local_histogram[group_index]);
}
