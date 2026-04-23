use crate::{
    Engine, Format, GraphImage, Image, ImageDesc, ImageDimension, ImageUsage, RenderFrame, Result,
    ShaderProgram,
};
use sturdy_engine_core::Extent3d;

/// A texture whose pixels are written by a GPU shader pass each frame.
///
/// Unlike CPU procedural textures (`Engine::generate_texture_2d`), this type
/// drives generation through a reflected Slang shader. The backing image lives
/// on the GPU across frames; only the push-constant parameters change per frame.
///
/// # Typical usage
///
/// ```rust,ignore
/// // At init time:
/// let lut_program = engine.load_shader("color_lut_gen.slang")?;
/// let color_lut = GpuProceduralTexture::new(&engine, "color_lut", 256, 1, Format::Rgba8Unorm, lut_program)?;
///
/// // Each frame:
/// color_lut.generate_with_constants(frame, &LutParams { phase: elapsed * 0.4 })?;
/// // Any shader with a `color_lut` binding now reads the freshly generated output.
/// ```
pub struct GpuProceduralTexture {
    image: Image,
    program: ShaderProgram,
    name: String,
}

impl GpuProceduralTexture {
    /// Create a GPU procedural texture.
    ///
    /// `format` must support `RENDER_TARGET` usage on the current device.
    /// `program` is the fullscreen fragment shader that generates the pixels;
    /// it receives push constants from `generate_with_constants` and writes
    /// its output to `SV_TARGET`.
    pub fn new(
        engine: &Engine,
        name: impl Into<String>,
        width: u32,
        height: u32,
        format: Format,
        program: ShaderProgram,
    ) -> Result<Self> {
        let name = name.into();
        let desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width,
                height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format,
            usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
            transient: false,
            clear_value: None,
            debug_name: None,
        };
        let image = engine.create_image(desc)?;
        let _ = image.set_debug_name(&format!("gpu-proc-{name}"));
        Ok(Self {
            image,
            program,
            name,
        })
    }

    /// Re-run the generator shader with no push constants.
    ///
    /// Use this when the shader produces a static result that only needs to be
    /// regenerated when the calling code decides (e.g. on resize or init).
    pub fn generate(&self, frame: &RenderFrame) -> Result<GraphImage> {
        let graph_img = frame.import_image(&self.name, &self.image)?;
        graph_img.execute_shader(&self.program)?;
        Ok(graph_img)
    }

    /// Re-run the generator shader with typed push constants.
    ///
    /// `constants` must be `bytemuck::Pod`; annotate the struct with
    /// `#[push_constants]` for the required impls. The stage mask is inferred
    /// from shader reflection (falls back to `FRAGMENT`).
    pub fn generate_with_constants<T: bytemuck::Pod>(
        &self,
        frame: &RenderFrame,
        constants: &T,
    ) -> Result<GraphImage> {
        let graph_img = frame.import_image(&self.name, &self.image)?;
        graph_img.execute_shader_with_constants_auto(&self.program, constants)?;
        Ok(graph_img)
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn program(&self) -> &ShaderProgram {
        &self.program
    }
}
