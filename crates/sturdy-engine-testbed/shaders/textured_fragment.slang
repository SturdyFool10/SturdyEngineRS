Texture2D<float4> base_color : register(t0, space0);
SamplerState base_sampler : register(s1, space0);

struct FragmentIn
{
    float4 position: SV_Position;
    float2 uv: TEXCOORD0;
};

[shader("fragment")]
float4 main(FragmentIn input): SV_Target0
{
    return base_color.Sample(base_sampler, input.uv);
}
