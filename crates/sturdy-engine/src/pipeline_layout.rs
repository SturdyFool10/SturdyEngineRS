use crate::{
    BindingKind, CanonicalBinding, CanonicalGroupLayout, CanonicalPipelineLayout, Engine,
    PipelineLayout, Result, StageMask, UpdateRate,
};

#[derive(Clone, Debug, Default)]
pub struct PipelineLayoutBuilder {
    layout: CanonicalPipelineLayout,
}

impl PipelineLayoutBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_constants_bytes(mut self, bytes: u32) -> Self {
        self.layout.push_constants_bytes = bytes;
        self
    }

    pub fn sampled_image(
        self,
        group: impl Into<String>,
        path: impl Into<String>,
        stage_mask: StageMask,
        update_rate: UpdateRate,
    ) -> Self {
        self.binding(
            group,
            path,
            BindingKind::SampledImage,
            stage_mask,
            update_rate,
        )
    }

    pub fn storage_image(
        self,
        group: impl Into<String>,
        path: impl Into<String>,
        stage_mask: StageMask,
        update_rate: UpdateRate,
    ) -> Self {
        self.binding(
            group,
            path,
            BindingKind::StorageImage,
            stage_mask,
            update_rate,
        )
    }

    pub fn uniform_buffer(
        self,
        group: impl Into<String>,
        path: impl Into<String>,
        stage_mask: StageMask,
        update_rate: UpdateRate,
    ) -> Self {
        self.binding(
            group,
            path,
            BindingKind::UniformBuffer,
            stage_mask,
            update_rate,
        )
    }

    pub fn storage_buffer(
        self,
        group: impl Into<String>,
        path: impl Into<String>,
        stage_mask: StageMask,
        update_rate: UpdateRate,
    ) -> Self {
        self.binding(
            group,
            path,
            BindingKind::StorageBuffer,
            stage_mask,
            update_rate,
        )
    }

    pub fn sampler(
        self,
        group: impl Into<String>,
        path: impl Into<String>,
        stage_mask: StageMask,
        update_rate: UpdateRate,
    ) -> Self {
        self.binding(group, path, BindingKind::Sampler, stage_mask, update_rate)
    }

    pub fn binding(
        mut self,
        group: impl Into<String>,
        path: impl Into<String>,
        kind: BindingKind,
        stage_mask: StageMask,
        update_rate: UpdateRate,
    ) -> Self {
        let group = group.into();
        let binding = CanonicalBinding {
            path: path.into(),
            kind,
            count: 1,
            stage_mask,
            update_rate,
        };

        if let Some(existing_group) = self
            .layout
            .groups
            .iter_mut()
            .find(|existing_group| existing_group.name == group)
        {
            existing_group.bindings.push(binding);
        } else {
            self.layout.groups.push(CanonicalGroupLayout {
                name: group,
                bindings: vec![binding],
            });
        }

        self
    }

    pub fn raw_layout(&self) -> &CanonicalPipelineLayout {
        &self.layout
    }

    pub fn into_raw_layout(self) -> CanonicalPipelineLayout {
        self.layout
    }

    pub fn build(self, engine: &Engine) -> Result<PipelineLayout> {
        engine.create_pipeline_layout(self.layout)
    }
}

impl Engine {
    pub fn pipeline_layout(&self) -> PipelineLayoutBuilder {
        PipelineLayoutBuilder::new()
    }
}
