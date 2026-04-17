use crate::{BindGroup, BindGroupDesc, BindGroupEntry, Buffer, Engine, Image, PipelineLayout};
use crate::{ResourceBinding, Result, Sampler};

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

    pub fn buffer(mut self, path: impl Into<String>, buffer: &Buffer) -> Self {
        self.entries.push(BindGroupEntry {
            path: path.into(),
            resource: ResourceBinding::Buffer(buffer.handle()),
        });
        self
    }

    pub fn sampler(mut self, path: impl Into<String>, sampler: &Sampler) -> Self {
        self.entries.push(BindGroupEntry {
            path: path.into(),
            resource: ResourceBinding::Sampler(sampler.handle()),
        });
        self
    }

    pub fn entry(mut self, path: impl Into<String>, resource: ResourceBinding) -> Self {
        self.entries.push(BindGroupEntry {
            path: path.into(),
            resource,
        });
        self
    }

    pub fn build(self) -> Result<BindGroup> {
        self.engine.create_bind_group(BindGroupDesc {
            layout: self.layout.handle(),
            entries: self.entries,
        })
    }
}

impl Engine {
    pub fn bind_group<'a>(&'a self, layout: &'a PipelineLayout) -> BindGroupBuilder<'a> {
        BindGroupBuilder::new(self, layout)
    }
}
