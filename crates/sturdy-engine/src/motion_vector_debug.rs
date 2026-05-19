use crate::{
    Engine, Extent3d, Format, GraphImage, ImageDesc, ImageDimension, ImageUsage, RenderFrame,
    Result, ShaderProgram, shader_program::builtin_shader_path,
};

pub struct MotionVectorDebugPass {
    program: ShaderProgram,
}

impl MotionVectorDebugPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Ok(Self {
            program: ShaderProgram::load_fragment(
                engine,
                builtin_shader_path("motion_vector_debug.slang"),
            )?,
        })
    }

    pub(crate) fn execute(
        &self,
        frame: &RenderFrame,
        motion_vectors: &GraphImage,
        width: u32,
        height: u32,
    ) -> Result<GraphImage> {
        motion_vectors.register_as("motion_source");
        frame.set_sampler("motion_sampler", crate::SamplerPreset::Linear);
        let target = frame.image(
            "motion_vector_debug",
            ImageDesc {
                dimension: ImageDimension::D2,
                extent: Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth: 1,
                },
                mip_levels: 1,
                layers: 1,
                samples: 1,
                format: Format::Rgba16Float,
                usage: ImageUsage::SAMPLED | ImageUsage::RENDER_TARGET,
                transient: false,
                clear_value: None,
                debug_name: Some("motion_vector_debug"),
                compression: Default::default(),
                min_lod_bits: None,
                msaa_resolve_to_single_sampled: false,
                drm_format_modifier: None,
            },
        )?;
        target.execute_shader_auto(&self.program)?;
        Ok(target)
    }
}
