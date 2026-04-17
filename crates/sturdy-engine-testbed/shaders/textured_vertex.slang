struct VertexOut
{
    float4 position: SV_Position;
    float2 uv: TEXCOORD0;
};

[shader("vertex")]
VertexOut main(float2 position: POSITION, float2 uv: TEXCOORD0)
{
    VertexOut output;
    output.position = float4(position, 0.0, 1.0);
    output.uv = uv;
    return output;
}
