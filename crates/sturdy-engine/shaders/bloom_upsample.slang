// Tent-filtered upsample pass for the bloom pyramid collapse.
// Reads bloom_accum (accumulated lower mip) and bloom_down (this level's
// downsampled bloom), outputs bloom_down + tent_upsample(bloom_accum).
// Called once per level going from the smallest mip back to mip 0.

Texture2D<float4> bloom_down;
SamplerState down_sampler;
Texture2D<float4> bloom_accum;
SamplerState accum_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct UpsampleConstants {
    // Texel size of bloom_accum (the source being upsampled), in UV space.
    // = 1 / accum_resolution; equals 2x the output texel size when exactly halved.
    float2 accumTexelSize;
};

// 3x3 tent filter. Weights sum to 1: corners 1/16, edges 2/16, centre 4/16.
float3 tentUpsample(float2 uv, float2 ts) {
    float3 r = float3(0.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2(-ts.x, -ts.y), 0.0).rgb * (1.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2( 0.0, -ts.y), 0.0).rgb * (2.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2( ts.x, -ts.y), 0.0).rgb * (1.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2(-ts.x,  0.0), 0.0).rgb * (2.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2( 0.0,  0.0), 0.0).rgb * (4.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2( ts.x,  0.0), 0.0).rgb * (2.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2(-ts.x,  ts.y), 0.0).rgb * (1.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2( 0.0,  ts.y), 0.0).rgb * (2.0 / 16.0);
    r += bloom_accum.SampleLevel(accum_sampler, uv + float2( ts.x,  ts.y), 0.0).rgb * (1.0 / 16.0);
    return r;
}

float4 main(VSOut input, uniform UpsampleConstants push) : SV_TARGET {
    float3 current = bloom_down.SampleLevel(down_sampler, input.uv, 0.0).rgb;
    float3 upsampled = tentUpsample(input.uv, push.accumTexelSize);
    return float4(current + upsampled, 1.0);
}
