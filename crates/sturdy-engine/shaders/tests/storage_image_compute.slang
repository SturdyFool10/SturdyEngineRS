Texture2D<float4> src;
RWTexture2D<float4> dst;

[numthreads(8, 8, 1)]
void main(uint3 dispatch_id : SV_DispatchThreadID) {
    uint2 dims;
    dst.GetDimensions(dims.x, dims.y);
    if (any(dispatch_id.xy >= dims)) {
        return;
    }

    dst[dispatch_id.xy] = src.Load(int3(dispatch_id.xy, 0));
}
