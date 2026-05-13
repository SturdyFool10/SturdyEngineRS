// Film grain post-processing pass.
// Adds animated per-pixel noise that simulates photographic film grain.
// Applied after tone mapping at display resolution so the noise doesn't
// interact with temporal accumulation.
//
// Grain pattern changes every frame (driven by `time`). For `colored` grain
// the R, G, B channels receive independent noise, giving a more organic look.
//
// Input:  grain_input (Texture2D)
// Output: render target (SV_TARGET)

Texture2D<float4> grain_input;
SamplerState linear_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv       : TEXCOORD0;
};

struct GrainConstants {
    float intensity; // [0, 1]  additive noise magnitude
    float size;      // pixels  grain texel size (1 = pixel-exact, >1 = coarser)
    float time;      // seconds monotonic clock, changes each frame for animation
    uint  colored;   // 0 = monochrome grain, 1 = RGB-independent grain
};

// Fast, low-correlation hash — bijection on [0,1]² → [0,1].
float hash1(float2 p) {
    p = frac(p * float2(443.8975, 397.2973));
    p += dot(p, p + 19.19);
    return frac(p.x * p.y);
}

float4 main(VSOut input, uniform GrainConstants push) : SV_TARGET {
    float4 color = grain_input.SampleLevel(linear_sampler, input.uv, 0.0);

    // Scale UV by grain size and offset by time to animate each frame.
    float inv_size = 1.0 / max(push.size * 0.001, 0.0001);
    float2 guv = input.uv * inv_size;

    // Three temporally-decorrelated offsets for RGB.
    float2 off0 = float2(push.time * 1.7319,  push.time * 0.9137);
    float2 off1 = float2(push.time * 0.6831,  push.time * 1.3742);
    float2 off2 = float2(push.time * 1.1245,  push.time * 0.4618);

    float3 grain;
    if (push.colored != 0u) {
        grain = float3(
            hash1(guv + off0),
            hash1(guv + off1),
            hash1(guv + off2)
        ) * 2.0 - 1.0;
    } else {
        float mono = hash1(guv + off0) * 2.0 - 1.0;
        grain = float3(mono, mono, mono);
    }

    return float4(saturate(color.rgb + grain * push.intensity), color.a);
}
