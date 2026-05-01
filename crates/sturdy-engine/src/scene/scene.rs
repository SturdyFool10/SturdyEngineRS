use glam::{Mat4, Vec3, Vec4};
use std::collections::HashMap;

use super::{
    batch::InstanceBatch,
    camera::{CameraId, CameraOutput, SceneCamera},
    object::{InstanceData, MeshId, ObjectId, ObjectKind, SceneObject},
};
use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, Error, Format, GraphImage, ImageDesc, ImageDimension,
    ImageUsage, Mesh, MeshProgram, RenderFrame, Result, push_constants,
};
use sturdy_engine_core::Extent3d;

/// A directional light illuminating the entire scene.
///
/// Set via [`Scene::directional_light`] before calling [`Scene::draw`].
/// The default is a warm sunlight from above-right with a dark-grey ambient.
#[derive(Clone, Debug)]
pub struct DirectionalLight {
    /// World-space direction the light shines **toward** (normalized before upload).
    pub direction: Vec3,
    /// Diffuse + specular colour of the light.
    pub color: Vec3,
    /// Multiplier applied to both diffuse and specular contributions.
    pub intensity: f32,
    /// Constant ambient term added to every fragment regardless of normal.
    pub ambient: Vec3,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: Vec3::new(0.45, -0.75, -0.55).normalize(),
            color: Vec3::new(1.0, 0.95, 0.88),
            intensity: 1.0,
            ambient: Vec3::new(0.08, 0.08, 0.10),
        }
    }
}

/// GPU-layout mirror of the data read by `lit_fragment.slang`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LightingUniforms {
    /// xyz = toward-light direction (normalised), w = intensity
    dir_direction: [f32; 4],
    /// xyz = light colour, w = 0
    dir_color: [f32; 4],
    /// xyz = ambient colour, w = 0
    ambient: [f32; 4],
    /// xyz = camera world position, w = 0
    camera_world_pos: [f32; 4],
}

impl LightingUniforms {
    fn from_light_and_camera(light: &DirectionalLight, camera_world_pos: Vec3) -> Self {
        let dir = (-light.direction).normalize();
        Self {
            dir_direction: [dir.x, dir.y, dir.z, light.intensity],
            dir_color: [light.color.x, light.color.y, light.color.z, 0.0],
            ambient: [light.ambient.x, light.ambient.y, light.ambient.z, 0.0],
            camera_world_pos: [camera_world_pos.x, camera_world_pos.y, camera_world_pos.z, 0.0],
        }
    }
}

/// Per-mesh material parameters for the built-in lit shader.
///
/// Set via [`Scene::set_material`] after calling [`Scene::add_mesh`]. The default
/// is a white, fully opaque, mid-roughness dielectric (no metallic, no emission).
///
/// # In the shader
///
/// `lit_fragment.slang` reads a `StructuredBuffer<MaterialConstants> material_desc`
/// buffer that [`Scene::draw`] binds automatically. Custom fragment shaders can
/// declare the same binding to receive these values.
#[derive(Clone, Debug)]
pub struct MaterialDescriptor {
    /// Base colour (linear RGB). Multiplied with the lighting result.
    pub albedo: Vec3,
    /// Opacity in \[0, 1\]. Values below 1.0 require an alpha-blend program.
    pub opacity: f32,
    /// Self-emission added after lighting (HDR values are valid).
    pub emissive: Vec3,
    /// Metallic factor in \[0, 1\]. 0 = dielectric, 1 = conductor.
    pub metallic: f32,
    /// Surface roughness in \[0, 1\]. 0 = mirror, 1 = fully diffuse.
    pub roughness: f32,
}

impl Default for MaterialDescriptor {
    fn default() -> Self {
        Self {
            albedo: Vec3::ONE,
            opacity: 1.0,
            emissive: Vec3::ZERO,
            metallic: 0.0,
            roughness: 0.5,
        }
    }
}

/// GPU-layout mirror of the data read by `lit_fragment.slang` as `material_desc`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialConstants {
    /// xyz = albedo, w = opacity
    albedo: [f32; 4],
    /// xyz = emissive, w = metallic
    emissive_metallic: [f32; 4],
    /// x = roughness, yzw = unused (padding)
    roughness_pad: [f32; 4],
}

impl MaterialConstants {
    fn from_descriptor(desc: &MaterialDescriptor) -> Self {
        Self {
            albedo: [desc.albedo.x, desc.albedo.y, desc.albedo.z, desc.opacity],
            emissive_metallic: [
                desc.emissive.x,
                desc.emissive.y,
                desc.emissive.z,
                desc.metallic,
            ],
            roughness_pad: [desc.roughness, 0.0, 0.0, 0.0],
        }
    }
}

