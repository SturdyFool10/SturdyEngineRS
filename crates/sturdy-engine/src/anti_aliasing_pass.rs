use crate::{
    AntiAliasingMode, Engine, Format, GraphImage, ImageDesc, ImageDimension, ImageUsage,
    RenderFrame, Result, ShaderProgram, StageMask, shader_program::builtin_shader_path,
};
use glam::Mat4;
use parking_lot::Mutex;
use sturdy_engine_core::Extent3d;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct AntiAliasingConstants {
    extent_and_history_jitter: [f32; 4],
    fxaa_params: [f32; 4],
    taa_params: [f32; 4],
    mode: u32,
    has_history: u32,
    _pad: [u32; 2],
}

pub struct AntiAliasingPass {
    program: ShaderProgram,
    passthrough: ShaderProgram,
    history: Mutex<AntiAliasingHistory>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct AntiAliasingHistoryKey {
    width: u32,
    height: u32,
    format: Format,
}

#[derive(Debug, Default)]
struct AntiAliasingHistory {
    key: Option<AntiAliasingHistoryKey>,
    frame_index: u64,
    previous_jitter_uv: [f32; 2],
}

#[derive(Copy, Clone, Debug)]
struct AntiAliasingFrameState {
    history_index: u64,
    has_history: bool,
    history_jitter_uv: [f32; 2],
}

impl AntiAliasingPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            program: ShaderProgram::load_fragment(
                engine,
                builtin_shader_path("anti_aliasing.slang"),
            )?,
            passthrough: ShaderProgram::passthrough(engine)?,
            history: Mutex::new(AntiAliasingHistory::default()),
        })
    }

    pub fn execute(
        &self,
        frame: &RenderFrame,
        source: &GraphImage,
        mode: AntiAliasingMode,
    ) -> Result<GraphImage> {
        self.execute_with_motion_vectors(frame, source, None, mode)
    }

    pub fn execute_with_motion_vectors(
        &self,
        frame: &RenderFrame,
        source: &GraphImage,
        motion_vectors: Option<&GraphImage>,
        mode: AntiAliasingMode,
    ) -> Result<GraphImage> {
        self.execute_with_motion_vectors_and_jitter(frame, source, motion_vectors, mode, None)
    }

    /// Run the AA pass with an explicit current-frame jitter UV.
    ///
    /// Pass `current_jitter_uv` when the geometry passes were rendered with a
    /// jittered projection (the standard TAA pipeline).  The AA pass uses it
    /// to compute the offset between the previous and current frame's history
    /// samples so the temporal reprojection stays sub-pixel aligned.
    ///
    /// When `current_jitter_uv` is `None`, the pass falls back to its internal
    /// Halton-sequence frame counter — legacy behaviour for callers that have
    /// not yet plumbed camera jitter end-to-end.
    pub fn execute_with_motion_vectors_and_jitter(
        &self,
        frame: &RenderFrame,
        source: &GraphImage,
        motion_vectors: Option<&GraphImage>,
        mode: AntiAliasingMode,
        current_jitter_uv: Option<[f32; 2]>,
    ) -> Result<GraphImage> {
        let output = self.output_image(frame, source)?;
        let frame_state = self.next_history_frame(source, current_jitter_uv);
        let history_read = self.history_image(frame, source, frame_state.history_index % 2)?;
        let history_write =
            self.history_image(frame, source, (frame_state.history_index + 1) % 2)?;

        source.register_as("source");
        history_read.register_as("history_source");
        motion_vectors
            .unwrap_or(source)
            .register_as("motion_source");
        frame.set_sampler("source_sampler", crate::SamplerPreset::Linear);
        frame.set_sampler("history_sampler", crate::SamplerPreset::Linear);
        frame.set_sampler("motion_sampler", crate::SamplerPreset::Linear);
        output.execute_shader_with_push_constants(
            &self.program,
            StageMask::FRAGMENT,
            bytemuck::bytes_of(&constants_for(
                source,
                mode,
                frame_state.has_history && mode.uses_taa(),
                motion_vectors.is_some(),
                frame_state.history_jitter_uv,
            )),
        )?;
        history_write.blit_from(&output, &self.passthrough)?;
        Ok(output)
    }

    fn output_image(&self, frame: &RenderFrame, source: &GraphImage) -> Result<GraphImage> {
        let src = source.desc();
        let desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: src.extent.width,
                height: src.extent.height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: src.format,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: Some("anti-aliasing-output"),
            compression: Default::default(),
            min_lod_bits: None,
            msaa_resolve_to_single_sampled: false,
            drm_format_modifier: None,
        };
        frame.image("anti_aliasing_output", desc)
    }

    fn history_image(
        &self,
        frame: &RenderFrame,
        source: &GraphImage,
        index: u64,
    ) -> Result<GraphImage> {
        let src = source.desc();
        let desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: src.extent.width,
                height: src.extent.height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: src.format,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: Some("anti-aliasing-history"),
            compression: Default::default(),
            min_lod_bits: None,
            msaa_resolve_to_single_sampled: false,
            drm_format_modifier: None,
        };
        frame.image(format!("anti_aliasing_history_{index}"), desc)
    }

    fn next_history_frame(
        &self,
        source: &GraphImage,
        explicit_jitter: Option<[f32; 2]>,
    ) -> AntiAliasingFrameState {
        let desc = source.desc();
        let key = AntiAliasingHistoryKey {
            width: desc.extent.width,
            height: desc.extent.height,
            format: desc.format,
        };
        //panic allowed, reason = "poisoned mutex is unrecoverable"
        let mut history = self.history.lock();
        if history.key != Some(key) {
            history.key = Some(key);
            history.frame_index = 0;
            history.previous_jitter_uv = [0.0, 0.0];
        }
        let frame_index = history.frame_index;
        let current_jitter_uv = explicit_jitter.unwrap_or_else(|| {
            taa_jitter_uv(
                frame_index,
                desc.extent.width.max(1),
                desc.extent.height.max(1),
            )
        });
        let history_jitter_uv = [
            history.previous_jitter_uv[0] - current_jitter_uv[0],
            history.previous_jitter_uv[1] - current_jitter_uv[1],
        ];
        history.frame_index = history.frame_index.saturating_add(1);
        history.previous_jitter_uv = current_jitter_uv;
        AntiAliasingFrameState {
            history_index: frame_index,
            has_history: frame_index >= 2,
            history_jitter_uv,
        }
    }
}

