use crate::{ComputeProgram, Engine, GraphImage, ImageDesc, Result, SamplerPreset};
use std::path::PathBuf;
use super::*;

fn srd_engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Compute shader programs used by the v0 Radiance Stabilizer executor.
pub struct SrdRadianceStabilizerPrograms {
    pub clear: ComputeProgram,
    pub surface_mask: ComputeProgram,
    pub reproject: ComputeProgram,
    pub accumulate: ComputeProgram,
    pub clamp: ComputeProgram,
    pub reconstruct: ComputeProgram,
    pub outlier_suppress: ComputeProgram,
    pub spatial_filter: ComputeProgram,
    pub atrous: ComputeProgram,
    pub post_blur: ComputeProgram,
}

impl SrdRadianceStabilizerPrograms {
    pub fn load(engine: &Engine) -> Result<Self> {
        Ok(Self {
            clear: ComputeProgram::load(engine, srd_engine_shader("srd_clear_history.slang"))?,
            surface_mask: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_surface_mask.slang"),
            )?,
            reproject: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_reproject.slang"),
            )?,
            accumulate: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_accumulate.slang"),
            )?,
            clamp: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_clamp.slang"),
            )?,
            reconstruct: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_reconstruct.slang"),
            )?,
            outlier_suppress: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_outlier_suppress.slang"),
            )?,
            spatial_filter: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_spatial_filter.slang"),
            )?,
            atrous: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_atrous.slang"),
            )?,
            post_blur: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_radiance_post_blur.slang"),
            )?,
        })
    }

    pub fn program_for_shader_label(&self, shader_label: &str) -> Option<&ComputeProgram> {
        match shader_label {
            "srd_clear_history" => Some(&self.clear),
            "srd_radiance_surface_mask" => Some(&self.surface_mask),
            "srd_radiance_reproject" => Some(&self.reproject),
            "srd_radiance_accumulate" => Some(&self.accumulate),
            "srd_radiance_clamp" => Some(&self.clamp),
            "srd_radiance_reconstruct" => Some(&self.reconstruct),
            "srd_radiance_outlier_suppress" => Some(&self.outlier_suppress),
            "srd_radiance_spatial_filter" => Some(&self.spatial_filter),
            "srd_radiance_atrous" => Some(&self.atrous),
            "srd_radiance_post_blur" => Some(&self.post_blur),
            _ => None,
        }
    }
}

/// External graph images consumed by the v0 Radiance Stabilizer dispatches.
pub struct SrdRadianceStabilizerInputs<'a> {
    pub combined_radiance: &'a GraphImage,
    pub motion_vectors: &'a GraphImage,
    pub linear_depth: &'a GraphImage,
    pub normal_roughness: &'a GraphImage,
    pub prev_linear_depth: Option<&'a GraphImage>,
    pub prev_normal_roughness: Option<&'a GraphImage>,
    pub hit_distance: Option<&'a GraphImage>,
    pub prev_hit_distance: Option<&'a GraphImage>,
    pub diffuse_radiance: Option<&'a GraphImage>,
    pub specular_radiance: Option<&'a GraphImage>,
}

/// Renderer-side executor for SRD Radiance Stabilizer compute dispatches.
pub struct SrdRadianceStabilizerExecutor<'a> {
    programs: &'a SrdRadianceStabilizerPrograms,
    history_sampler: SamplerPreset,
}

impl<'a> SrdRadianceStabilizerExecutor<'a> {
    pub const fn new(programs: &'a SrdRadianceStabilizerPrograms) -> Self {
        Self {
            programs,
            history_sampler: SamplerPreset::Linear,
        }
    }

    pub const fn with_history_sampler(
        programs: &'a SrdRadianceStabilizerPrograms,
        history_sampler: SamplerPreset,
    ) -> Self {
        Self {
            programs,
            history_sampler,
        }
    }

    pub fn execute_plan(
        &self,
        frame: &crate::RenderFrame,
        instance: &SrdInstance,
        inputs: SrdRadianceStabilizerInputs<'_>,
        plan: SrdRadianceStabilizerPlan,
        output_name: &str,
    ) -> Result<GraphImage> {
        let graph_resources = SrdRadianceGraphResources::build(frame, instance, output_name)?;
        for dispatch in instance.dispatches() {
            self.execute_dispatch(dispatch, instance, &inputs, &graph_resources, frame)?;
        }
        let output = graph_resources.final_output(plan.final_output)?;
        output.register_as(output_name);
        Ok(output.clone())
    }

