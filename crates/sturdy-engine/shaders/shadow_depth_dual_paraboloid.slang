// Dual-paraboloid depth pass for point light shadow maps.
//
// Renders the scene into two paraboloid depth maps (front and back hemisphere)
// in a single pass using geometry deformation in the vertex shader.
//
// Paraboloid mapping:
//   Given a world-space position P relative to the light:
//     d = length(P_light)
//     P_n = normalize(P_light)
//     front hemisphere (P_n.z > 0): uv = (P_n.xy / (1 + P_n.z), depth)
//     back  hemisphere (P_n.z < 0): flip z, same formula
//
// The front paraboloid covers z > 0 (front hemisphere) and the back covers z < 0.
// Two separate render passes write into two separate `Depth32Float` maps.
//
// Roadmap: Track 6e — Point light shadow maps.

struct InstanceData { float4x4 model; };
StructuredBuffer<InstanceData> instances;

struct PointShadowConstants {
    float4x4 light_world_to_local;  // world → light-local transform (no projection)
    float     near;
    float     far;
    float     is_back;              // 1.0 for back hemisphere, 0.0 for front
    float     _pad;
};

struct VSInput {
    float3 position : POSITION;
    float3 normal   : NORMAL;
    float2 uv       : TEXCOORD0;
};

struct VSOut {
    float4 position : SV_POSITION;
    float  depth    : TEXCOORD0;   // linear depth [0, 1]
};

VSOut main(VSInput input, uint instance_id : SV_InstanceID,
           uniform PointShadowConstants push) {
    float4x4 model     = instances[instance_id].model;
    float3   world_pos = mul(model, float4(input.position, 1.0)).xyz;

    // Transform to light-local space.
    float3 lp = mul(push.light_world_to_local, float4(world_pos, 1.0)).xyz;

    // Flip Z for back hemisphere.
    if (push.is_back > 0.5) lp.z = -lp.z;

    float dist = length(lp);
    float lin_depth = (dist - push.near) / (push.far - push.near);

    // Paraboloid projection: map hemisphere onto a disk.
    float3 lp_n = lp / max(dist, 1e-5);
    float2 dp_uv = lp_n.xy / (1.0 + lp_n.z);  // [-1, 1]

    VSOut o;
    o.position = float4(dp_uv.x, -dp_uv.y, lin_depth, 1.0);
    o.depth    = lin_depth;
    return o;
}
