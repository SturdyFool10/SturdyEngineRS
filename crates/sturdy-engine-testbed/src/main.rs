use std::env;
use std::process::ExitCode;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use sturdy_engine::NativeSurfaceDesc;
use sturdy_engine::{
    BackendKind, Engine, Error, Extent3d, Format, ImageDesc, ImageUsage, RenderMesh, RenderShader,
    RenderVertex, Surface, SurfaceSize, Vec2, Vec3, spirv_words_from_bytes,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowAttributes, WindowId};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sturdy-engine-testbed failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    let args = Args::parse()?;
    if args.headless {
        return run_headless_smoke(args.backend);
    }

    show_window(args.backend)
}

fn run_headless_smoke(backend: BackendKind) -> Result<(), Error> {
    let engine = Engine::with_backend(backend)?;
    print_backend_info(&engine);
    let target = engine.create_image(ImageDesc {
        extent: Extent3d {
            width: 640,
            height: 360,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::RENDER_TARGET | ImageUsage::COPY_SRC,
    })?;
    let mesh = RenderMesh::new(&engine, triangle_vertices().as_slice())?;
    let mut shader = triangle_shader(&engine)?;

    engine.render_image(&target, |renderer| renderer.draw_mesh(&mesh, &mut shader))?;

    println!(
        "hello triangle frame flushed and backend idle wait completed; target_format={:?}",
        target.desc().format
    );
    Ok(())
}

fn show_window(backend: BackendKind) -> Result<(), Error> {
    let event_loop = EventLoop::new()
        .map_err(|error| Error::Unknown(format!("failed to create event loop: {error}")))?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = GraphicalApp::new(backend);
    event_loop
        .run_app(&mut app)
        .map_err(|error| Error::Unknown(format!("window event loop failed: {error}")))
}

struct GraphicalApp {
    backend: BackendKind,
    window: Option<Arc<Window>>,
    app: Option<HelloTriangleApp>,
}

impl GraphicalApp {
    fn new(backend: BackendKind) -> Self {
        Self {
            backend,
            window: None,
            app: None,
        }
    }

    fn paint(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Error> {
        let Some(app) = self.app.as_mut() else {
            return Ok(());
        };
        app.render_frame()?;
        event_loop.set_control_flow(ControlFlow::Wait);
        Ok(())
    }
}

impl ApplicationHandler for GraphicalApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = WindowAttributes::default()
            .with_title("Sturdy Engine Testbed")
            .with_inner_size(LogicalSize::new(960.0, 540.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };
        self.app = match HelloTriangleApp::new(self.backend, &window) {
            Ok(app) => Some(app),
            Err(error) => {
                eprintln!("failed to create hello triangle app: {error}");
                event_loop.exit();
                return;
            }
        };
        self.window = Some(window.clone());
        window.request_redraw();
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
                if let Some(app) = self.app.as_mut() {
                    let _ = app.surface.resize(SurfaceSize {
                        width: size.width.max(1),
                        height: size.height.max(1),
                    });
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.paint(event_loop) {
                    eprintln!("{error}");
                    event_loop.exit();
                }
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

struct HelloTriangleApp {
    engine: Engine,
    surface: Surface,
    mesh: RenderMesh,
    shader: RenderShader,
}

impl HelloTriangleApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(backend: BackendKind, window: &Window) -> Result<Self, Error> {
        let engine = Engine::with_backend(backend)?;
        print_backend_info(&engine);

        let size = window.inner_size();
        let display_handle = window
            .display_handle()
            .map_err(|error| Error::Unknown(format!("failed to get display handle: {error}")))?
            .as_raw();
        let window_handle = window
            .window_handle()
            .map_err(|error| Error::Unknown(format!("failed to get window handle: {error}")))?
            .as_raw();
        let surface = engine.create_surface(NativeSurfaceDesc {
            display_handle,
            window_handle,
            size: SurfaceSize {
                width: size.width.max(1),
                height: size.height.max(1),
            },
        })?;
        let mesh = RenderMesh::new(&engine, triangle_vertices().as_slice())?;
        let shader = triangle_shader(&engine)?;

        println!("hello triangle swapchain created");
        Ok(Self {
            engine,
            surface,
            mesh,
            shader,
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn new(_backend: BackendKind, _window: &Window) -> Result<Self, Error> {
        Err(Error::Unsupported(
            "native engine surfaces are not available on wasm32",
        ))
    }

    fn render_frame(&mut self) -> Result<(), Error> {
        self.engine.render_surface(&self.surface, |renderer| {
            renderer.draw_mesh(&self.mesh, &mut self.shader)
        })
    }
}

fn triangle_vertices() -> [RenderVertex; 3] {
    [
        RenderVertex::new(Vec2::new(0.0, -0.6), Vec3::new(1.0, 0.15, 0.1)),
        RenderVertex::new(Vec2::new(0.6, 0.6), Vec3::new(0.1, 0.8, 0.25)),
        RenderVertex::new(Vec2::new(-0.6, 0.6), Vec3::new(0.2, 0.35, 1.0)),
    ]
}

fn triangle_shader(engine: &Engine) -> Result<RenderShader, Error> {
    RenderShader::new(
        engine,
        included_spirv("triangle_vertex.spv")?,
        included_spirv("triangle_fragment.spv")?,
    )
}

fn included_spirv(name: &str) -> Result<Vec<u32>, Error> {
    match name {
        "triangle_vertex.spv" => spirv_words_from_bytes(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/triangle_vertex.spv"
        ))),
        "triangle_fragment.spv" => spirv_words_from_bytes(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/triangle_fragment.spv"
        ))),
        _ => Err(Error::InvalidInput(format!(
            "unknown included SPIR-V: {name}"
        ))),
    }
}

fn print_backend_info(engine: &Engine) {
    println!("backend: {:?}", engine.backend_kind());
    if let Some(adapter_name) = engine.adapter_name() {
        println!("adapter: {adapter_name}");
    }

    let caps = engine.caps();
    println!(
        "caps: raytracing={} mesh={} bindless={} max_mips={} frames_in_flight={}",
        caps.supports_raytracing,
        caps.supports_mesh_shading,
        caps.supports_bindless,
        caps.max_mip_levels,
        caps.max_frames_in_flight
    );
}

fn parse_backend(arg: Option<&str>) -> Result<BackendKind, Error> {
    match arg.unwrap_or("auto").to_ascii_lowercase().as_str() {
        "auto" => Ok(BackendKind::Auto),
        "vulkan" | "vk" => Ok(BackendKind::Vulkan),
        "d3d12" | "dx12" => Ok(BackendKind::D3d12),
        "metal" | "mtl" => Ok(BackendKind::Metal),
        "null" => Ok(BackendKind::Null),
        other => Err(Error::InvalidInput(format!(
            "unknown backend '{other}', expected auto|vulkan|d3d12|metal|null"
        ))),
    }
}

struct Args {
    backend: BackendKind,
    headless: bool,
}

impl Args {
    fn parse() -> Result<Self, Error> {
        let mut backend = None;
        let mut headless = false;

        for arg in env::args().skip(1) {
            match arg.as_str() {
                "--headless" => headless = true,
                "--windowed" => headless = false,
                other if backend.is_none() => backend = Some(parse_backend(Some(other))?),
                other => {
                    return Err(Error::InvalidInput(format!(
                        "unknown argument '{other}', expected backend and optional --headless"
                    )));
                }
            }
        }

        Ok(Self {
            backend: backend.unwrap_or(BackendKind::Auto),
            headless,
        })
    }
}
