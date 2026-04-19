// Final display tonemap pass.
// Reads "hdr_composite" (linear HDR, scene + bloom already composited),
// applies ACES filmic tonemap, adds a subtle vignette, encodes to sRGB.
//
// This is intentionally separate from the bloom pass: other HDR effects
// (fog, lens flares, colour grading) can be inserted before this pass.

Texture2D<float4> hdr_composite;
SamplerState hdr_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

// Stephen Hill's ACES filmic approximation.
// Maps linear scene-referred HDR to display-referred [0,1].
// Soft roll-off into highlights, no harsh clipping, no piecewise branches.
float3 aces(float3 x) {
    return saturate((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14));
}

float4 main(VSOut input) : SV_TARGET {
    float3 hdr = hdr_composite.SampleLevel(hdr_sampler, input.uv, 0.0).rgb;

    float2 uv = input.uv - 0.5;
    float vignette = 1.0 - dot(uv, uv) * 0.3;
    hdr *= vignette;

    float3 ldr = aces(hdr);

    // sRGB gamma encoding for UNORM swapchain surface.
    ldr = pow(saturate(ldr), float3(1.0 / 2.2));

    return float4(ldr, 1.0);
}
