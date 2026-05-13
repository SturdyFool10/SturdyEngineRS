// Vignette post-processing pass.
// Darkens screen corners by a configurable amount, producing the lens-falloff
// look common in photography and cinematic rendering.
//
// Input:  vignette_input (Texture2D)
// Output: render target (SV_TARGET)

Texture2D<float4> vignette_input;
SamplerState linear_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

struct VignetteConstants {
    float intensity; // [0, 1]  how dark the corners become
    float radius;    // [0, 1]  normalised distance at which darkening starts
    float feather;   // [0, 1]  transition width beyond radius
    float _pad;
};

float4 main(VSOut input, uniform VignetteConstants push) : SV_TARGET {
    float4 color = vignette_input.SampleLevel(linear_sampler, input.uv, 0.0);

    // Normalised distance from centre: 0 at centre, ~1 at the corners (√2 max).
    // Dividing by √2 maps the corner to exactly 1.0.
    float dist = length(input.uv - float2(0.5, 0.5)) * 1.41421356;

    // Smooth ramp: 0 inside radius, ramps up to intensity by radius+feather.
    float vignette = 1.0 - smoothstep(push.radius, push.radius + push.feather, dist) * push.intensity;

    return float4(color.rgb * vignette, color.a);
}
