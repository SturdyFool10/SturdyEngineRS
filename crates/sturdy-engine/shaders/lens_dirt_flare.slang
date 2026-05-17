// Lens dirt and simple flare post-processing pass.
//
// Input:  lens_input (Texture2D)
// Output: render target (SV_TARGET)

Texture2D<float4> lens_input;
SamplerState linear_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

struct LensConstants {
    float dirtStrength;
    uint  flareEnabled;
    float flareStrength;
    float _pad;
};

float hash1(float2 p) {
    p = frac(p * float2(127.1, 311.7));
    p += dot(p, p + 17.17);
    return frac(p.x * p.y);
}

float dirt_mask(float2 uv) {
    float2 p = uv * float2(32.0, 18.0);
    float coarse = hash1(floor(p) / float2(32.0, 18.0));
    float fine = hash1(floor(p * 4.0) / float2(128.0, 72.0));
    float smudge = smoothstep(0.68, 1.0, coarse) * 0.65 + smoothstep(0.82, 1.0, fine) * 0.35;
    float edge = smoothstep(0.15, 0.95, length(uv - 0.5) * 1.41421356);
    return smudge * edge;
}

float3 flare(float2 uv, float brightness, float strength) {
    float2 centered = uv - 0.5;
    float radial = length(centered);
    float axis = 1.0 - smoothstep(0.0, 0.035, abs(centered.y));
    float halo = 1.0 - smoothstep(0.18, 0.82, radial);
    float ghost_a = 1.0 - smoothstep(0.0, 0.08, abs(radial - 0.32));
    float ghost_b = 1.0 - smoothstep(0.0, 0.05, abs(radial - 0.58));
    float amount = brightness * strength * (axis * 0.18 + halo * 0.08 + ghost_a * 0.05 + ghost_b * 0.035);
    return float3(1.0, 0.88, 0.68) * amount;
}

float4 main(VSOut input, uniform LensConstants push) : SV_TARGET {
    float4 color = lens_input.SampleLevel(linear_sampler, input.uv, 0.0);
    float brightness = max(max(color.r, color.g), color.b);

    float3 result = color.rgb;
    if (push.dirtStrength > 0.0) {
        result += float3(1.0, 0.94, 0.82) * dirt_mask(input.uv) * brightness * push.dirtStrength;
    }
    if (push.flareEnabled != 0u) {
        result += flare(input.uv, brightness, push.flareStrength);
    }
    return float4(saturate(result), color.a);
}
