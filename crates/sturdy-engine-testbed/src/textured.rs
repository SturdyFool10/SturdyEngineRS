use std::collections::HashMap;

use sturdy_engine::{
    BindGroup, BindGroupDesc, BindGroupEntry, BindingKind, Buffer, BufferDesc, BufferUsage,
    CanonicalBinding, CanonicalGroupLayout, CanonicalPipelineLayout, ColorTargetDesc, CullMode,
    Engine, Error, FilterMode, Format, Frame, FrontFace, GraphicsPipelineDesc, Image, ImageUsage,
    MipmapMode, Pipeline, PipelineLayout, PrimitiveTopology, RasterState, ResourceBinding, Sampler,
    SamplerDesc, Shader, ShaderDesc, ShaderSource, ShaderStage, StageMask, SurfaceImage,
    TextureUploadDesc, UpdateRate, VertexAttributeDesc, VertexBufferLayout, VertexFormat,
    VertexInputRate, spirv_words_from_bytes,
};

const TEXTURE_WIDTH: u32 = 4;
const TEXTURE_HEIGHT: u32 = 4;

#[repr(C)]
#[derive(Copy, Clone)]
struct TexturedVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

pub struct TexturedQuadDemo {
    engine: Engine,
    vertex_buffer: Buffer,
    vertex_count: u32,
    vertex_shader: Shader,
    fragment_shader: Shader,
    pipelines: HashMap<Format, Pipeline>,
    pipeline_layout: PipelineLayout,
    sampler: Sampler,
}

pub struct TexturedFrameResources {
    _texture: Image,
    _bind_group: BindGroup,
}

impl TexturedQuadDemo {
    pub fn new(engine: &Engine) -> Result<Self, Error> {
        let vertex_data = textured_vertices();
        let vertex_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(vertex_data.as_slice()) as u64,
            usage: BufferUsage::VERTEX,
        })?;
        vertex_buffer.write(0, bytes_of_slice(vertex_data.as_slice()))?;

        let pipeline_layout = engine.create_pipeline_layout(texture_pipeline_layout())?;
        let sampler = engine.create_sampler(SamplerDesc {
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_mode: MipmapMode::Nearest,
            ..SamplerDesc::default()
        })?;
        let vertex_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::Spirv(included_spirv("textured_vertex.spv")?),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        })?;
        let fragment_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::Spirv(included_spirv("textured_fragment.spv")?),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Fragment,
        })?;

        Ok(Self {
            engine: engine.clone(),
            vertex_buffer,
            vertex_count: vertex_data.len() as u32,
            vertex_shader,
            fragment_shader,
            pipelines: HashMap::new(),
            pipeline_layout,
            sampler,
        })
    }

    pub fn draw(
        &mut self,
        frame: &mut Frame,
        target: &SurfaceImage,
    ) -> Result<TexturedFrameResources, Error> {
        let texture = upload_checker_texture(frame)?;
        let bind_group = self.bind_texture(&texture)?;
        let pipeline = self.pipeline(target.desc().format)?;
        frame
            .draw_pass("draw-textured-quad")
            .color(target)
            .clear_color([0.02, 0.025, 0.03, 1.0])
            .sample(&texture)
            .pipeline(pipeline)
            .bind(&bind_group)
            .vertex_buffer(&self.vertex_buffer, 0, 0)
            .draw(self.vertex_count)
            .submit()?;
        Ok(TexturedFrameResources {
            _texture: texture,
            _bind_group: bind_group,
        })
    }

    fn bind_texture(&self, texture: &Image) -> Result<BindGroup, Error> {
        self.engine.create_bind_group(BindGroupDesc {
            layout: self.pipeline_layout.handle(),
            entries: vec![
                BindGroupEntry {
                    path: "base_color".to_owned(),
                    resource: ResourceBinding::Image(texture.handle()),
                },
                BindGroupEntry {
                    path: "base_sampler".to_owned(),
                    resource: ResourceBinding::Sampler(self.sampler.handle()),
                },
            ],
        })
    }

    fn pipeline(&mut self, format: Format) -> Result<&Pipeline, Error> {
        if !self.pipelines.contains_key(&format) {
            let pipeline = self.engine.create_graphics_pipeline(GraphicsPipelineDesc {
                vertex_shader: self.vertex_shader.handle(),
                fragment_shader: Some(self.fragment_shader.handle()),
                layout: Some(self.pipeline_layout.handle()),
                vertex_buffers: vec![VertexBufferLayout {
                    binding: 0,
                    stride: std::mem::size_of::<TexturedVertex>() as u32,
                    input_rate: VertexInputRate::Vertex,
                }],
                vertex_attributes: vec![
                    VertexAttributeDesc {
                        location: 0,
                        binding: 0,
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(TexturedVertex, position) as u32,
                    },
                    VertexAttributeDesc {
                        location: 1,
                        binding: 0,
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(TexturedVertex, uv) as u32,
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
            .ok_or_else(|| Error::Unknown("textured pipeline cache miss".into()))
    }
}

fn upload_checker_texture(frame: &mut Frame) -> Result<Image, Error> {
    frame.upload_texture_2d(
        "checker-texture",
        TextureUploadDesc {
            width: TEXTURE_WIDTH,
            height: TEXTURE_HEIGHT,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED,
        },
        checker_pixels().as_slice(),
    )
}

fn texture_pipeline_layout() -> CanonicalPipelineLayout {
    CanonicalPipelineLayout {
        groups: vec![CanonicalGroupLayout {
            name: "material".to_owned(),
            bindings: vec![
                CanonicalBinding {
                    path: "base_color".to_owned(),
                    kind: BindingKind::SampledImage,
                    count: 1,
                    stage_mask: StageMask::FRAGMENT,
                    update_rate: UpdateRate::Material,
                },
                CanonicalBinding {
                    path: "base_sampler".to_owned(),
                    kind: BindingKind::Sampler,
                    count: 1,
                    stage_mask: StageMask::FRAGMENT,
                    update_rate: UpdateRate::Material,
                },
            ],
        }],
        push_constants_bytes: 0,
    }
}

fn textured_vertices() -> Vec<TexturedVertex> {
    vec![
        TexturedVertex {
            position: [-0.75, -0.75],
            uv: [0.0, 1.0],
        },
        TexturedVertex {
            position: [0.75, -0.75],
            uv: [1.0, 1.0],
        },
        TexturedVertex {
            position: [0.75, 0.75],
            uv: [1.0, 0.0],
        },
        TexturedVertex {
            position: [-0.75, -0.75],
            uv: [0.0, 1.0],
        },
        TexturedVertex {
            position: [0.75, 0.75],
            uv: [1.0, 0.0],
        },
        TexturedVertex {
            position: [-0.75, 0.75],
            uv: [0.0, 0.0],
        },
    ]
}

fn checker_pixels() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((TEXTURE_WIDTH * TEXTURE_HEIGHT * 4) as usize);
    for y in 0..TEXTURE_HEIGHT {
        for x in 0..TEXTURE_WIDTH {
            let rgba = if (x + y) % 2 == 0 {
                [245, 245, 235, 255]
            } else {
                [30, 150, 220, 255]
            };
            pixels.extend_from_slice(&rgba);
        }
    }
    pixels
}

fn included_spirv(name: &str) -> Result<Vec<u32>, Error> {
    match name {
        "textured_vertex.spv" => spirv_words_from_bytes(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/textured_vertex.spv"
        ))),
        "textured_fragment.spv" => spirv_words_from_bytes(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/textured_fragment.spv"
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
