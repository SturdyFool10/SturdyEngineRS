use crate::{ComputeProgram, Engine, GraphImage, RenderFrame, Result, StageMask};
use std::path::PathBuf;
use super::*;

/// Shader programs used by the current graph-backed SRD reference executor.
#[derive(Copy, Clone)]
pub struct SrdReferenceTemporalPrograms<'a> {
    pub temporal: &'a crate::ShaderProgram,
    pub clear: Option<&'a crate::ShaderProgram>,
}

impl<'a> SrdReferenceTemporalPrograms<'a> {
    pub const fn new(
        temporal: &'a crate::ShaderProgram,
        clear: Option<&'a crate::ShaderProgram>,
    ) -> Self {
        Self { temporal, clear }
    }
}

/// Reference-temporal executor for the existing render graph.
///
/// This is intentionally a renderer bridge, not the SRD algorithm itself. It
/// consumes `SrdDispatchDesc` values, resolves their SRD resource descriptors to
/// graph images, binds SRD-owned shader names, and emits the current fullscreen
/// implementation.
pub struct SrdReferenceTemporalExecutor<'a> {
    programs: SrdReferenceTemporalPrograms<'a>,
    bindings: SrdTemporalBindings,
}

impl<'a> SrdReferenceTemporalExecutor<'a> {
    pub const fn new(
        programs: SrdReferenceTemporalPrograms<'a>,
        bindings: SrdTemporalBindings,
    ) -> Self {
        Self { programs, bindings }
    }

    pub fn execute(
        &self,
        frame: &RenderFrame,
        input: &GraphImage,
        output_name: &str,
        history_state: &mut crate::GraphImageHistory,
        settings: SrdDenoiserSettings,
    ) -> Result<GraphImage> {
        if settings.mode != SrdDenoiserMode::ReferenceTemporal {
            return Err(crate::Error::InvalidInput(format!(
                "SRD graph executor only supports ReferenceTemporal, got {:?}",
                settings.mode
            )));
        }

        let mut history_desc = input.desc();
        history_desc.usage |= crate::ImageUsage::RENDER_TARGET | crate::ImageUsage::SAMPLED;
        let history = frame.history_images(history_state, output_name, history_desc)?;
        let mut instance = self.plan_instance(input, &history, settings)?;
        let pipelines =
            instance.prepare_reference_temporal(SrdDenoiserId::new(0), history_desc.format)?;
        let constants = SrdTemporalConstants {
            frame_index: history.frame_index.min(u32::MAX as u64) as u32,
            has_history: u32::from(history.has_history),
            max_frames: settings.max_frames,
            mode: settings.mode as u32,
        };
        instance.plan_reference_temporal_passes_with_pipelines(
            SrdDenoiserId::new(0),
            pipelines,
            constants,
        )?;

        let resources = SrdReferenceTemporalGraphResources {
            current_signal: input,
            history_read: &history.read,
            history_write: &history.write,
        };
        for dispatch in instance.dispatches() {
            self.execute_dispatch(dispatch, &instance, pipelines, &resources, frame, settings)?;
        }

        history.write.register_as("scene_color");
        Ok(history.write)
    }

