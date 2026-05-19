use crate::{BufferHandle, ImageHandle, VideoSessionHandle};

/// GFX-4a: A managed video decode session with pre-allocated DPB reference images.
///
/// Created by `Device::create_video_decode_session`. The session owns:
/// - An underlying `VkVideoSessionKHR` via its `VideoSessionHandle`
/// - Pre-allocated DPB (Decoded Picture Buffer) images for reference frames
/// - An output image into which the current frame is decoded
///
/// After decoding a frame, `output_image()` can be imported into the render graph
/// as `RgState::ShaderRead` for use in post-processing or display passes.
pub struct VideoDecodeSession {
    /// Underlying Vulkan video session handle.
    pub session_handle: VideoSessionHandle,
    /// Pre-allocated DPB reference frame images (YCbCr, VIDEO_DECODE_DPB usage).
    pub dpb_images: Vec<ImageHandle>,
    /// The image into which the current frame is decoded (VIDEO_DECODE_DST usage).
    pub output_image: ImageHandle,
    pub width: u32,
    pub height: u32,
    pub codec: VideoCodec,
}

impl VideoDecodeSession {
    /// The decoded output image, ready for `frame.graph_mut(|g| g.import_image(session.output_image(), ...))`
    /// followed by use in a render pass as `RgState::ShaderRead`.
    pub fn output_image(&self) -> ImageHandle {
        self.output_image
    }
}

/// GFX-4b: A managed video encode session with an internal output bitstream buffer.
///
/// Created by `Device::create_video_encode_session`. The session owns:
/// - An underlying `VkVideoSessionKHR`
/// - An output buffer for the compressed bitstream
///
/// After encoding a frame via `PassWork::EncodeVideoFrame`, call `read_bitstream()`
/// on `Device` to copy the compressed output to a `Vec<u8>`.
pub struct VideoEncodeSession {
    /// Underlying Vulkan video session handle.
    pub session_handle: VideoSessionHandle,
    /// Buffer that receives the compressed bitstream output.
    pub output_buffer: BufferHandle,
    /// Maximum bitstream size in bytes (the output_buffer's size).
    pub max_bitstream_bytes: u64,
    pub config: VideoEncodeConfig,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VideoCodec {
    H264,
    H265,
    Av1,
    Vp9,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum VideoSessionKind {
    Decode,
    Encode,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BitRateControl {
    Cbr { bitrate_bps: u32 },
    Vbr { target_bps: u32, peak_bps: u32 },
    Cqp { qp: u32 },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum QualityPreset {
    Fast,
    Medium,
    Slow,
}

#[derive(Copy, Clone, Debug)]
pub struct VideoSessionDesc {
    pub kind: VideoSessionKind,
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub max_dpb_slots: u32,
}

impl VideoSessionDesc {
    pub fn decode(codec: VideoCodec, width: u32, height: u32) -> Self {
        Self {
            kind: VideoSessionKind::Decode,
            codec,
            width,
            height,
            max_dpb_slots: 4,
        }
    }

    pub fn encode(codec: VideoCodec, width: u32, height: u32) -> Self {
        Self {
            kind: VideoSessionKind::Encode,
            codec,
            width,
            height,
            max_dpb_slots: 2,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeFrameDesc {
    pub session: VideoSessionHandle,
    pub bitstream_buffer: BufferHandle,
    pub bitstream_offset: u64,
    pub bitstream_size: u64,
    pub output_image: ImageHandle,
    pub output_layer: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct EncodeFrameDesc {
    pub session: VideoSessionHandle,
    pub input_image: ImageHandle,
    pub output_buffer: BufferHandle,
    pub output_offset: u64,
    pub quantization_map: Option<ImageHandle>,
}

#[derive(Copy, Clone, Debug)]
pub struct VideoEncodeConfig {
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub bitrate: BitRateControl,
    pub quality: QualityPreset,
}

impl Default for VideoEncodeConfig {
    fn default() -> Self {
        Self {
            codec: VideoCodec::H265,
            width: 1920,
            height: 1080,
            bitrate: BitRateControl::Cbr {
                bitrate_bps: 10_000_000,
            },
            quality: QualityPreset::Medium,
        }
    }
}
