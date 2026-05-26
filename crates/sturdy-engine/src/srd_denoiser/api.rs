use crate::{Error, Format, GraphImage, GraphImageHistory, ImageDesc, RenderFrame, Result};
use glam::UVec2;
use std::{collections::HashSet, mem};

use super::clear_history;
use super::dispatch::{
    SrdConstantArena, SrdConstantRange, SrdDispatchDesc, SrdPassBuilder, SrdRadianceAccumulateConstants,
    SrdRadianceReconstructConstants, SrdRadianceReprojectConstants, SrdRadianceSurfaceMaskConstants,
    SRD_RADIANCE_SURFACE_MASK_TILE_SIZE, reference_grid_size,
};
use super::pipeline::{
    SrdPipelineDesc, SrdReferenceTemporalPipelines, SrdRadianceAccumulateResources,
    SrdRadianceSurfaceMaskResources, SrdRadianceReconstructResources, SrdRadianceReprojectResources,
};
use super::reference_temporal_executor::SrdReferenceTemporalPrograms;
use super::resources::{
    SrdDenoiserId, SrdHistoryRing, SrdPoolClass, SrdResourceSlot,
    SrdTextureDesc, push_pool_texture, clear_extent_for_srd_texture,
};
use super::settings::{
    SrdCapabilities, SrdCommonSettings, SrdDenoiserMode, SrdDenoiserSettings,
    SrdFamilySettings, SrdHistoryMode, SrdRadianceSettings,
    SrdTemporalBindings, SrdTemporalConstants,
};

/// Engine-standard temporal denoiser for sparse realtime rendering signals.
///
/// `SrdDenoiser` is the Rust-facing entry point for SRD (Sturdy Real-Time
/// Denoiser). The current implementation provides the SRD reference temporal
/// accumulation path used by the hardware path-tracing testbed.
pub struct SrdDenoiser {
    history: GraphImageHistory,
    settings: SrdDenoiserSettings,
    bindings: SrdTemporalBindings,
}

impl SrdDenoiser {
    pub fn new(max_frames: u32) -> Self {
        Self::with_settings(SrdDenoiserSettings::new(max_frames))
    }

    pub fn with_settings(settings: SrdDenoiserSettings) -> Self {
        Self {
            history: GraphImageHistory::new(),
            settings: settings.normalized(),
            bindings: SrdTemporalBindings::default(),
        }
    }