    fn plan_instance(
        &self,
        input: &GraphImage,
        history: &crate::GraphImageHistoryFrame,
        settings: SrdDenoiserSettings,
    ) -> Result<SrdInstance> {
        let extent = input.desc().extent;
        let size = glam::UVec2::new(extent.width.max(1), extent.height.max(1));
        let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
            SrdDenoiserDesc::reference_temporal(0),
        ]))?;
        instance.set_denoiser_settings(
            SrdDenoiserId::new(0),
            SrdFamilySettings::Reference(SrdReferenceSettings {
                history_frame_budget: settings.max_frames,
            }),
        )?;
        instance.set_common_settings(SrdCommonSettings {
            frame_index: history.frame_index,
            history_mode: if history.has_history {
                SrdHistoryMode::KeepAccumulating
            } else {
                SrdHistoryMode::ZeroHistory
            },
            resource_size: size,
            resource_size_prev: size,
            rect_size: size,
            rect_size_prev: size,
            ..SrdCommonSettings::default()
        })?;
        Ok(instance)
    }

    fn execute_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        instance: &SrdInstance,
        pipelines: SrdReferenceTemporalPipelines,
        resources: &SrdReferenceTemporalGraphResources<'_>,
        frame: &RenderFrame,
        settings: SrdDenoiserSettings,
    ) -> Result<()> {
        if dispatch.pipeline_index == pipelines.clear {
            self.execute_clear_dispatch(dispatch, resources)
        } else if dispatch.pipeline_index == pipelines.temporal {
            self.execute_temporal_dispatch(dispatch, instance, resources, frame, settings)
        } else {
            Err(crate::Error::InvalidInput(format!(
                "SRD dispatch '{}' references an unknown reference-temporal pipeline {}",
                dispatch.name, dispatch.pipeline_index
            )))
        }
    }

    fn execute_clear_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        resources: &SrdReferenceTemporalGraphResources<'_>,
    ) -> Result<()> {
        let Some(clear_program) = self.programs.clear else {
            tracing::debug!(
                dispatch = dispatch.name,
                "SRD clear dispatch skipped because no clear shader was supplied"
            );
            return Ok(());
        };
        let Some(target) = reference_dispatch_storage_target(dispatch, resources)? else {
            return Err(crate::Error::InvalidInput(format!(
                "SRD clear dispatch '{}' has no storage target",
                dispatch.name
            )));
        };
        target.execute_shader_auto(clear_program)
    }

    fn execute_temporal_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        instance: &SrdInstance,
        resources: &SrdReferenceTemporalGraphResources<'_>,
        frame: &RenderFrame,
        settings: SrdDenoiserSettings,
    ) -> Result<()> {
        validate_reference_temporal_resources(dispatch)?;
        let Some(constants_range) = dispatch.constants_range else {
            return Err(crate::Error::InvalidInput(format!(
                "SRD temporal dispatch '{}' is missing constants",
                dispatch.name
            )));
        };
        resources
            .current_signal
            .register_as(self.bindings.current_signal);
        resources
            .history_read
            .register_as(self.bindings.history_signal);
        resources
            .history_write
            .register_as("srd_reference_temporal_output");
        frame.set_sampler(self.bindings.current_sampler, settings.current_sampler);
        frame.set_sampler(self.bindings.history_sampler, settings.history_sampler);
        resources.history_write.execute_shader_with_push_constants(
            self.programs.temporal,
            StageMask::FRAGMENT,
            instance.constant_bytes(constants_range),
        )
    }
}

struct SrdReferenceTemporalGraphResources<'a> {
    current_signal: &'a GraphImage,
    history_read: &'a GraphImage,
    history_write: &'a GraphImage,
}

fn validate_reference_temporal_resources(dispatch: &SrdDispatchDesc) -> Result<()> {
    let mut has_current_signal = false;
    let mut has_history_read = false;
    let mut has_combined_output = false;
    let mut has_history_write = false;

    for resource in &dispatch.resources {
        match (resource.descriptor_type, resource.slot, resource.pool_index) {
            (SrdDescriptorType::Texture, SrdResourceSlot::CombinedRadianceInput, None) => {
                has_current_signal = true;
            }
            (SrdDescriptorType::Texture, SrdResourceSlot::HistoryPool, Some(0)) => {
                has_history_read = true;
            }
            (SrdDescriptorType::StorageTexture, SrdResourceSlot::CombinedRadianceOutput, None) => {
                has_combined_output = true;
            }
            (SrdDescriptorType::StorageTexture, SrdResourceSlot::HistoryPool, Some(1)) => {
                has_history_write = true;
            }
            (descriptor_type, slot, pool_index) => {
                return Err(crate::Error::InvalidInput(format!(
                    "SRD reference temporal dispatch '{}' contains unsupported resource {descriptor_type:?} {slot:?} pool {pool_index:?}",
                    dispatch.name
                )));
            }
        }
    }

    if !has_current_signal || !has_history_read || !has_combined_output || !has_history_write {
        return Err(crate::Error::InvalidInput(format!(
            "SRD reference temporal dispatch '{}' is missing required resources: current={}, history_read={}, output={}, history_write={}",
            dispatch.name,
            has_current_signal,
            has_history_read,
            has_combined_output,
            has_history_write
        )));
    }
    Ok(())
}

