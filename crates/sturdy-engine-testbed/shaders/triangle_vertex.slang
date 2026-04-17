struct VertexOut
{
    float4 position: SV_Position;
    float3 color: COLOR0;
};

[shader("vertex")]
VertexOut main(float2 position: POSITION, float3 color: COLOR0)
{
    VertexOut output;
    output.position = float4(position, 0.0, 1.0);
    output.color = color;
    return output;
}
