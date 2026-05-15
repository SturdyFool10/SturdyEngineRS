// GPU frustum culling compute shader (Track 7c / Track 8b).
//
// One thread per instance. Each thread tests the instance's world-space
// bounding sphere against the six camera frustum planes and writes one
// DrawIndexedIndirectCommand into the output buffer at its slot.
//
// Visible instances: { index_count, 1, first_index, vertex_offset, instance_idx }
// Invisible instances: { 0, 0, 0, 0, instance_idx }  (GPU skips instance_count==0)
//
// The CPU draws the entire slot range; the GPU silently skips zero-instance
// commands, so no atomics or readback are needed.
//
// ## Track 8b upgrade
//
// The shader now reads from a flat `StructuredBuffer<GpuInstanceData>` that
// covers ALL scene instances (not just one batch). The caller passes
// `batch_base_idx` so each per-batch dispatch reads the correct slice.
// This eliminates the separate `instance_bounds` buffer that was previously
// populated on the CPU; bounds are now derived directly from `GpuInstanceData`.
//
// Roadmap: Track 7c + Track 8b — GPU frustum culling via unified scene buffer.

struct DrawIndexedIndirectCommand {
    uint index_count;
    uint instance_count;
    uint first_index;
    int  vertex_offset;
    uint first_instance;
};

/// Unified per-object GPU data (matches scene/gpu_instance.rs::GpuInstanceData).
struct GpuInstanceData {
    float4x4 model;         // world-space model matrix (column-major)
    float4   bounds;        // world-space bounding sphere: xyz=center, w=radius
    uint     mesh_id;
    uint     material_id;
    float    lod_bias;
    uint     flags;         // bit 0 = STATIC, bit 1 = CAST_SHADOW, etc.
};

StructuredBuffer<GpuInstanceData>              scene_instances;
RWStructuredBuffer<DrawIndexedIndirectCommand> indirect_commands;

struct CullConstants {
    float4 frustum_planes[6]; // Gribb-Hartmann: dot(plane, float4(pos,1)) < -r → outside
    uint   instance_count;    // number of instances in this batch
    uint   batch_base_idx;    // first index in scene_instances for this batch
    uint   index_count;       // draw parameters forwarded to indirect command
    uint   first_index;
    int    vertex_offset;
    uint   _pad;
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
    uint local_idx = tid.x;
    if (local_idx >= push.instance_count) return;

    // Read bounds from the scene buffer slice for this batch.
    uint global_idx = push.batch_base_idx + local_idx;
    float4 sphere  = scene_instances[global_idx].bounds;
    bool   visible = in_frustum(sphere.xyz, sphere.w);

    DrawIndexedIndirectCommand cmd;
    cmd.index_count    = visible ? push.index_count : 0u;
    cmd.instance_count = visible ? 1u               : 0u;
    cmd.first_index    = push.first_index;
    cmd.vertex_offset  = push.vertex_offset;
    cmd.first_instance = local_idx;       // local within batch
    indirect_commands[local_idx] = cmd;
}