fn reference_dispatch_storage_target<'a>(
    dispatch: &SrdDispatchDesc,
    resources: &'a SrdReferenceTemporalGraphResources<'a>,
) -> Result<Option<&'a GraphImage>> {
    let mut target: Option<&GraphImage> = None;
    for resource in &dispatch.resources {
        if resource.descriptor_type != SrdDescriptorType::StorageTexture {
            continue;
        }
        let resolved = match (resource.slot, resource.pool_index) {
            (SrdResourceSlot::CombinedRadianceOutput, None) => Some(resources.history_write),
            (SrdResourceSlot::HistoryPool, Some(0)) => Some(resources.history_read),
            (SrdResourceSlot::HistoryPool, Some(1)) => Some(resources.history_write),
            (slot, pool_index) => {
                return Err(crate::Error::InvalidInput(format!(
                    "SRD reference executor cannot resolve storage resource {slot:?} pool {pool_index:?}"
                )));
            }
        };
        if let Some(existing) = target
            && resolved.map(GraphImage::handle) != Some(existing.handle())
        {
            return Err(crate::Error::InvalidInput(format!(
                "SRD dispatch '{}' maps to multiple graph storage targets",
                dispatch.name
            )));
        }
        target = resolved;
    }
    Ok(target)
}

// ─── Compute executor ────────────────────────────────────────────────────────

fn srd_engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

/// Compute shader programs used by the SRD reference temporal compute executor.
pub struct SrdReferenceTemporalComputePrograms {
    pub temporal: ComputeProgram,
    pub clear: ComputeProgram,
}

impl SrdReferenceTemporalComputePrograms {
    pub fn load(engine: &Engine) -> Result<Self> {
        Ok(Self {
            temporal: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_reference_temporal.slang"),
            )?,
            clear: ComputeProgram::load(
                engine,
                srd_engine_shader("srd_clear_history.slang"),
            )?,
        })
    }

    pub fn program_for_shader_label(&self, label: &str) -> Option<&ComputeProgram> {
        match label {
            "srd_temporal_accumulate" | "srd_reference_temporal" => Some(&self.temporal),
            "srd_clear_history" => Some(&self.clear),
            _ => None,
        }
    }
}

/// Compute-based renderer executor for the SRD Reference Temporal path.
///
/// Replaces the fullscreen-fragment `SrdReferenceTemporalExecutor` with a compute
/// path that uses `frame.shader_pass(...).compute(program, grid)`, consistent with
/// all other SRD stabilizer executors.
pub struct SrdReferenceTemporalComputeExecutor<'a> {
    programs: &'a SrdReferenceTemporalComputePrograms,
}

impl<'a> SrdReferenceTemporalComputeExecutor<'a> {
    pub const fn new(programs: &'a SrdReferenceTemporalComputePrograms) -> Self {
        Self { programs }
    }

