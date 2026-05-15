use std::fmt;

use crate::{Rect, Size, UiColor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiDownsampleFilter {
    Nearest,
    Box,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiAntialiasingMode {
    Native,
    Supersampled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiAntialiasing {
    pub mode: UiAntialiasingMode,
    pub samples_per_axis: u32,
    pub downsample_filter: UiDownsampleFilter,
}

impl UiAntialiasing {
    pub const fn native() -> Self {
        Self {
            mode: UiAntialiasingMode::Native,
            samples_per_axis: 1,
            downsample_filter: UiDownsampleFilter::Box,
        }
    }

    pub const fn supersampled(samples_per_axis: u32) -> Self {
        Self {
            mode: UiAntialiasingMode::Supersampled,
            samples_per_axis,
            downsample_filter: UiDownsampleFilter::Box,
        }
    }

    pub const fn with_downsample_filter(mut self, filter: UiDownsampleFilter) -> Self {
        self.downsample_filter = filter;
        self
    }

    pub fn resolved_samples_per_axis(self) -> u32 {
        match self.mode {
            UiAntialiasingMode::Native => 1,
            UiAntialiasingMode::Supersampled => self.samples_per_axis.clamp(1, 8),
        }
    }
}

impl Default for UiAntialiasing {
    fn default() -> Self {
        Self::native()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiImageSampling {
    Nearest,
    Linear,
    MipmapLinear,
    Anisotropic { max_anisotropy: u8 },
}

impl Default for UiImageSampling {
    fn default() -> Self {
        Self::Linear
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiImageFit {
    Stretch,
    Contain,
    Cover,
    None,
    ScaleDown,
}

impl Default for UiImageFit {
    fn default() -> Self {
        Self::Contain
    }
}

impl UiImageFit {
    pub fn fitted_rect(self, container: Rect, natural_size: Size, align: glam::Vec2) -> Rect {
        let natural = natural_size.to_vec2().max(glam::Vec2::splat(1.0));
        let available = container.size.to_vec2().max(glam::Vec2::ZERO);
        if available.x <= f32::EPSILON || available.y <= f32::EPSILON {
            return Rect::new(container.origin.x, container.origin.y, 0.0, 0.0);
        }

        let size = match self {
            Self::Stretch => available,
            Self::None => natural.min(available),
            Self::ScaleDown => {
                if natural.x <= available.x && natural.y <= available.y {
                    natural
                } else {
                    natural * (available.x / natural.x).min(available.y / natural.y)
                }
            }
            Self::Contain => natural * (available.x / natural.x).min(available.y / natural.y),
            Self::Cover => natural * (available.x / natural.x).max(available.y / natural.y),
        };
        let offset = (available - size) * align.clamp(glam::Vec2::ZERO, glam::Vec2::ONE);
        Rect::new(
            container.origin.x + offset.x,
            container.origin.y + offset.y,
            size.x,
            size.y,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiImageOptions {
    pub fit: UiImageFit,
    pub sampling: UiImageSampling,
    pub edge_antialiasing: UiAntialiasing,
    pub align: glam::Vec2,
}

impl Default for UiImageOptions {
    fn default() -> Self {
        Self {
            fit: UiImageFit::Contain,
            sampling: UiImageSampling::Linear,
            edge_antialiasing: UiAntialiasing::native(),
            align: glam::Vec2::splat(0.5),
        }
    }
}

impl UiImageOptions {
    pub fn fit(mut self, fit: UiImageFit) -> Self {
        self.fit = fit;
        self
    }

    pub fn sampling(mut self, sampling: UiImageSampling) -> Self {
        self.sampling = sampling;
        self
    }

    pub fn edge_antialiasing(mut self, antialiasing: UiAntialiasing) -> Self {
        self.edge_antialiasing = antialiasing;
        self
    }

    pub fn align(mut self, align: glam::Vec2) -> Self {
        self.align = align.clamp(glam::Vec2::ZERO, glam::Vec2::ONE);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiPixelFormat {
    Rgba8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiRasterImage {
    pub width: u32,
    pub height: u32,
    pub format: UiPixelFormat,
    pub pixels: Vec<u8>,
}

impl UiRasterImage {
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, UiImageError> {
        let expected_len = rgba_len(width, height)?;
        if pixels.len() != expected_len {
            return Err(UiImageError::InvalidPixelLength {
                expected: expected_len,
                actual: pixels.len(),
            });
        }

        Ok(Self {
            width,
            height,
            format: UiPixelFormat::Rgba8,
            pixels,
        })
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, UiImageError> {
        let image = image::load_from_memory(encoded)
            .map_err(|error| UiImageError::DecodeImage(error.to_string()))?
            .to_rgba8();
        let (width, height) = image.dimensions();
        Self::from_rgba8(width, height, image.into_raw())
    }

    pub fn size(&self) -> Size {
        Size::new(self.width as f32, self.height as f32)
    }

    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgRasterOptions {
    pub scale: f32,
    pub target_size: Option<Size>,
    pub antialiasing: UiAntialiasing,
    pub pixel_snap: bool,
    pub background: Option<UiColor>,
    pub load_system_fonts: bool,
    pub max_output_pixels: u64,
}

impl Default for SvgRasterOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            target_size: None,
            antialiasing: UiAntialiasing::native(),
            pixel_snap: true,
            background: None,
            load_system_fonts: false,
            max_output_pixels: 4096 * 4096,
        }
    }
}

impl SvgRasterOptions {
    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub fn target_size(mut self, target_size: Size) -> Self {
        self.target_size = Some(target_size);
        self
    }

    pub fn antialiasing(mut self, antialiasing: UiAntialiasing) -> Self {
        self.antialiasing = antialiasing;
        self
    }

    pub fn pixel_snap(mut self, pixel_snap: bool) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    pub fn background(mut self, background: UiColor) -> Self {
        self.background = Some(background);
        self
    }

    pub fn load_system_fonts(mut self, load_system_fonts: bool) -> Self {
        self.load_system_fonts = load_system_fonts;
        self
    }

    pub fn max_output_pixels(mut self, max_output_pixels: u64) -> Self {
        self.max_output_pixels = max_output_pixels.max(1);
        self
    }
}

#[derive(Clone, Debug)]
pub struct SvgDocument {
    tree: resvg::usvg::Tree,
    size: Size,
}

impl SvgDocument {
    pub fn parse(data: &[u8]) -> Result<Self, UiImageError> {
        Self::parse_with_options(data, &SvgRasterOptions::default())
    }

    pub fn parse_with_options(
        data: &[u8],
        options: &SvgRasterOptions,
    ) -> Result<Self, UiImageError> {
        let mut parse_options = resvg::usvg::Options::default();
        if options.load_system_fonts {
            parse_options.fontdb_mut().load_system_fonts();
        }
        let tree = resvg::usvg::Tree::from_data(data, &parse_options)
            .map_err(|error| UiImageError::ParseSvg(error.to_string()))?;
        let size = tree.size();
        Ok(Self {
            tree,
            size: Size::new(size.width(), size.height()),
        })
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn rasterize(&self, options: SvgRasterOptions) -> Result<UiRasterImage, UiImageError> {
        let output = SvgRasterTarget::new(self.size, options)?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(output.raster_width, output.raster_height)
            .ok_or(UiImageError::PixmapAllocation {
            width: output.raster_width,
            height: output.raster_height,
        })?;

        if let Some(color) = options.background {
            let [r, g, b, a] = color.to_f32_array();
            if let Some(color) = resvg::tiny_skia::Color::from_rgba(r, g, b, a) {
                pixmap.fill(color);
            }
        }

        let transform = resvg::tiny_skia::Transform::from_scale(output.scale_x, output.scale_y);
        resvg::render(&self.tree, transform, &mut pixmap.as_mut());

        let pixels = if output.samples_per_axis == 1 {
            pixmap.take_demultiplied()
        } else {
            downsample_premultiplied_rgba(
                pixmap.data(),
                output.raster_width,
                output.raster_height,
                output.width,
                output.height,
                output.samples_per_axis,
                options.antialiasing.downsample_filter,
            )
        };

        UiRasterImage::from_rgba8(output.width, output.height, pixels)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SvgRasterTarget {
    width: u32,
    height: u32,
    raster_width: u32,
    raster_height: u32,
    samples_per_axis: u32,
    scale_x: f32,
    scale_y: f32,
}

impl SvgRasterTarget {
    fn new(source_size: Size, options: SvgRasterOptions) -> Result<Self, UiImageError> {
        let scale = if options.scale.is_finite() {
            options.scale.max(f32::EPSILON)
        } else {
            1.0
        };
        let target = options.target_size.unwrap_or(Size::new(
            source_size.width * scale,
            source_size.height * scale,
        ));
        let width = resolve_extent(target.width, options.pixel_snap)?;
        let height = resolve_extent(target.height, options.pixel_snap)?;
        let samples_per_axis = options.antialiasing.resolved_samples_per_axis();
        let raster_width =
            width
                .checked_mul(samples_per_axis)
                .ok_or(UiImageError::RenderTargetTooLarge {
                    width,
                    height,
                    samples_per_axis,
                    max_output_pixels: options.max_output_pixels,
                })?;
        let raster_height =
            height
                .checked_mul(samples_per_axis)
                .ok_or(UiImageError::RenderTargetTooLarge {
                    width,
                    height,
                    samples_per_axis,
                    max_output_pixels: options.max_output_pixels,
                })?;
        let raster_pixels = u64::from(raster_width) * u64::from(raster_height);
        if raster_pixels > options.max_output_pixels {
            return Err(UiImageError::RenderTargetTooLarge {
                width,
                height,
                samples_per_axis,
                max_output_pixels: options.max_output_pixels,
            });
        }

        Ok(Self {
            width,
            height,
            raster_width,
            raster_height,
            samples_per_axis,
            scale_x: raster_width as f32 / source_size.width.max(f32::EPSILON),
            scale_y: raster_height as f32 / source_size.height.max(f32::EPSILON),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UiImageError {
    DecodeImage(String),
    InvalidDimensions,
    InvalidPixelLength {
        expected: usize,
        actual: usize,
    },
    ParseSvg(String),
    PixmapAllocation {
        width: u32,
        height: u32,
    },
    RenderTargetTooLarge {
        width: u32,
        height: u32,
        samples_per_axis: u32,
        max_output_pixels: u64,
    },
}

impl fmt::Display for UiImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DecodeImage(error) => write!(f, "failed to decode image: {error}"),
            Self::InvalidDimensions => f.write_str("image dimensions must be finite and positive"),
            Self::InvalidPixelLength { expected, actual } => {
                write!(
                    f,
                    "invalid RGBA pixel length: expected {expected}, got {actual}"
                )
            }
            Self::ParseSvg(error) => write!(f, "failed to parse SVG: {error}"),
            Self::PixmapAllocation { width, height } => {
                write!(f, "failed to allocate SVG pixmap {width}x{height}")
            }
            Self::RenderTargetTooLarge {
                width,
                height,
                samples_per_axis,
                max_output_pixels,
            } => write!(
                f,
                "SVG render target {width}x{height} at {samples_per_axis}x AA exceeds max {max_output_pixels} pixels"
            ),
        }
    }
}

impl std::error::Error for UiImageError {}

fn resolve_extent(value: f32, pixel_snap: bool) -> Result<u32, UiImageError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(UiImageError::InvalidDimensions);
    }

    let value = if pixel_snap {
        value.round()
    } else {
        value.ceil()
    };
    if value < 1.0 || value > u32::MAX as f32 {
        return Err(UiImageError::InvalidDimensions);
    }
    Ok(value as u32)
}

fn rgba_len(width: u32, height: u32) -> Result<usize, UiImageError> {
    if width == 0 || height == 0 {
        return Err(UiImageError::InvalidDimensions);
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(UiImageError::InvalidDimensions)
}

fn downsample_premultiplied_rgba(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    samples_per_axis: u32,
    filter: UiDownsampleFilter,
) -> Vec<u8> {
    let mut output = vec![0; rgba_len(width, height).unwrap_or(0)];
    for y in 0..height {
        for x in 0..width {
            let [r, g, b, a] = match filter {
                UiDownsampleFilter::Nearest => {
                    sample_nearest(source, source_width, source_height, x, y, samples_per_axis)
                }
                UiDownsampleFilter::Box => {
                    sample_box(source, source_width, source_height, x, y, samples_per_axis)
                }
            };
            let index = ((y * width + x) * 4) as usize;
            output[index] = r;
            output[index + 1] = g;
            output[index + 2] = b;
            output[index + 3] = a;
        }
    }
    output
}

fn sample_nearest(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    x: u32,
    y: u32,
    samples_per_axis: u32,
) -> [u8; 4] {
    let source_x = (x * samples_per_axis + samples_per_axis / 2).min(source_width - 1);
    let source_y = (y * samples_per_axis + samples_per_axis / 2).min(source_height - 1);
    let index = ((source_y * source_width + source_x) * 4) as usize;
    demultiply([
        source[index],
        source[index + 1],
        source[index + 2],
        source[index + 3],
    ])
}

fn sample_box(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    x: u32,
    y: u32,
    samples_per_axis: u32,
) -> [u8; 4] {
    let mut sum = [0u32; 4];
    let mut count = 0u32;
    for sample_y in 0..samples_per_axis {
        for sample_x in 0..samples_per_axis {
            let source_x = (x * samples_per_axis + sample_x).min(source_width - 1);
            let source_y = (y * samples_per_axis + sample_y).min(source_height - 1);
            let index = ((source_y * source_width + source_x) * 4) as usize;
            sum[0] += u32::from(source[index]);
            sum[1] += u32::from(source[index + 1]);
            sum[2] += u32::from(source[index + 2]);
            sum[3] += u32::from(source[index + 3]);
            count += 1;
        }
    }

    demultiply([
        (sum[0] / count) as u8,
        (sum[1] / count) as u8,
        (sum[2] / count) as u8,
        (sum[3] / count) as u8,
    ])
}

fn demultiply(pixel: [u8; 4]) -> [u8; 4] {
    let alpha = u32::from(pixel[3]);
    if alpha == 0 {
        return [0, 0, 0, 0];
    }

    [
        ((u32::from(pixel[0]) * 255 + alpha / 2) / alpha).min(255) as u8,
        ((u32::from(pixel[1]) * 255 + alpha / 2) / alpha).min(255) as u8,
        ((u32::from(pixel[2]) * 255 + alpha / 2) / alpha).min(255) as u8,
        pixel[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_fit_computes_contain_and_cover_rects() {
        let container = Rect::new(0.0, 0.0, 200.0, 100.0);
        let natural = Size::new(50.0, 50.0);

        assert_eq!(
            UiImageFit::Contain.fitted_rect(container, natural, glam::Vec2::splat(0.5)),
            Rect::new(50.0, 0.0, 100.0, 100.0)
        );
        assert_eq!(
            UiImageFit::Cover.fitted_rect(container, natural, glam::Vec2::splat(0.5)),
            Rect::new(0.0, -50.0, 200.0, 200.0)
        );
    }

    #[test]
    fn raster_image_validates_rgba_length() {
        let image = UiRasterImage::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]).unwrap();

        assert_eq!(image.size(), Size::new(2.0, 1.0));
        assert_eq!(image.byte_len(), 8);
        assert!(matches!(
            UiRasterImage::from_rgba8(2, 1, vec![0, 0, 0]).unwrap_err(),
            UiImageError::InvalidPixelLength {
                expected: 8,
                actual: 3
            }
        ));
    }

    #[test]
    fn decodes_png_to_rgba8() {
        let mut png = Vec::new();
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut png),
            &[255, 0, 0, 255],
            1,
            1,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();

        let image = UiRasterImage::decode(&png).unwrap();

        assert_eq!(image.width, 1);
        assert_eq!(image.height, 1);
        assert_eq!(image.pixels.len(), 4);
        assert_eq!(image.pixels[3], 255);
    }

    #[test]
    fn svg_rasterization_exposes_supersampling_dials() {
        let svg =
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 8 8">
            <circle cx="4" cy="4" r="3" fill="white"/>
        </svg>"#;
        let document = SvgDocument::parse(svg).unwrap();
        let raster = document
            .rasterize(
                SvgRasterOptions::default()
                    .target_size(Size::new(16.0, 16.0))
                    .antialiasing(UiAntialiasing::supersampled(3)),
            )
            .unwrap();

        assert_eq!(document.size(), Size::new(8.0, 8.0));
        assert_eq!(raster.size(), Size::new(16.0, 16.0));
        assert!(raster.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn svg_rasterization_rejects_oversized_supersampling() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <rect width="100" height="100" fill="white"/>
        </svg>"#;
        let document = SvgDocument::parse(svg).unwrap();

        let error = document
            .rasterize(
                SvgRasterOptions::default()
                    .antialiasing(UiAntialiasing::supersampled(4))
                    .max_output_pixels(100),
            )
            .unwrap_err();

        assert!(matches!(error, UiImageError::RenderTargetTooLarge { .. }));
    }
}
