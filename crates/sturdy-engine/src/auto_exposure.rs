// Auto-exposure pipeline.
//
// Two compute passes:
//   1. histogram — bins the pre-exposure HDR scene into 256 log-luminance bins
//   2. adapt     — reduces the histogram to a target EV, blends toward the
//                  previous frame's EV at the configured speed, writes the
//                  result into a persistent exposure state buffer, and clears
//                  the histogram for the next frame
//
// The exposure state buffer holds four floats:
//   [0] current adapted EV100 — read by the tonemap shader
//   [1] last target EV100     — diagnostics
//   [2] sample count          — diagnostics
//   [3] avg linear luminance  — diagnostics

use std::sync::Mutex;

use crate::{
    AutoExposureConfig, Buffer, BufferDesc, BufferUsage, ComputeProgram, Engine, GraphImage,
    RenderFrame, Result, SamplerPreset, shader_program::builtin_shader_path,
};

const HISTOGRAM_BINS: u64 = 256;
const HISTOGRAM_BYTES: u64 = HISTOGRAM_BINS * 4;
const EXPOSURE_STATE_FLOATS: u64 = 4;
const EXPOSURE_STATE_BYTES: u64 = EXPOSURE_STATE_FLOATS * 4;

const DEFAULT_MIN_LOG_LUMA: f32 = -10.0;
const DEFAULT_MAX_LOG_LUMA: f32 = 12.0;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct HistogramConstants {
    inv_resolution: [f32; 2],
    min_log_luma: f32,
    inv_log_luma_range: f32,
    resolution: [u32; 2],
    /// Linear luminance floor: pixels below this go to bin 0 (excluded from mean).
    min_luma: f32,
    _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct AdaptConstants {
    min_log_luma: f32,
    log_luma_range: f32,
    min_ev: f32,
    max_ev: f32,
    target_percentile: f32,
    adaptation_rate: f32,
    dt_seconds: f32,
    _pad: f32,
}

/// GPU-driven auto-exposure (luminance histogram + adaptation).
///
/// Owns two persistent storage buffers — the histogram and the exposure state
/// — and the two compute programs that operate on them.  Reuse one pass across
/// frames; do not allocate a new one each frame.
///
/// The pass writes the adapted EV100 into `exposure_state_buffer()`, which can
/// be bound by the tonemap shader as `RWStructuredBuffer<float> exposure_state`
/// (or read-only `StructuredBuffer<float>`).  Tone-map shader element `[0]` is
/// the value to apply to scene colour as `exp2(ev - 14)` (or via the standard
/// EV100 → exposure mapping the user prefers).
pub struct AutoExposurePass {
    histogram_program: ComputeProgram,
    adapt_program: ComputeProgram,
    histogram_buffer: Buffer,
    exposure_buffer: Buffer,
    initialized: Mutex<bool>,
}

impl AutoExposurePass {
    pub fn new(engine: &Engine) -> Result<Self> {
        let histogram_program =
            ComputeProgram::load(engine, builtin_shader_path("auto_exposure_histogram.slang"))?;
        let adapt_program =
            ComputeProgram::load(engine, builtin_shader_path("auto_exposure_adapt.slang"))?;

        let histogram_buffer = engine.create_buffer(BufferDesc {
            size: HISTOGRAM_BYTES,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        let exposure_buffer = engine.create_buffer(BufferDesc {
            size: EXPOSURE_STATE_BYTES,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;

        // Zero both buffers so the first adapt dispatch reads zero rather than
        // uninitialised memory.
        let zeros_hist = vec![0u8; HISTOGRAM_BYTES as usize];
        histogram_buffer.write(0, &zeros_hist)?;
        let zeros_state = vec![0u8; EXPOSURE_STATE_BYTES as usize];
        exposure_buffer.write(0, &zeros_state)?;

        Ok(Self {
            histogram_program,
            adapt_program,
            histogram_buffer,
            exposure_buffer,
            initialized: Mutex::new(false),
        })
    }

    /// Persistent storage buffer holding `[adapted_ev, target_ev, sample_count,
    /// avg_luma]`.  Bind it in the tonemap shader as
    /// `RWStructuredBuffer<float> exposure_state` (or read-only) to consume the
    /// GPU-written exposure value without a CPU round trip.
    pub fn exposure_state_buffer(&self) -> &Buffer {
        &self.exposure_buffer
    }

    /// Persistent histogram buffer (256 `uint` bins).  Exposed for tools that
    /// want to read or visualise the raw histogram.
    pub fn histogram_buffer(&self) -> &Buffer {
        &self.histogram_buffer
    }

    /// Run the histogram + adapt passes against the provided HDR scene image.
    ///
    /// `dt_seconds` is the frame delta time; the adapt pass uses it to convert
    /// `config.adaptation_speed` into a per-frame blend factor.
    ///
    /// The scene image is bound under the name `scene_color`; the histogram
    /// buffer is bound as `histogram` and the exposure state buffer as
    /// `exposure_state`.  A linear sampler is bound as `auto_exposure_sampler`.
    pub fn execute(
        &self,
        frame: &RenderFrame,
        hdr_scene: &GraphImage,
        config: &AutoExposureConfig,
        dt_seconds: f32,
    ) -> Result<()> {
        if !config.enabled {
            return Ok(());
        }

        let ext = hdr_scene.desc().extent;
        let width = ext.width.max(1);
        let height = ext.height.max(1);

        let min_log_luma = DEFAULT_MIN_LOG_LUMA;
        let max_log_luma = DEFAULT_MAX_LOG_LUMA;
        let log_luma_range = max_log_luma - min_log_luma;
        let inv_log_luma_range = if log_luma_range > 0.0 {
            1.0 / log_luma_range
        } else {
            1.0
        };

        hdr_scene.register_as("scene_color");
        frame.set_sampler("auto_exposure_sampler", SamplerPreset::Linear);
        frame.bind_buffer("histogram", &self.histogram_buffer);

        let hist_constants = HistogramConstants {
            inv_resolution: [1.0 / width as f32, 1.0 / height as f32],
            min_log_luma,
            inv_log_luma_range,
            resolution: [width, height],
            min_luma: config.metering_floor.max(0.0),
            _pad: 0,
        };
        let groups_x = (width + 15) / 16;
        let groups_y = (height + 15) / 16;
        frame.dispatch_compute_auto(
            "auto_exposure_histogram",
            &self.histogram_program,
            &hist_constants,
            [groups_x, groups_y, 1],
        )?;

        // adapt pass — clears the histogram for next frame and writes EV.
        frame.bind_buffer("histogram", &self.histogram_buffer);
        frame.bind_buffer("exposure_state", &self.exposure_buffer);

        // On the very first frame we have no previous EV, so jump directly to
        // the target rather than blending from zero.  Subsequent frames blend.
        let mut initialized = self.initialized.lock().unwrap_or_else(|p| p.into_inner());
        let adaptation_rate = if !*initialized {
            *initialized = true;
            1.0
        } else {
            let speed = config.adaptation_speed.max(0.0);
            1.0 - (-dt_seconds.max(0.0) * speed).exp()
        };

        let adapt_constants = AdaptConstants {
            min_log_luma,
            log_luma_range,
            min_ev: config.min_ev,
            max_ev: config.max_ev,
            target_percentile: config.target_percentile.clamp(0.0, 1.0),
            adaptation_rate,
            dt_seconds: dt_seconds.max(0.0),
            _pad: 0.0,
        };
        frame.dispatch_compute_auto(
            "auto_exposure_adapt",
            &self.adapt_program,
            &adapt_constants,
            [1, 1, 1],
        )?;

        Ok(())
    }

    /// Read back the current adapted EV100 from the exposure state buffer.
    ///
    /// **This causes a CPU↔GPU sync stall** and is intended for diagnostics or
    /// for callers that want to feed the EV back into a host-side tonemap
    /// constant.  The fast path is to bind `exposure_state_buffer()` directly
    /// in the tonemap shader instead.
    ///
    /// Returns `None` if the buffer has not been written yet.
    pub fn read_back_state(&self) -> Result<Option<AutoExposureReadback>> {
        let mut bytes = [0u8; EXPOSURE_STATE_BYTES as usize];
        self.exposure_buffer.read(0, &mut bytes)?;
        let initialized = *self.initialized.lock().unwrap_or_else(|p| p.into_inner());
        if !initialized {
            return Ok(None);
        }
        let mut values = [0f32; 4];
        for (i, chunk) in bytes.chunks_exact(4).enumerate() {
            //panic allowed, reason = "fixed-size 16-byte buffer chunked into four 4-byte floats"
            values[i] = f32::from_le_bytes(chunk.try_into().expect("4 bytes"));
        }
        Ok(Some(AutoExposureReadback {
            adapted_ev: values[0],
            target_ev: values[1],
            sample_count: values[2],
            avg_luminance: values[3],
        }))
    }
}

/// Diagnostic snapshot read back from the exposure state buffer.
#[derive(Copy, Clone, Debug)]
pub struct AutoExposureReadback {
    /// The exposure value (EV100) currently being applied; this is what the
    /// tonemap shader reads.
    pub adapted_ev: f32,
    /// The unblended target EV100 derived from this frame's histogram.
    pub target_ev: f32,
    /// Number of pixels that contributed to the histogram (sanity check).
    pub sample_count: f32,
    /// Mean linear luminance derived from the histogram (sanity check).
    pub avg_luminance: f32,
}
