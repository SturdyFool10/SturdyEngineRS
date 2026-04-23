use std::collections::HashMap;
use glam::Mat4;

use crate::{Engine, GraphImage, Mesh, MeshProgram, RenderFrame, Result, push_constants};
use super::{
    batch::InstanceBatch,
    camera::{CameraId, CameraOutput, SceneCamera},
    object::{InstanceData, MeshId, ObjectId, ObjectKind, SceneObject},
};

/// Push constants sent to the vertex shader for each camera draw call.
///
/// Vertex shaders must declare `uniform CameraConstants cam` and use
/// `cam.view_proj` to transform vertices into clip space.
#[push_constants]
pub struct CameraConstants {
    pub view_proj: [[f32; 4]; 4],
    pub previous_view_proj: [[f32; 4]; 4],
}

impl CameraConstants {
    pub fn identity() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            previous_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
        }
    }

    pub fn from_camera(camera: &SceneCamera) -> Self {
        Self {
            view_proj: camera.view_proj().to_cols_array_2d(),
            previous_view_proj: camera.previous_view_proj.to_cols_array_2d(),
        }
    }
}

/// A managed collection of meshes, object instances, and cameras.
///
/// # Workflow
///
/// ```rust
/// // At init:
/// let mesh_id = scene.add_mesh(mesh, program);
/// let obj_id  = scene.add_object(mesh_id, Mat4::IDENTITY, ObjectKind::Static);
/// let crt_cam = scene.add_camera(SceneCamera::offscreen(view, proj, crt_target));
///
/// // Each frame:
/// scene.set_transform(dynamic_obj, new_transform);
/// scene.prepare(engine)?;                         // upload dirty instance data
/// scene.render(frame)?;                           // draw all offscreen cameras
/// scene.render_camera_to(main_cam, &hdr_out, frame)?;  // draw main camera
/// ```
///
/// # Instance buffer convention
///
/// Every draw pass binds the instance storage buffer under the name `"instances"`.
/// Vertex shaders must declare:
/// ```slang
/// struct InstanceData { float4x4 model; };
/// StructuredBuffer<InstanceData> instances;
/// ```
/// and read `instances[SV_InstanceID].model` for the per-instance model matrix.
pub struct Scene {
    meshes: Vec<(Mesh, MeshProgram)>,
    objects: Vec<SceneObject>,
    cameras: Vec<SceneCamera>,
    batches: HashMap<u32, InstanceBatch>,
    next_object_id: u32,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            meshes: Vec::new(),
            objects: Vec::new(),
            cameras: Vec::new(),
            batches: HashMap::new(),
            next_object_id: 0,
        }
    }

    /// Register a mesh+program pair. Returns a `MeshId` used to spawn objects.
    pub fn add_mesh(&mut self, mesh: Mesh, program: MeshProgram) -> MeshId {
        let id = MeshId(self.meshes.len() as u32);
        self.meshes.push((mesh, program));
        id
    }

    /// Add an object instance. Returns an `ObjectId` for later transform updates.
    pub fn add_object(&mut self, mesh_id: MeshId, transform: Mat4, kind: ObjectKind) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
        self.objects.push(SceneObject::new(mesh_id, transform, kind));
        id
    }

    /// Update a dynamic object's transform.
    ///
    /// Static objects ignore this call. To move a static object, remove and re-add it.
    pub fn set_transform(&mut self, id: ObjectId, transform: Mat4) {
        if let Some(obj) = self.objects.get_mut(id.0 as usize) {
            obj.transform = transform;
            if matches!(obj.kind, ObjectKind::Static) {
                obj.static_dirty = true;
            }
        }
    }

    /// Add a camera with an offscreen `RenderTarget`. Returns a `CameraId`.
    pub fn add_camera(&mut self, camera: SceneCamera) -> CameraId {
        let id = CameraId(self.cameras.len() as u32);
        self.cameras.push(camera);
        id
    }

    /// Borrow a camera mutably — update view/projection before calling `render`.
    pub fn camera_mut(&mut self, id: CameraId) -> Option<&mut SceneCamera> {
        self.cameras.get_mut(id.0 as usize)
    }

    /// Upload instance data for all dirty batches.
    ///
    /// Call once per frame after all `set_transform` calls, before `render`.
    pub fn prepare(&mut self, engine: &Engine) -> Result<()> {
        // Clear dynamic lists; static lists persist across frames.
        for batch in self.batches.values_mut() {
            batch.dynamic_instances.clear();
        }

        // First pass: detect which static batches are dirty.
        for obj in &mut self.objects {
            if matches!(obj.kind, ObjectKind::Static) && obj.static_dirty {
                if let Some(batch) = self.batches.get_mut(&obj.mesh_id.0) {
                    batch.static_dirty = true;
                }
                obj.static_dirty = false;
            }
        }

        // Clear static lists for dirty batches before rebuilding.
        for batch in self.batches.values_mut() {
            if batch.static_dirty {
                batch.static_instances.clear();
            }
        }

        // Second pass: fill instance lists.
        for obj in &self.objects {
            let batch = self.batches.entry(obj.mesh_id.0).or_insert_with(|| {
                InstanceBatch::new(obj.mesh_id.0)
            });
            match obj.kind {
                ObjectKind::Static => {
                    if batch.static_dirty || batch.static_instances.is_empty() {
                        batch.static_instances.push(InstanceData::from_transform(obj.transform));
                    }
                }
                ObjectKind::Dynamic => {
                    batch.dynamic_instances.push(InstanceData::from_transform(obj.transform));
                }
            }
        }

        for batch in self.batches.values_mut() {
            batch.prepare(engine)?;
        }

        Ok(())
    }

    /// Draw all offscreen cameras into the render frame.
    ///
    /// Call this before the main camera pass so offscreen textures are available
    /// as named frame images when the main scene draws objects that sample them.
    pub fn render(&self, frame: &RenderFrame) -> Result<()> {
        for camera in &self.cameras {
            let CameraOutput::Offscreen(rt) = &camera.output;
            let target = rt.as_frame_image(frame)?;
            self.draw_batches(camera, &target, frame)?;
        }
        Ok(())
    }

    /// Draw a single camera into an explicit output image.
    ///
    /// Use this for the main camera whose output is a frame-managed image
    /// (e.g. `frame.hdr_color_image("scene_color")?`).
    pub fn render_camera_to(
        &self,
        camera_id: CameraId,
        output: &GraphImage,
        frame: &RenderFrame,
    ) -> Result<()> {
        let camera = self.cameras.get(camera_id.0 as usize).ok_or_else(|| {
            crate::Error::InvalidInput(format!("camera {:?} not found in scene", camera_id))
        })?;
        self.draw_batches(camera, output, frame)
    }

    fn draw_batches(
        &self,
        camera: &SceneCamera,
        output: &GraphImage,
        frame: &RenderFrame,
    ) -> Result<()> {
        let constants = CameraConstants::from_camera(camera);

        for batch in self.batches.values() {
            let buf = match &batch.gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            let total = batch.total_count();
            if total == 0 {
                continue;
            }

            let (mesh, program) = &self.meshes[batch.mesh_idx as usize];
            frame.bind_buffer("instances", buf);
            output.draw_mesh_instanced_with_push_constants(mesh, program, buf, total, &constants)?;
        }

        Ok(())
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}
