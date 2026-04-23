use crate::{Engine, Format, GraphImage, Image, ImageDesc, ImageDimension, ImageUsage, RenderFrame, Result};
use sturdy_engine_core::Extent3d;

/// A persistent GPU image that a [`SceneCamera`](super::SceneCamera) renders into.
///
/// Unlike transient frame images, a `RenderTarget` survives across frames.
/// After a camera writes to it, other passes in the same frame can sample it
/// by name — enabling effects like CRT screens that display a secondary camera's
/// output as an emission texture.
pub struct RenderTarget {
    image: Image,
    name: String,
    width: u32,
    height: u32,
    samples: u8,
}

impl RenderTarget {
    /// Create a render target with the given dimensions and format.
    ///
    /// `format` must support `RENDER_TARGET` usage. `Rgba16Float` is recommended
    /// for HDR-capable targets; `Rgba8Unorm` for display-referred outputs.
    pub fn new(
        engine: &Engine,
        name: impl Into<String>,
        width: u32,
        height: u32,
        format: Format,
    ) -> Result<Self> {
        Self::with_samples(engine, name, width, height, format, 1)
    }

    pub fn with_samples(
        engine: &Engine,
        name: impl Into<String>,
        width: u32,
        height: u32,
        format: Format,
        samples: u8,
    ) -> Result<Self> {
        let name = name.into();
        let samples = samples.clamp(1, engine.caps().max_color_sample_count.max(1)).min(16);
        let desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d { width, height, depth: 1 },
            mip_levels: 1,
            layers: 1,
            samples,
            format,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: None,
        };
        let image = engine.create_image(desc)?;
        let _ = image.set_debug_name(&format!("render-target-{name}"));
        Ok(Self { image, name, width, height, samples })
    }

    /// Register this target as a writable frame image and return it.
    ///
    /// The returned `GraphImage` can be passed to a camera's draw passes as the
    /// write destination. It is also registered under `self.name()` so downstream
    /// shaders can sample it by name.
    pub fn as_frame_image<'a>(&'a self, frame: &RenderFrame) -> Result<GraphImage> {
        frame.import_image(&self.name, &self.image)
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn samples(&self) -> u8 {
        self.samples
    }
}