    fn execute_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        instance: &SrdInstance,
        inputs: &SrdRadianceStabilizerInputs<'_>,
        graph_resources: &SrdRadianceGraphResources,
        frame: &crate::RenderFrame,
    ) -> Result<()> {
        let pipeline = instance
            .pipelines()
            .get(dispatch.pipeline_index)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD radiance dispatch '{}' references missing pipeline {}",
                    dispatch.name, dispatch.pipeline_index
                ))
            })?;
        let program = self
            .programs
            .program_for_shader_label(&pipeline.shader_label)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD radiance dispatch '{}' references unsupported shader label '{}'",
                    dispatch.name, pipeline.shader_label
                ))
            })?;
        let storage_resource = single_storage_resource(dispatch)?;
        let target = resolve_graph_resource(storage_resource, inputs, graph_resources)?;
        let target_binding =
            srd_storage_binding_for_resource(&pipeline.shader_label, storage_resource)?;
        let mut pass = frame
            .shader_pass(dispatch.name.clone())
            .target_as(target_binding, target);

        let mut scratch_texture_ordinal = 0usize;
        for resource in &dispatch.resources {
            match resource.descriptor_type {
                SrdDescriptorType::Texture => {
                    if resource.slot == SrdResourceSlot::ScratchPool {
                        scratch_texture_ordinal += 1;
                    }
                    let binding = srd_sampled_binding_for_resource(
                        &pipeline.shader_label,
                        resource,
                        scratch_texture_ordinal,
                    )?;
                    let image = resolve_graph_resource(resource, inputs, graph_resources)?;
                    pass = pass.image(binding, image);
                }
                SrdDescriptorType::StorageTexture => {}
                SrdDescriptorType::Sampler | SrdDescriptorType::UniformBuffer => {
                    return Err(crate::Error::InvalidInput(format!(
                        "SRD radiance dispatch '{}' contains unsupported descriptor type {:?}",
                        dispatch.name, resource.descriptor_type
                    )));
                }
            }
        }

        if pipeline.shader_label == "srd_radiance_accumulate" {
            pass = pass.sampler("srd_radiance_history_sampler", self.history_sampler);
        }
        if pipeline.has_constants || dispatch.constants_size > 0 {
            let range = dispatch.constants_range.ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD radiance dispatch '{}' is missing constants",
                    dispatch.name
                ))
            })?;
            let bytes = instance.constant_bytes(range);
            if bytes.len() != dispatch.constants_size {
                return Err(crate::Error::InvalidInput(format!(
                    "SRD radiance dispatch '{}' constants size mismatch: dispatch={} bytes={}",
                    dispatch.name,
                    dispatch.constants_size,
                    bytes.len()
                )));
            }
            pass = pass.push_constants(crate::StageMask::COMPUTE, bytes);
        }

        pass.compute(program, dispatch.grid_size)
    }
}

pub(super) struct SrdRadianceGraphResources {
    pub(super) history: Vec<GraphImage>,
    pub(super) scratch: Vec<GraphImage>,
}

impl SrdRadianceGraphResources {
    fn build(frame: &crate::RenderFrame, instance: &SrdInstance, output_name: &str) -> Result<Self> {
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
        Ok(Self { history, scratch })
    }

    fn final_output(&self, output: SrdRadianceOutputResource) -> Result<&GraphImage> {
        let index = output.scratch_index();
        self.scratch.get(index as usize).ok_or_else(|| {
            crate::Error::InvalidInput(format!(
                "SRD radiance final output references missing scratch index {index}"
            ))
        })
    }
}

pub(super) fn build_pool_images(
    frame: &crate::RenderFrame,
    common_settings: &SrdCommonSettings,
    output_name: &str,
    pool_label: &str,
    textures: &[SrdTextureDesc],
) -> Result<Vec<GraphImage>> {
    textures
        .iter()
        .map(|texture| {
            frame.image(
                format!("{output_name}_{pool_label}_{}", texture.name),
                image_desc_for_srd_texture(common_settings, texture),
            )
        })
        .collect()
}

pub(super) fn image_desc_for_srd_texture(
    common_settings: &SrdCommonSettings,
    texture: &SrdTextureDesc,
) -> ImageDesc {
    let factor = texture.downsample_factor.max(1);
    let extent = crate::Extent3d {
        width: common_settings.rect_size.x.div_ceil(factor).max(1),
        height: common_settings.rect_size.y.div_ceil(factor).max(1),
        depth: 1,
    };
    ImageDesc {
        dimension: crate::ImageDimension::D2,
        extent,
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: texture.format,
        usage: crate::ImageUsage::SAMPLED | crate::ImageUsage::STORAGE,
        transient: texture.pool == Some(SrdPoolClass::Scratch),
        clear_value: None,
        debug_name: None,
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
        drm_format_modifier: None,
    }
}

