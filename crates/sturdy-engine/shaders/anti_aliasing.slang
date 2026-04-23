Texture2D<float4> source;
SamplerState source_sampler;
Texture2D<float4> history_source;
SamplerState history_sampler;
Texture2D<float4> motion_source;
SamplerState motion_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct AntiAliasingConstants {
    // xy: inverse extent, zw: previous jitter - current jitter in UV units.
    float4 extentAndHistoryJitter;
    // x: subpixel quality, y: edge threshold, z: edge threshold min, w: motion scale.
    float4 fxaaParams;
    // x: history weight, y: jitter scale, z: clamp factor, w: use motion vectors.
    float4 taaParams;
    uint mode;
    uint hasHistory;
    uint _pad0;
    uint _pad1;
};

float luma(float3 rgb) {
    return dot(rgb, float3(0.299, 0.587, 0.114));
}

float4 premultiply(float4 color) {
    return float4(color.rgb * color.a, color.a);
}

float4 unpremultiply(float4 color) {
    if (color.a <= 0.00001) {
        return float4(0.0, 0.0, 0.0, 0.0);
    }
    return float4(color.rgb / color.a, color.a);
}

float4 sampleSource(float2 uv) {
    return source.SampleLevel(source_sampler, uv, 0.0);
}

float4 sampleHistory(float2 uv) {
    return history_source.SampleLevel(history_sampler, uv, 0.0);
}

float2 sampleMotion(float2 uv, AntiAliasingConstants push) {
    if (push.taaParams.w < 0.5) {
        return float2(0.0, 0.0);
    }
    return motion_source.SampleLevel(motion_sampler, uv, 0.0).xy * push.fxaaParams.w;
}

float4 fxaa(float2 uv, AntiAliasingConstants push) {
    float2 px = push.extentAndHistoryJitter.xy;
    float4 center = sampleSource(uv);
    float4 premulM = premultiply(center);
    float4 premulN = premultiply(sampleSource(uv + float2(0.0, -px.y)));
    float4 premulS = premultiply(sampleSource(uv + float2(0.0,  px.y)));
    float4 premulE = premultiply(sampleSource(uv + float2( px.x, 0.0)));
    float4 premulW = premultiply(sampleSource(uv + float2(-px.x, 0.0)));

    float lM = luma(premulM.rgb);
    float lMin = min(lM, min(min(luma(premulN.rgb), luma(premulS.rgb)), min(luma(premulE.rgb), luma(premulW.rgb))));
    float lMax = max(lM, max(max(luma(premulN.rgb), luma(premulS.rgb)), max(luma(premulE.rgb), luma(premulW.rgb))));
    float contrast = lMax - lMin;
    float threshold = max(push.fxaaParams.z, lMax * push.fxaaParams.y);
    if (contrast < threshold) {
        return center;
    }

    float2 dir = float2(luma(premulW.rgb) - luma(premulE.rgb), luma(premulN.rgb) - luma(premulS.rgb));
    float dirLen = max(abs(dir.x), abs(dir.y));
    dir = dirLen > 0.00001 ? dir / dirLen : float2(0.0, 0.0);
    dir *= px * push.fxaaParams.x;

    float4 rgbaA = 0.5 * (
        premultiply(sampleSource(uv + dir * (1.0 / 3.0 - 0.5))) +
        premultiply(sampleSource(uv + dir * (2.0 / 3.0 - 0.5))));
    float4 rgbaB = rgbaA * 0.5 + 0.25 * (
        premultiply(sampleSource(uv + dir * -0.5)) +
        premultiply(sampleSource(uv + dir *  0.5)));
    float lB = luma(rgbaB.rgb);
    return unpremultiply((lB < lMin || lB > lMax) ? rgbaA : rgbaB);
}

float4 taaResolve(float2 uv, AntiAliasingConstants push, float4 current) {
    if (push.hasHistory == 0) {
        return current;
    }

    float2 px = push.extentAndHistoryJitter.xy;
    float2 jitter = px * push.taaParams.y * 0.5;
    float4 jitteredCurrent = sampleSource(uv + jitter);
    float4 currentPremul = lerp(
        premultiply(current),
        premultiply(jitteredCurrent),
        saturate(push.taaParams.y) * 0.125);

    float4 premulN = premultiply(sampleSource(uv + float2(0.0, -px.y)));
    float4 premulS = premultiply(sampleSource(uv + float2(0.0,  px.y)));
    float4 premulE = premultiply(sampleSource(uv + float2( px.x, 0.0)));
    float4 premulW = premultiply(sampleSource(uv + float2(-px.x, 0.0)));
    float4 minRgba = min(currentPremul, min(min(premulN, premulS), min(premulE, premulW)));
    float4 maxRgba = max(currentPremul, max(max(premulN, premulS), max(premulE, premulW)));
    float4 range = maxRgba - minRgba;

    float2 historyUv = uv + push.extentAndHistoryJitter.zw + sampleMotion(uv, push);
    float4 history = premultiply(sampleHistory(historyUv));
    float clampScale = max(push.taaParams.z, 0.0);
    float4 clampedHistory = clamp(
        history,
        minRgba - range * clampScale,
        maxRgba + range * clampScale);

    float historyWeight = saturate(push.taaParams.x);
    return unpremultiply(lerp(currentPremul, clampedHistory, historyWeight));
}

float4 main(VSOut input, uniform AntiAliasingConstants push) : SV_TARGET {
    float4 current = sampleSource(input.uv);
    if (push.mode == 1) {
        return current;
    }
    if (push.mode == 2) {
        return fxaa(input.uv, push);
    }
    if (push.mode == 3) {
        return taaResolve(input.uv, push, current);
    }
    if (push.mode == 4) {
        return taaResolve(input.uv, push, fxaa(input.uv, push));
    }
    return current;
}
