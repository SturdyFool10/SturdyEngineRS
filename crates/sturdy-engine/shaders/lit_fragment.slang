// Built-in lit fragment shader.
//
// Implements Lambert diffuse + Blinn-Phong specular driven by a directional
// light and per-mesh material parameters.
//
// Bindings (all written and bound by Scene::draw / Scene::prepare):
//   "lighting"      - StructuredBuffer<LightingUniforms>  (one element)
//   "material_desc" - StructuredBuffer<MaterialConstants> (one element)

// ── LightingUniforms (written by Scene::draw) ─────────────────────────────

struct LightingUniforms {
    float4 dir_direction;     // xyz = world-space toward-light direction, w = intensity
    float4 dir_color;         // xyz = light colour, w = unused
    float4 ambient;           // xyz = ambient colour, w = unused
    float4 camera_world_pos;  // xyz = camera position in world space, w = unused
};
StructuredBuffer<LightingUniforms> lighting;

// ── MaterialConstants (written by Scene::prepare) ─────────────────────────

struct MaterialConstants {
    float4 albedo;            // xyz = base colour (linear), w = opacity
    float4 emissive_metallic; // xyz = emissive (HDR OK), w = metallic [0,1]
    float4 roughness_pad;     // x = roughness [0,1], yzw = unused
};
StructuredBuffer<MaterialConstants> material_desc;

// ── Vertex output from mesh_vertex_3d.slang ───────────────────────────────

struct VSOut {
    float4 position  : SV_POSITION;
    float3 normal    : NORMAL;
    float2 uv        : TEXCOORD0;
    float3 world_pos : TEXCOORD1;
};

// ── Main ──────────────────────────────────────────────────────────────────

float4 main(VSOut input) : SV_TARGET {
    LightingUniforms L   = lighting[0];
    MaterialConstants M  = material_desc[0];

    float3 albedo    = M.albedo.xyz;
    float  opacity   = M.albedo.w;
    float3 emissive  = M.emissive_metallic.xyz;
    float  metallic  = M.emissive_metallic.w;
    float  roughness = M.roughness_pad.x;

    float3 N = normalize(input.normal);
    float3 L_dir = normalize(L.dir_direction.xyz);   // toward-light
    float3 V = normalize(L.camera_world_pos.xyz - input.world_pos);
    float3 H = normalize(L_dir + V);

    float intensity = L.dir_direction.w;

    // Lambert diffuse — metallic surfaces have no diffuse (absorbed into spec)
    float  diff          = max(dot(N, L_dir), 0.0);
    float3 diffuse_color = L.dir_color.xyz * diff * intensity * albedo * (1.0 - metallic);

    // Blinn-Phong specular — shininess from roughness; tint from metallic
    //   roughness = 0 → shininess = 1024 (near-mirror)
    //   roughness = 1 → shininess = 2    (very rough)
    float  shininess     = pow(2.0, 10.0 * (1.0 - roughness));
    float3 spec_color    = lerp(float3(0.04, 0.04, 0.04), albedo, metallic);
    float3 specular      = spec_color * pow(max(dot(N, H), 0.0), shininess) * intensity;

    // Ambient — attenuate by albedo so dark materials don't over-lighten
    float3 ambient = L.ambient.xyz * albedo;

    float3 color = ambient + diffuse_color + specular + emissive;
    return float4(color, opacity);
}