    /// Execute one frame of reference temporal accumulation via compute dispatch.
    pub fn execute(
        &self,
        frame: &RenderFrame,
        input: &GraphImage,
        output_name: &str,
        history_state: &mut crate::GraphImageHistory,
        settings: SrdDenoiserSettings,
    ) -> Result<GraphImage> {
        if settings.mode != SrdDenoiserMode::ReferenceTemporal {
            return Err(crate::Error::InvalidInput(format!(
                "SRD compute reference temporal executor only supports ReferenceTemporal, got {:?}",
                settings.mode
            )));
        }

        let mut history_desc = input.desc();
        history_desc.usage |=
            crate::ImageUsage::RENDER_TARGET | crate::ImageUsage::SAMPLED | crate::ImageUsage::STORAGE;
        let history = frame.history_images(history_state, output_name, history_desc)?;

        let mut instance = plan_compute_instance(input, &history, settings)?;
        let pipelines =
            instance.prepare_reference_temporal(SrdDenoiserId::new(0), history_desc.format)?;
        let constants = SrdTemporalConstants {
            frame_index: history.frame_index.min(u32::MAX as u64) as u32,
            has_history: u32::from(history.has_history),
            max_frames: settings.max_frames,
            mode: settings.mode as u32,
        };
        instance.plan_reference_temporal_passes_with_pipelines(
            SrdDenoiserId::new(0),
            pipelines,
            constants,
        )?;

        let ring = instance
            .history_rings()
            .iter()
            .find(|r| r.denoiser_id == SrdDenoiserId::new(0))
            .cloned()
            .ok_or_else(|| {
                crate::Error::InvalidInput("SRD compute executor: missing history ring".into())
            })?;

        let graph_resources = SrdRtComputeResources {
            current_signal: input,
            history_read: &history.read,
            history_write: &history.write,
            history_read_pool_idx: ring.read_index,
            history_write_pool_idx: ring.write_index,
        };

        for dispatch in instance.dispatches() {
            self.execute_dispatch(dispatch, &instance, pipelines, &graph_resources, frame)?;
        }

        history.write.register_as(output_name);
        Ok(history.write)
    }

