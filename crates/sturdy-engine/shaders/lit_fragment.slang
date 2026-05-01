// Built-in lit fragment shader.
//
// Implements Lambert diffuse + Blinn-Phong specular driven by a directional
// light.  Bind a storage buffer named "lighting" containing one LightingUniforms
// element (written by Scene::draw) before issuing draw calls.
//
// The diffuse tint comes from the interpolated UV encoded as a placeholder
// (uv.x, uv.y, 1-uv.x) until a material albedo buffer is bound.  Override this
// shader for real per-material colour.

struct LightingUniforms {
    float4 dir_direction;     // xyz = world-space toward-light direction, w = intensity
    float4 dir_color;         // xyz = light colour, w = unused
    float4 ambient;           // xyz = ambient colour, w = unused
    float4 camera_world_pos;  // xyz = camera world position, w = unused
};
StructuredBuffer<LightingUniforms> lighting;

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
};

float4 main(VSOut input) : SV_TARGET {
    LightingUniforms L = lighting[0];

    float3 N = normalize(input.normal);
    float3 light_dir = normalize(L.dir_direction.xyz);

    // View direction for specular
    float3 V = normalize(L.camera_world_pos.xyz - input.world_pos);
    float3 H = normalize(light_dir + V);

    // Lambert diffuse
    float diff = max(dot(N, light_dir), 0.0);

    // Blinn-Phong specular (shininess = 32)
    float spec = pow(max(dot(N, H), 0.0), 32.0);

    float intensity = L.dir_direction.w;
    float3 ambient  = L.ambient.xyz;
    float3 diffuse  = L.dir_color.xyz * diff * intensity;
    float3 specular = float3(0.5, 0.5, 0.5) * spec * intensity;

    // Placeholder albedo from UVs until material textures are wired up
    float3 albedo = float3(input.uv.x, input.uv.y, 1.0 - input.uv.x);

    float3 color = ambient + (diffuse + specular) * albedo;
    return float4(color, 1.0);
}
