use std::collections::HashMap;

use sturdy_engine::{
    Buffer, BufferDesc, BufferUsage, CanonicalPipelineLayout, ColorTargetDesc, CullMode, Engine,
    Error, Format, Frame, FrontFace, GraphicsPipelineDesc, IndexFormat, Pipeline, PipelineLayout,
    PrimitiveTopology, RasterState, Shader, ShaderDesc, ShaderSource, ShaderStage, StageMask,
    SurfaceImage, VertexAttributeDesc, VertexBufferLayout, VertexFormat, VertexInputRate,
};

use super::bytes::{bytes_of_slice, bytes_of_value};
use super::geometry::{push_indices, push_vertices};
use super::push_data::{animated_push_data, PUSH_CONSTANT_BYTES};
use super::push_vertex::PushVertex;
use super::shader_assets::included_spirv;

pub struct PushConstantsDemo {
    engine: Engine,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    index_count: u32,
    vertex_shader: Shader,
    fragment_shader: Shader,
    pipelines: HashMap<Format, Pipeline>,
    pipeline_layout: PipelineLayout,
}

impl PushConstantsDemo {
    pub fn new(engine: &Engine) -> Result<Self, Error> {
        let vertices = push_vertices();
        let vertex_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(vertices.as_slice()) as u64,
            usage: BufferUsage::VERTEX,
        })?;
        vertex_buffer.write(0, bytes_of_slice(vertices.as_slice()))?;

        let indices = push_indices();
        let index_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(indices.as_slice()) as u64,
            usage: BufferUsage::INDEX,
        })?;
        index_buffer.write(0, bytes_of_slice(indices.as_slice()))?;

        let pipeline_layout = engine.create_pipeline_layout(CanonicalPipelineLayout {
            groups: Vec::new(),
            push_constants_bytes: PUSH_CONSTANT_BYTES,
        })?;
        let vertex_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::Spirv(included_spirv("push_vertex.spv")?),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        })?;
        let fragment_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::Spirv(included_spirv("push_fragment.spv")?),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Fragment,
        })?;

        Ok(Self {
            engine: engine.clone(),
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            vertex_shader,
            fragment_shader,
            pipelines: HashMap::new(),
            pipeline_layout,
        })
    }

    pub fn draw(
        &mut self,
        frame: &mut Frame,
        target: &SurfaceImage,
        time_seconds: f32,
    ) -> Result<(), Error> {
        let pipeline = self.pipeline(target.desc().format)?;
        let push = animated_push_data(time_seconds);
        frame
            .draw_pass("draw-indexed-push-constant-quad")
            .color(target)
            .clear_color([0.02, 0.025, 0.03, 1.0])
            .pipeline(pipeline)
            .push_constants(
                StageMask::VERTEX | StageMask::FRAGMENT,
                bytes_of_value(&push),
            )
            .vertex_buffer(&self.vertex_buffer, 0, 0)
            .index_buffer(&self.index_buffer, IndexFormat::Uint16, 0)
            .draw(self.index_count)
            .submit()
    }

    fn pipeline(&mut self, format: Format) -> Result<&Pipeline, Error> {
        if !self.pipelines.contains_key(&format) {
            let pipeline = self.engine.create_graphics_pipeline(GraphicsPipelineDesc {
                vertex_shader: self.vertex_shader.handle(),
                fragment_shader: Some(self.fragment_shader.handle()),
                layout: Some(self.pipeline_layout.handle()),
                vertex_buffers: vec![VertexBufferLayout {
                    binding: 0,
                    stride: std::mem::size_of::<PushVertex>() as u32,
                    input_rate: VertexInputRate::Vertex,
                }],
                vertex_attributes: vec![
                    VertexAttributeDesc {
                        location: 0,
                        binding: 0,
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(PushVertex, position) as u32,
                    },
                    VertexAttributeDesc {
                        location: 1,
                        binding: 0,
                        format: VertexFormat::Float32x3,
                        offset: std::mem::offset_of!(PushVertex, color) as u32,
                    },
                ],
                color_targets: vec![ColorTargetDesc { format }],
                depth_format: None,
                topology: PrimitiveTopology::TriangleList,
                raster: RasterState {
                    cull_mode: CullMode::None,
                    front_face: FrontFace::CounterClockwise,
                },
            })?;
            self.pipelines.insert(format, pipeline);
        }
        self.pipelines
            .get(&format)
            .ok_or_else(|| Error::Unknown("push-constant pipeline cache miss".into()))
    }
}
