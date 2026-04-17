use crate::{BufferHandle, ImageHandle, PipelineLayoutHandle, SamplerHandle};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BindingKind {
    SampledImage,
    StorageImage,
    UniformBuffer,
    StorageBuffer,
    Sampler,
    AccelerationStructure,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct StageMask(pub u32);

impl StageMask {
    pub const VERTEX: Self = Self(1 << 0);
    pub const FRAGMENT: Self = Self(1 << 1);
    pub const COMPUTE: Self = Self(1 << 2);
    pub const MESH: Self = Self(1 << 3);
    pub const TASK: Self = Self(1 << 4);
    pub const RAY_TRACING: Self = Self(1 << 5);
    pub const ALL: Self = Self(u32::MAX);
}

impl std::ops::BitOr for StageMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for StageMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UpdateRate {
    Frame,
    Pass,
    Material,
    Draw,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalBinding {
    pub path: String,
    pub kind: BindingKind,
    pub count: u32,
    pub stage_mask: StageMask,
    pub update_rate: UpdateRate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalGroupLayout {
    pub name: String,
    pub bindings: Vec<CanonicalBinding>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CanonicalPipelineLayout {
    pub groups: Vec<CanonicalGroupLayout>,
    pub push_constants_bytes: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ResourceBinding {
    Image(ImageHandle),
    Buffer(BufferHandle),
    Sampler(SamplerHandle),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindGroupEntry {
    pub path: String,
    pub resource: ResourceBinding,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BindGroupDesc {
    pub layout: PipelineLayoutHandle,
    pub entries: Vec<BindGroupEntry>,
}
