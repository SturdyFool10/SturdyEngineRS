[[vk::binding(0, 0)]]
RWStructuredBuffer<uint> outputBuffer : register(u0, space0);

[shader("compute")]
[numthreads(1, 1, 1)]
void main(uint3 dispatchThreadId: SV_DispatchThreadID)
{
    outputBuffer[0] = 0x53545552u;
}
