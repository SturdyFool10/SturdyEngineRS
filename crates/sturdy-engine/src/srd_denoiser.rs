use crate::{Error, Format, GraphImage, GraphImageHistory, ImageDesc, RenderFrame, Result};
use glam::{Mat4, UVec2, Vec2};
use std::{collections::HashSet, mem};

mod reference_temporal_executor;
pub use reference_temporal_executor::{SrdReferenceTemporalExecutor, SrdReferenceTemporalPrograms};

/// Engine-standard temporal denoiser for sparse realtime rendering signals.
///
/// `SrdDenoiser` is the Rust-facing entry point for SRD (Sturdy Real-Time
/// Denoiser). The current implementation provides the SRD reference temporal
/// accumulation path used by the hardware path-tracing testbed. More advanced
/// SRD families can build on this module without inheriting vendor-specific API
/// names or shader binding conventions.
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
        SrdReferenceTemporalExecutor::new(programs, self.bindings).execute(
            frame,
            input,
            output_name,
            &mut self.history,
            self.settings,
        )
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdDenoiserSettings {
    pub max_frames: u32,
    pub mode: SrdDenoiserMode,
    pub current_sampler: crate::SamplerPreset,
    pub history_sampler: crate::SamplerPreset,
}

impl SrdDenoiserSettings {
    pub fn new(max_frames: u32) -> Self {
        Self {
            max_frames: max_frames.max(1),
            mode: SrdDenoiserMode::ReferenceTemporal,
            current_sampler: crate::SamplerPreset::Linear,
            history_sampler: crate::SamplerPreset::Linear,
        }
    }

    pub fn normalized(mut self) -> Self {
        self.max_frames = self.max_frames.max(1);
        self
    }

    pub fn validate(self) -> Result<()> {
        if self.max_frames == 0 {
            return Err(Error::InvalidInput(
                "SRD max accumulation frame count must be at least 1".into(),
            ));
        }
        Ok(())
    }
}

impl Default for SrdDenoiserSettings {
    fn default() -> Self {
        Self::new(64)
    }
}

/// Engine-blessed SRD denoiser modes. Names describe SturdyEngine reconstruction
/// behavior; they intentionally do not mirror third-party denoiser family names.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SrdDenoiserMode {
    /// Reference temporal accumulation for progressive samples.
    ReferenceTemporal = 0,
    /// Reserved for the engine-standard radiance stabilizer family.
    RadianceStabilizer = 1,
    /// Reserved for the engine-standard shadow stabilizer family.
    ShadowStabilizer = 2,
    /// Reserved for the engine-standard AO/directional-occlusion stabilizer family.
    OcclusionStabilizer = 3,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdTemporalBindings {
    pub current_signal: &'static str,
    pub history_signal: &'static str,
    pub current_sampler: &'static str,
    pub history_sampler: &'static str,
}

