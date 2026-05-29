use crate::{ComputeProgram, Engine, Error, Format, GraphImage, Result};
use glam::UVec2;
use std::path::PathBuf;
use super::*;
use super::radiance_stabilizer_executor::{
    build_pool_images, single_storage_resource,
    srd_sampled_binding_for_resource, srd_storage_binding_for_resource,
};

/// Summary returned by `SrdInstance::plan_spectral_radiance_passes`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdSpectralRadiancePlan {
    pub final_output_scratch_index: u16,
    pub dispatch_count: usize,
}

/// External graph images consumed by the Spectral Radiance Stabilizer dispatches.
pub struct SrdSpectralRadianceStabilizerInputs<'a> {
    pub spectral_radiance: &'a GraphImage,
    pub motion_vectors: &'a GraphImage,
    pub linear_depth: &'a GraphImage,
    pub normal_roughness: &'a GraphImage,
    pub prev_linear_depth: Option<&'a GraphImage>,
    pub prev_normal_roughness: Option<&'a GraphImage>,
}

/// Compute shader programs used by the Spectral Radiance Stabilizer executor.
pub struct SrdSpectralRadianceStabilizerPrograms {
    pub clear: ComputeProgram,
    pub surface_mask: ComputeProgram,
    pub reproject: ComputeProgram,
    pub accumulate: ComputeProgram,
    pub filter: ComputeProgram,
}

impl SrdSpectralRadianceStabilizerPrograms {
    fn srd_engine_shader(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("shaders")
            .join(name)
    }

    pub fn load(engine: &Engine) -> Result<Self> {
        Ok(Self {
            clear: ComputeProgram::load(engine, Self::srd_engine_shader("srd_clear_history.slang"))?,
            surface_mask: ComputeProgram::load(
                engine,
                Self::srd_engine_shader("srd_radiance_surface_mask.slang"),
            )?,
            reproject: ComputeProgram::load(
                engine,
                Self::srd_engine_shader("srd_radiance_reproject.slang"),
            )?,
            accumulate: ComputeProgram::load(
                engine,
                Self::srd_engine_shader("srd_spectral_accumulate.slang"),
            )?,
            filter: ComputeProgram::load(
                engine,
                Self::srd_engine_shader("srd_spectral_filter.slang"),
            )?,
        })
    }

    pub fn program_for_shader_label(&self, shader_label: &str) -> Option<&ComputeProgram> {
        match shader_label {
            "srd_clear_history" => Some(&self.clear),
            "srd_radiance_surface_mask" => Some(&self.surface_mask),
            "srd_radiance_reproject" => Some(&self.reproject),
            "srd_spectral_accumulate" => Some(&self.accumulate),
            "srd_spectral_filter" => Some(&self.filter),
            _ => None,
        }
    }
}

/// Renderer-side executor for SRD Spectral Radiance Stabilizer compute dispatches.
pub struct SrdSpectralRadianceStabilizerExecutor<'a> {
    programs: &'a SrdSpectralRadianceStabilizerPrograms,
}

impl<'a> SrdSpectralRadianceStabilizerExecutor<'a> {
    pub const fn new(programs: &'a SrdSpectralRadianceStabilizerPrograms) -> Self {
        Self { programs }
    }

