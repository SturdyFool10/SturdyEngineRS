use crate::MeshId;

use super::LodGroupId;

/// ECS component selecting the mesh (or LOD group) rendered by an entity.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderMesh {
    pub mesh: MeshId,
    pub lod_group: Option<LodGroupId>,
}

impl RenderMesh {
    pub fn new(mesh: MeshId) -> Self {
        Self {
            mesh,
            lod_group: None,
        }
    }

    pub fn with_lod_group(mut self, lod_group: LodGroupId) -> Self {
        self.lod_group = Some(lod_group);
        self
    }

    pub fn without_lod_group(mut self) -> Self {
        self.lod_group = None;
        self
    }
}