    pub fn settings(&self) -> SrdDenoiserSettings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: SrdDenoiserSettings) {
        self.settings = settings.normalized();
    }

    pub fn validate_settings(&self) -> Result<()> {
        self.settings.validate()
    }

    pub fn reset(&mut self) {
        self.history.reset();
    }

    pub fn next_frame_index(&mut self, frame: &RenderFrame, input_desc: ImageDesc) -> u32 {
        let mut history_desc = input_desc;
        history_desc.usage |= crate::ImageUsage::RENDER_TARGET;
        frame
            .next_history_frame_index(&mut self.history, history_desc)
            .min(u32::MAX as u64) as u32
    }

    pub fn accumulate(
        &mut self,
        frame: &RenderFrame,
        input: &GraphImage,
        output_name: &str,
        program: &crate::ShaderProgram,
    ) -> Result<GraphImage> {
        self.accumulate_with_programs(
            frame,
            input,
            output_name,
            SrdReferenceTemporalPrograms::new(program, None),
        )
    }

    pub fn accumulate_with_programs(
        &mut self,
        frame: &RenderFrame,
        input: &GraphImage,
        output_name: &str,
        programs: SrdReferenceTemporalPrograms<'_>,
    ) -> Result<GraphImage> {
        super::reference_temporal_executor::SrdReferenceTemporalExecutor::new(
            programs,
            self.bindings,
        )
        .execute(frame, input, output_name, &mut self.history, self.settings)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdDenoiserDesc {
    pub id: SrdDenoiserId,
    pub mode: SrdDenoiserMode,
}

impl SrdDenoiserDesc {
    pub const fn reference_temporal(id: u32) -> Self {
        Self {
            id: SrdDenoiserId::new(id),
            mode: SrdDenoiserMode::ReferenceTemporal,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SrdInstanceDesc {
    pub denoisers: Vec<SrdDenoiserDesc>,
}

impl SrdInstanceDesc {
    pub fn new(denoisers: impl Into<Vec<SrdDenoiserDesc>>) -> Self {
        Self {
            denoisers: denoisers.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        let mut seen = HashSet::with_capacity(self.denoisers.len());
        for denoiser in &self.denoisers {
            if !seen.insert(denoiser.id) {
                return Err(Error::InvalidInput(format!(
                    "SRD denoiser id {} is not unique",
                    denoiser.id.get()
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SrdInstance {
    pub(super) desc: SrdInstanceDesc,
    pub(super) common_settings: SrdCommonSettings,
    pub(super) denoiser_settings: Vec<(SrdDenoiserId, SrdFamilySettings)>,
    pub(super) capabilities: SrdCapabilities,
    pub(super) history_pool: Vec<SrdTextureDesc>,
    pub(super) scratch_pool: Vec<SrdTextureDesc>,
    pub(super) history_rings: Vec<SrdHistoryRing>,
    pub(super) pipelines: Vec<SrdPipelineDesc>,
    pub(super) dispatches: Vec<SrdDispatchDesc>,
    pub(super) constants: SrdConstantArena,
}

impl SrdInstance {
    pub fn new(desc: SrdInstanceDesc) -> Result<Self> {
        desc.validate()?;
        let denoiser_settings = desc
            .denoisers
            .iter()
            .map(|denoiser| {
                (
                    denoiser.id,
                    SrdFamilySettings::default_for_mode(denoiser.mode),
                )
            })
            .collect();
        Ok(Self {
            desc,
            common_settings: SrdCommonSettings::default(),
            denoiser_settings,
            capabilities: SrdCapabilities::default(),
            history_pool: Vec::new(),
            scratch_pool: Vec::new(),
            history_rings: Vec::new(),
            pipelines: Vec::new(),
            dispatches: Vec::new(),
            constants: SrdConstantArena::default(),
        })
    }

    pub fn desc(&self) -> &SrdInstanceDesc {
        &self.desc
    }

    pub fn common_settings(&self) -> &SrdCommonSettings {
        &self.common_settings
    }

    pub fn capabilities(&self) -> SrdCapabilities {
        self.capabilities
    }

    pub fn set_capabilities(&mut self, capabilities: SrdCapabilities) -> Result<()> {
        capabilities.validate()?;
        self.capabilities = capabilities;
        Ok(())
    }

    pub fn set_common_settings(&mut self, settings: SrdCommonSettings) -> Result<()> {
        settings.validate()?;
        self.common_settings = settings;
        Ok(())
    }

    pub fn set_denoiser_settings(
        &mut self,
        id: SrdDenoiserId,
        settings: SrdFamilySettings,
    ) -> Result<()> {
        let mode = self.mode_for_id(id).ok_or_else(|| {
            Error::InvalidInput(format!("SRD denoiser id {} does not exist", id.get()))
        })?;
        if settings.mode() != mode {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} expects {:?} settings, got {:?}",
                id.get(),
                mode,
                settings.mode()
            )));
        }
        settings.validate()?;
        let (_, existing) = self
            .denoiser_settings
            .iter_mut()
            .find(|(existing_id, _)| *existing_id == id)
            .expect("validated denoiser id disappeared");
        *existing = settings;
        Ok(())
    }

    pub fn denoiser_settings(&self, id: SrdDenoiserId) -> Option<&SrdFamilySettings> {
        self.denoiser_settings
            .iter()
            .find_map(|(existing_id, settings)| (*existing_id == id).then_some(settings))
    }

    pub fn mode_for_id(&self, id: SrdDenoiserId) -> Option<SrdDenoiserMode> {
        self.desc
            .denoisers
            .iter()
            .find_map(|denoiser| (denoiser.id == id).then_some(denoiser.mode))
    }

    pub fn add_history_texture(&mut self, texture: SrdTextureDesc) -> Result<u16> {
        texture.validate()?;
        if texture.pool != Some(SrdPoolClass::History) {
            return Err(Error::InvalidInput(format!(
                "SRD history texture '{}' must use SrdPoolClass::History",
                texture.name
            )));
        }
        push_pool_texture(&mut self.history_pool, texture)
    }

    pub fn add_scratch_texture(&mut self, texture: SrdTextureDesc) -> Result<u16> {
        self.validate_scratch_texture(&texture)?;
        if let Some(index) = self.scratch_pool.iter().position(|existing| {
            existing.slot == texture.slot && existing.downsample_factor == texture.downsample_factor
        }) {
            return Ok(index as u16);
        }
        push_pool_texture(&mut self.scratch_pool, texture)
    }

    pub fn add_unique_scratch_texture(&mut self, texture: SrdTextureDesc) -> Result<u16> {
        self.validate_scratch_texture(&texture)?;
        push_pool_texture(&mut self.scratch_pool, texture)
    }

    fn validate_scratch_texture(&self, texture: &SrdTextureDesc) -> Result<()> {
        texture.validate()?;
        if texture.pool != Some(SrdPoolClass::Scratch) {
            return Err(Error::InvalidInput(format!(
                "SRD scratch texture '{}' must use SrdPoolClass::Scratch",
                texture.name
            )));
        }
        Ok(())
    }

    pub fn history_pool(&self) -> &[SrdTextureDesc] {
        &self.history_pool
    }

    pub fn scratch_pool(&self) -> &[SrdTextureDesc] {
        &self.scratch_pool
    }

    pub fn add_history_ring(&mut self, ring: SrdHistoryRing) -> Result<()> {
        ring.validate(&self.history_pool)?;
        self.history_rings.push(ring);
        Ok(())
    }

    pub fn history_rings(&self) -> &[SrdHistoryRing] {
        &self.history_rings
    }

    pub fn rotate_history_ring(&mut self, denoiser_id: SrdDenoiserId) {
        for ring in &mut self.history_rings {
            if ring.denoiser_id == denoiser_id {
                mem::swap(&mut ring.write_index, &mut ring.read_index);
            }
        }
    }

    pub fn register_pipeline(&mut self, pipeline: SrdPipelineDesc) -> Result<usize> {
        pipeline.validate()?;
        self.pipelines.push(pipeline);
        Ok(self.pipelines.len() - 1)
    }

    pub fn register_clear_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Clear History".into(),
            debug_label: "SRD Clear History".into(),
            shader_label: "srd_clear_history".into(),
            has_constants: true,
            workgroup_size: clear_history::SRD_CLEAR_HISTORY_WORKGROUP_SIZE,
        })
    }

    pub fn register_reference_temporal_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Reference Temporal".into(),
            debug_label: "SRD Reference Temporal".into(),
            shader_label: "srd_temporal_accumulate".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    pub fn prepare_reference_temporal(
        &mut self,
        denoiser_id: SrdDenoiserId,
        format: Format,
    ) -> Result<SrdReferenceTemporalPipelines> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::ReferenceTemporal) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a ReferenceTemporal denoiser",
                denoiser_id.get()
            )));
        }
        let label = format!("reference_history_{}", denoiser_id.get());
        if self
            .history_rings
            .iter()
            .any(|ring| ring.denoiser_id == denoiser_id && ring.label == label)
        {
            return Err(Error::InvalidInput(format!(
                "SRD ReferenceTemporal denoiser id {} already has prepared history resources",
                denoiser_id.get()
            )));
        }
        let current = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_current"),
            debug_label: format!("SRD Reference History {} Current", denoiser_id.get()),
            slot: SrdResourceSlot::HistoryPool,
            format,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        let previous = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_previous"),
            debug_label: format!("SRD Reference History {} Previous", denoiser_id.get()),
            slot: SrdResourceSlot::HistoryPool,
            format,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        self.add_history_ring(SrdHistoryRing {
            denoiser_id,
            write_index: current,
            read_index: previous,
            label,
        })?;
        Ok(SrdReferenceTemporalPipelines {
            temporal: self.register_reference_temporal_pipeline()?,
            clear: self.register_clear_pipeline()?,
        })
    }

    pub fn register_radiance_surface_mask_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Radiance Surface Mask".into(),
            debug_label: "SRD Radiance Surface Mask".into(),
            shader_label: "srd_radiance_surface_mask".into(),
            has_constants: true,
            workgroup_size: [
                SRD_RADIANCE_SURFACE_MASK_TILE_SIZE,
                SRD_RADIANCE_SURFACE_MASK_TILE_SIZE,
                1,
            ],
        })
    }

    pub fn prepare_radiance_surface_mask(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceSurfaceMaskResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        let name = format!("radiance_surface_mask_{}", denoiser_id.get());
        let scratch_index = self.add_scratch_texture(SrdTextureDesc {
            name: name.clone(),
            debug_label: format!("SRD Radiance Surface Mask {}", denoiser_id.get()),
            slot: SrdResourceSlot::SurfaceMaskOutput,
            format: Format::R8Unorm,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: SRD_RADIANCE_SURFACE_MASK_TILE_SIZE,
        })?;
        let pipeline = self.register_radiance_surface_mask_pipeline()?;
        Ok(SrdRadianceSurfaceMaskResources {
            scratch_index,
            pipeline,
        })
    }

    pub fn register_radiance_reproject_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Radiance Reproject".into(),
            debug_label: "SRD Radiance Reproject".into(),
            shader_label: "srd_radiance_reproject".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    pub fn prepare_radiance_reproject(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceReprojectResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        let scratch_index = self.add_scratch_texture(SrdTextureDesc {
            name: format!("radiance_reproject_{}", denoiser_id.get()),
            debug_label: format!("SRD Radiance Reproject {}", denoiser_id.get()),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;
        let pipeline = self.register_radiance_reproject_pipeline()?;
        Ok(SrdRadianceReprojectResources {
            scratch_index,
            pipeline,
        })
    }

    pub fn plan_radiance_reproject_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceReprojectResources,
        surface_mask: Option<SrdRadianceSurfaceMaskResources>,
        rect_size: UVec2,
    ) -> Result<&[SrdDispatchDesc]> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        if resources.pipeline >= self.pipelines.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance reproject references missing pipeline index {}",
                resources.pipeline
            )));
        }
        if (resources.scratch_index as usize) >= self.scratch_pool.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance reproject references missing scratch index {}",
                resources.scratch_index
            )));
        }
        let rejection = match self.denoiser_settings(denoiser_id) {
            Some(SrdFamilySettings::Radiance(settings)) => settings.history_rejection,
            _ => super::settings::SrdHistoryRejectionSettings::default(),
        };
        let tile = SRD_RADIANCE_SURFACE_MASK_TILE_SIZE;
        let mask_extent = UVec2::new(
            rect_size.x.div_ceil(tile).max(1),
            rect_size.y.div_ceil(tile).max(1),
        );
        let motion_scale = self.common_settings.motion_vector_scale.to_array();
        let constants = SrdRadianceReprojectConstants {
            rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
            mask_extent: [mask_extent.x, mask_extent.y],
            motion_scale,
            depth_tolerance_relative: rejection.depth_threshold,
            depth_tolerance_absolute: rejection.depth_threshold.max(0.01),
            normal_cos_threshold: (1.0 - rejection.normal_threshold).clamp(0.0, 1.0),
            use_surface_mask: surface_mask.is_some() as u32,
            tile_size: tile,
            _pad: 0,
        };
        let constants_range = self.push_typed_constants(&constants);
        let mut builder =
            SrdPassBuilder::new("SRD Radiance Reproject", denoiser_id, resources.pipeline)
                .read(SrdResourceSlot::MotionVectorsInput)
                .read(SrdResourceSlot::LinearDepthInput)
                .read(SrdResourceSlot::NormalRoughnessInput)
                .read(SrdResourceSlot::PrevLinearDepthInput)
                .read(SrdResourceSlot::PrevNormalRoughnessInput);
        if let Some(mask) = surface_mask {
            if (mask.scratch_index as usize) >= self.scratch_pool.len() {
                return Err(Error::InvalidInput(format!(
                    "SRD radiance reproject references missing surface-mask scratch {}",
                    mask.scratch_index
                )));
            }
            builder = builder.read_pool(SrdPoolClass::Scratch, mask.scratch_index);
        }
        let dispatch = builder
            .write_pool(SrdPoolClass::Scratch, resources.scratch_index)
            .constants_range(constants_range)
            .grid_size([
                rect_size.x.div_ceil(8).max(1),
                rect_size.y.div_ceil(8).max(1),
                1,
            ])
            .build()?;
        self.push_dispatch(dispatch)?;
        Ok(self.dispatches())
    }

    pub fn register_radiance_accumulate_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Radiance Accumulate".into(),
            debug_label: "SRD Radiance Accumulate".into(),
            shader_label: "srd_radiance_accumulate".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    pub fn prepare_radiance_accumulate(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceAccumulateResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        let label = format!("radiance_history_{}", denoiser_id.get());
        if self
            .history_rings
            .iter()
            .any(|ring| ring.denoiser_id == denoiser_id && ring.label == label)
        {
            return Err(Error::InvalidInput(format!(
                "SRD radiance history ring already prepared for denoiser id {}",
                denoiser_id.get()
            )));
        }
        let write_index = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_write"),
            debug_label: format!("SRD Radiance History {} Write", denoiser_id.get()),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        let read_index = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_read"),
            debug_label: format!("SRD Radiance History {} Read", denoiser_id.get()),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        self.add_history_ring(SrdHistoryRing {
            denoiser_id,
            write_index,
            read_index,
            label,
        })?;
        let history_ring_index = self.history_rings.len() - 1;
        let pipeline = self.register_radiance_accumulate_pipeline()?;
        Ok(SrdRadianceAccumulateResources {
            history_ring_index,
            pipeline,
        })
    }

    pub fn rotate_history_ring_at(&mut self, index: usize) -> Result<()> {
        if index >= self.history_rings.len() {
            return Err(Error::InvalidInput(format!(
                "SRD history ring index {index} out of bounds (have {})",
                self.history_rings.len()
            )));
        }
        let ring = &mut self.history_rings[index];
        mem::swap(&mut ring.write_index, &mut ring.read_index);
        Ok(())
    }

    pub fn plan_radiance_accumulate_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceAccumulateResources,
        reproject: SrdRadianceReprojectResources,
        surface_mask: Option<SrdRadianceSurfaceMaskResources>,
        rect_size: UVec2,
    ) -> Result<&[SrdDispatchDesc]> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        if resources.pipeline >= self.pipelines.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance accumulate references missing pipeline index {}",
                resources.pipeline
            )));
        }
        if resources.history_ring_index >= self.history_rings.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance accumulate references missing history ring {}",
                resources.history_ring_index
            )));
        }
        if (reproject.scratch_index as usize) >= self.scratch_pool.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance accumulate references missing reproject scratch {}",
                reproject.scratch_index
            )));
        }
        let ring = self.history_rings[resources.history_ring_index].clone();
        if ring.denoiser_id != denoiser_id {
            return Err(Error::InvalidInput(format!(
                "SRD radiance accumulate ring belongs to denoiser id {} not {}",
                ring.denoiser_id.get(),
                denoiser_id.get()
            )));
        }
        let radiance_settings = match self.denoiser_settings(denoiser_id) {
            Some(SrdFamilySettings::Radiance(settings)) => *settings,
            _ => SrdRadianceSettings::default(),
        };
        let tile = SRD_RADIANCE_SURFACE_MASK_TILE_SIZE;
        let mask_extent = UVec2::new(
            rect_size.x.div_ceil(tile).max(1),
            rect_size.y.div_ceil(tile).max(1),
        );
        let constants = SrdRadianceAccumulateConstants {
            rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
            mask_extent: [mask_extent.x, mask_extent.y],
            fast_history_budget: radiance_settings.fast_history_budget.max(1) as f32,
            history_frame_budget: radiance_settings.history_frame_budget.max(1) as f32,
            use_surface_mask: surface_mask.is_some() as u32,
            tile_size: tile,
        };
        let constants_range = self.push_typed_constants(&constants);
        let mut builder =
            SrdPassBuilder::new("SRD Radiance Accumulate", denoiser_id, resources.pipeline)
                .read(SrdResourceSlot::CombinedRadianceInput)
                .read_pool(SrdPoolClass::Scratch, reproject.scratch_index);
        if let Some(mask) = surface_mask {
            if (mask.scratch_index as usize) >= self.scratch_pool.len() {
                return Err(Error::InvalidInput(format!(
                    "SRD radiance accumulate references missing surface-mask scratch {}",
                    mask.scratch_index
                )));
            }
            builder = builder.read_pool(SrdPoolClass::Scratch, mask.scratch_index);
        }
        let dispatch = builder
            .read_pool(SrdPoolClass::History, ring.read_index)
            .write_pool(SrdPoolClass::History, ring.write_index)
            .constants_range(constants_range)
            .grid_size([
                rect_size.x.div_ceil(8).max(1),
                rect_size.y.div_ceil(8).max(1),
                1,
            ])
            .build()?;
        self.push_dispatch(dispatch)?;
        Ok(self.dispatches())
    }

    pub fn register_radiance_reconstruct_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Radiance Reconstruct".into(),
            debug_label: "SRD Radiance Reconstruct".into(),
            shader_label: "srd_radiance_reconstruct".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    pub fn prepare_radiance_reconstruct(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdRadianceReconstructResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        let scratch_index = self.add_scratch_texture(SrdTextureDesc {
            name: format!("radiance_reconstruct_{}", denoiser_id.get()),
            debug_label: format!("SRD Radiance Reconstruct {}", denoiser_id.get()),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;
        let pipeline = self.register_radiance_reconstruct_pipeline()?;
        Ok(SrdRadianceReconstructResources {
            scratch_index,
            pipeline,
        })
    }

    pub fn plan_radiance_reconstruct_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceReconstructResources,
        accumulate: SrdRadianceAccumulateResources,
        surface_mask: Option<SrdRadianceSurfaceMaskResources>,
        rect_size: UVec2,
    ) -> Result<&[SrdDispatchDesc]> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        if resources.pipeline >= self.pipelines.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance reconstruct references missing pipeline index {}",
                resources.pipeline
            )));
        }
        if (resources.scratch_index as usize) >= self.scratch_pool.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance reconstruct references missing scratch index {}",
                resources.scratch_index
            )));
        }
        if accumulate.history_ring_index >= self.history_rings.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance reconstruct references missing history ring {}",
                accumulate.history_ring_index
            )));
        }
        let ring = self.history_rings[accumulate.history_ring_index].clone();
        if ring.denoiser_id != denoiser_id {
            return Err(Error::InvalidInput(format!(
                "SRD radiance reconstruct ring belongs to denoiser id {} not {}",
                ring.denoiser_id.get(),
                denoiser_id.get()
            )));
        }
        let radiance_settings = match self.denoiser_settings(denoiser_id) {
            Some(SrdFamilySettings::Radiance(settings)) => *settings,
            _ => SrdRadianceSettings::default(),
        };
        let history_rejection = radiance_settings.history_rejection;
        let tile = SRD_RADIANCE_SURFACE_MASK_TILE_SIZE;
        let mask_extent = UVec2::new(
            rect_size.x.div_ceil(tile).max(1),
            rect_size.y.div_ceil(tile).max(1),
        );
        const SHORT_HISTORY_CUTOFF: f32 = 4.0;
        const NORMAL_SHARPNESS: f32 = 16.0;
        let constants = SrdRadianceReconstructConstants {
            rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
            mask_extent: [mask_extent.x, mask_extent.y],
            short_history_cutoff: SHORT_HISTORY_CUTOFF,
            depth_tolerance_relative: history_rejection.depth_threshold,
            depth_tolerance_absolute: history_rejection.depth_threshold.max(0.01),
            normal_sharpness: NORMAL_SHARPNESS,
            roughness_tolerance: history_rejection.roughness_threshold,
            use_surface_mask: surface_mask.is_some() as u32,
            tile_size: tile,
            _pad: 0,
        };
        let constants_range = self.push_typed_constants(&constants);
        let mut builder =
            SrdPassBuilder::new("SRD Radiance Reconstruct", denoiser_id, resources.pipeline)
                .read_pool(SrdPoolClass::History, ring.write_index)
                .read(SrdResourceSlot::LinearDepthInput)
                .read(SrdResourceSlot::NormalRoughnessInput);
        if let Some(mask) = surface_mask {
            if (mask.scratch_index as usize) >= self.scratch_pool.len() {
                return Err(Error::InvalidInput(format!(
                    "SRD radiance reconstruct references missing surface-mask scratch {}",
                    mask.scratch_index
                )));
            }
            builder = builder.read_pool(SrdPoolClass::Scratch, mask.scratch_index);
        }
        let dispatch = builder
            .write_pool(SrdPoolClass::Scratch, resources.scratch_index)
            .constants_range(constants_range)
            .grid_size([
                rect_size.x.div_ceil(8).max(1),
                rect_size.y.div_ceil(8).max(1),
                1,
            ])
            .build()?;
        self.push_dispatch(dispatch)?;
        Ok(self.dispatches())
    }

    pub fn plan_radiance_surface_mask_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdRadianceSurfaceMaskResources,
        rect_size: UVec2,
    ) -> Result<&[SrdDispatchDesc]> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::RadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a RadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }
        if resources.pipeline >= self.pipelines.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance surface mask references missing pipeline index {}",
                resources.pipeline
            )));
        }
        if (resources.scratch_index as usize) >= self.scratch_pool.len() {
            return Err(Error::InvalidInput(format!(
                "SRD radiance surface mask references missing scratch index {}",
                resources.scratch_index
            )));
        }
        let tile = SRD_RADIANCE_SURFACE_MASK_TILE_SIZE;
        let mask_extent = UVec2::new(
            rect_size.x.div_ceil(tile).max(1),
            rect_size.y.div_ceil(tile).max(1),
        );
        let constants = SrdRadianceSurfaceMaskConstants {
            input_extent: [rect_size.x.max(1), rect_size.y.max(1)],
            mask_extent: [mask_extent.x, mask_extent.y],
            effective_range: self.common_settings.effective_range,
            tile_size: tile,
            _pad: [0, 0],
        };
        let constants_range = self.push_typed_constants(&constants);
        let dispatch =
            SrdPassBuilder::new("SRD Radiance Surface Mask", denoiser_id, resources.pipeline)
                .read(SrdResourceSlot::LinearDepthInput)
                .write_pool(SrdPoolClass::Scratch, resources.scratch_index)
                .constants_range(constants_range)
                .grid_size([mask_extent.x, mask_extent.y, 1])
                .build()?;
        self.push_dispatch(dispatch)?;
        Ok(self.dispatches())
    }

    pub fn pipelines(&self) -> &[SrdPipelineDesc] {
        &self.pipelines
    }

    pub fn dispatches(&self) -> &[SrdDispatchDesc] {
        &self.dispatches
    }

    pub fn constant_bytes(&self, range: SrdConstantRange) -> &[u8] {
        self.constants.get(range)
    }

    pub fn clear_dispatches(&mut self) {
        self.dispatches.clear();
        self.constants.clear();
    }

    pub fn push_dispatch(&mut self, mut dispatch: SrdDispatchDesc) -> Result<()> {
        dispatch.validate()?;
        if dispatch.pipeline_index >= self.pipelines.len() {
            return Err(Error::InvalidInput(format!(
                "SRD dispatch '{}' references missing pipeline index {}",
                dispatch.name, dispatch.pipeline_index
            )));
        }
        if self.mode_for_id(dispatch.denoiser_id).is_none() {
            return Err(Error::InvalidInput(format!(
                "SRD dispatch '{}' references missing denoiser id {}",
                dispatch.name,
                dispatch.denoiser_id.get()
            )));
        }
        self.mark_constant_reuse(&mut dispatch);
        self.dispatches.push(dispatch);
        Ok(())
    }

    pub fn push_constants(&mut self, bytes: &[u8]) -> SrdConstantRange {
        self.constants.push(bytes)
    }

    pub fn push_typed_constants<T: bytemuck::Pod>(&mut self, constants: &T) -> SrdConstantRange {
        self.push_constants(bytemuck::bytes_of(constants))
    }

    pub fn plan_reference_temporal_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        pipeline_index: usize,
        constants: SrdTemporalConstants,
    ) -> Result<&[SrdDispatchDesc]> {
        self.plan_reference_temporal_passes_with_pipelines(
            denoiser_id,
            SrdReferenceTemporalPipelines {
                temporal: pipeline_index,
                clear: pipeline_index,
            },
            constants,
        )
    }

    pub fn plan_reference_temporal_passes_with_pipelines(
        &mut self,
        denoiser_id: SrdDenoiserId,
        pipelines: SrdReferenceTemporalPipelines,
        constants: SrdTemporalConstants,
    ) -> Result<&[SrdDispatchDesc]> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::ReferenceTemporal) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a ReferenceTemporal denoiser",
                denoiser_id.get()
            )));
        }
        self.clear_dispatches();
        if self.common_settings.history_mode == SrdHistoryMode::ZeroHistory {
            self.push_clear_dispatches_for(denoiser_id, pipelines.clear)?;
        }
        self.rotate_history_ring(denoiser_id);
        let history = self
            .history_rings
            .iter()
            .find(|ring| ring.denoiser_id == denoiser_id)
            .cloned()
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "SRD ReferenceTemporal denoiser id {} has no history ring",
                    denoiser_id.get()
                ))
            })?;
        let constants_range = self.push_typed_constants(&constants);
        let dispatch =
            SrdPassBuilder::new("SRD Reference Temporal", denoiser_id, pipelines.temporal)
                .read(SrdResourceSlot::CombinedRadianceInput)
                .read_pool(SrdPoolClass::History, history.read_index)
                .write(SrdResourceSlot::CombinedRadianceOutput)
                .write_pool(SrdPoolClass::History, history.write_index)
                .constants_range(constants_range)
                .grid_size(reference_grid_size(self.common_settings.rect_size))
                .build()?;
        self.push_dispatch(dispatch)?;
        Ok(self.dispatches())
    }

    fn mark_constant_reuse(&self, dispatch: &mut SrdDispatchDesc) {
        let Some(current) = dispatch.constants_range else {
            return;
        };
        let Some(previous) = self
            .dispatches
            .last()
            .and_then(|dispatch| dispatch.constants_range)
        else {
            return;
        };
        dispatch.reuses_previous_constants =
            self.constants.get(current) == self.constants.get(previous);
    }

    pub fn push_clear_dispatches(&mut self, pipeline_index: usize) -> Result<usize> {
        self.validate_clear_pipeline_index(pipeline_index)?;
        let rings = self.history_rings.clone();
        self.push_clear_dispatches_for_rings(pipeline_index, rings)
    }

    pub fn push_clear_dispatches_for(
        &mut self,
        denoiser_id: SrdDenoiserId,
        pipeline_index: usize,
    ) -> Result<usize> {
        self.validate_clear_pipeline_index(pipeline_index)?;
        let rings = self
            .history_rings
            .iter()
            .filter(|ring| ring.denoiser_id == denoiser_id)
            .cloned()
            .collect();
        self.push_clear_dispatches_for_rings(pipeline_index, rings)
    }

    fn validate_clear_pipeline_index(&self, pipeline_index: usize) -> Result<()> {
        if pipeline_index >= self.pipelines.len() {
            return Err(Error::InvalidInput(format!(
                "SRD clear dispatches reference missing pipeline index {pipeline_index}"
            )));
        }
        Ok(())
    }

    fn push_clear_dispatches_for_rings(
        &mut self,
        pipeline_index: usize,
        rings: Vec<SrdHistoryRing>,
    ) -> Result<usize> {
        let mut pushed = 0;
        for ring in rings {
            let texture = self
                .history_pool
                .get(ring.write_index as usize)
                .ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "SRD clear dispatch references missing history texture {}",
                        ring.write_index
                    ))
                })?;
            let extent = clear_extent_for_srd_texture(&self.common_settings, texture);
            let constants = clear_history::SrdClearConstants {
                extent: [extent.x, extent.y],
                _pad: [0, 0],
            };
            let constants_range = self.push_typed_constants(&constants);
            let dispatch = SrdPassBuilder::new(
                format!("SRD Clear {}", ring.label),
                ring.denoiser_id,
                pipeline_index,
            )
            .write_pool(SrdPoolClass::History, ring.write_index)
            .constants_range(constants_range)
            .grid_size(clear_history::clear_grid_size(extent))
            .build()?;
            self.push_dispatch(dispatch)?;
            pushed += 1;
        }
        Ok(pushed)
    }
}
