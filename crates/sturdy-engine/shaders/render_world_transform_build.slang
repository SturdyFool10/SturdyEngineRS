// RenderWorld compact transform expansion.
//
// One thread expands one GpuTransformSourceData record into GPU-derived tables:
// current world matrix, previous world matrix, normal matrix rows, and
// world-space bounding sphere. This is the first compute stage after compact
// render-world source upload and before GPU culling/draw generation.

struct GpuTransformSourceData {
  float3 translation;
  uint parent_index;
  float4 rotation;
  float3 scale;
  uint flags;
  float3 previous_translation;
  float local_sphere_radius;    // local bounding sphere radius
  float4 previous_rotation;
  float3 previous_scale;
  uint _pad1;
  float3 local_sphere_center;   // local bounding sphere center
  uint _pad2;
};

struct NormalMatrixData {
  float4 row0;
  float4 row1;
  float4 row2;
};

StructuredBuffer<GpuTransformSourceData> transform_sources;
RWStructuredBuffer<float4x4> current_matrices;
RWStructuredBuffer<float4x4> previous_matrices;
RWStructuredBuffer<NormalMatrixData> normal_matrices;
RWStructuredBuffer<float4> world_bounds;

struct TransformBuildConstants {
  uint object_count;
  uint _pad0;
  uint _pad1;
  uint _pad2;
};
uniform TransformBuildConstants push;

float4 quat_normalize(float4 q) {
  float len2 = max(dot(q, q), 1.0e-20);
  return q * rsqrt(len2);
}

float3x3 rotation_matrix_from_quat(float4 q_in) {
  float4 q = quat_normalize(q_in);
  float x = q.x;
  float y = q.y;
  float z = q.z;
  float w = q.w;

  float xx = x * x;
  float yy = y * y;
  float zz = z * z;
  float xy = x * y;
  float xz = x * z;
  float yz = y * z;
  float wx = w * x;
  float wy = w * y;
  float wz = w * z;

  return float3x3(1.0 - 2.0 * (yy + zz), 2.0 * (xy + wz), 2.0 * (xz - wy),
                  2.0 * (xy - wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz + wx),
                  2.0 * (xz + wy), 2.0 * (yz - wx), 1.0 - 2.0 * (xx + yy));
}

float4x4 trs_matrix(float3 translation, float4 rotation, float3 scale) {
  float3x3 r = rotation_matrix_from_quat(rotation);
  float3 c0 = r[0] * scale.x;
  float3 c1 = r[1] * scale.y;
  float3 c2 = r[2] * scale.z;

  return float4x4(float4(c0.x, c0.y, c0.z, 0.0), float4(c1.x, c1.y, c1.z, 0.0),
                  float4(c2.x, c2.y, c2.z, 0.0), float4(translation, 1.0));
}

NormalMatrixData normal_matrix_from_trs(float4 rotation, float3 scale) {
  float3 safe_scale = max(abs(scale), float3(1.0e-8, 1.0e-8, 1.0e-8));
  float3 inv_scale = 1.0 / safe_scale;
  float3x3 r = rotation_matrix_from_quat(rotation);

  NormalMatrixData out_matrix;
  out_matrix.row0 = float4(r[0] * inv_scale.x, 0.0);
  out_matrix.row1 = float4(r[1] * inv_scale.y, 0.0);
  out_matrix.row2 = float4(r[2] * inv_scale.z, 0.0);
  return out_matrix;
}

// Compute world-space bounding sphere from local sphere + TRS.
// The sphere is first scaled (non-uniform scale turns it into an ellipsoid),
// then rotated and translated. A conservative bounding sphere is recovered by
// using max(|scale|) as the world radius multiplier.
float4 world_bounding_sphere(float3 translation, float4 rotation, float3 scale,
                             float3 local_center, float local_radius) {
  float3x3 r = rotation_matrix_from_quat(rotation);
  // Scale then rotate the local center to get world offset, then add translation.
  float3 world_center = mul(r, scale * local_center) + translation;
  // Conservative world radius: scale the local radius by the largest scale axis.
  float world_radius = max(max(abs(scale.x), abs(scale.y)), abs(scale.z)) * local_radius;
  return float4(world_center, world_radius);
}

[numthreads(64, 1, 1)]
void main(uint3 tid: SV_DispatchThreadID) {
  uint object_index = tid.x;
  if (object_index >= push.object_count)
    return;

  GpuTransformSourceData source = transform_sources[object_index];

  current_matrices[object_index] =
      trs_matrix(source.translation, source.rotation, source.scale);
  previous_matrices[object_index] =
      trs_matrix(source.previous_translation, source.previous_rotation,
                 source.previous_scale);
  normal_matrices[object_index] =
      normal_matrix_from_trs(source.rotation, source.scale);

  world_bounds[object_index] = world_bounding_sphere(
      source.translation, source.rotation, source.scale,
      source.local_sphere_center, source.local_sphere_radius);
}
