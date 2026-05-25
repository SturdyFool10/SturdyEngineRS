// RenderWorld GPU draw generation v0.
//
// One thread scans one persistent bin's object range and writes one indirect
// command for that bin. This is intentionally conservative: it establishes the
// GPU-owned draw-generation contract before a later dense compaction pass.

struct DrawIndexedIndirectCommand {
  uint index_count;
  uint instance_count;
  uint first_index;
  int vertex_offset;
  uint first_instance;
};

struct RenderWorldGpuBinData {
  uint first_object_slot;
  uint object_count;
  uint mesh_id;
  uint pipeline_key;
  uint index_count;
  uint first_index;
  int vertex_offset;
  uint _pad;
};

StructuredBuffer<RenderWorldGpuBinData> render_world_bins;
StructuredBuffer<uint> visible_object_flags;
RWStructuredBuffer<DrawIndexedIndirectCommand> render_world_indirect_commands;
RWStructuredBuffer<uint> render_world_visible_draw_count;

struct DrawGenerationConstants {
  uint bin_count;
  uint _pad0;
  uint _pad1;
  uint _pad2;
};
uniform DrawGenerationConstants push;

[numthreads(64, 1, 1)]
void main(uint3 tid: SV_DispatchThreadID) {
  uint bin_id = tid.x;
  if (bin_id >= push.bin_count)
    return;

  RenderWorldGpuBinData bin = render_world_bins[bin_id];
  uint visible_count = 0;
  for (uint i = 0; i < bin.object_count; ++i) {
    uint object_slot = bin.first_object_slot + i;
    visible_count += visible_object_flags[object_slot] != 0u ? 1u : 0u;
  }

  DrawIndexedIndirectCommand cmd;
  cmd.index_count = bin.index_count;
  cmd.instance_count = visible_count;
  cmd.first_index = bin.first_index;
  cmd.vertex_offset = bin.vertex_offset;
  cmd.first_instance = bin.first_object_slot;
  render_world_indirect_commands[bin_id] = cmd;

  if (visible_count > 0u) {
    uint ignored;
    InterlockedAdd(render_world_visible_draw_count[0], 1u, ignored);
  }
}