fn constants_for(
    source: &GraphImage,
    mode: AntiAliasingMode,
    has_history: bool,
    use_motion_vectors: bool,
    history_jitter_uv: [f32; 2],
) -> AntiAliasingConstants {
    let desc = source.desc();
    let inverse_extent = [
        1.0 / desc.extent.width.max(1) as f32,
        1.0 / desc.extent.height.max(1) as f32,
    ];
    let mut constants = AntiAliasingConstants {
        extent_and_history_jitter: [
            inverse_extent[0],
            inverse_extent[1],
            history_jitter_uv[0],
            history_jitter_uv[1],
        ],
        fxaa_params: [0.75, 0.125, 0.0312, 1.0],
        taa_params: [0.9, 1.0, 1.0, if use_motion_vectors { 1.0 } else { 0.0 }],
        mode: 0,
        has_history: u32::from(has_history),
        _pad: [0, 0],
    };
    match mode {
        AntiAliasingMode::Off => constants.mode = 0,
        AntiAliasingMode::Msaa(_) => constants.mode = 1,
        AntiAliasingMode::Fxaa(settings) => {
            constants.mode = 2;
            constants.fxaa_params[0] = settings.subpixel_quality;
            constants.fxaa_params[1] = settings.edge_threshold;
            constants.fxaa_params[2] = settings.edge_threshold_min;
        }
        AntiAliasingMode::Taa(settings) => {
            constants.mode = 3;
            constants.taa_params[0] = settings.history_weight;
            constants.taa_params[1] = settings.jitter_scale;
            constants.taa_params[2] = settings.clamp_factor;
        }
        AntiAliasingMode::FxaaTaa { fxaa, taa } => {
            constants.mode = 4;
            constants.fxaa_params[0] = fxaa.subpixel_quality;
            constants.fxaa_params[1] = fxaa.edge_threshold;
            constants.fxaa_params[2] = fxaa.edge_threshold_min;
            constants.taa_params[0] = taa.history_weight;
            constants.taa_params[1] = taa.jitter_scale;
            constants.taa_params[2] = taa.clamp_factor;
        }
    }
    constants
}

pub fn taa_jitter_uv(frame_index: u64, width: u32, height: u32) -> [f32; 2] {
    let x = halton(frame_index + 1, 2) - 0.5;
    let y = halton(frame_index + 1, 3) - 0.5;
    [x / width.max(1) as f32, y / height.max(1) as f32]
}

pub fn taa_jittered_projection(mut projection: Mat4, jitter_uv: [f32; 2]) -> Mat4 {
    projection.w_axis.x += jitter_uv[0] * 2.0;
    projection.w_axis.y += jitter_uv[1] * 2.0;
    projection
}

fn halton(mut index: u64, base: u64) -> f32 {
    let mut result = 0.0;
    let mut fraction = 1.0 / base as f32;
    while index > 0 {
        result += (index % base) as f32 * fraction;
        index /= base;
        fraction /= base as f32;
    }
    result
}