impl Default for SrdTemporalBindings {
    fn default() -> Self {
        Self {
            current_signal: "srd_current_signal",
            history_signal: "srd_history_signal",
            current_sampler: "srd_current_sampler",
            history_sampler: "srd_history_sampler",
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdTemporalConstants {
    pub frame_index: u32,
    pub has_history: u32,
    pub max_frames: u32,
    pub mode: u32,
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct SrdDenoiserId(u32);

impl SrdDenoiserId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
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

/// Engine-native history lifecycle. Distinct from vendor SDK accumulation modes
/// in both naming and explicit "zero" semantics.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SrdHistoryMode {
    /// Continue temporal integration using existing history.
    KeepAccumulating = 0,
    /// Drop history references without clearing storage; next frame bootstraps fresh.
    InvalidateHistory = 1,
    /// Drop history and explicitly zero out persistent storage this frame.
    ZeroHistory = 2,
}

/// Public input/output slot taxonomy. Suffix-based direction tagging is chosen
/// deliberately so the namespace does not echo vendor SDK `IN_*` / `OUT_*`
/// conventions.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdResourceSlot {
    MotionVectorsInput,
    NormalRoughnessInput,
    LinearDepthInput,
    MaterialInput,
    ConfidenceInput,
    DiffuseRadianceInput,
    SpecularRadianceInput,
    CombinedRadianceInput,
    OcclusionInput,
    DirectionalOcclusionInput,
    PenumbraInput,
    TranslucencyInput,
    SpectralRadianceInput,
    DiffuseRadianceOutput,
    SpecularRadianceOutput,
    CombinedRadianceOutput,
    OcclusionOutput,
    DirectionalOcclusionOutput,
    ShadowTranslucencyOutput,
    ValidationOutput,
    HistoryPool,
    ScratchPool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdNormalPacking {
    Rgba8SignedOctahedralRoughness,
    Rgba16FloatXyzRoughness,
    Rgba32FloatXyzRoughness,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdMotionVectorConvention {
    PreviousMinusCurrentPixels,
    PreviousMinusCurrentUv,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdDepthConvention {
    LinearViewDepth,
    HardwareDepth,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdSpectralLayout {
    Disabled,
    Rgb,
    FixedBins { bins: u8 },
    CompactCoefficients { coefficients: u8 },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdShaderContract {
    pub normal_packing: SrdNormalPacking,
    pub motion_vectors: SrdMotionVectorConvention,
    pub depth: SrdDepthConvention,
    pub spectral_layout: SrdSpectralLayout,
}

impl Default for SrdShaderContract {
    fn default() -> Self {
        Self {
            normal_packing: SrdNormalPacking::Rgba8SignedOctahedralRoughness,
            motion_vectors: SrdMotionVectorConvention::PreviousMinusCurrentPixels,
            depth: SrdDepthConvention::LinearViewDepth,
            spectral_layout: SrdSpectralLayout::Rgb,
        }
    }
}

impl SrdShaderContract {
    pub fn validate(self) -> Result<()> {
        match self.spectral_layout {
            SrdSpectralLayout::FixedBins { bins } if !(2..=16).contains(&bins) => {
                return Err(Error::InvalidInput(format!(
                    "SRD fixed spectral bin count must be in 2..=16, got {bins}"
                )));
            }
            SrdSpectralLayout::CompactCoefficients { coefficients }
                if !(1..=8).contains(&coefficients) =>
            {
                return Err(Error::InvalidInput(format!(
                    "SRD compact spectral coefficient count must be in 1..=8, got {coefficients}"
                )));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdCommonSettings {
    pub frame_index: u64,
    pub history_mode: SrdHistoryMode,
    pub resource_size: UVec2,
    pub resource_size_prev: UVec2,
    pub rect_size: UVec2,
    pub rect_size_prev: UVec2,
    pub camera_jitter: Vec2,
    pub camera_jitter_prev: Vec2,
    pub view_to_clip: Mat4,
    pub view_to_clip_prev: Mat4,
    pub world_to_view: Mat4,
    pub world_to_view_prev: Mat4,
    pub motion_vector_scale: Vec2,
    pub linear_depth_scale: f32,
    pub effective_range: f32,
    pub split_screen: f32,
    pub enable_validation: bool,
    pub shader_contract: SrdShaderContract,
}

impl Default for SrdCommonSettings {
    fn default() -> Self {
        Self {
            frame_index: 0,
            history_mode: SrdHistoryMode::ZeroHistory,
            resource_size: UVec2::ONE,
            resource_size_prev: UVec2::ONE,
            rect_size: UVec2::ONE,
            rect_size_prev: UVec2::ONE,
            camera_jitter: Vec2::ZERO,
            camera_jitter_prev: Vec2::ZERO,
            view_to_clip: Mat4::IDENTITY,
            view_to_clip_prev: Mat4::IDENTITY,
            world_to_view: Mat4::IDENTITY,
            world_to_view_prev: Mat4::IDENTITY,
            motion_vector_scale: Vec2::ONE,
            linear_depth_scale: 1.0,
            effective_range: 1.0,
            split_screen: 0.0,
            enable_validation: false,
            shader_contract: SrdShaderContract::default(),
        }
    }
}

impl SrdCommonSettings {
    pub fn validate(&self) -> Result<()> {
        validate_nonzero_size("resource_size", self.resource_size)?;
        validate_nonzero_size("resource_size_prev", self.resource_size_prev)?;
        validate_nonzero_size("rect_size", self.rect_size)?;
        validate_nonzero_size("rect_size_prev", self.rect_size_prev)?;
        validate_finite_vec2("camera_jitter", self.camera_jitter)?;
        validate_finite_vec2("camera_jitter_prev", self.camera_jitter_prev)?;
        validate_finite_vec2("motion_vector_scale", self.motion_vector_scale)?;
        validate_positive_finite("linear_depth_scale", self.linear_depth_scale)?;
        validate_positive_finite("effective_range", self.effective_range)?;
        if !(0.0..=1.0).contains(&self.split_screen) || !self.split_screen.is_finite() {
            return Err(Error::InvalidInput(
                "SRD split_screen must be finite and in the inclusive range [0, 1]".into(),
            ));
        }
        self.shader_contract.validate()?;
        Ok(())
    }
}

fn validate_nonzero_size(name: &str, size: UVec2) -> Result<()> {
    if size.x == 0 || size.y == 0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be non-zero in both dimensions, got {}x{}",
            size.x, size.y
        )));
    }
    Ok(())
}

fn validate_positive_finite(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || value <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be positive and finite, got {value}"
        )));
    }
    Ok(())
}

fn validate_finite_vec2(name: &str, value: Vec2) -> Result<()> {
    if !value.x.is_finite() || !value.y.is_finite() {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be finite, got ({}, {})",
            value.x, value.y
        )));
    }
    Ok(())
}

