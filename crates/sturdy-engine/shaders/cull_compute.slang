// GPU frustum culling compute shader.
//
// One thread per instance. Each thread tests the instance's pre-transformed
// world-space bounding sphere against the six camera frustum planes and writes
// one DrawIndexedIndirectCommand into the output buffer at its own slot.
//
// Visible instances: { index_count, 1, first_index, vertex_offset, instance_idx }
// Invisible instances: { 0, 0, 0, 0, instance_idx }  (GPU skips instance_count==0)
//
// The CPU draws the entire slot range with draw_count = instance_count; the GPU
// silently skips zero-instance commands, so no atomics or readback are needed.
//
// Roadmap: Track 7c — GPU frustum culling.

struct DrawIndexedIndirectCommand {
    uint index_count;
    uint instance_count;
    uint first_index;
    int  vertex_offset;
    uint first_instance;
};

/// Pre-transformed world-space bounding sphere (xyz = center, w = radius).
StructuredBuffer<float4> instance_bounds;
RWStructuredBuffer<DrawIndexedIndirectCommand> indirect_commands;

struct CullConstants {
    float4 frustum_planes[6]; // Gribb-Hartmann planes (nx,ny,nz,d); test: dot(plane,[pos,1]) < -r
    uint   instance_count;
    uint   index_count;
    uint   first_index;
    int    vertex_offset;
};
uniform CullConstants push;

bool in_frustum(float3 center, float radius) {
    [unroll]
    for (int i = 0; i < 6; ++i) {
        if (dot(push.frustum_planes[i], float4(center, 1.0)) < -radius)
            return false;
    }
    return true;
}

[numthreads(64, 1, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    uint idx = tid.x;
    if (idx >= push.instance_count) return;

    float4 sphere  = instance_bounds[idx];
    bool   visible = in_frustum(sphere.xyz, sphere.w);

    DrawIndexedIndirectCommand cmd;
    cmd.index_count    = visible ? push.index_count  : 0u;
    cmd.instance_count = visible ? 1u                : 0u;
    cmd.first_index    = push.first_index;
    cmd.vertex_offset  = push.vertex_offset;
    cmd.first_instance = idx;
    indirect_commands[idx] = cmd;
}
