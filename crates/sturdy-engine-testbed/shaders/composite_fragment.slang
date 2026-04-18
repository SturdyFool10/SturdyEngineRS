// Two-pass composite: samples scene_color from the previous pass and applies
// chromatic aberration. The engine automatically resolves scene_color from the
// frame's named images via Slang reflection.

Texture2D scene_color;
SamplerState scene_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct CompositeConstants {
    float strength;
    float _pad0;
    float _pad1;
    float _pad2;
};

float4 main(VSOut input, uniform CompositeConstants push) : SV_TARGET {
    float2 uv = input.uv;
    float2 center = uv - 0.5;
    float dist = length(center);

    // Chromatic aberration: R channel shifts outward, B shifts inward.
    float aberration = push.strength * dist * dist;
    float2 offset = center * aberration;

    float r = scene_color.Sample(scene_sampler, uv + offset).r;
    float g = scene_color.Sample(scene_sampler, uv).g;
    float b = scene_color.Sample(scene_sampler, uv - offset).b;

    return float4(r, g, b, 1.0);
}
