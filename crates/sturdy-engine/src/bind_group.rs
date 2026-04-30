use crate::{
    BindGroup, BindGroupDesc, BindGroupEntry, BindingKind, Buffer, Engine, Error, Image,
    PipelineLayout, ResourceBinding, Result, Sampler,
};

pub struct BindGroupBuilder<'a> {
    engine: &'a Engine,
    layout: &'a PipelineLayout,
    entries: Vec<BindGroupEntry>,
}

impl<'a> BindGroupBuilder<'a> {
    pub(crate) fn new(engine: &'a Engine, layout: &'a PipelineLayout) -> Self {
        Self {
            engine,
            layout,
            entries: Vec::new(),
        }
    }

    pub fn image(mut self, path: impl Into<String>, image: &Image) -> Self {
        self.entries.push(BindGroupEntry {
            path: path.into(),
            resource: ResourceBinding::Image(image.handle()),
        });
        self
    }

    pub fn image_binding(self, set: usize, binding: u32, image: &Image) -> Result<Self> {
        self.entry_binding(set, binding, ResourceBinding::Image(image.handle()))
    }

    pub fn image_auto(self, image: &Image) -> Result<Self> {
        self.entry_auto(ResourceBinding::Image(image.handle()))
    }

    pub fn buffer(mut self, path: impl Into<String>, buffer: &Buffer) -> Self {
        self.entries.push(BindGroupEntry {
            path: path.into(),
            resource: ResourceBinding::Buffer(buffer.handle()),
        });
        self
    }

    pub fn buffer_binding(self, set: usize, binding: u32, buffer: &Buffer) -> Result<Self> {
        self.entry_binding(set, binding, ResourceBinding::Buffer(buffer.handle()))
    }

    pub fn buffer_auto(self, buffer: &Buffer) -> Result<Self> {
        self.entry_auto(ResourceBinding::Buffer(buffer.handle()))
    }

    pub fn sampler(mut self, path: impl Into<String>, sampler: &Sampler) -> Self {
        self.entries.push(BindGroupEntry {
            path: path.into(),
            resource: ResourceBinding::Sampler(sampler.handle()),
        });
        self
    }

    pub fn sampler_binding(self, set: usize, binding: u32, sampler: &Sampler) -> Result<Self> {
        self.entry_binding(set, binding, ResourceBinding::Sampler(sampler.handle()))
    }

    pub fn sampler_auto(self, sampler: &Sampler) -> Result<Self> {
        self.entry_auto(ResourceBinding::Sampler(sampler.handle()))
    }

    pub fn entry(mut self, path: impl Into<String>, resource: ResourceBinding) -> Self {
        self.entries.push(BindGroupEntry {
            path: path.into(),
            resource,
        });
        self
    }

    pub fn entry_binding(
        mut self,
        set: usize,
        binding: u32,
        resource: ResourceBinding,
    ) -> Result<Self> {
        let path = self.path_for_binding(set, binding, resource)?;
        self.entries.push(BindGroupEntry { path, resource });
        Ok(self)
    }

    pub fn entry_auto(mut self, resource: ResourceBinding) -> Result<Self> {
        let path = self.unambiguous_path_for_resource(resource)?;
        self.entries.push(BindGroupEntry { path, resource });
        Ok(self)
    }

    pub fn build(self) -> Result<BindGroup> {
        self.engine.create_bind_group(BindGroupDesc {
            layout: self.layout.handle(),
            entries: self.entries,
        })
    }

    fn path_for_binding(
        &self,
        set: usize,
        binding_slot: u32,
        resource: ResourceBinding,
    ) -> Result<String> {
        let group = self.layout.layout().groups.get(set).ok_or_else(|| {
            Error::InvalidInput(format!("pipeline layout has no descriptor set {set}"))
        })?;
        let binding = group
            .bindings
            .iter()
            .find(|binding| binding.binding == binding_slot)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "pipeline layout set {set} has no binding slot {binding_slot}"
                ))
            })?;
        if !resource_matches_binding(resource, binding.kind) {
            return Err(Error::InvalidInput(format!(
                "pipeline layout binding '{}', set {set}, slot {binding_slot} expects {:?}, got {}",
                binding.path,
                binding.kind,
                resource_label(resource)
            )));
        }
        Ok(binding.path.clone())
    }

    fn unambiguous_path_for_resource(&self, resource: ResourceBinding) -> Result<String> {
        let matches = self
            .layout
            .layout()
            .groups
            .iter()
            .flat_map(|group| group.bindings.iter())
            .filter(|binding| {
                resource_matches_binding(resource, binding.kind)
                    && !self.entries.iter().any(|entry| entry.path == binding.path)
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [binding] => Ok(binding.path.clone()),
            [] => Err(Error::InvalidInput(format!(
                "pipeline layout has no unbound binding compatible with {}",
                resource_label(resource)
            ))),
            _ => Err(Error::InvalidInput(format!(
                "pipeline layout has {} unbound bindings compatible with {}; use *_binding(set, binding, ...) or bind by path",
                matches.len(),
                resource_label(resource)
            ))),
        }
    }
}

fn resource_matches_binding(resource: ResourceBinding, kind: BindingKind) -> bool {
    matches!(
        (resource, kind),
        (
            ResourceBinding::Image(_) | ResourceBinding::ImageView { .. },
            BindingKind::SampledImage | BindingKind::StorageImage
        ) | (
            ResourceBinding::Buffer(_),
            BindingKind::UniformBuffer | BindingKind::StorageBuffer
        ) | (ResourceBinding::Sampler(_), BindingKind::Sampler)
    )
}

fn resource_label(resource: ResourceBinding) -> &'static str {
    match resource {
        ResourceBinding::Image(_) => "image",
        ResourceBinding::ImageView { .. } => "image view",
        ResourceBinding::Buffer(_) => "buffer",
        ResourceBinding::Sampler(_) => "sampler",
    }
}

impl Engine {
    pub fn bind_group<'a>(&'a self, layout: &'a PipelineLayout) -> BindGroupBuilder<'a> {
        BindGroupBuilder::new(self, layout)
    }
}
