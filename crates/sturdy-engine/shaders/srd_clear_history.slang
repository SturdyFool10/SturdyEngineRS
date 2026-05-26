// SRD global history-clear pass.
//
// Writes zero to a single storage texture. Dispatched by SrdInstance whenever
// SrdHistoryMode::ZeroHistory is in effect; lets every other pass assume the
// history slot is well-defined on its very first read.
//
// Reads:  (none)
// Writes: srd_clear_target  (RWTexture2D<float4>, the history slot to zero)
// Constants: SrdClearConstants (extent of the target in pixels)
// Dispatch: 8x8x1 thread groups covering the target extent

RWTexture2D<float4> srd_clear_target;

struct SrdClearConstants {
    uint2 extent;
    uint2 _pad;
};
uniform SrdClearConstants push;

[numthreads(8, 8, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    if (any(tid.xy >= push.extent)) return;
    srd_clear_target[tid.xy] = float4(0.0f, 0.0f, 0.0f, 0.0f);
}
