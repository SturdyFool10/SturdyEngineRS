struct InstanceData { float4x4 model; };
StructuredBuffer<InstanceData> instances;

struct CameraConstants {
    float4x4 view_proj;
    float4x4 previous_view_proj;
    float    time;
    float3   _pad;
};
uniform CameraConstants cam;

struct VSInput {
    float3 position : POSITION;
    float3 normal   : NORMAL;
    float2 uv       : TEXCOORD0;
    float4 tangent  : TANGENT;
};

struct VSOut {
    float4 position   : SV_POSITION;
    float3 normal     : NORMAL;
    float2 uv         : TEXCOORD0;
    float3 world_pos  : TEXCOORD1;
    // tangent.xyz in world space; .w = bitangent handedness sign (+1 or -1)
    float4 tangent    : TANGENT;
};

VSOut main(VSInput input, uint instance_id : SV_InstanceID) {
    float4x4 model  = instances[instance_id].model;
    float4   world  = mul(model, float4(input.position, 1.0));
    float3x3 model3 = (float3x3)model;
    VSOut o;
    o.position  = mul(cam.view_proj, world);
    o.world_pos = world.xyz;
    o.normal    = mul(model3, input.normal);
    o.uv        = input.uv;
    o.tangent   = float4(mul(model3, input.tangent.xyz), input.tangent.w);
    return o;
}