/// Per-mesh material state owned by the scene.
struct MeshMaterial {
    descriptor: MaterialDescriptor,
    gpu_buffer: Option<Buffer>,
    dirty: bool,
}

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
/// ```ignore
/// // At init:
/// let mesh_id = scene.add_mesh(Mesh::cube(&engine, 1.0)?, MeshProgram::unlit(&engine)?);
/// let _obj    = scene.add_object(mesh_id, ObjectKind::Static);
///
/// // Each frame:
/// scene.prepare(&engine)?;
/// scene.draw(cam.view_matrix(), cam.projection_matrix(aspect), &hdr_out, &frame)?;
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
    /// Parallel to `meshes` — one material entry per registered mesh.
    materials: Vec<MeshMaterial>,
    objects: Vec<SceneObject>,
    cameras: Vec<SceneCamera>,
    batches: HashMap<u32, InstanceBatch>,
    next_object_id: u32,
    /// Directional light applied when drawing with [`MeshProgram::lit`].
    pub directional_light: DirectionalLight,
    /// Persistent GPU buffer holding the current [`LightingUniforms`].
    light_buffer: Option<Buffer>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            meshes: Vec::new(),
            materials: Vec::new(),
            objects: Vec::new(),
            cameras: Vec::new(),
            batches: HashMap::new(),
            next_object_id: 0,
            directional_light: DirectionalLight::default(),
            light_buffer: None,
        }
    }

    /// Register a mesh+program pair. Returns a `MeshId` used to spawn objects.
    ///
    /// A default [`MaterialDescriptor`] (white, opaque, roughness=0.5) is assigned.
    /// Override it with [`set_material`](Self::set_material).
    pub fn add_mesh(&mut self, mesh: Mesh, program: MeshProgram) -> MeshId {
        let id = MeshId(self.meshes.len() as u32);
        self.meshes.push((mesh, program));
        self.materials.push(MeshMaterial {
            descriptor: MaterialDescriptor::default(),
            gpu_buffer: None,
            dirty: true,
        });
        id
    }

    /// Set the material parameters for a registered mesh.
    ///
    /// Changes take effect on the next [`prepare`](Self::prepare) call.
    pub fn set_material(&mut self, id: MeshId, descriptor: MaterialDescriptor) {
        if let Some(mat) = self.materials.get_mut(id.0 as usize) {
            mat.descriptor = descriptor;
            mat.dirty = true;
        }
    }

    /// Return the current material descriptor for a mesh.
    pub fn material(&self, id: MeshId) -> Option<&MaterialDescriptor> {
        self.materials.get(id.0 as usize).map(|m| &m.descriptor)
    }

    /// Add an object instance at the world origin. Returns an `ObjectId` for later
    /// transform updates via [`set_transform`](Self::set_transform).
    pub fn add_object(&mut self, mesh_id: MeshId, kind: ObjectKind) -> ObjectId {
        self.add_object_at(mesh_id, Mat4::IDENTITY, kind)
    }

    /// Add an object instance with an explicit initial transform.
    pub fn add_object_at(
        &mut self,
        mesh_id: MeshId,
        transform: Mat4,
        kind: ObjectKind,
    ) -> ObjectId {
        let id = ObjectId(self.next_object_id);
        self.next_object_id += 1;
        self.objects
            .push(SceneObject::new(mesh_id, transform, kind));
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
            let batch = self
                .batches
                .entry(obj.mesh_id.0)
                .or_insert_with(|| InstanceBatch::new(obj.mesh_id.0));
            match obj.kind {
                ObjectKind::Static => {
                    if batch.static_dirty || batch.static_instances.is_empty() {
                        batch
                            .static_instances
                            .push(InstanceData::from_transform(obj.transform));
                    }
                }
                ObjectKind::Dynamic => {
                    batch
                        .dynamic_instances
                        .push(InstanceData::from_transform(obj.transform));
                }
            }
        }

        for batch in self.batches.values_mut() {
            batch.prepare(engine)?;
        }

        // Ensure the lighting uniform buffer exists.
        if self.light_buffer.is_none() {
            self.light_buffer = Some(engine.create_buffer(BufferDesc {
                size: std::mem::size_of::<LightingUniforms>() as u64,
                usage: BufferUsage::STORAGE,
            })?);
        }

        // Create or update per-mesh material buffers.
        for mat in &mut self.materials {
            if mat.gpu_buffer.is_none() {
                mat.gpu_buffer = Some(engine.create_buffer(BufferDesc {
                    size: std::mem::size_of::<MaterialConstants>() as u64,
                    usage: BufferUsage::STORAGE,
                })?);
                mat.dirty = true;
            }
            if mat.dirty {
                if let Some(buf) = &mat.gpu_buffer {
                    let constants = MaterialConstants::from_descriptor(&mat.descriptor);
                    buf.write(0, bytemuck::bytes_of(&constants))?;
                }
                mat.dirty = false;
            }
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
            self.draw_batches_for_camera(camera, &target, frame)?;
        }
        Ok(())
    }

    /// Draw the scene into `output` using the given view and projection matrices.
    ///
    /// This is the primary render path for a main camera. No camera object needs
    /// to be registered first — pass the matrices directly.
    ///
    /// A depth buffer matching `output`'s extent and sample count is allocated
    /// automatically as a frame-managed image and cleared to 1.0 before drawing.
    ///
    /// ```ignore
    /// let view = cam.view_matrix();
    /// let proj = cam.projection_matrix(width as f32 / height as f32);
    /// scene.prepare(&engine)?;
    /// scene.draw(view, proj, &hdr_output, &frame)?;
    /// ```
    pub fn draw(
        &self,
        view: Mat4,
        proj: Mat4,
        output: &GraphImage,
        frame: &RenderFrame,
    ) -> Result<()> {
        let out_desc = output.desc();

        if matches!(out_desc.format, Format::Depth32Float | Format::Depth24Stencil8) {
            return Err(Error::InvalidInput(format!(
                "Scene::draw output '{}' has depth format {:?}; use a colour-renderable format",
                output.name(),
                out_desc.format,
            )));
        }
        if !out_desc.usage.contains(ImageUsage::RENDER_TARGET) {
            return Err(Error::InvalidInput(format!(
                "Scene::draw output '{}' requires ImageUsage::RENDER_TARGET but was created with {:?}",
                output.name(),
                out_desc.usage,
            )));
        }

        let constants = CameraConstants {
            view_proj: (proj * view).to_cols_array_2d(),
            previous_view_proj: (proj * view).to_cols_array_2d(),
        };

        // Extract camera world position from the view matrix inverse.
        // view * cam_pos = [0,0,0,1], so cam_pos = view_inv * [0,0,0,1].
        let cam_world = view.inverse() * Vec4::new(0.0, 0.0, 0.0, 1.0);
        let lighting = LightingUniforms::from_light_and_camera(
            &self.directional_light,
            Vec3::new(cam_world.x, cam_world.y, cam_world.z),
        );
        if let Some(buf) = &self.light_buffer {
            buf.write(0, bytemuck::bytes_of(&lighting))?;
        }

        let ext = out_desc.extent;
        let depth = frame.image(
            "_scene_depth",
            ImageDesc {
                dimension: ImageDimension::D2,
                extent: Extent3d {
                    width: ext.width,
                    height: ext.height,
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: output.desc().samples,
                format: Format::Depth32Float,
                usage: ImageUsage::DEPTH_STENCIL,
                transient: false,
                clear_value: None,
                debug_name: None,
            },
        )?;
        self.draw_batches(&constants, output, Some(&depth), frame)
    }

    /// Draw a single registered camera into an explicit output image.
    ///
    /// Use this for offscreen cameras (portal cameras, reflection probes, etc.)
    /// where the camera was registered with `add_camera`. For the main camera,
    /// prefer `Scene::draw`.
    pub fn render_camera_to(
        &self,
        camera_id: CameraId,
        output: &GraphImage,
        frame: &RenderFrame,
    ) -> Result<()> {
        let camera = self.cameras.get(camera_id.0 as usize).ok_or_else(|| {
            crate::Error::InvalidInput(format!("camera {:?} not found in scene", camera_id))
        })?;
        let constants = CameraConstants::from_camera(camera);
        self.draw_batches(&constants, output, None, frame)
    }

    fn draw_batches(
        &self,
        constants: &CameraConstants,
        output: &GraphImage,
        depth: Option<&GraphImage>,
        frame: &RenderFrame,
    ) -> Result<()> {
        for batch in self.batches.values() {
            let buf = match &batch.gpu_buffer {
                Some(b) => b,
                None => continue,
            };
            let total = batch.total_count();
            if total == 0 {
                continue;
            }

            let mesh_idx = batch.mesh_idx as usize;
            let (mesh, program) = &self.meshes[mesh_idx];
            frame.bind_buffer("instances", buf);
            if let Some(light_buf) = &self.light_buffer {
                frame.bind_buffer("lighting", light_buf);
            }
            if let Some(mat) = self.materials.get(mesh_idx) {
                if let Some(mat_buf) = &mat.gpu_buffer {
                    frame.bind_buffer("material_desc", mat_buf);
                }
            }
            let effective_depth = if program.uses_depth { depth } else { None };
            output.draw_mesh_instanced_with_push_constants_and_depth(
                mesh,
                program,
                buf,
                total,
                constants,
                effective_depth,
            )?;
        }

        Ok(())
    }

    fn draw_batches_for_camera(
        &self,
        camera: &SceneCamera,
        output: &GraphImage,
        frame: &RenderFrame,
    ) -> Result<()> {
        let constants = CameraConstants::from_camera(camera);
        self.draw_batches(&constants, output, None, frame)
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}
