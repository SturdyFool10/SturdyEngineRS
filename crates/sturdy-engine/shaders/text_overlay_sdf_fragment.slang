Texture2D<float4> text_atlas;
SamplerState text_atlas_sampler;

struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

float4 main(FSInput input) : SV_TARGET {
    float4 sample = text_atlas.SampleLevel(text_atlas_sampler, input.uv, 0.0);
    float distance = sample.r;
    float width = max(fwidth(distance), 1.0 / 255.0);
    float alpha = smoothstep(0.5 - width, 0.5 + width, distance);
    return float4(input.color.rgb, input.color.a * alpha);
}
