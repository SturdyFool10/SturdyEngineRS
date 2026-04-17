struct PushData
{
    float2 offset;
    float2 scale;
    float4 tint;
};

[[vk::push_constant]]
ConstantBuffer<PushData> push_data;

struct VertexOut
{
    float4 position: SV_Position;
    float3 color: COLOR0;
};

[shader("vertex")]
VertexOut main(float2 position: POSITION, float3 color: COLOR0)
{
    VertexOut output;
    float2 p = position * push_data.scale + push_data.offset;
    output.position = float4(p, 0.0, 1.0);
    output.color = color * push_data.tint.rgb;
    return output;
}
