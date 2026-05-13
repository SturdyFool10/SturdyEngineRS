// Chromatic aberration post-processing pass.
// Simulates the wavelength-dependent refraction of a real camera lens by
// sampling the R, G, B channels at slightly different UV positions. The
// shift is strongest at screen edges and zero at the centre (radial model).
//
// Input:  ca_input (Texture2D)
// Output: render target (SV_TARGET)

Texture2D<float4> ca_input;
SamplerState linear_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

struct CaConstants {
    float strength;       // [0, 0.01]  separation magnitude at the screen edge
    float radial_falloff; // exponent for edge weighting; 2.0 = quadratic
    float2 _pad;
};

float4 main(VSOut input, uniform CaConstants push) : SV_TARGET {
    float2 uv       = input.uv;
    float2 centered = uv - 0.5;
    float  dist     = length(centered);

    // Radial falloff: 0 at centre, 1 at normalised corner distance (√2/2 ≈ 0.707).
    // pow(..., falloff) sharpens the fall-off curve.
    float magnitude = push.strength * pow(saturate(dist * 1.41421356), push.radial_falloff);

    // Direction of shift: outward from centre.  Epsilon avoids divide-by-zero at centre.
    float2 dir = normalize(centered + float2(0.0001, 0.0001));
    float2 shift = dir * magnitude;

    // Red shifts outward, blue shifts inward — classic refractive CA signature.
    float r = ca_input.SampleLevel(linear_sampler, uv + shift,  0.0).r;
    float g = ca_input.SampleLevel(linear_sampler, uv,          0.0).g;
    float b = ca_input.SampleLevel(linear_sampler, uv - shift,  0.0).b;
    float a = ca_input.SampleLevel(linear_sampler, uv,          0.0).a;

    return float4(r, g, b, a);
}
