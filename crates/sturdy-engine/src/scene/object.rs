/// Object, material, and instance data types for the scene system.
///
/// This module provides the identifiers, object kinds, scene objects, and
/// instance data for rendering. Materials are rendering-mode-agnostic: they work
/// across rasterized, hybrid, raytraced, and path traced rendering.
///
/// # Workflow
///
/// ```rust
/// // At init:
/// let scene = Scene::new();
/// let mat = Material::new("pbr_material").build(engine)?;
///
/// // At render:
/// scene.add_material_mesh(mesh_id, mat);
/// scene.render(frame)?;
/// ```
///
/// # Rendering Mode Support
///
/// All scene components are designed to work across all rendering modes without
/// breaking down:
/// - **Rasterized** — Traditional raster pipeline
/// - **Hybrid** — Raster + raytraced elements
/// - **Raytraced** — Primary raytraced pipeline
/// - **Path Traced** — Offline rendering, full path tracing
///
/// The material system ensures that:
/// - Material definitions are rendering-mode-agnostic
/// - Material parameters translate across all modes
/// - Material shaders compile across all IR targets
/// - Material caching works across modes
/// - Material graph composition supports mode-specific nodes

// ------------------------------------------------------------------
// Object Identifiers
// ------------------------------------------------------------------

/// An object identifier for scene objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectId {
    /// The unique ID for this object.
    pub id: u32,
}

impl ObjectId {
    /// Create a new object ID.
    pub fn new(id: u32) -> Self {
        Self { id }
    }
}

/// An object kind for scene objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    /// A mesh object.
    Mesh,
    /// A camera object.
    Camera,
    /// A render target object.
    RenderTarget,
    /// A text object.
    Text,
}

impl ObjectKind {
    /// Get the rendering mode for this object kind.
    pub fn rendering_mode(&self) -> SceneRenderingMode {
        match self {
            ObjectKind::Mesh => SceneRenderingMode::Rasterized,
            ObjectKind::Camera => SceneRenderingMode::Rasterized,
            ObjectKind::RenderTarget => SceneRenderingMode::Rasterized,
            ObjectKind::Text => SceneRenderingMode::Rasterized,
        }
    }
}

// ------------------------------------------------------------------
// Scene Objects
// ------------------------------------------------------------------

/// A scene object that is rendered with a material.
#[derive(Clone, Debug)]
pub struct SceneObject {
    /// The object ID.
    pub object_id: ObjectId,
    /// The object kind.
    pub object_kind: ObjectKind,
    /// The material for this object.
    pub material: Material,
    /// The mesh ID (for mesh objects).
    pub mesh_id: Option<MeshId>,
    /// The position (for mesh objects).
    pub position: Option<Vec3>,
    /// The scale (for mesh objects).
    pub scale: Option<Vec3>,
    /// The rotation (for mesh objects).
    pub rotation: Option<Vec3>,
}

impl Default for ObjectKind {
    fn default() -> Self {
        ObjectKind::Mesh
    }
}

/// A scene object that represents a mesh, camera, or other entity.
#[derive(Clone, Debug)]
pub struct SceneObject {
    /// The object's kind (mesh, camera, etc).
    pub kind: ObjectKind,
    /// The object's ID.
    pub id: ObjectId,
    /// The object's material.
    pub material: Material,
    /// The object's instance data.
    pub instance_data: Option<InstanceData>,
}

impl SceneObject {
    /// Create a scene object from the given kind and ID.
    pub fn new(kind: ObjectKind, id: ObjectId) -> Self {
        Self {
            kind,
            id,
            material: Material::default(),
            instance_data: None,
        }
    }

    /// Get the object's kind.
    pub fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// Get the object's ID.
    pub fn id(&self) -> ObjectId {
        self.id
    }

    /// Get the object's material.
    pub fn material(&self) -> &Material {
        &self.material
    }

    /// Get the object's instance data.
    pub fn instance_data(&self) -> &Option<InstanceData> {
        &self.instance_data
    }
}

/// Object, material, and instance data types for the scene system.
///
/// This module provides the identifiers, object kinds, scene objects, and
/// instance data for the scene system.
