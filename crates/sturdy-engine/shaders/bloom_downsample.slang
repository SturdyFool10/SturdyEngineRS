// CoD: Advanced Warfare 13-tap downsampling filter for bloom mip-chain.
// Reference: "Next Generation Post Processing in Call of Duty: Advanced
// Warfare" Jorge Jimenez, SIGGRAPH 2014.

Texture2D<float4> source_tex;
SamplerState sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct DownsampleConstants {
    float2 texelSize;
};

float4 main(VSOut input, uniform DownsampleConstants push) : SV_TARGET {
    float2 uv = input.uv;
    float2 ts = push.texelSize;

    float4 a = source_tex.SampleLevel(sampler, uv + float2(-2.0, -2.0) * ts, 0.0);
    float4 b = source_tex.SampleLevel(sampler, uv + float2(0.0, -2.0) * ts, 0.0);
    float4 c = source_tex.SampleLevel(sampler, uv + float2(2.0, -2.0) * ts, 0.0);
    float4 d = source_tex.SampleLevel(sampler, uv + float2(-2.0, 0.0) * ts, 0.0);
    float4 e = source_tex.SampleLevel(sampler, uv + float2(0.0, 0.0) * ts, 0.0);
    float4 f = source_tex.SampleLevel(sampler, uv + float2(2.0, 0.0) * ts, 0.0);
    float4 g = source_tex.SampleLevel(sampler, uv + float2(-2.0, 2.0) * ts, 0.0);
    float4 h = source_tex.SampleLevel(sampler, uv + float2(0.0, 2.0) * ts, 0.0);
    float4 i = source_tex.SampleLevel(sampler, uv + float2(2.0, 2.0) * ts, 0.0);

    // Inner diagonal bilinear taps (at ±1 texel offsets)
    float4 j = source_tex.SampleLevel(sampler, uv + float2(-1.0, -1.0) * ts, 0.0);
    float4 k = source_tex.SampleLevel(sampler, uv + float2(1.0, -1.0) * ts, 0.0);
    float4 l = source_tex.SampleLevel(sampler, uv + float2(-1.0, 1.0) * ts, 0.0);
    float4 m = source_tex.SampleLevel(sampler, uv + float2(1.0, 1.0) * ts, 0.0);

    // Weighted sum (inner diagonals 0.5, outer center 0.125, outer edges 0.25,
    // outer corners 0.125)
    return e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 +
           (j + k + l + m) * 0.125;
}
