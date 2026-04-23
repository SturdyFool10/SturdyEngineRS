RWStructuredBuffer<float4> output_buffer : register(u0, space0);

[numthreads(8, 8, 1)]
void main(uint3 dispatch_id : SV_DispatchThreadID) {
  output_buffer[dispatch_id.x] = float4(dispatch_id, 1.0);
}
