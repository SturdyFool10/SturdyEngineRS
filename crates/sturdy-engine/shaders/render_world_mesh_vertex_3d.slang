struct CameraConstants {
  float4x4 view_proj;
  float4x4 previous_view_proj;
  float time;
  float3 _pad;
};

struct VSInput {
  float3 position : POSITION;
  float3 normal : NORMAL;
  float2 uv : TEXCOORD0;
  float4 tangent : TANGENT;
};

struct VSOut {
  float4 position : SV_POSITION;
  float3 normal : NORMAL;
  float2 uv : TEXCOORD0;
  float3 world_pos : TEXCOORD1;
  float4 tangent : TANGENT;
};

StructuredBuffer<uint> render_world_visible_instances;
StructuredBuffer<float4x4> current_matrices;

VSOut main(VSInput input, uint instance_id: SV_InstanceID,
           uniform CameraConstants cam) {
  uint object_slot = render_world_visible_instances[instance_id];
  float4x4 model = current_matrices[object_slot];
  float4 world = mul(model, float4(input.position, 1.0));
  float3x3 model3 = (float3x3)model;

  VSOut o;
  o.position = mul(cam.view_proj, world);
  o.world_pos = world.xyz;
  o.normal = mul(model3, input.normal);
  o.uv = input.uv;
  o.tangent = float4(mul(model3, input.tangent.xyz), input.tangent.w);
  return o;
}