    pub fn execute_plan(
        &self,
        frame: &crate::RenderFrame,
        instance: &SrdInstance,
        inputs: SrdSpectralRadianceStabilizerInputs<'_>,
        plan: SrdSpectralRadiancePlan,
        output_name: &str,
    ) -> Result<GraphImage> {
        let history = build_pool_images(
            frame,
            instance.common_settings(),
            output_name,
            "history",
            instance.history_pool(),
        )?;
        let scratch = build_pool_images(
            frame,
            instance.common_settings(),
            output_name,
            "scratch",
            instance.scratch_pool(),
        )?;
        for dispatch in instance.dispatches() {
            self.execute_dispatch(dispatch, instance, &inputs, &history, &scratch, frame)?;
        }
        let output = scratch
            .get(plan.final_output_scratch_index as usize)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD spectral radiance final output references missing scratch index {}",
                    plan.final_output_scratch_index
                ))
            })?;
        output.register_as(output_name);
        Ok(output.clone())
    }

    fn execute_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        instance: &SrdInstance,
        inputs: &SrdSpectralRadianceStabilizerInputs<'_>,
        history: &[GraphImage],
        scratch: &[GraphImage],
        frame: &crate::RenderFrame,
    ) -> Result<()> {
        let pipeline = instance
            .pipelines()
            .get(dispatch.pipeline_index)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD spectral radiance dispatch '{}' references missing pipeline {}",
                    dispatch.name, dispatch.pipeline_index
                ))
            })?;
        let program = self
            .programs
            .program_for_shader_label(&pipeline.shader_label)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD spectral radiance dispatch '{}' references unsupported shader label '{}'",
                    dispatch.name, pipeline.shader_label
                ))
            })?;
        let storage_resource = single_storage_resource(dispatch)?;
        let target = resolve_spectral_resource(storage_resource, inputs, history, scratch)?;
        let target_binding =
            srd_storage_binding_for_resource(&pipeline.shader_label, storage_resource)?;
        let mut pass = frame
            .shader_pass(dispatch.name.clone())
            .target_as(target_binding, target);

        let mut scratch_ordinal = 0usize;
        for resource in &dispatch.resources {
            match resource.descriptor_type {
                SrdDescriptorType::Texture => {
                    if resource.slot == SrdResourceSlot::ScratchPool {
                        scratch_ordinal += 1;
                    }
                    let binding = srd_sampled_binding_for_resource(
                        &pipeline.shader_label,
                        resource,
                        scratch_ordinal,
                    )?;
                    let image = resolve_spectral_resource(resource, inputs, history, scratch)?;
                    pass = pass.image(binding, image);
                }
                SrdDescriptorType::StorageTexture => {}
                SrdDescriptorType::Sampler | SrdDescriptorType::UniformBuffer => {
                    return Err(crate::Error::InvalidInput(format!(
                        "SRD spectral radiance dispatch '{}' contains unsupported descriptor type {:?}",
                        dispatch.name, resource.descriptor_type
                    )));
                }
            }
        }

        if pipeline.has_constants || dispatch.constants_size > 0 {
            let range = dispatch.constants_range.ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD spectral radiance dispatch '{}' is missing constants",
                    dispatch.name
                ))
            })?;
            let bytes = instance.constant_bytes(range);
            pass = pass.push_constants(crate::StageMask::COMPUTE, bytes);
        }

        pass.compute(program, dispatch.grid_size)
    }
}

fn resolve_spectral_resource<'a>(
    resource: &SrdResourceDesc,
    inputs: &'a SrdSpectralRadianceStabilizerInputs<'a>,
    history: &'a [GraphImage],
    scratch: &'a [GraphImage],
) -> Result<&'a GraphImage> {
    match (resource.slot, resource.pool_index) {
        (SrdResourceSlot::SpectralRadianceInput, None) => Ok(inputs.spectral_radiance),
        (SrdResourceSlot::MotionVectorsInput, None) => Ok(inputs.motion_vectors),
        (SrdResourceSlot::LinearDepthInput, None) => Ok(inputs.linear_depth),
        (SrdResourceSlot::NormalRoughnessInput, None) => Ok(inputs.normal_roughness),
        (SrdResourceSlot::PrevLinearDepthInput, None) => {
            Ok(inputs.prev_linear_depth.unwrap_or(inputs.linear_depth))
        }
        (SrdResourceSlot::PrevNormalRoughnessInput, None) => {
            Ok(inputs.prev_normal_roughness.unwrap_or(inputs.normal_roughness))
        }
        (SrdResourceSlot::HistoryPool, Some(index)) => history
            .get(index as usize)
            .ok_or_else(|| missing_pool_resource(resource)),
        (SrdResourceSlot::ScratchPool, Some(index)) => scratch
            .get(index as usize)
            .ok_or_else(|| missing_pool_resource(resource)),
        (slot, pool_index) => Err(crate::Error::InvalidInput(format!(
            "SRD spectral radiance executor cannot resolve resource {slot:?} pool {pool_index:?}"
        ))),
    }
}

fn missing_pool_resource(resource: &SrdResourceDesc) -> crate::Error {
    crate::Error::InvalidInput(format!(
        "SRD spectral radiance executor missing {:?} pool resource {:?}",
        resource.slot, resource.pool_index
    ))
}

impl SrdInstance {
    pub fn register_spectral_accumulate_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Spectral Accumulate".into(),
            debug_label: "SRD Spectral Accumulate".into(),
            shader_label: "srd_spectral_accumulate".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    pub fn register_spectral_filter_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Spectral Filter".into(),
            debug_label: "SRD Spectral Filter".into(),
            shader_label: "srd_spectral_filter".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    /// Prepare all spectral radiance stabilizer resources and pipelines.
    pub fn prepare_spectral_radiance(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdSpectralResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::SpectralRadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a SpectralRadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }

        let clear_pipeline = self.register_clear_pipeline()?;
        let surface_mask = self.prepare_radiance_surface_mask(denoiser_id)?;
        let reproject = self.prepare_radiance_reproject(denoiser_id)?;

        let id = denoiser_id.get();
        let label = format!("spectral_history_{id}");
        let write_index = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_write"),
            debug_label: format!("SRD Spectral History {id} Write"),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        let read_index = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_read"),
            debug_label: format!("SRD Spectral History {id} Read"),
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
        let accumulate_ring_index = self.history_rings().len() - 1;
        let accumulate_pipeline = self.register_spectral_accumulate_pipeline()?;

