use std::path::Path;

use sturdy_engine_core::{
    Access, BufferHandle, BufferUse, CopyImageToBufferDesc, ImageHandle, ImageUse, PassDesc,
    PassWork, QueueType, RgState, SubresourceRange,
};

use crate::{Buffer, BufferDesc, BufferUsage, Engine, Format, Frame, ImageRef, Result};

/// Captures a GPU image to a CPU-readable buffer and optionally saves it to disk.
///
/// # Usage
///
/// ```ignore
/// // At frame-record time (before flush/wait):
/// let capture = ScreenshotCapture::new(&engine, width, height, format)?;
/// capture.record_readback(&mut frame, &scene_color_image)?;
/// frame.flush()?;
/// frame.wait()?;
///
/// // After wait — the buffer is ready on CPU:
/// capture.save_png("screenshot.png")?;
/// ```
pub struct ScreenshotCapture {
    engine: Engine,
    buffer: Buffer,
    width: u32,
    height: u32,
    format: Format,
}

impl ScreenshotCapture {
    /// Allocate a readback buffer sized for one full image of `width × height` in `format`.
    ///
    /// Supports `Rgba8Unorm`, `Bgra8Unorm`, `Rgba8Srgb`, `Bgra8Srgb`, and `Rgba16Float`.
    pub fn new(engine: &Engine, width: u32, height: u32, format: Format) -> Result<Self> {
        let bytes_per_pixel = bytes_per_pixel(format)?;
        let size = width as u64 * height as u64 * bytes_per_pixel as u64;
        let buffer = engine.create_buffer(BufferDesc {
            size,
            usage: BufferUsage::COPY_DST,
        })?;
        Ok(Self {
            engine: engine.clone(),
            buffer,
            width,
            height,
            format,
        })
    }

    /// Add a GPU image-to-buffer copy pass to `frame`.
    ///
    /// Call this while recording the frame, then `flush()` and `wait()` before
    /// reading pixels or saving.
    pub fn record_readback(&self, frame: &mut Frame, source: &impl ImageRef) -> Result<()> {
        let image_handle: ImageHandle = source.image_handle();
        let buffer_handle: BufferHandle = self.buffer.handle();
        frame.add_pass(PassDesc {
            name: "screenshot-readback".to_owned(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::CopyImageToBuffer(CopyImageToBufferDesc {
                image: image_handle,
                buffer: buffer_handle,
                buffer_offset: 0,
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: self.width,
                height: self.height,
                depth: 1,
            }),
            reads: vec![ImageUse {
                image: image_handle,
                access: Access::Read,
                state: RgState::CopySrc,
                subresource: SubresourceRange::WHOLE,
            }],
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: vec![BufferUse {
                buffer: buffer_handle,
                access: Access::Write,
                state: RgState::CopyDst,
                offset: 0,
                size: self.buffer.desc().size,
            }],
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    /// Read the captured pixels back to the CPU as raw bytes.
    ///
    /// Must be called after `frame.flush()` and `frame.wait()`.
    /// The returned bytes are in the native format of the source image.
    pub fn read_raw_pixels(&self) -> Result<Vec<u8>> {
        let size = self.buffer.desc().size as usize;
        let mut bytes = vec![0u8; size];
        self.engine.read_buffer(&self.buffer, 0, &mut bytes)?;
        Ok(bytes)
    }

    /// Read pixels and normalize them to RGBA8 for writing.
    ///
    /// Converts `Bgra8*` to `Rgba8`, half-precision float to u8 (clamped [0,1]).
    pub fn read_rgba8_pixels(&self) -> Result<Vec<u8>> {
        let raw = self.read_raw_pixels()?;
        let pixels = to_rgba8(&raw, self.format, self.width, self.height)?;
        Ok(pixels)
    }

    /// Save the captured image as a PNG file at `path`.
    ///
    /// Must be called after `frame.flush()` and `frame.wait()`.
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<()> {
        let pixels = self.read_rgba8_pixels()?;
        write_png(path.as_ref(), self.width, self.height, &pixels)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn format(&self) -> Format {
        self.format
    }
}

fn bytes_per_pixel(format: Format) -> Result<u32> {
    match format {
        Format::Rgba8Unorm | Format::Bgra8Unorm => Ok(4),
        Format::Rgba16Float => Ok(8),
        other => Err(crate::Error::InvalidInput(format!(
            "ScreenshotCapture: unsupported format {other:?}"
        ))),
    }
}

fn to_rgba8(raw: &[u8], format: Format, width: u32, height: u32) -> Result<Vec<u8>> {
    let count = (width * height) as usize;
    let mut out = vec![0u8; count * 4];
    match format {
        Format::Rgba8Unorm => {
            out.copy_from_slice(raw);
        }
        Format::Bgra8Unorm => {
            for (i, chunk) in raw.chunks_exact(4).enumerate() {
                out[i * 4] = chunk[2];
                out[i * 4 + 1] = chunk[1];
                out[i * 4 + 2] = chunk[0];
                out[i * 4 + 3] = chunk[3];
            }
        }
        Format::Rgba16Float => {
            for (i, chunk) in raw.chunks_exact(8).enumerate() {
                let r = f16_to_f32(u16::from_le_bytes([chunk[0], chunk[1]]));
                let g = f16_to_f32(u16::from_le_bytes([chunk[2], chunk[3]]));
                let b = f16_to_f32(u16::from_le_bytes([chunk[4], chunk[5]]));
                let a = f16_to_f32(u16::from_le_bytes([chunk[6], chunk[7]]));
                out[i * 4] = (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                out[i * 4 + 1] = (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                out[i * 4 + 2] = (b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                out[i * 4 + 3] = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
        other => {
            return Err(crate::Error::InvalidInput(format!(
                "to_rgba8: unsupported format {other:?}"
            )));
        }
    }
    Ok(out)
}

fn write_png(path: &Path, width: u32, height: u32, rgba8: &[u8]) -> Result<()> {
    let file = std::fs::File::create(path).map_err(|e| {
        crate::Error::Unknown(format!(
            "screenshot: failed to create {}: {e}",
            path.display()
        ))
    })?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| crate::Error::Unknown(format!("screenshot: PNG header error: {e}")))?;
    writer
        .write_image_data(rgba8)
        .map_err(|e| crate::Error::Unknown(format!("screenshot: PNG write error: {e}")))?;
    Ok(())
}

fn f16_to_f32(bits: u16) -> f32 {
    // IEEE 754 half-precision to single-precision.
    let sign = ((bits >> 15) as u32) << 31;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mantissa = (bits & 0x3ff) as u32;
    let (exp32, mant32) = if exp == 0 {
        // Denormal
        (0, mantissa << 13)
    } else if exp == 31 {
        // Inf / NaN
        (0xff, mantissa << 13)
    } else {
        (exp + 127 - 15, mantissa << 13)
    };
    f32::from_bits(sign | (exp32 << 23) | mant32)
}
