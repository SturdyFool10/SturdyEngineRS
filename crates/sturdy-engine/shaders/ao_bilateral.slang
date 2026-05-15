// AO bilateral spatial filter.
//
// Applies a 3×3 cross-bilateral filter to the raw GTAO output.
// The filter is weighted by:
//   - Depth similarity  : exp(-abs(center_depth - sample_depth) / sigma_depth)
//   - Normal similarity : max(0, dot(center_normal, sample_normal))^k
//
// This removes high-frequency noise left after the temporal accumulation pass
// while preserving sharp contact shadow edges.

Texture2D<float>  ao_raw;              // raw or temporally-accumulated AO
Texture2D<float>  gbuffer_depth;
Texture2D<float4> gbuffer_normal_rough;
RWTexture2D<float> ao_filtered;

struct BilateralConstants {
    float2 inv_resolution;  // 1 / (width, height)
    float  sigma_depth;     // depth similarity bandwidth (default 0.5)
    float  normal_power;    // normal similarity exponent (default 8.0)
};
uniform BilateralConstants push;

float3 decode_normal(float2 enc) {
    float3 n;
    n.xy = enc * 2.0 - 1.0;
    n.z  = sqrt(max(0.0, 1.0 - dot(n.xy, n.xy)));
    return n;
}

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    uint2 dims;
    ao_filtered.GetDimensions(dims.x, dims.y);
    if (any(tid.xy >= dims)) return;

    float center_depth = gbuffer_depth.Load(int3(tid.xy, 0)).r;
    if (center_depth >= 0.9999) {
        ao_filtered[tid.xy] = 1.0;
        return;
    }

    float2 center_enc_n = gbuffer_normal_rough.Load(int3(tid.xy, 0)).xy;
    float3 center_n     = decode_normal(center_enc_n);

    float ao_sum     = 0.0;
    float weight_sum = 0.0;

    for (int dy = -1; dy <= 1; ++dy) {
        for (int dx = -1; dx <= 1; ++dx) {
            int2 sample_px = int2(tid.xy) + int2(dx, dy);
            if (any(sample_px < 0) || any(uint2(sample_px) >= dims)) continue;

            float sample_depth = gbuffer_depth.Load(int3(sample_px, 0)).r;
            float2 sample_enc  = gbuffer_normal_rough.Load(int3(sample_px, 0)).xy;
            float3 sample_n    = decode_normal(sample_enc);
            float  sample_ao   = ao_raw.Load(int3(sample_px, 0)).r;

            // Depth weight: exponential falloff by NDC depth difference.
            float depth_diff = abs(center_depth - sample_depth);
            float w_depth    = exp(-depth_diff / push.sigma_depth);

            // Normal weight: cosine similarity raised to a power.
            float w_normal   = pow(max(0.0, dot(center_n, sample_n)), push.normal_power);

            float w = w_depth * w_normal;
            ao_sum     += sample_ao * w;
            weight_sum += w;
        }
    }

    float ao = (weight_sum > 0.001) ? (ao_sum / weight_sum) : ao_raw.Load(int3(tid.xy, 0)).r;
    ao_filtered[tid.xy] = saturate(ao);
}
