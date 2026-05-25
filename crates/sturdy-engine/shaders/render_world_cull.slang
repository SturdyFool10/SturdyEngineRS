// Scene-wide RenderWorld frustum + Hi-Z occlusion culling.
//
// Reads GPU-derived world bounds and writes one visibility flag per object.
// Frustum planes are derived from view_proj (Gribb-Hartmann) so the same
// push constants serve both frustum and Hi-Z projection.
//
// Hi-Z test (when use_hiz != 0):
//   Projects the world-space bounding sphere into screen UV space, selects the
//   smallest Hi-Z mip level where the sphere footprint fits in one texel, and
//   checks the sphere near depth against the stored max depth.  If the sphere
//   is entirely behind previously-rendered geometry it is culled.
//
// Mip bindings hiz_l0..hiz_l7 hold pyramid levels 0..7 from the previous
// frame.  Levels beyond hiz_mip_count are not bound and must not be sampled.

StructuredBuffer<float4> world_bounds;
RWStructuredBuffer<uint> visible_object_flags;

// Hi-Z pyramid from the previous frame — up to 8 levels.
// Only valid when use_hiz != 0 and level < hiz_mip_count.
Texture2D<float> hiz_l0;
Texture2D<float> hiz_l1;
Texture2D<float> hiz_l2;
Texture2D<float> hiz_l3;
Texture2D<float> hiz_l4;
Texture2D<float> hiz_l5;
Texture2D<float> hiz_l6;
Texture2D<float> hiz_l7;

struct RenderWorldCullConstants {
  float4x4 view_proj;      // 64 bytes — used for both frustum and Hi-Z projection
  uint object_count;       // 4
  uint use_hiz;            // 4 — 0 = frustum only, 1 = frustum + Hi-Z
  uint screen_width;       // 4 — full-res width for mip level sizing
  uint screen_height;      // 4 — full-res height for mip level sizing
  uint hiz_mip_count;      // 4 — number of valid Hi-Z pyramid levels
  float proj_y_scale;      // 4 — proj[1][1], used to compute screen-space radius
  float proj_z_scale;      // 4 — proj[2][2], used to compute near-plane depth
  uint _pad;               // 4
  // Total: 96 bytes
};
uniform RenderWorldCullConstants push;

// ── Frustum ───────────────────────────────────────────────────────────────────

// Extract six inward-pointing frustum planes from a column-major VP matrix
// using the Gribb-Hartmann method (D3D/Vulkan depth [0,1] convention).
// Each plane is float4(nx, ny, nz, d) with dot(plane, float4(pos,1)) >= 0
// meaning the point is inside.
float4[6] extract_frustum_planes(float4x4 m) {
  // m[col][row] in HLSL/Slang column-major.
  // Row i: float4(m[0][i], m[1][i], m[2][i], m[3][i])
  float4 r0 = float4(m[0][0], m[1][0], m[2][0], m[3][0]);
  float4 r1 = float4(m[0][1], m[1][1], m[2][1], m[3][1]);
  float4 r2 = float4(m[0][2], m[1][2], m[2][2], m[3][2]);
  float4 r3 = float4(m[0][3], m[1][3], m[2][3], m[3][3]);

  float4 planes[6];
  planes[0] = r3 + r0; // left
  planes[1] = r3 - r0; // right
  planes[2] = r3 + r1; // bottom
  planes[3] = r3 - r1; // top
  planes[4] = r3 + r2; // near  (Vulkan NDC [0,1])
  planes[5] = r3 - r2; // far

  [unroll]
  for (int i = 0; i < 6; ++i) {
    float len = length(planes[i].xyz);
    if (len > 1.0e-6)
      planes[i] /= len;
  }
  return planes;
}

bool in_frustum(float4[6] planes, float3 center, float radius) {
  [unroll]
  for (int i = 0; i < 6; ++i) {
    if (dot(planes[i], float4(center, 1.0)) < -radius)
      return false;
  }
  return true;
}

// ── Hi-Z occlusion ────────────────────────────────────────────────────────────

