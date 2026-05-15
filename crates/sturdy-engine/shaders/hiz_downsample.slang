// Hierarchical-Z pyramid builder.
//
// Level 0 copies hardware depth into a sampled/storage-friendly float image.
// Deeper levels store the maximum depth from each 2x2 block of the previous
// level. The max-depth convention is conservative for standard [0, 1] depth.

Texture2D<float> hiz_source;
RWTexture2D<float> hiz_out;

struct HizConstants {
    uint2 source_extent;
    uint  copy_source;
    uint  _pad;
};
uniform HizConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    uint2 out_dims;
    hiz_out.GetDimensions(out_dims.x, out_dims.y);
    if (any(tid.xy >= out_dims)) return;

    uint2 max_source = push.source_extent - uint2(1, 1);

    if (push.copy_source != 0) {
        uint2 src_px = min(tid.xy, max_source);
        hiz_out[tid.xy] = hiz_source.Load(int3(src_px, 0)).r;
        return;
    }

    uint2 base_px = tid.xy * 2;
    float max_depth = 0.0;

    [unroll]
    for (uint y = 0; y < 2; ++y) {
        [unroll]
        for (uint x = 0; x < 2; ++x) {
            uint2 src_px = min(base_px + uint2(x, y), max_source);
            max_depth = max(max_depth, hiz_source.Load(int3(src_px, 0)).r);
        }
    }

    hiz_out[tid.xy] = max_depth;
}
