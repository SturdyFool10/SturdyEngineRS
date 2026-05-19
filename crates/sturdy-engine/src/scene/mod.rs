mod atomic_transform;
mod batch;
mod camera;
mod commands;
mod gpu_constants;
mod gpu_culling;
mod gpu_instance;
mod instance_metadata;
mod lights;
pub mod material;
mod material_state;
mod object;
mod orbit_camera;
mod queries;
mod render_target;
mod scene;

pub use camera::{CameraId, CameraOutput, SceneCamera};
pub use commands::{SceneCommands, SceneView};
pub use gpu_constants::CameraConstants;
pub use gpu_instance::GpuInstanceData;
pub use lights::{DirectionalLight, DiskLight, PointLight, RectLight, SphereLight, SpotLight};
pub use material::{
    MaterialDomain, MaterialExpr, MaterialInput, RenderState, ShadingModel, UnifiedMaterial,
    UnifiedMaterialBuilder, UvSource, gbuffer,
};
pub use material_state::MaterialDescriptor;
pub use object::{InstanceData, MeshId, ObjectId, ObjectKind};
pub use orbit_camera::OrbitCamera;
pub use render_target::RenderTarget;
pub use scene::Scene;

#[cfg(test)]
mod tests;
