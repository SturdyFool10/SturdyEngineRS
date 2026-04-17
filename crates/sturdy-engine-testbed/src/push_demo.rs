use std::collections::HashMap;

use sturdy_engine::{
    Buffer, BufferDesc, BufferUsage, CanonicalPipelineLayout, ColorTargetDesc, CullMode, Engine,
    Error, Format, Frame, FrontFace, GraphicsPipelineDesc, Pipeline, PipelineLayout,
    PrimitiveTopology, RasterState, Shader, ShaderDesc, ShaderSource, ShaderStage, StageMask,
    SurfaceImage, VertexAttributeDesc, VertexBufferLayout, VertexFormat, VertexInputRate,
    spirv_words_from_bytes,
};

const PUSH_CONSTANT_BYTES: u32 = std::mem::size_of::<PushData>() as u32;

#[repr(C)]
#[derive(Copy, Clone)]
struct PushVertex {
    position: [f32; 2],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct PushData {
    offset: [f32; 2],
    scale: [f32; 2],
    tint: [f32; 4],
}

pub struct PushConstantsDemo {
    engine: Engine,
    vertex_buffer: Buffer,
    vertex_count: u32,
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
            vertex_count: vertices.len() as u32,
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
            .draw_pass("draw-push-constant-triangle")
            .color(target)
            .clear_color([0.02, 0.025, 0.03, 1.0])
            .pipeline(pipeline)
            .push_constants(
                StageMask::VERTEX | StageMask::FRAGMENT,
                bytes_of_value(&push),
            )
            .vertex_buffer(&self.vertex_buffer, 0, 0)
            .draw(self.vertex_count)
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

fn animated_push_data(time_seconds: f32) -> PushData {
    let x = time_seconds.sin() * 0.35;
    let y = (time_seconds * 0.7).cos() * 0.18;
    let pulse = 0.65 + 0.25 * (time_seconds * 1.7).sin();
    PushData {
        offset: [x, y],
        scale: [0.75 + pulse * 0.2, 0.75 + pulse * 0.2],
        tint: [0.75 + pulse * 0.25, 0.9, 1.15 - pulse * 0.25, 1.0],
    }
}

fn push_vertices() -> Vec<PushVertex> {
    vec![
        PushVertex {
            position: [0.0, -0.55],
            color: [1.0, 0.2, 0.1],
        },
        PushVertex {
            position: [0.55, 0.45],
            color: [0.1, 0.85, 0.25],
        },
        PushVertex {
            position: [-0.55, 0.45],
            color: [0.2, 0.35, 1.0],
        },
    ]
}

fn included_spirv(name: &str) -> Result<Vec<u32>, Error> {
    match name {
        "push_vertex.spv" => {
            spirv_words_from_bytes(include_bytes!(concat!(env!("OUT_DIR"), "/push_vertex.spv")))
        }
        "push_fragment.spv" => spirv_words_from_bytes(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/push_fragment.spv"
        ))),
        _ => Err(Error::InvalidInput(format!(
            "unknown included SPIR-V: {name}"
        ))),
    }
}

fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    let len = std::mem::size_of_val(values);
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), len) }
}

fn bytes_of_value<T>(value: &T) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
    }
}
