struct PushData
{
    float2 offset;
    float2 scale;
    float4 tint;
};

[[vk::push_constant]]
ConstantBuffer<PushData> push_data;

struct FragmentIn
{
    float4 position: SV_Position;
    float3 color: COLOR0;
};

[shader("fragment")]
float4 main(FragmentIn input): SV_Target0
{
    return float4(input.color, push_data.tint.a);
}
