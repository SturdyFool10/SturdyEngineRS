use glam::Mat4;

/// Stable handle to a mesh+program pair registered with a [`Scene`](super::Scene).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct MeshId(pub(super) u32);

/// Stable handle to an object instance within a [`Scene`](super::Scene).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ObjectId(pub(super) u32);

/// Whether an object moves every frame or stays fixed.
///
/// Static objects have their instance data uploaded once and reused until the
/// transform changes. Dynamic objects are re-uploaded every frame. Choose
/// `Static` for world geometry; choose `Dynamic` for animated or player-driven
/// objects.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ObjectKind {
    Static,
    Dynamic,
}

/// Per-instance GPU data — a 4×4 model matrix sent to the vertex shader.
///
/// The default vertex shader reads this as:
/// ```slang
/// struct InstanceData { float4x4 model; };
/// StructuredBuffer<InstanceData> instances;
/// float4 clip = mul(cam.view_proj, mul(instances[SV_InstanceID].model, float4(pos, 1.0)));
/// ```
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceData {
    pub model: [[f32; 4]; 4],
}

impl InstanceData {
    pub fn from_transform(transform: Mat4) -> Self {
        Self {
            model: transform.to_cols_array_2d(),
        }
    }
}

/// A single renderable instance within a [`Scene`](super::Scene).
pub struct SceneObject {
    pub mesh_id: MeshId,
    pub transform: Mat4,
    pub kind: ObjectKind,
    /// True when static instance data needs to be re-uploaded.
    pub(super) static_dirty: bool,
}

impl SceneObject {
    pub fn new(mesh_id: MeshId, transform: Mat4, kind: ObjectKind) -> Self {
        Self {
            mesh_id,
            transform,
            kind,
            static_dirty: true,
        }
    }
}
