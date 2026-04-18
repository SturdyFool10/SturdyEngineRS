use std::env;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

mod push_demo;
mod textured;

#[cfg(not(target_arch = "wasm32"))]
use sturdy_engine::NativeSurfaceDesc;
use sturdy_engine::{
    spirv_words_from_bytes, AdapterSelection, BackendKind, BufferDesc, BufferUsage, DeviceDesc,
    Engine, Error, Extent3d, Format, ImageCopyRegion, ImageDesc, ImageUsage, RenderMesh,
    RenderShader, RenderVertex, Surface, SurfaceHdrPreference, SurfacePresentMode, SurfaceSize,
    TextureUploadDesc, Vec2, Vec3,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowAttributes, WindowId};

use push_demo::PushConstantsDemo;
use textured::TexturedQuadDemo;

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
        return run_headless_smoke(&args);
    }

    show_window(args)
}

fn run_headless_smoke(args: &Args) -> Result<(), Error> {
    let engine = Engine::with_desc(args.device_desc())?;
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
    verify_headless_texture_upload_readback(&engine)?;

    println!(
        "hello triangle and texture readback completed; target_format={:?}",
        target.desc().format
    );
    Ok(())
}

fn verify_headless_texture_upload_readback(engine: &Engine) -> Result<(), Error> {
    if engine.backend_kind() == BackendKind::Null {
        println!("texture upload readback skipped for null backend");
        return Ok(());
    }

    const WIDTH: u32 = 2;
    const HEIGHT: u32 = 2;
    let pixels = [
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    let readback = engine.create_buffer(BufferDesc {
        size: pixels.len() as u64,
        usage: BufferUsage::COPY_DST,
    })?;
    let mut frame = engine.begin_frame()?;
    let texture = frame.upload_texture_2d(
        "headless-readback-texture",
        TextureUploadDesc {
            width: WIDTH,
            height: HEIGHT,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED | ImageUsage::COPY_SRC,
        },
        &pixels,
    )?;
    frame.copy_image_to_buffer(
        "headless-readback-copy",
        &texture,
        &readback,
        ImageCopyRegion::whole_2d(WIDTH, HEIGHT),
    )?;
    frame.flush()?;
    frame.wait()?;

    let mut actual = vec![0u8; pixels.len()];
    readback.read(0, &mut actual)?;
    if actual != pixels {
        return Err(Error::Unknown(format!(
            "texture upload readback mismatch: expected {:?}, got {:?}",
            pixels, actual
        )));
    }
    println!("texture upload readback verified: {} bytes", actual.len());
    Ok(())
}

fn show_window(args: Args) -> Result<(), Error> {
    let event_loop = EventLoop::new()
        .map_err(|error| Error::Unknown(format!("failed to create event loop: {error}")))?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = GraphicalApp::new(args);
    event_loop
        .run_app(&mut app)
        .map_err(|error| Error::Unknown(format!("window event loop failed: {error}")))
}

struct GraphicalApp {
    args: Args,
    app: Option<TestbedApp>,
    window: Option<Arc<Window>>,
}

impl GraphicalApp {
    fn new(args: Args) -> Self {
        Self {
            args,
            app: None,
            window: None,
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

impl Drop for GraphicalApp {
    fn drop(&mut self) {
        self.app.take();
        self.window.take();
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
        self.app = match TestbedApp::new(&self.args, &window) {
            Ok(app) => Some(app),
            Err(error) => {
                eprintln!("failed to create testbed app: {error}");
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

struct TestbedApp {
    engine: Engine,
    surface: Surface,
    push_demo: PushConstantsDemo,
    textured_demo: TexturedQuadDemo,
    started_at: Instant,
    demo: DemoMode,
}

impl TestbedApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn new(args: &Args, window: &Window) -> Result<Self, Error> {
        let engine = Engine::with_desc(args.device_desc())?;
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
            hdr: args.hdr.clone(),
            preferred_present_mode: args.present_mode.clone(),
        })?;
        let push_demo = PushConstantsDemo::new(&engine)?;
        let textured_demo = TexturedQuadDemo::new(&engine)?;

        println!("{:?} demo swapchain created", args.demo);
        Ok(Self {
            engine,
            surface,
            push_demo,
            textured_demo,
            started_at: Instant::now(),
            demo: args.demo,
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn new(_backend: BackendKind, _window: &Window) -> Result<Self, Error> {
        Err(Error::Unsupported(
            "native engine surfaces are not available on wasm32",
        ))
    }

    fn render_frame(&mut self) -> Result<(), Error> {
        let surface_image = self.surface.acquire_image()?;
        let mut frame = self.engine.begin_frame()?;
        let time_seconds = self.started_at.elapsed().as_secs_f32();
        let mut textured_frame_resources = None;
        match self.demo {
            DemoMode::Showcase => {
                textured_frame_resources = Some(self.textured_demo.draw(
                    &mut frame,
                    &surface_image,
                    time_seconds,
                    true,
                )?);
                self.push_demo
                    .draw_gallery(&mut frame, &surface_image, time_seconds)?;
            }
            DemoMode::Push => {
                self.push_demo
                    .draw(&mut frame, &surface_image, time_seconds)?;
            }
            DemoMode::Textured => {
                textured_frame_resources = Some(self.textured_demo.draw(
                    &mut frame,
                    &surface_image,
                    time_seconds,
                    true,
                )?);
            }
        }
        frame.present_image(&surface_image)?;
        frame.flush()?;
        frame.wait()?;
        drop(textured_frame_resources);
        self.surface.present()
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

struct Args {
    backend: BackendKind,
    adapter: AdapterSelection,
    validation: Option<bool>,
    present_mode: Option<SurfacePresentMode>,
    hdr: SurfaceHdrPreference,
    headless: bool,
    demo: DemoMode,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DemoMode {
    Showcase,
    Push,
    Textured,
}

impl Args {
    fn parse() -> Result<Self, Error> {
        let mut backend = None;
        let mut adapter = AdapterSelection::Auto;
        let mut validation = None;
        let mut present_mode = None;
        let mut hdr = SurfaceHdrPreference::Sdr;
        let mut headless = false;
        let mut demo = DemoMode::Showcase;
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--headless" => headless = true,
                "--windowed" => headless = false,
                "--validation" | "--validation=true" => validation = Some(true),
                "--no-validation" | "--validation=false" => validation = Some(false),
                "--adapter" => {
                    let val = args.next().ok_or(Error::InvalidInput(
                        "--adapter requires a value: index|name|discrete|integrated|cpu".into(),
                    ))?;
                    adapter = parse_adapter(&val)?;
                }
                "--present-mode" => {
                    let val = args.next().ok_or(Error::InvalidInput(
                        "--present-mode requires a value: fifo|mailbox|immediate|relaxed".into(),
                    ))?;
                    present_mode = Some(parse_present_mode(&val)?);
                }
                "--hdr" => {
                    let val = args.next().ok_or(Error::InvalidInput(
                        "--hdr requires a value: sdr|hdr10|scrgb".into(),
                    ))?;
                    hdr = parse_hdr(&val)?;
                }
                "--demo" => {
                    let val = args.next().ok_or(Error::InvalidInput(
                        "--demo requires a value: showcase|push|textured".into(),
                    ))?;
                    demo = parse_demo(&val)?;
                }
                other if other.starts_with("--backend=") => {
                    backend = Some(parse_backend(other.strip_prefix("--backend="))?);
                }
                other if other.starts_with("--demo=") => {
                    demo = parse_demo(other.strip_prefix("--demo=").unwrap_or_default())?;
                }
                other if backend.is_none() && !other.starts_with('-') => {
                    backend = Some(parse_backend(Some(other))?);
                }
                other => {
                    return Err(Error::InvalidInput(format!(
                        "unknown argument '{other}'\n\
                         Usage: testbed [backend] [--headless] [--validation] [--no-validation]\n\
                         \x20        [--adapter <index|name|discrete|integrated|cpu>]\n\
                         \x20        [--present-mode <fifo|mailbox|immediate|relaxed>]\n\
                         \x20        [--hdr <sdr|hdr10|scrgb>]\n\
                         \x20        [--demo <showcase|push|textured>]"
                    )));
                }
            }
        }

        Ok(Self {
            backend: backend.unwrap_or(BackendKind::Auto),
            adapter,
            validation,
            present_mode,
            hdr,
            headless,
            demo,
        })
    }

    fn device_desc(&self) -> DeviceDesc {
        DeviceDesc {
            backend: self.backend,
            validation: self.validation.unwrap_or(cfg!(debug_assertions)),
            adapter: self.adapter.clone(),
            ..DeviceDesc::default()
        }
    }
}

fn parse_demo(val: &str) -> Result<DemoMode, Error> {
    match val.to_ascii_lowercase().as_str() {
        "showcase" | "all" => Ok(DemoMode::Showcase),
        "push" | "push-constants" => Ok(DemoMode::Push),
        "textured" | "texture" => Ok(DemoMode::Textured),
        other => Err(Error::InvalidInput(format!(
            "unknown demo '{other}', expected showcase|push|textured"
        ))),
    }
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

fn parse_adapter(val: &str) -> Result<AdapterSelection, Error> {
    if let Ok(idx) = val.parse::<usize>() {
        return Ok(AdapterSelection::ByIndex(idx));
    }
    match val.to_ascii_lowercase().as_str() {
        "discrete" | "dgpu" => Ok(AdapterSelection::ByKind(
            sturdy_engine::AdapterKind::DiscreteGpu,
        )),
        "integrated" | "igpu" => Ok(AdapterSelection::ByKind(
            sturdy_engine::AdapterKind::IntegratedGpu,
        )),
        "cpu" | "software" => Ok(AdapterSelection::ByKind(sturdy_engine::AdapterKind::Cpu)),
        name => Ok(AdapterSelection::ByName(name.to_owned())),
    }
}

fn parse_present_mode(val: &str) -> Result<SurfacePresentMode, Error> {
    match val.to_ascii_lowercase().as_str() {
        "fifo" | "vsync" => Ok(SurfacePresentMode::Fifo),
        "mailbox" | "triple" => Ok(SurfacePresentMode::Mailbox),
        "immediate" | "none" => Ok(SurfacePresentMode::Immediate),
        "relaxed" | "relaxed-fifo" => Ok(SurfacePresentMode::RelaxedFifo),
        other => Err(Error::InvalidInput(format!(
            "unknown present mode '{other}', expected fifo|mailbox|immediate|relaxed"
        ))),
    }
}

fn parse_hdr(val: &str) -> Result<SurfaceHdrPreference, Error> {
    match val.to_ascii_lowercase().as_str() {
        "sdr" | "off" => Ok(SurfaceHdrPreference::Sdr),
        "hdr10" | "hdr" => Ok(SurfaceHdrPreference::Hdr10),
        "scrgb" | "scrgb-linear" => Ok(SurfaceHdrPreference::ScRgb),
        other => Err(Error::InvalidInput(format!(
            "unknown HDR preference '{other}', expected sdr|hdr10|scrgb"
        ))),
    }
}
