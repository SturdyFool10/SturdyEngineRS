use crate::{ComputeProgram, Engine, Error, Format, GraphImage, Result};
use glam::UVec2;
use std::path::PathBuf;
use super::*;
use super::radiance_stabilizer_executor::{
    build_pool_images, single_storage_resource,
    srd_sampled_binding_for_resource, srd_storage_binding_for_resource,
};

/// Summary returned by `SrdInstance::plan_radiance_occlusion_passes`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdOcclusionPlan {
    pub final_output_scratch_index: u16,
    pub dispatch_count: usize,
}

/// External graph images consumed by the v0 Occlusion Stabilizer dispatches.
pub struct SrdOcclusionStabilizerInputs<'a> {
    pub occlusion: &'a GraphImage,
    pub motion_vectors: &'a GraphImage,
    pub linear_depth: &'a GraphImage,
    pub normal_roughness: &'a GraphImage,
    pub prev_linear_depth: Option<&'a GraphImage>,
    pub prev_normal_roughness: Option<&'a GraphImage>,
}

/// Compute shader programs used by the v0 Occlusion Stabilizer executor.
pub struct SrdOcclusionStabilizerPrograms {
    pub clear: ComputeProgram,
    pub surface_mask: ComputeProgram,
    pub reproject: ComputeProgram,
    pub accumulate: ComputeProgram,
    pub filter: ComputeProgram,
}

impl SrdOcclusionStabilizerPrograms {
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
                Self::srd_engine_shader("srd_occlusion_accumulate.slang"),
            )?,
            filter: ComputeProgram::load(
                engine,
                Self::srd_engine_shader("srd_occlusion_filter.slang"),
            )?,
        })
    }

    pub fn program_for_shader_label(&self, shader_label: &str) -> Option<&ComputeProgram> {
        match shader_label {
            "srd_clear_history" => Some(&self.clear),
            "srd_radiance_surface_mask" => Some(&self.surface_mask),
            "srd_radiance_reproject" => Some(&self.reproject),
            "srd_occlusion_accumulate" => Some(&self.accumulate),
            "srd_occlusion_filter" => Some(&self.filter),
            _ => None,
        }
    }
}

/// Renderer-side executor for SRD Occlusion Stabilizer compute dispatches.
pub struct SrdOcclusionStabilizerExecutor<'a> {
    programs: &'a SrdOcclusionStabilizerPrograms,
}

impl<'a> SrdOcclusionStabilizerExecutor<'a> {
    pub const fn new(programs: &'a SrdOcclusionStabilizerPrograms) -> Self {
        Self { programs }
    }

    pub fn execute_plan(
        &self,
        frame: &crate::RenderFrame,
        instance: &SrdInstance,
        inputs: SrdOcclusionStabilizerInputs<'_>,
        plan: SrdOcclusionPlan,
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
                    "SRD occlusion final output references missing scratch index {}",
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
        inputs: &SrdOcclusionStabilizerInputs<'_>,
        history: &[GraphImage],
        scratch: &[GraphImage],
        frame: &crate::RenderFrame,
    ) -> Result<()> {
        let pipeline = instance
            .pipelines()
            .get(dispatch.pipeline_index)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD occlusion dispatch '{}' references missing pipeline {}",
                    dispatch.name, dispatch.pipeline_index
                ))
            })?;
        let program = self
            .programs
            .program_for_shader_label(&pipeline.shader_label)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD occlusion dispatch '{}' references unsupported shader label '{}'",
                    dispatch.name, pipeline.shader_label
                ))
            })?;
        let storage_resource = single_storage_resource(dispatch)?;
        let target = resolve_occlusion_resource(storage_resource, inputs, history, scratch)?;
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
                    let image =
                        resolve_occlusion_resource(resource, inputs, history, scratch)?;
                    pass = pass.image(binding, image);
                }
                SrdDescriptorType::StorageTexture => {}
                SrdDescriptorType::Sampler | SrdDescriptorType::UniformBuffer => {
                    return Err(crate::Error::InvalidInput(format!(
                        "SRD occlusion dispatch '{}' contains unsupported descriptor type {:?}",
                        dispatch.name, resource.descriptor_type
                    )));
                }
            }
        }

        if pipeline.has_constants || dispatch.constants_size > 0 {
            let range = dispatch.constants_range.ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD occlusion dispatch '{}' is missing constants",
                    dispatch.name
                ))
            })?;
            let bytes = instance.constant_bytes(range);
            pass = pass.push_constants(crate::StageMask::COMPUTE, bytes);
        }

        pass.compute(program, dispatch.grid_size)
    }
}

fn resolve_occlusion_resource<'a>(
    resource: &SrdResourceDesc,
    inputs: &'a SrdOcclusionStabilizerInputs<'a>,
    history: &'a [GraphImage],
    scratch: &'a [GraphImage],
) -> Result<&'a GraphImage> {
    match (resource.slot, resource.pool_index) {
        (SrdResourceSlot::OcclusionInput, None) => Ok(inputs.occlusion),
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
            "SRD occlusion executor cannot resolve resource {slot:?} pool {pool_index:?}"
        ))),
    }
}