pub(super) fn single_storage_resource(dispatch: &SrdDispatchDesc) -> Result<&SrdResourceDesc> {
    let mut storage_resources = dispatch
        .resources
        .iter()
        .filter(|resource| resource.descriptor_type == SrdDescriptorType::StorageTexture);
    let Some(resource) = storage_resources.next() else {
        return Err(crate::Error::InvalidInput(format!(
            "SRD radiance dispatch '{}' has no storage output",
            dispatch.name
        )));
    };
    if storage_resources.next().is_some() {
        return Err(crate::Error::InvalidInput(format!(
            "SRD radiance dispatch '{}' has multiple storage outputs; the v0 graph executor supports one",
            dispatch.name
        )));
    }
    Ok(resource)
}

fn resolve_graph_resource<'a>(
    resource: &SrdResourceDesc,
    inputs: &'a SrdRadianceStabilizerInputs<'a>,
    graph_resources: &'a SrdRadianceGraphResources,
) -> Result<&'a GraphImage> {
    match (resource.slot, resource.pool_index) {
        (SrdResourceSlot::CombinedRadianceInput, None) => Ok(inputs.combined_radiance),
        (SrdResourceSlot::MotionVectorsInput, None) => Ok(inputs.motion_vectors),
        (SrdResourceSlot::LinearDepthInput, None) => Ok(inputs.linear_depth),
        (SrdResourceSlot::NormalRoughnessInput, None) => Ok(inputs.normal_roughness),
        (SrdResourceSlot::PrevLinearDepthInput, None) => {
            Ok(inputs.prev_linear_depth.unwrap_or(inputs.linear_depth))
        }
        (SrdResourceSlot::PrevNormalRoughnessInput, None) => Ok(inputs
            .prev_normal_roughness
            .unwrap_or(inputs.normal_roughness)),
        (SrdResourceSlot::HitDistanceInput, None) => {
            inputs.hit_distance.ok_or_else(|| {
                crate::Error::InvalidInput(
                    "SRD radiance executor: HitDistanceInput requested but not provided".into(),
                )
            })
        }
        (SrdResourceSlot::PrevHitDistanceInput, None) => {
            let img = inputs.prev_hit_distance.or(inputs.hit_distance).ok_or_else(|| {
                crate::Error::InvalidInput(
                    "SRD radiance executor: PrevHitDistanceInput requested but not provided".into(),
                )
            })?;
            Ok(img)
        }
        (SrdResourceSlot::DiffuseRadianceInput, None) => {
            inputs.diffuse_radiance.ok_or_else(|| {
                crate::Error::InvalidInput(
                    "SRD radiance executor: DiffuseRadianceInput requested but not provided".into(),
                )
            })
        }
        (SrdResourceSlot::SpecularRadianceInput, None) => {
            inputs.specular_radiance.ok_or_else(|| {
                crate::Error::InvalidInput(
                    "SRD radiance executor: SpecularRadianceInput requested but not provided".into(),
                )
            })
        }
        (SrdResourceSlot::HistoryPool, Some(index)) => graph_resources
            .history
            .get(index as usize)
            .ok_or_else(|| missing_pool_resource(resource)),
        (SrdResourceSlot::ScratchPool, Some(index))
        | (SrdResourceSlot::ClampedRadianceInput, Some(index)) => graph_resources
            .scratch
            .get(index as usize)
            .ok_or_else(|| missing_pool_resource(resource)),
        (slot, pool_index) => Err(crate::Error::InvalidInput(format!(
            "SRD radiance executor cannot resolve resource {slot:?} pool {pool_index:?}"
        ))),
    }
}

fn missing_pool_resource(resource: &SrdResourceDesc) -> crate::Error {
    crate::Error::InvalidInput(format!(
        "SRD radiance executor missing {:?} pool resource {:?}",
        resource.slot, resource.pool_index
    ))
}

pub(super) fn srd_storage_binding_for_resource(
    shader_label: &str,
    resource: &SrdResourceDesc,
) -> Result<&'static str> {
    match (shader_label, resource.slot) {
        ("srd_clear_history", SrdResourceSlot::HistoryPool) => Ok("srd_clear_target"),
        ("srd_radiance_surface_mask", SrdResourceSlot::ScratchPool) => {
            Ok("srd_radiance_surface_mask")
        }
        ("srd_radiance_reproject", SrdResourceSlot::ScratchPool) => {
            Ok("srd_radiance_reproject_out")
        }
        ("srd_radiance_accumulate", SrdResourceSlot::HistoryPool) => {
            Ok("srd_radiance_history_write")
        }
        ("srd_radiance_clamp", SrdResourceSlot::ScratchPool) => Ok("srd_radiance_clamped_out"),
        ("srd_radiance_reconstruct", SrdResourceSlot::ScratchPool) => {
            Ok("srd_radiance_reconstruct_out")
        }
        ("srd_radiance_outlier_suppress", SrdResourceSlot::ScratchPool) => {
            Ok("srd_radiance_clamped_out")
        }
        ("srd_radiance_spatial_filter", SrdResourceSlot::ScratchPool) => {
            Ok("srd_radiance_filtered_out")
        }
        ("srd_radiance_atrous", SrdResourceSlot::ScratchPool) => Ok("srd_radiance_atrous_out"),
        ("srd_radiance_post_blur", SrdResourceSlot::ScratchPool) => Ok("srd_radiance_post_blur_out"),
        ("srd_occlusion_accumulate", SrdResourceSlot::HistoryPool) => Ok("srd_occlusion_history_write"),
        ("srd_occlusion_filter", SrdResourceSlot::ScratchPool) => Ok("srd_occlusion_out"),
        _ => Err(crate::Error::InvalidInput(format!(
            "SRD shader '{shader_label}' has no storage binding for {:?}",
            resource.slot
        ))),
    }
}

