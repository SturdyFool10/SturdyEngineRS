// HDR bloom composite pass.
// Additively blends bloom_base into scene_color in HDR linear space.
// Outputs an HDR FP16 result — no tonemapping.
// The caller is responsible for a subsequent tonemap pass.
//
// Frame image naming contract:
//   "scene_color" — the linear-HDR scene buffer
//   "bloom_base"  — the collapsed bloom pyramid (bloom_up_0)
// Output is written to whatever GraphImage this shader is executed on;
// by convention the caller names that image "hdr_composite".

Texture2D<float4> scene_color;
SamplerState scene_sampler;
Texture2D<float4> bloom_base;
SamplerState bloom_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct BloomCompositeConstants {
    float bloomIntensity;
    float3 _pad;
};

float4 main(VSOut input, uniform BloomCompositeConstants push) : SV_TARGET {
    float3 scene = scene_color.SampleLevel(scene_sampler, input.uv, 0.0).rgb;
    float3 bloom  = bloom_base.SampleLevel(bloom_sampler,  input.uv, 0.0).rgb;
    return float4(scene + bloom * push.bloomIntensity, 1.0);
}
