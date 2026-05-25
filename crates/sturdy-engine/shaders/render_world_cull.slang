// Scene-wide RenderWorld frustum culling.
//
// Reads GPU-derived world bounds and writes one visibility flag per object.
// This is intentionally object-wide rather than batch-wide; later draw
// generation and compaction stages consume `visible_object_flags` to build
// indirect commands.

StructuredBuffer<float4> world_bounds;
RWStructuredBuffer<uint> visible_object_flags;

struct RenderWorldCullConstants {
  float4 frustum_planes[6];
  uint object_count;
  uint _pad0;
  uint _pad1;
  uint _pad2;
};
uniform RenderWorldCullConstants push;

bool in_frustum(float3 center, float radius) {
  [unroll]
  for (int i = 0; i < 6; ++i) {
    if (dot(push.frustum_planes[i], float4(center, 1.0)) < -radius)
      return false;
  }
  return true;
}

[numthreads(64, 1, 1)]
void main(uint3 tid: SV_DispatchThreadID) {
  uint object_index = tid.x;
  if (object_index >= push.object_count)
    return;

  float4 bounds = world_bounds[object_index];
  visible_object_flags[object_index] =
      in_frustum(bounds.xyz, bounds.w) ? 1u : 0u;
}