pub(super) fn srd_sampled_binding_for_resource(
    shader_label: &str,
    resource: &SrdResourceDesc,
    scratch_texture_ordinal: usize,
) -> Result<&'static str> {
    match resource.slot {
        SrdResourceSlot::CombinedRadianceInput => Ok("srd_radiance_input"),
        SrdResourceSlot::MotionVectorsInput => Ok("srd_guide_motion"),
        SrdResourceSlot::LinearDepthInput => Ok("srd_guide_view_depth"),
        SrdResourceSlot::NormalRoughnessInput => Ok("srd_guide_normal_roughness"),
        SrdResourceSlot::PrevLinearDepthInput => Ok("srd_prev_guide_view_depth"),
        SrdResourceSlot::PrevNormalRoughnessInput => Ok("srd_prev_guide_normal_roughness"),
        SrdResourceSlot::HitDistanceInput => Ok("srd_guide_hit_distance"),
        SrdResourceSlot::PrevHitDistanceInput => Ok("srd_prev_guide_hit_distance"),
        SrdResourceSlot::DiffuseRadianceInput => Ok("srd_radiance_input"),
        SrdResourceSlot::SpecularRadianceInput => Ok("srd_radiance_input"),
        SrdResourceSlot::OcclusionInput => Ok("srd_occlusion_input"),
        SrdResourceSlot::ClampedRadianceInput => Ok("srd_radiance_accumulated"),
        SrdResourceSlot::HistoryPool => match shader_label {
            "srd_radiance_accumulate" => Ok("srd_radiance_history_read"),
            "srd_radiance_clamp" => Ok("srd_radiance_accumulated"),
            "srd_radiance_reconstruct" => Ok("srd_radiance_accumulated"),
            "srd_occlusion_accumulate" => Ok("srd_occlusion_history_read"),
            "srd_occlusion_filter" => Ok("srd_occlusion_accumulated"),
            _ => Err(crate::Error::InvalidInput(format!(
                "SRD shader '{shader_label}' has no sampled history binding"
            ))),
        },
        SrdResourceSlot::ScratchPool => match (shader_label, scratch_texture_ordinal) {
            ("srd_radiance_reproject", 1) => Ok("srd_radiance_surface_mask"),
            ("srd_radiance_accumulate", 1) => Ok("srd_radiance_reproject"),
            ("srd_radiance_accumulate", 2) => Ok("srd_radiance_surface_mask"),
            ("srd_radiance_clamp", 1) => Ok("srd_radiance_surface_mask"),
            ("srd_radiance_reconstruct", 1) => Ok("srd_radiance_surface_mask"),
            ("srd_radiance_outlier_suppress", 1) => Ok("srd_radiance_input"),
            ("srd_radiance_outlier_suppress", 2) => Ok("srd_radiance_surface_mask"),
            ("srd_radiance_spatial_filter", 1) => Ok("srd_radiance_input"),
            ("srd_radiance_spatial_filter", 2) => Ok("srd_radiance_surface_mask"),
            ("srd_radiance_atrous", 1) => Ok("srd_radiance_input"),
            ("srd_radiance_atrous", 2) => Ok("srd_radiance_surface_mask"),
            ("srd_radiance_post_blur", 1) => Ok("srd_radiance_input"),
            ("srd_radiance_post_blur", 2) => Ok("srd_radiance_surface_mask"),
            ("srd_occlusion_accumulate", 1) => Ok("srd_occlusion_reproject"),
            ("srd_occlusion_accumulate", 2) => Ok("srd_radiance_surface_mask"),
            ("srd_occlusion_filter", 1) => Ok("srd_radiance_surface_mask"),
            _ => Err(crate::Error::InvalidInput(format!(
                "SRD shader '{shader_label}' has no sampled scratch binding for ordinal {scratch_texture_ordinal}"
            ))),
        },
        _ => Err(crate::Error::InvalidInput(format!(
            "SRD shader '{shader_label}' has no sampled binding for {:?}",
            resource.slot
        ))),
    }
}
