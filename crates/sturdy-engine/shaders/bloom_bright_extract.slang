// Bright-pass filter: keeps only pixels above the threshold using a soft knee.
// Reads scene_color (linear HDR), outputs the bloom-eligible contribution.

Texture2D<float4> scene_color;
SamplerState scene_sampler;

struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct BrightExtractConstants {
    float threshold;
    float knee;
    float2 _pad;
};

float4 main(VSOut input, uniform BrightExtractConstants push) : SV_TARGET {
    float3 color = scene_color.SampleLevel(scene_sampler, input.uv, 0.0).rgb;
    float brightness = dot(color, float3(0.2126, 0.7152, 0.0722));

    // Quadratic soft knee
    float rq = clamp(brightness - push.threshold + push.knee, 0.0, 2.0 * push.knee);
    rq = (push.knee > 0.0001) ? (rq * rq) / (4.0 * push.knee) : 0.0;
    float weight = max(rq, brightness - push.threshold) / max(brightness, 0.00001);

    return float4(color * weight, 1.0);
}
