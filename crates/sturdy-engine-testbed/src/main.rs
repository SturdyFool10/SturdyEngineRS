use std::{path::PathBuf, time::Instant};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use sturdy_engine::{
    BackendKind, Buffer, BufferDesc, BufferUsage, ColorTargetDesc, CullMode, Engine, FrontFace,
    GraphicsPipelineDesc, NativeSurfaceDesc, Pipeline, PrimitiveTopology, RasterState,
    Result as EngineResult, Shader, ShaderDesc, ShaderSource, ShaderStage, StageMask, Surface,
    SurfaceSize, VertexAttributeDesc, VertexBufferLayout, VertexFormat, VertexInputRate,
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

#[allow(dead_code)]
const VERTEX_SHADER: &str = r#"
struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

VSOut vs_main(uint vertex_id : SV_VertexID) {
    float2 positions[3] = {
        float2(-1.0, -3.0),
        float2(-1.0,  1.0),
        float2( 3.0,  1.0),
    };

    VSOut output;
    output.position = float4(positions[vertex_id], 0.0, 1.0);
    output.uv = positions[vertex_id] * 0.5 + 0.5;
    return output;
}
"#;

#[allow(dead_code)]
const FRAGMENT_SHADER: &str = r#"
struct VSOut {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

struct FrameGraphConstants {
    float time;
    float aspect;
    float2 resolution;
};

float2 rotate2(float2 p, float angle) {
    float s = sin(angle);
    float c = cos(angle);
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

float opSmoothUnion(float a, float b, float k) {
    float h = saturate(0.5 + 0.5 * (b - a) / k);
    return lerp(b, a, h) - k * h * (1.0 - h);
}

float sphere(float3 p, float radius) {
    return length(p) - radius;
}

float torus(float3 p, float2 radius) {
    float2 q = float2(length(p.xz) - radius.x, p.y);
    return length(q) - radius.y;
}

float graphDistance(float3 p, float time) {
    p.xz = rotate2(p.xz, time * 0.42);
    p.xy = rotate2(p.xy, sin(time * 0.31) * 0.55);

    float shell = torus(p, float2(0.78 + 0.08 * sin(time * 1.7), 0.22));

    float3 core_p = p;
    core_p.y += sin(time * 1.3) * 0.12;
    float core = sphere(core_p, 0.42);

    float3 satellite_p = p;
    satellite_p.xz = rotate2(satellite_p.xz, -time * 1.25);
    satellite_p -= float3(0.76, 0.22 * sin(time * 2.1), 0.0);
    float satellite = sphere(satellite_p, 0.18);

    float shape = opSmoothUnion(shell, core, 0.24);
    return opSmoothUnion(shape, satellite, 0.18);
}

float3 estimateNormal(float3 p, float time) {
    float2 e = float2(0.0015, 0.0);
    return normalize(float3(
        graphDistance(p + e.xyy, time) - graphDistance(p - e.xyy, time),
        graphDistance(p + e.yxy, time) - graphDistance(p - e.yxy, time),
        graphDistance(p + e.yyx, time) - graphDistance(p - e.yyx, time)
    ));
}

float3 shadeGraph(float3 p, float3 ray_dir, float time) {
    float3 n = estimateNormal(p, time);
    float3 light_a = normalize(float3(0.45, 0.75, -0.55));
    float3 light_b = normalize(float3(-0.65, 0.20, 0.70));
    float rim = pow(saturate(1.0 + dot(n, ray_dir)), 3.0);
    float bands = 0.5 + 0.5 * sin(9.0 * n.x + 7.0 * n.y + time * 2.2);

    float3 ink = float3(0.045, 0.055, 0.075);
    float3 copper = float3(0.95, 0.43, 0.18);
    float3 teal = float3(0.08, 0.82, 0.78);
    float3 violet = float3(0.45, 0.30, 0.96);
    float3 material = lerp(copper, teal, bands);

    float diffuse = saturate(dot(n, light_a)) * 0.85 + saturate(dot(n, light_b)) * 0.35;
    float specular = pow(saturate(dot(reflect(light_a, n), ray_dir)), 22.0);
    return ink + material * diffuse + violet * rim + float3(1.0, 0.88, 0.62) * specular;
}

float4 ps_main(VSOut input, uniform FrameGraphConstants graph) : SV_TARGET {
    float2 uv = input.uv * 2.0 - 1.0;
    uv.x *= graph.aspect;

    float3 origin = float3(0.0, 0.0, -3.35);
    float3 ray_dir = normalize(float3(uv, 1.65));
    float depth = 0.0;
    float hit = 0.0;

    for (int i = 0; i < 96; ++i) {
        float3 p = origin + ray_dir * depth;
        float dist = graphDistance(p, graph.time);
        if (dist < 0.0015) {
            hit = 1.0;
            break;
        }
        depth += dist * 0.82;
        if (depth > 7.0) {
            break;
        }
    }

    float3 background = float3(0.015, 0.018, 0.026) + 0.17 * float3(uv.y + 0.7, 0.3 + uv.x * 0.2, 0.8);
    float3 color = background;
    if (hit > 0.5) {
        float3 p = origin + ray_dir * depth;
        color = shadeGraph(p, ray_dir, graph.time);
        color *= exp(-0.045 * depth * depth);
    }

    float vignette = smoothstep(1.75, 0.25, length(uv));
    color *= 0.65 + 0.35 * vignette;
    return float4(pow(saturate(color), float3(0.4545)), 1.0);
}
"#;

#[repr(C)]
#[derive(Copy, Clone)]
struct FullscreenVertex {
    position: [f32; 2],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct FrameGraphConstants {
    time: f32,
    aspect: f32,
    resolution: [f32; 2],
}

struct App {
    renderer: Option<Renderer>,
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("SturdyEngine reflection shader graph")
                .with_inner_size(LogicalSize::new(1280.0, 720.0)),
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };

        match Renderer::new(&window) {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                self.window = Some(window);
            }
            Err(error) => {
                eprintln!("failed to initialize renderer: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let Some(renderer) = self.renderer.as_mut() {
                        if let Err(error) = renderer.resize(size.width, size.height) {
                            eprintln!("resize failed: {error}");
                            event_loop.exit();
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = self.renderer.as_mut() {
                    if let Err(error) = renderer.render() {
                        eprintln!("render failed: {error}");
                        event_loop.exit();
                        return;
                    }
                }
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

struct Renderer {
    engine: Engine,
    surface: Surface,
    _vertex_shader: Shader,
    _fragment_shader: Shader,
    pipeline: Pipeline,
    fullscreen_triangle: Buffer,
    started_at: Instant,
}

impl Renderer {
    fn new(window: &Window) -> EngineResult<Self> {
        let engine = Engine::with_backend(BackendKind::Auto)?;
        let surface = engine.create_surface(native_surface_desc(window)?)?;
        let surface_info = surface.info();

        let vertex_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::File(shader_path("shader_graph_vertex.slang")),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Vertex,
        })?;
        let fragment_shader = engine.create_shader(ShaderDesc {
            source: ShaderSource::File(shader_path("shader_graph_fragment.slang")),
            entry_point: "main".to_owned(),
            stage: ShaderStage::Fragment,
        })?;

        let pipeline = engine.create_graphics_pipeline(GraphicsPipelineDesc {
            vertex_shader: vertex_shader.handle(),
            fragment_shader: Some(fragment_shader.handle()),
            layout: None,
            vertex_buffers: vec![VertexBufferLayout {
                binding: 0,
                stride: std::mem::size_of::<FullscreenVertex>() as u32,
                input_rate: VertexInputRate::Vertex,
            }],
            vertex_attributes: vec![
                VertexAttributeDesc {
                    location: 0,
                    binding: 0,
                    format: VertexFormat::Float32x2,
                    offset: std::mem::offset_of!(FullscreenVertex, position) as u32,
                },
                VertexAttributeDesc {
                    location: 1,
                    binding: 0,
                    format: VertexFormat::Float32x2,
                    offset: std::mem::offset_of!(FullscreenVertex, uv) as u32,
                },
            ],
            color_targets: vec![ColorTargetDesc {
                format: surface_info.format,
            }],
            depth_format: None,
            topology: PrimitiveTopology::TriangleList,
            raster: RasterState {
                cull_mode: CullMode::None,
                front_face: FrontFace::CounterClockwise,
            },
        })?;
        pipeline.set_debug_name("reflection-driven-raymarch-graph")?;

        let vertices = [
            FullscreenVertex {
                position: [-1.0, -3.0],
                uv: [0.0, -1.0],
            },
            FullscreenVertex {
                position: [-1.0, 1.0],
                uv: [0.0, 1.0],
            },
            FullscreenVertex {
                position: [3.0, 1.0],
                uv: [2.0, 1.0],
            },
        ];
        let fullscreen_triangle = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(&vertices) as u64,
            usage: BufferUsage::VERTEX,
        })?;
        fullscreen_triangle.write(0, bytes_of_slice(&vertices))?;
        fullscreen_triangle.set_debug_name("shader-graph-fullscreen-triangle")?;

        println!(
            "rendering on {:?} using {:?}; surface {:?} at {}x{}",
            engine.adapter_name(),
            engine.backend_kind(),
            surface_info.format,
            surface_info.size.width,
            surface_info.size.height
        );
        println!(
            "pipeline layout is derived from Slang reflection; frame animation uses reflected push constants"
        );

        Ok(Self {
            engine,
            surface,
            _vertex_shader: vertex_shader,
            _fragment_shader: fragment_shader,
            pipeline,
            fullscreen_triangle,
            started_at: Instant::now(),
        })
    }

    fn resize(&mut self, width: u32, height: u32) -> EngineResult<()> {
        self.surface.resize(SurfaceSize { width, height })
    }

    fn render(&mut self) -> EngineResult<()> {
        let surface_image = self.surface.acquire_image()?;
        let desc = surface_image.desc();
        let constants = FrameGraphConstants {
            time: self.started_at.elapsed().as_secs_f32(),
            aspect: desc.extent.width as f32 / desc.extent.height.max(1) as f32,
            resolution: [desc.extent.width as f32, desc.extent.height as f32],
        };

        let mut frame = self.engine.begin_frame()?;
        frame
            .draw_pass("animated-reflection-shader-graph")
            .color(&surface_image)
            .clear_color([0.015, 0.018, 0.026, 1.0])
            .pipeline(&self.pipeline)
            .push_constants(StageMask::FRAGMENT, bytes_of(&constants))
            .vertex_buffer(&self.fullscreen_triangle, 0, 0)
            .draw(3)
            .submit()?;
        frame.present_image(&surface_image)?;
        frame.flush()?;
        frame.wait()?;
        self.surface.present()
    }
}

fn native_surface_desc(window: &Window) -> EngineResult<NativeSurfaceDesc> {
    let size = window.inner_size();
    let display = window
        .display_handle()
        .map_err(|error| sturdy_engine::Error::InvalidInput(error.to_string()))?
        .as_raw();
    let window_handle = window
        .window_handle()
        .map_err(|error| sturdy_engine::Error::InvalidInput(error.to_string()))?
        .as_raw();
    Ok(NativeSurfaceDesc::new(
        display,
        window_handle,
        SurfaceSize {
            width: size.width.max(1),
            height: size.height.max(1),
        },
    ))
}

fn shader_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

fn bytes_of<T>(value: &T) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
    }
}

fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(values.as_ptr().cast::<u8>(), std::mem::size_of_val(values))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut App {
        renderer: None,
        window: None,
    })?;
    Ok(())
}