        let filter_scratch_index = self.add_unique_scratch_texture(SrdTextureDesc {
            name: format!("spectral_filter_{id}"),
            debug_label: format!("SRD Spectral Filter {id}"),
            slot: SrdResourceSlot::SpectralRadianceOutput,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;
        let filter_pipeline = self.register_spectral_filter_pipeline()?;

        Ok(SrdSpectralResources {
            clear_pipeline,
            surface_mask,
            reproject,
            accumulate: SrdSpectralAccumulateResources {
                history_ring_index: accumulate_ring_index,
                pipeline: accumulate_pipeline,
            },
            filter: SrdSpectralFilterResources {
                scratch_index: filter_scratch_index,
                pipeline: filter_pipeline,
            },
        })
    }

    /// Plan one frame of spectral radiance denoising.
    ///
    /// `channel_count` is derived from the active `SrdSpectralLayout` in the
    /// common settings shader contract. Emitted order: optional clear, surface
    /// mask, reproject, accumulate, filter.
    pub fn plan_spectral_radiance_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdSpectralResources,
        rect_size: UVec2,
    ) -> Result<SrdSpectralRadiancePlan> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::SpectralRadianceStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not a SpectralRadianceStabilizer denoiser",
                denoiser_id.get()
            )));
        }

        self.clear_dispatches();
        if self.common_settings().history_mode == SrdHistoryMode::ZeroHistory {
            self.push_clear_dispatches_for(denoiser_id, resources.clear_pipeline)?;
        }
        self.rotate_history_ring_at(resources.accumulate.history_ring_index)?;

        self.plan_radiance_surface_mask_passes(denoiser_id, resources.surface_mask, rect_size)?;
        self.plan_radiance_reproject_passes(
            denoiser_id,
            resources.reproject,
            Some(resources.surface_mask),
            rect_size,
        )?;

        let ring = self.history_rings()[resources.accumulate.history_ring_index].clone();
        let tile = SRD_RADIANCE_SURFACE_MASK_TILE_SIZE;
        let mask_extent = UVec2::new(
            rect_size.x.div_ceil(tile).max(1),
            rect_size.y.div_ceil(tile).max(1),
        );
        let spectral_settings = match self.denoiser_settings(denoiser_id) {
            Some(SrdFamilySettings::SpectralRadiance(s)) => *s,
            _ => super::settings::SrdSpectralRadianceSettings::default(),
        };
        let layout = self.common_settings().shader_contract.spectral_layout;
        let channel_count = (layout.channel_count() as u32).min(3).max(1);

        // Spectral Accumulate: spectral_input + reproject + surface_mask + history_read → history_write
        {
            let constants = SrdSpectralAccumulateConstants {
                rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
                mask_extent: [mask_extent.x, mask_extent.y],
                history_frame_budget: spectral_settings.history_frame_budget.max(1) as f32,
                channel_count,
                use_surface_mask: 1,
                tile_size: tile,
            };
            let constants_range = self.push_typed_constants(&constants);
            let dispatch = SrdPassBuilder::new(
                "SRD Spectral Accumulate",
                denoiser_id,
                resources.accumulate.pipeline,
            )
            .read(SrdResourceSlot::SpectralRadianceInput)
            .read_pool(SrdPoolClass::Scratch, resources.reproject.scratch_index)
            .read_pool(SrdPoolClass::Scratch, resources.surface_mask.scratch_index)
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
        }

        // Spectral Filter: accumulated history + depth + normal + surface_mask → filtered output
        {
            let constants = SrdSpectralFilterConstants {
                rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
                mask_extent: [mask_extent.x, mask_extent.y],
                normal_weight_power: spectral_settings.normal_weight_power,
                spatial_radius: spectral_settings.spatial_radius,
                use_surface_mask: 1,
                tile_size: tile,
            };
            let constants_range = self.push_typed_constants(&constants);
            let dispatch = SrdPassBuilder::new(
                "SRD Spectral Filter",
                denoiser_id,
                resources.filter.pipeline,
            )
            .read_pool(SrdPoolClass::History, ring.write_index)
            .read(SrdResourceSlot::LinearDepthInput)
            .read(SrdResourceSlot::NormalRoughnessInput)
            .read_pool(SrdPoolClass::Scratch, resources.surface_mask.scratch_index)
            .write_pool(SrdPoolClass::Scratch, resources.filter.scratch_index)
            .constants_range(constants_range)
            .grid_size([
                rect_size.x.div_ceil(8).max(1),
                rect_size.y.div_ceil(8).max(1),
                1,
            ])
            .build()?;
            self.push_dispatch(dispatch)?;
        }

        Ok(SrdSpectralRadiancePlan {
            final_output_scratch_index: resources.filter.scratch_index,
            dispatch_count: self.dispatches().len(),
        })
    }
}
