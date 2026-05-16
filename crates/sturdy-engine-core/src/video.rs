use crate::{BufferHandle, ImageHandle, VideoSessionHandle};

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
