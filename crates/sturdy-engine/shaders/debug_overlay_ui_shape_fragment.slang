struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

struct UiShapeConstants {
    float4 sizeRadiusBorder;
    float4 fillColor;
    float4 borderColor;
};

float roundedBoxSdf(float2 p, float2 halfSize, float radius) {
    float2 q = abs(p) - max(halfSize - radius, float2(0.0, 0.0));
    return length(max(q, float2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

float4 main(FSInput input, uniform UiShapeConstants push) : SV_TARGET {
    float2 size = max(push.sizeRadiusBorder.xy, float2(1.0, 1.0));
    float radius = clamp(push.sizeRadiusBorder.z, 0.0, min(size.x, size.y) * 0.5);
    float border = max(push.sizeRadiusBorder.w, 0.0);
    float2 p = input.uv - size * 0.5;
    float sd = roundedBoxSdf(p, size * 0.5, radius);
    float aa = max(fwidth(sd), 0.75);
    float outer = 1.0 - smoothstep(-aa, aa, sd);

    if (border > 0.0 && push.borderColor.a > 0.0) {
        float fillCoverage = outer * (1.0 - smoothstep(-border - aa, -border + aa, sd));
        float borderCoverage = max(outer - fillCoverage, 0.0);
        float fillAlpha = push.fillColor.a * fillCoverage;
        float borderAlpha = push.borderColor.a * borderCoverage;
        float alpha = fillAlpha + borderAlpha;
        float3 rgb = alpha > 0.0
            ? (push.fillColor.rgb * fillAlpha + push.borderColor.rgb * borderAlpha) / alpha
            : float3(0.0, 0.0, 0.0);
        return float4(rgb, alpha);
    }

    return float4(push.fillColor.rgb, push.fillColor.a * outer);
}
