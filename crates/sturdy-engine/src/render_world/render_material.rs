use super::MaterialId;

/// ECS component selecting the material-table entry rendered by an entity.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RenderMaterial {
    pub material: MaterialId,
}

impl RenderMaterial {
    pub fn new(material: MaterialId) -> Self {
        Self { material }
    }
}
