// Depth-only vertex shader for shadow map generation.
//
// Transforms mesh vertices into the light's clip space.
// No fragment shader is needed — the rasterizer writes depth automatically.
// Bindings are identical to mesh_vertex_3d.slang so the same instance buffer works.

struct InstanceData { float4x4 model; };
StructuredBuffer<InstanceData> instances;

struct ShadowConstants {
    float4x4 light_view_proj;
};
uniform ShadowConstants shadow;

struct VSInput {
    float3 position : POSITION;
    float3 normal   : NORMAL;   // unused — present to satisfy the vertex layout
    float2 uv       : TEXCOORD0; // unused
};

float4 main(VSInput input, uint instance_id : SV_InstanceID) : SV_POSITION {
    float4x4 model = instances[instance_id].model;
    float4 world   = mul(model, float4(input.position, 1.0));
    return mul(shadow.light_view_proj, world);
}