// Load one texel from the appropriate pyramid level using integer coordinates.
// Each pyramid level is a separate Texture2D; we branch on level at compile time.
float load_hiz_level(float2 uv, uint level) {
  // Level dimensions halve each step: (screen_width >> level, screen_height >> level).
  uint lw = max(push.screen_width  >> level, 1u);
  uint lh = max(push.screen_height >> level, 1u);
  int2 coord = clamp(int2(uv * float2(lw, lh)), int2(0, 0), int2(lw - 1, lh - 1));

  if (level == 0) return hiz_l0.Load(int3(coord, 0));
  if (level == 1) return hiz_l1.Load(int3(coord, 0));
  if (level == 2) return hiz_l2.Load(int3(coord, 0));
  if (level == 3) return hiz_l3.Load(int3(coord, 0));
  if (level == 4) return hiz_l4.Load(int3(coord, 0));
  if (level == 5) return hiz_l5.Load(int3(coord, 0));
  if (level == 6) return hiz_l6.Load(int3(coord, 0));
  return hiz_l7.Load(int3(coord, 0));
}

// Returns true when the sphere is occluded by the Hi-Z depth pyramid.
// Uses a conservative single-texel test at the smallest mip where the sphere
// footprint fits within one texel.
bool hiz_occluded(float3 world_center, float world_radius) {
  float4 clip = mul(push.view_proj, float4(world_center, 1.0));

  // Sphere near point must be in front of the camera.
  // clip.w approximately equals linear view depth; if the near face is behind
  // the camera the test cannot be performed safely.
  float near_clip_w = clip.w - world_radius;
  if (near_clip_w <= 0.0)
    return false;

  float inv_w = 1.0 / clip.w;
  float2 ndc = clip.xy * inv_w;

  // Screen-space radius of the sphere in normalized [0,1] UV coordinates.
  // proj_y_scale = proj[1][1] = 1/tan(fov_y/2); multiply by half screen height
  // to get pixels, then divide by screen_height to get UV fraction.
  float screen_radius_uv = abs(push.proj_y_scale) * world_radius * inv_w * 0.5;

  // UV bounding box of the sphere footprint, clamped to [0,1].
  float2 uv_center = ndc * float2(0.5, -0.5) + 0.5;
  float2 uv_min = saturate(uv_center - screen_radius_uv);
  float2 uv_max = saturate(uv_center + screen_radius_uv);

  // Screen-space footprint diameter in pixels.
  float2 footprint_px = (uv_max - uv_min) * float2(push.screen_width, push.screen_height);
  float  diameter_px  = max(footprint_px.x, footprint_px.y);

  // Smallest mip level where the footprint fits in one texel:
  //   2^mip >= diameter_px → mip = ceil(log2(diameter_px))
  uint mip = (uint)ceil(log2(max(diameter_px, 1.0)));
  mip = clamp(mip, 0u, push.hiz_mip_count - 1u);

  // Near-plane NDC depth of the sphere front face.
  // For a right-handed perspective with clip.w ≈ -z_view:
  //   near_clip.z ≈ clip.z + proj_z_scale * world_radius
  //   near_clip.w = clip.w - world_radius
  float near_clip_z  = clip.z + push.proj_z_scale * world_radius;
  float near_ndc_z   = near_clip_z / near_clip_w;

  // Sample max depth at the selected mip.
  float hiz_depth = load_hiz_level((uv_min + uv_max) * 0.5, mip);

  // Occluded when the sphere's nearest projected depth is farther than the
  // maximum stored depth in the Hi-Z region (standard depth: 0=near, 1=far).
  return near_ndc_z > hiz_depth;
}

// ── Main ──────────────────────────────────────────────────────────────────────

[numthreads(64, 1, 1)]
void main(uint3 tid: SV_DispatchThreadID) {
  uint object_index = tid.x;
  if (object_index >= push.object_count)
    return;

  float4 bounds = world_bounds[object_index];
  float3 center = bounds.xyz;
  float  radius = bounds.w;

  float4[6] planes = extract_frustum_planes(push.view_proj);
  if (!in_frustum(planes, center, radius)) {
    visible_object_flags[object_index] = 0u;
    return;
  }

  if (push.use_hiz != 0u && hiz_occluded(center, radius)) {
    visible_object_flags[object_index] = 0u;
    return;
  }

  visible_object_flags[object_index] = 1u;
}
