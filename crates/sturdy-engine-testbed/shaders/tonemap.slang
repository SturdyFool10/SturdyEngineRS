// Final display tonemap pass.
// Reads "hdr_composite" (linear HDR, scene + bloom already composited),
// applies a selectable tonemapper, adds a subtle vignette, and encodes for
// the selected output mode.
//
// This is intentionally separate from the bloom pass: other HDR effects
// (fog, lens flares, colour grading) can be inserted before this pass.

Texture2D<float4> hdr_composite;
SamplerState hdr_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct TonemapConstants {
    uint tonemapOp;
    uint hdrOutput;
    float exposure;
    float whitePoint;
    float displayGain;
    float outputGamma;
    float acesA;
    float acesB;
    float acesC;
    float acesD;
    float acesE;
    float reinhardWhite;
    float hermiteContrast;
    float linearWhite;
};

// Stephen Hill's ACES filmic approximation.
// Maps linear scene-referred HDR to display-referred [0,1].
// Soft roll-off into highlights, no harsh clipping, no piecewise branches.
float3 aces(float3 x, uniform TonemapConstants push) {
    return saturate((x * (push.acesA * x + push.acesB)) / (x * (push.acesC * x + push.acesD) + push.acesE));
}

float3 normalizedAces(float3 x, uniform TonemapConstants push) {
    float white = max(push.whitePoint, 0.001);
    float3 whiteMapped = max(aces(float3(white), push), float3(0.001));
    return saturate(aces(x, push) / whiteMapped);
}

float3 reinhard(float3 x, uniform TonemapConstants push) {
    float white = max(push.reinhardWhite, 0.001);
    return saturate((x * (1.0 + x / (white * white))) / (1.0 + x));
}

float3 hermite(float3 x, uniform TonemapConstants push) {
    float white = max(push.whitePoint, 0.001);
    float contrast = max(push.hermiteContrast, 0.001);
    float3 y = saturate(x / white);
    y = pow(y, float3(1.0 / contrast));
    return y * y * (3.0 - 2.0 * y);
}

float3 linearMap(float3 x, uniform TonemapConstants push) {
    return saturate(x / max(push.linearWhite, 0.001));
}

float3 applyTonemap(float3 hdr, uniform TonemapConstants push) {
    float3 x = max(hdr * push.exposure, float3(0.0));
    float3 color;
    if (push.tonemapOp == 1) {
        color = reinhard(x, push);
    } else if (push.tonemapOp == 2) {
        color = hermite(x, push);
    } else if (push.tonemapOp == 3) {
        color = linearMap(x, push);
    } else {
        color = normalizedAces(x, push);
    }
    return saturate(color * push.displayGain);
}

float4 main(VSOut input, uniform TonemapConstants push) : SV_TARGET {
    float3 hdr = hdr_composite.SampleLevel(hdr_sampler, input.uv, 0.0).rgb;

    float2 uv = input.uv - 0.5;
    float vignette = 1.0 - dot(uv, uv) * 0.3;
    hdr *= vignette;

    float3 color = applyTonemap(hdr, push);

    if (push.hdrOutput == 0) {
        color = pow(saturate(color), float3(1.0 / max(push.outputGamma, 0.001)));
    }

    return float4(color, 1.0);
}