pub const SRD_TEMPORAL_CONSTANTS_SIZE: usize = mem::size_of::<SrdTemporalConstants>();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdConstantRange {
    pub offset: usize,
    pub size: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SrdConstantArena {
    bytes: Vec<u8>,
}

impl SrdConstantArena {
    pub fn push(&mut self, bytes: &[u8]) -> SrdConstantRange {
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(bytes);
        SrdConstantRange {
            offset,
            size: bytes.len(),
        }
    }

    pub fn get(&self, range: SrdConstantRange) -> &[u8] {
        &self.bytes[range.offset..range.offset + range.size]
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SrdInstance {
    desc: SrdInstanceDesc,
    common_settings: SrdCommonSettings,
    denoiser_settings: Vec<(SrdDenoiserId, SrdFamilySettings)>,
    capabilities: SrdCapabilities,
    history_pool: Vec<SrdTextureDesc>,
    scratch_pool: Vec<SrdTextureDesc>,
    history_rings: Vec<SrdHistoryRing>,
    pipelines: Vec<SrdPipelineDesc>,
    dispatches: Vec<SrdDispatchDesc>,
    constants: SrdConstantArena,
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
        texture.validate()?;
        if texture.pool != Some(SrdPoolClass::Scratch) {
            return Err(Error::InvalidInput(format!(
                "SRD scratch texture '{}' must use SrdPoolClass::Scratch",
                texture.name
            )));
        }
        if let Some(index) = self.scratch_pool.iter().position(|existing| {
            existing.slot == texture.slot && existing.downsample_factor == texture.downsample_factor
        }) {
            return Ok(index as u16);
        }
        push_pool_texture(&mut self.scratch_pool, texture)
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
            has_constants: false,
            workgroup_size: [8, 8, 1],
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
            let dispatch = SrdPassBuilder::new(
                format!("SRD Clear {}", ring.label),
                ring.denoiser_id,
                pipeline_index,
            )
            .write_pool(SrdPoolClass::History, ring.write_index)
            .grid_size([1, 1, 1])
            .build()?;
            self.push_dispatch(dispatch)?;
            pushed += 1;
        }
        Ok(pushed)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdCapabilities {
    pub temporal_history: bool,
    pub compute: bool,
    pub storage_textures: bool,
    pub half_float: bool,
    pub subgroup_ops: bool,
    pub ray_tracing_guides: bool,
    pub max_workgroup_size: [u32; 3],
}

impl Default for SrdCapabilities {
    fn default() -> Self {
        Self {
            temporal_history: true,
            compute: true,
            storage_textures: true,
            half_float: true,
            subgroup_ops: false,
            ray_tracing_guides: false,
            max_workgroup_size: [1024, 1024, 64],
        }
    }
}

impl SrdCapabilities {
    pub fn minimal_reference_temporal() -> Self {
        Self {
            temporal_history: true,
            compute: false,
            storage_textures: false,
            half_float: false,
            subgroup_ops: false,
            ray_tracing_guides: false,
            max_workgroup_size: [1, 1, 1],
        }
    }

    pub fn validate(self) -> Result<()> {
        if !self.temporal_history {
            return Err(Error::InvalidInput(
                "SRD requires temporal history support for the current implementation".into(),
            ));
        }
        if self.max_workgroup_size.iter().any(|v| *v == 0) {
            return Err(Error::InvalidInput(
                "SRD max_workgroup_size must be non-zero in all dimensions".into(),
            ));
        }
        Ok(())
    }
}

/// Per-denoiser history rotation. Each frame the `write_index` slot becomes the
/// next frame's `read_index` slot via `rotate_history_ring`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdHistoryRing {
    pub denoiser_id: SrdDenoiserId,
    pub write_index: u16,
    pub read_index: u16,
    pub label: String,
}

impl SrdHistoryRing {
    pub fn validate(&self, history_pool: &[SrdTextureDesc]) -> Result<()> {
        let len = history_pool.len();
        if self.write_index as usize >= len || self.read_index as usize >= len {
            return Err(Error::InvalidInput(format!(
                "SRD history ring '{}' references pool indices {} and {} but history pool has {len} textures",
                self.label, self.write_index, self.read_index
            )));
        }
        if self.write_index == self.read_index {
            return Err(Error::InvalidInput(format!(
                "SRD history ring '{}' write and read indices must differ",
                self.label
            )));
        }
        if self.label.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD history ring label must not be empty".into(),
            ));
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SrdSignalMomentsConstants {
    pub frame_index: u32,
    pub history_length: u32,
    pub variance_window_radius: f32,
    pub _pad: u32,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdVarianceSettings {
    pub track_luminance_moments: bool,
    pub track_signal_variance: bool,
    pub neighborhood_radius: f32,
}

impl Default for SrdVarianceSettings {
    fn default() -> Self {
        Self {
            track_luminance_moments: true,
            track_signal_variance: true,
            neighborhood_radius: 1.5,
        }
    }
}

impl SrdVarianceSettings {
    pub fn validate(self) -> Result<()> {
        validate_nonnegative_finite("variance neighborhood_radius", self.neighborhood_radius)
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdHistoryRejectionSettings {
    pub depth_threshold: f32,
    pub normal_threshold: f32,
    pub roughness_threshold: f32,
    pub material_mismatch_rejects: bool,
}

impl Default for SrdHistoryRejectionSettings {
    fn default() -> Self {
        Self {
            depth_threshold: 0.02,
            normal_threshold: 0.2,
            roughness_threshold: 0.25,
            material_mismatch_rejects: true,
        }
    }
}

impl SrdHistoryRejectionSettings {
    pub fn validate(self) -> Result<()> {
        validate_nonnegative_finite("history rejection depth_threshold", self.depth_threshold)?;
        validate_nonnegative_finite("history rejection normal_threshold", self.normal_threshold)?;
        validate_nonnegative_finite(
            "history rejection roughness_threshold",
            self.roughness_threshold,
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdOutlierClampSettings {
    pub enabled: bool,
    pub luminance_sigma: f32,
    pub max_relative_luminance: f32,
}

impl Default for SrdOutlierClampSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            luminance_sigma: 4.0,
            max_relative_luminance: 16.0,
        }
    }
}

impl SrdOutlierClampSettings {
    pub fn validate(self) -> Result<()> {
        validate_positive_finite("outlier clamp luminance_sigma", self.luminance_sigma)?;
        validate_positive_finite(
            "outlier clamp max_relative_luminance",
            self.max_relative_luminance,
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SrdFamilySettings {
    Reference(SrdReferenceSettings),
    Radiance(SrdRadianceSettings),
    Shadow(SrdShadowSettings),
    Occlusion(SrdOcclusionSettings),
}

impl SrdFamilySettings {
    pub fn default_for_mode(mode: SrdDenoiserMode) -> Self {
        match mode {
            SrdDenoiserMode::ReferenceTemporal => Self::Reference(SrdReferenceSettings::default()),
            SrdDenoiserMode::RadianceStabilizer => Self::Radiance(SrdRadianceSettings::default()),
            SrdDenoiserMode::ShadowStabilizer => Self::Shadow(SrdShadowSettings::default()),
            SrdDenoiserMode::OcclusionStabilizer => {
                Self::Occlusion(SrdOcclusionSettings::default())
            }
        }
    }

    pub fn mode(self) -> SrdDenoiserMode {
        match self {
            Self::Reference(_) => SrdDenoiserMode::ReferenceTemporal,
            Self::Radiance(_) => SrdDenoiserMode::RadianceStabilizer,
            Self::Shadow(_) => SrdDenoiserMode::ShadowStabilizer,
            Self::Occlusion(_) => SrdDenoiserMode::OcclusionStabilizer,
        }
    }

    pub fn validate(self) -> Result<()> {
        match self {
            Self::Reference(settings) => settings.validate(),
            Self::Radiance(settings) => settings.validate(),
            Self::Shadow(settings) => settings.validate(),
            Self::Occlusion(settings) => settings.validate(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdReferenceSettings {
    pub history_frame_budget: u32,
}

impl Default for SrdReferenceSettings {
    fn default() -> Self {
        Self {
            history_frame_budget: 64,
        }
    }
}

impl SrdReferenceSettings {
    pub fn validate(self) -> Result<()> {
        if self.history_frame_budget == 0 {
            return Err(Error::InvalidInput(
                "SRD reference history_frame_budget must be at least 1".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdRadianceSettings {
    pub history_frame_budget: u32,
    pub fast_history_budget: u32,
    pub spatial_radius: f32,
    pub variance: SrdVarianceSettings,
    pub history_rejection: SrdHistoryRejectionSettings,
    pub outlier_clamp: SrdOutlierClampSettings,
}

impl Default for SrdRadianceSettings {
    fn default() -> Self {
        Self {
            history_frame_budget: 64,
            fast_history_budget: 8,
            spatial_radius: 3.0,
            variance: SrdVarianceSettings::default(),
            history_rejection: SrdHistoryRejectionSettings::default(),
            outlier_clamp: SrdOutlierClampSettings::default(),
        }
    }
}

impl SrdRadianceSettings {
    pub fn validate(self) -> Result<()> {
        validate_frame_count("radiance history_frame_budget", self.history_frame_budget)?;
        validate_frame_count("radiance fast_history_budget", self.fast_history_budget)?;
        validate_nonnegative_finite("radiance spatial_radius", self.spatial_radius)?;
        self.variance.validate()?;
        self.history_rejection.validate()?;
        self.outlier_clamp.validate()
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdShadowSettings {
    pub stabilization_frame_budget: u32,
    pub plane_offset_tolerance: f32,
    pub sun_direction: [f32; 3],
}

impl Default for SrdShadowSettings {
    fn default() -> Self {
        Self {
            stabilization_frame_budget: 16,
            plane_offset_tolerance: 0.02,
            sun_direction: [0.0, -1.0, 0.0],
        }
    }
}

impl SrdShadowSettings {
    pub fn validate(self) -> Result<()> {
        validate_positive_finite("shadow plane_offset_tolerance", self.plane_offset_tolerance)?;
        if !self.sun_direction.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidInput(
                "SRD shadow sun_direction must contain finite components".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SrdOcclusionSettings {
    pub history_frame_budget: u32,
    pub spatial_radius: f32,
    pub normal_weight_power: f32,
}

impl Default for SrdOcclusionSettings {
    fn default() -> Self {
        Self {
            history_frame_budget: 32,
            spatial_radius: 2.0,
            normal_weight_power: 8.0,
        }
    }
}

impl SrdOcclusionSettings {
    pub fn validate(self) -> Result<()> {
        validate_frame_count("occlusion history_frame_budget", self.history_frame_budget)?;
        validate_nonnegative_finite("occlusion spatial_radius", self.spatial_radius)?;
        validate_positive_finite("occlusion normal_weight_power", self.normal_weight_power)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdDescriptorType {
    Texture,
    StorageTexture,
    UniformBuffer,
    Sampler,
}

/// Texture pool classification. `History` slots persist across frames and are
/// SRD-owned; `Scratch` slots may be aliased and reused outside SRD between
/// dispatches.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SrdPoolClass {
    History,
    Scratch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdTextureDesc {
    pub name: String,
    pub debug_label: String,
    pub slot: SrdResourceSlot,
    pub format: Format,
    pub pool: Option<SrdPoolClass>,
    pub downsample_factor: u32,
}

impl SrdTextureDesc {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD texture name must not be empty".into(),
            ));
        }
        if self.debug_label.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "SRD texture '{}' debug_label must not be empty",
                self.name
            )));
        }
        if self.downsample_factor == 0 {
            return Err(Error::InvalidInput(format!(
                "SRD texture '{}' downsample_factor must be at least 1",
                self.name
            )));
        }
        validate_slot_format(self.slot, self.format)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdResourceFormatDesc {
    pub slot: SrdResourceSlot,
    pub format: Format,
}

impl SrdResourceFormatDesc {
    pub fn validate(self) -> Result<()> {
        validate_slot_format(self.slot, self.format)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdResourceDesc {
    pub descriptor_type: SrdDescriptorType,
    pub slot: SrdResourceSlot,
    pub pool_index: Option<u16>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SrdReferenceTemporalPipelines {
    pub temporal: usize,
    pub clear: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdPipelineDesc {
    pub name: String,
    pub debug_label: String,
    pub shader_label: String,
    pub has_constants: bool,
    pub workgroup_size: [u32; 3],
}

impl SrdPipelineDesc {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD pipeline name must not be empty".into(),
            ));
        }
        if self.debug_label.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "SRD pipeline '{}' debug_label must not be empty",
                self.name
            )));
        }
        if self.shader_label.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "SRD pipeline '{}' shader_label must not be empty",
                self.name
            )));
        }
        if self.workgroup_size.iter().any(|v| *v == 0) {
            return Err(Error::InvalidInput(format!(
                "SRD pipeline '{}' workgroup_size must be non-zero in all dimensions",
                self.name
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SrdDispatchDesc {
    pub name: String,
    pub denoiser_id: SrdDenoiserId,
    pub pipeline_index: usize,
    pub resources: Vec<SrdResourceDesc>,
    pub constants_size: usize,
    pub constants_range: Option<SrdConstantRange>,
    /// Set when this dispatch's constant bytes are bit-identical to the
    /// immediately preceding dispatch. Allows executors to skip re-upload.
    pub reuses_previous_constants: bool,
    pub grid_size: [u32; 3],
}

impl SrdDispatchDesc {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::InvalidInput(
                "SRD dispatch name must not be empty".into(),
            ));
        }
        if self.grid_size.iter().any(|v| *v == 0) {
            return Err(Error::InvalidInput(format!(
                "SRD dispatch '{}' grid_size must be non-zero in all dimensions",
                self.name
            )));
        }
        Ok(())
    }
}

pub struct SrdPassBuilder {
    dispatch: SrdDispatchDesc,
}

impl SrdPassBuilder {
    pub fn new(name: impl Into<String>, denoiser_id: SrdDenoiserId, pipeline_index: usize) -> Self {
        Self {
            dispatch: SrdDispatchDesc {
                name: name.into(),
                denoiser_id,
                pipeline_index,
                resources: Vec::new(),
                constants_size: 0,
                constants_range: None,
                reuses_previous_constants: false,
                grid_size: [1, 1, 1],
            },
        }
    }

    pub fn read(mut self, slot: SrdResourceSlot) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::Texture,
            slot,
            pool_index: None,
        });
        self
    }

    pub fn write(mut self, slot: SrdResourceSlot) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::StorageTexture,
            slot,
            pool_index: None,
        });
        self
    }

    pub fn read_pool(mut self, pool: SrdPoolClass, index: u16) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::Texture,
            slot: pool.resource_slot(),
            pool_index: Some(index),
        });
        self
    }

    pub fn write_pool(mut self, pool: SrdPoolClass, index: u16) -> Self {
        self.dispatch.resources.push(SrdResourceDesc {
            descriptor_type: SrdDescriptorType::StorageTexture,
            slot: pool.resource_slot(),
            pool_index: Some(index),
        });
        self
    }

    pub fn constants_size(mut self, constants_size: usize) -> Self {
        self.dispatch.constants_size = constants_size;
        self
    }

    pub fn constants_range(mut self, constants_range: SrdConstantRange) -> Self {
        self.dispatch.constants_size = constants_range.size;
        self.dispatch.constants_range = Some(constants_range);
        self
    }

    pub fn reuses_previous_constants(mut self, reuses_previous: bool) -> Self {
        self.dispatch.reuses_previous_constants = reuses_previous;
        self
    }

    pub fn grid_size(mut self, grid_size: [u32; 3]) -> Self {
        self.dispatch.grid_size = grid_size;
        self
    }

    pub fn build(self) -> Result<SrdDispatchDesc> {
        self.dispatch.validate()?;
        Ok(self.dispatch)
    }
}

impl SrdPoolClass {
    fn resource_slot(self) -> SrdResourceSlot {
        match self {
            Self::History => SrdResourceSlot::HistoryPool,
            Self::Scratch => SrdResourceSlot::ScratchPool,
        }
    }
}

fn reference_grid_size(rect_size: UVec2) -> [u32; 3] {
    [
        rect_size.x.div_ceil(8).max(1),
        rect_size.y.div_ceil(8).max(1),
        1,
    ]
}

fn validate_slot_format(slot: SrdResourceSlot, format: Format) -> Result<()> {
    if format == Format::Unknown {
        return Err(Error::InvalidInput(format!(
            "SRD resource slot {slot:?} requires a known texture format"
        )));
    }
    let valid = match slot {
        SrdResourceSlot::MotionVectorsInput => matches!(
            format,
            Format::Rg8Unorm | Format::Rgba16Float | Format::Rgba32Float
        ),
        SrdResourceSlot::NormalRoughnessInput | SrdResourceSlot::MaterialInput => {
            matches!(
                format,
                Format::Rgba8Unorm | Format::Rgba16Float | Format::Rgba32Float
            )
        }
        SrdResourceSlot::LinearDepthInput => matches!(
            format,
            Format::R8Unorm | Format::Rgba16Float | Format::Rgba32Float | Format::Depth32Float
        ),
        SrdResourceSlot::ConfidenceInput
        | SrdResourceSlot::OcclusionInput
        | SrdResourceSlot::DirectionalOcclusionInput
        | SrdResourceSlot::PenumbraInput => matches!(
            format,
            Format::R8Unorm | Format::Rgba16Float | Format::Rgba32Float
        ),
        SrdResourceSlot::DiffuseRadianceInput
        | SrdResourceSlot::SpecularRadianceInput
        | SrdResourceSlot::CombinedRadianceInput
        | SrdResourceSlot::TranslucencyInput
        | SrdResourceSlot::SpectralRadianceInput
        | SrdResourceSlot::DiffuseRadianceOutput
        | SrdResourceSlot::SpecularRadianceOutput
        | SrdResourceSlot::CombinedRadianceOutput
        | SrdResourceSlot::ShadowTranslucencyOutput
        | SrdResourceSlot::ValidationOutput
        | SrdResourceSlot::HistoryPool
        | SrdResourceSlot::ScratchPool => matches!(
            format,
            Format::Rgba16Float | Format::Rgba32Float | Format::Rgba8Unorm
        ),
        SrdResourceSlot::OcclusionOutput | SrdResourceSlot::DirectionalOcclusionOutput => {
            matches!(
                format,
                Format::R8Unorm | Format::Rgba16Float | Format::Rgba32Float
            )
        }
    };
    if !valid {
        return Err(Error::InvalidInput(format!(
            "SRD resource slot {slot:?} does not accept format {format:?}"
        )));
    }
    Ok(())
}

fn push_pool_texture(pool: &mut Vec<SrdTextureDesc>, texture: SrdTextureDesc) -> Result<u16> {
    let index = pool.len();
    if index > u16::MAX as usize {
        return Err(Error::InvalidInput(
            "SRD texture pool cannot contain more than 65535 textures".into(),
        ));
    }
    pool.push(texture);
    Ok(index as u16)
}

fn validate_frame_count(name: &str, value: u32) -> Result<()> {
    if value == 0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be at least 1"
        )));
    }
    Ok(())
}

fn validate_nonnegative_finite(name: &str, value: f32) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        return Err(Error::InvalidInput(format!(
            "SRD {name} must be non-negative and finite, got {value}"
        )));
    }
    Ok(())
}

#[deprecated(
    since = "0.1.0",
    note = "use SrdDenoiser; SRD is the SturdyEngine-standard denoiser API"
)]
pub type RealtimeRayTracingDenoiser = SrdDenoiser;

#[cfg(test)]
#[path = "srd_denoiser_tests.rs"]
mod srd_denoiser_tests;
