struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct LutParams {
    float phase;
};

float4 main(VSOut input, uniform LutParams params) : SV_TARGET {
    float t = input.uv.x;
    float warm = sin(params.phase) * 0.5 + 0.5;
    float r = min(0.55 + t * (0.45 + 0.20 * warm), 1.0);
    float g = min(0.58 + t * 0.42, 1.0);
    float b = min(0.80 - t * (0.40 * warm), 1.0);
    return float4(r, g, b, 1.0);
}
