Texture2D<float4> sampled_image;
RWStructuredBuffer<float4> output_buffer;

struct SampleImageConstants {
    uint2 pixel;
    uint  _pad0;
    uint  _pad1;
};
uniform SampleImageConstants push;

[numthreads(1, 1, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (tid.x != 0 || tid.y != 0 || tid.z != 0) return;
    output_buffer[0] = sampled_image.Load(int3(push.pixel, 0));
}