    fn execute_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        instance: &SrdInstance,
        pipelines: SrdReferenceTemporalPipelines,
        resources: &SrdRtComputeResources<'_>,
        frame: &RenderFrame,
    ) -> Result<()> {
        let pipeline = instance
            .pipelines()
            .get(dispatch.pipeline_index)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD reference temporal compute dispatch '{}' references missing pipeline {}",
                    dispatch.name, dispatch.pipeline_index
                ))
            })?;
        let program = self
            .programs
            .program_for_shader_label(&pipeline.shader_label)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD reference temporal compute dispatch '{}' references unsupported shader '{}'",
                    dispatch.name, pipeline.shader_label
                ))
            })?;

        if dispatch.pipeline_index == pipelines.clear {
            self.execute_clear_dispatch(dispatch, instance, resources, frame, program)
        } else if dispatch.pipeline_index == pipelines.temporal {
            self.execute_temporal_dispatch(dispatch, instance, resources, frame, program)
        } else {
            Err(crate::Error::InvalidInput(format!(
                "SRD reference temporal compute dispatch '{}' references unknown pipeline {}",
                dispatch.name, dispatch.pipeline_index
            )))
        }
    }

    fn execute_clear_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        instance: &SrdInstance,
        resources: &SrdRtComputeResources<'_>,
        frame: &RenderFrame,
        program: &ComputeProgram,
    ) -> Result<()> {
        let storage = dispatch
            .resources
            .iter()
            .find(|r| r.descriptor_type == SrdDescriptorType::StorageTexture)
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD clear dispatch '{}' has no storage target",
                    dispatch.name
                ))
            })?;
        let image = resources.resolve_pool(storage)?;
        let range = dispatch.constants_range.ok_or_else(|| {
            crate::Error::InvalidInput(format!(
                "SRD clear dispatch '{}' is missing constants",
                dispatch.name
            ))
        })?;
        frame
            .shader_pass(dispatch.name.clone())
            .target_as("srd_clear_target", image)
            .push_constants(StageMask::COMPUTE, instance.constant_bytes(range))
            .compute(program, dispatch.grid_size)
    }

    fn execute_temporal_dispatch(
        &self,
        dispatch: &SrdDispatchDesc,
        instance: &SrdInstance,
        resources: &SrdRtComputeResources<'_>,
        frame: &RenderFrame,
        program: &ComputeProgram,
    ) -> Result<()> {
        // Use HistoryPool storage resource as the write target.
        let write_resource = dispatch
            .resources
            .iter()
            .find(|r| {
                r.descriptor_type == SrdDescriptorType::StorageTexture
                    && r.slot == SrdResourceSlot::HistoryPool
            })
            .ok_or_else(|| {
                crate::Error::InvalidInput(format!(
                    "SRD temporal compute dispatch '{}' has no HistoryPool storage target",
                    dispatch.name
                ))
            })?;
        let write_image = resources.resolve_pool(write_resource)?;

        let range = dispatch.constants_range.ok_or_else(|| {
            crate::Error::InvalidInput(format!(
                "SRD temporal compute dispatch '{}' is missing constants",
                dispatch.name
            ))
        })?;

        let mut pass = frame
            .shader_pass(dispatch.name.clone())
            .target_as("srd_history_write", write_image);

        for resource in &dispatch.resources {
            if resource.descriptor_type != SrdDescriptorType::Texture {
                continue;
            }
            let binding = match resource.slot {
                SrdResourceSlot::CombinedRadianceInput => "srd_current_signal",
                SrdResourceSlot::HistoryPool => "srd_history_signal",
                slot => {
                    return Err(crate::Error::InvalidInput(format!(
                        "SRD temporal compute dispatch '{}' has unsupported sampled slot {slot:?}",
                        dispatch.name
                    )));
                }
            };
            let image = match resource.slot {
                SrdResourceSlot::CombinedRadianceInput => resources.current_signal,
                SrdResourceSlot::HistoryPool => resources.resolve_pool(resource)?,
                _ => unreachable!(),
            };
            pass = pass.image(binding, image);
        }

        pass.push_constants(StageMask::COMPUTE, instance.constant_bytes(range))
            .compute(program, dispatch.grid_size)
    }
}

struct SrdRtComputeResources<'a> {
    current_signal: &'a GraphImage,
    history_read: &'a GraphImage,
    history_write: &'a GraphImage,
    history_read_pool_idx: u16,
    history_write_pool_idx: u16,
}

impl<'a> SrdRtComputeResources<'a> {
    fn resolve_pool(&self, resource: &SrdResourceDesc) -> Result<&'a GraphImage> {
        match resource.pool_index {
            Some(idx) if idx == self.history_write_pool_idx => Ok(self.history_write),
            Some(idx) if idx == self.history_read_pool_idx => Ok(self.history_read),
            other => Err(crate::Error::InvalidInput(format!(
                "SRD reference temporal compute: cannot resolve HistoryPool index {other:?}"
            ))),
        }
    }
}

fn plan_compute_instance(
    input: &GraphImage,
    history: &crate::GraphImageHistoryFrame,
    settings: SrdDenoiserSettings,
) -> Result<SrdInstance> {
    let extent = input.desc().extent;
    let size = glam::UVec2::new(extent.width.max(1), extent.height.max(1));
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(0),
    ]))?;
    instance.set_denoiser_settings(
        SrdDenoiserId::new(0),
        SrdFamilySettings::Reference(SrdReferenceSettings {
            history_frame_budget: settings.max_frames,
        }),
    )?;
    instance.set_common_settings(SrdCommonSettings {
        frame_index: history.frame_index,
        history_mode: if history.has_history {
            SrdHistoryMode::KeepAccumulating
        } else {
            SrdHistoryMode::ZeroHistory
        },
        resource_size: size,
        resource_size_prev: size,
        rect_size: size,
        rect_size_prev: size,
        ..SrdCommonSettings::default()
    })?;
    Ok(instance)
}
