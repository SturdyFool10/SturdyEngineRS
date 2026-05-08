// Skinned mesh vertex shader — Linear Blend Skinning (LBS).
//
// Reads up to 4 bone indices and weights per vertex, blends the model matrices
// from `bone_matrices`, and outputs the same VSOut as mesh_vertex_3d.slang so
// the G-Buffer and shadow fragment shaders need no changes.
//
// Bind the bone matrix buffer before each skinned draw:
//   frame.bind_buffer("bone_matrices", &skin.bone_buffer);

struct InstanceData  { float4x4 model; };
StructuredBuffer<InstanceData> instances;

struct CameraConstants {
    float4x4 view_proj;
    float4x4 previous_view_proj;
    float    time;
    float3   _pad;
};
uniform CameraConstants cam;

/// Skinning matrices: one float4x4 per joint in the skin, in joint order.
StructuredBuffer<float4x4> bone_matrices;

struct VSInput {
    float3 position   : POSITION;
    float3 normal     : NORMAL;
    float2 uv         : TEXCOORD0;
    float4 tangent    : TANGENT;
    // Joint indices stored as float4 (GPU vertex format constraint); cast to uint4 below.
    float4 joint_idx_f : TEXCOORD2;
    float4 joint_wt    : TEXCOORD3;
};

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
    float4 tangent   : TANGENT;
};

VSOut main(VSInput input, uint instance_id : SV_InstanceID) {
    // Cast float indices back to uint for buffer indexing.
    uint4 ji = uint4(input.joint_idx_f);

    // Blend skinning matrix from up to 4 bone contributions.
    float4x4 skin_mat = bone_matrices[ji.x] * input.joint_wt.x
                      + bone_matrices[ji.y] * input.joint_wt.y
                      + bone_matrices[ji.z] * input.joint_wt.z
                      + bone_matrices[ji.w] * input.joint_wt.w;

    float4x4 model  = instances[instance_id].model;
    float4x4 final_m = mul(model, skin_mat);
    float3x3 norm_m  = (float3x3)final_m;

    float4 world = mul(final_m, float4(input.position, 1.0));
    VSOut o;
    o.position  = mul(cam.view_proj, world);
    o.world_pos = world.xyz;
    o.normal    = mul(norm_m, input.normal);
    o.uv        = input.uv;
    o.tangent   = float4(mul(norm_m, input.tangent.xyz), input.tangent.w);
    return o;
}