fn missing_pool_resource(resource: &SrdResourceDesc) -> crate::Error {
    crate::Error::InvalidInput(format!(
        "SRD occlusion executor missing {:?} pool resource {:?}",
        resource.slot, resource.pool_index
    ))
}

impl SrdInstance {
    pub fn register_occlusion_accumulate_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Occlusion Accumulate".into(),
            debug_label: "SRD Occlusion Accumulate".into(),
            shader_label: "srd_occlusion_accumulate".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    pub fn register_occlusion_filter_pipeline(&mut self) -> Result<usize> {
        self.register_pipeline(SrdPipelineDesc {
            name: "SRD Occlusion Filter".into(),
            debug_label: "SRD Occlusion Filter".into(),
            shader_label: "srd_occlusion_filter".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
    }

    /// Prepare all occlusion stabilizer resources and pipelines.
    pub fn prepare_radiance_occlusion(
        &mut self,
        denoiser_id: SrdDenoiserId,
    ) -> Result<SrdOcclusionResources> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::OcclusionStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not an OcclusionStabilizer denoiser",
                denoiser_id.get()
            )));
        }

        let clear_pipeline = self.register_clear_pipeline()?;
        let surface_mask = self.prepare_radiance_surface_mask(denoiser_id)?;
        let reproject = self.prepare_radiance_reproject(denoiser_id)?;

        let id = denoiser_id.get();
        let label = format!("occlusion_history_{id}");
        let write_index = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_write"),
            debug_label: format!("SRD Occlusion History {id} Write"),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })?;
        let read_index = self.add_history_texture(SrdTextureDesc {
            name: format!("{label}_read"),
            debug_label: format!("SRD Occlusion History {id} Read"),
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
        let accumulate_pipeline = self.register_occlusion_accumulate_pipeline()?;

        let filter_scratch_index = self.add_unique_scratch_texture(SrdTextureDesc {
            name: format!("occlusion_filter_{id}"),
            debug_label: format!("SRD Occlusion Filter {id}"),
            slot: SrdResourceSlot::OcclusionOutput,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })?;
        let filter_pipeline = self.register_occlusion_filter_pipeline()?;

        Ok(SrdOcclusionResources {
            clear_pipeline,
            surface_mask,
            reproject,
            accumulate: SrdOcclusionAccumulateResources {
                history_ring_index: accumulate_ring_index,
                pipeline: accumulate_pipeline,
            },
            filter: SrdOcclusionFilterResources {
                scratch_index: filter_scratch_index,
                pipeline: filter_pipeline,
            },
        })
    }

    /// Plan one frame of occlusion denoising.
    ///
    /// Emitted order: optional clear, surface mask, reproject, accumulate, filter.
    pub fn plan_radiance_occlusion_passes(
        &mut self,
        denoiser_id: SrdDenoiserId,
        resources: SrdOcclusionResources,
        rect_size: UVec2,
    ) -> Result<SrdOcclusionPlan> {
        if self.mode_for_id(denoiser_id) != Some(SrdDenoiserMode::OcclusionStabilizer) {
            return Err(Error::InvalidInput(format!(
                "SRD denoiser id {} is not an OcclusionStabilizer denoiser",
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
        let occlusion_settings = match self.denoiser_settings(denoiser_id) {
            Some(SrdFamilySettings::Occlusion(s)) => *s,
            _ => super::settings::SrdOcclusionSettings::default(),
        };

        // Occlusion Accumulate: current AO + reproject + surface mask + history_read → history_write
        {
            let constants = SrdOcclusionAccumulateConstants {
                rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
                mask_extent: [mask_extent.x, mask_extent.y],
                history_frame_budget: occlusion_settings.history_frame_budget.max(1) as f32,
                use_surface_mask: 1,
                tile_size: tile,
                _pad: 0,
            };
            let constants_range = self.push_typed_constants(&constants);
            let dispatch = SrdPassBuilder::new(
                "SRD Occlusion Accumulate",
                denoiser_id,
                resources.accumulate.pipeline,
            )
            .read(SrdResourceSlot::OcclusionInput)
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

        // Occlusion Filter: accumulated history + depth + normal + surface mask → filtered output
        {
            let constants = SrdOcclusionFilterConstants {
                rect_extent: [rect_size.x.max(1), rect_size.y.max(1)],
                mask_extent: [mask_extent.x, mask_extent.y],
                normal_weight_power: occlusion_settings.normal_weight_power,
                spatial_radius: occlusion_settings.spatial_radius,
                use_surface_mask: 1,
                tile_size: tile,
            };
            let constants_range = self.push_typed_constants(&constants);
            let dispatch = SrdPassBuilder::new(
                "SRD Occlusion Filter",
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

        Ok(SrdOcclusionPlan {
            final_output_scratch_index: resources.filter.scratch_index,
            dispatch_count: self.dispatches().len(),
        })
    }
}
