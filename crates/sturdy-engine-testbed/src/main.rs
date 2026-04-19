use std::time::Instant;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use sturdy_engine::{
    BackendKind, BloomConfig, BloomPass, Engine, Format, HdrPipelineDesc, HdrPreference,
    ImageBuilder, ImageRole, NativeSurfaceDesc, Result as EngineResult, ShaderProgram, StageMask,
    Surface, SurfaceSize,
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

#[repr(C)]
#[derive(Copy, Clone)]
struct FrameGraphConstants {
    time: f32,
    aspect: f32,
    resolution: [f32; 2],
}
unsafe impl bytemuck::Pod for FrameGraphConstants {}
unsafe impl bytemuck::Zeroable for FrameGraphConstants {}

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
                .with_title("SturdyEngine HDR bloom testbed")
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
    scene_program: ShaderProgram,
    tonemap_program: ShaderProgram,
    bloom_pass: BloomPass,
    bloom_config: BloomConfig,
    started_at: Instant,
}

impl Renderer {
    fn new(window: &Window) -> EngineResult<Self> {
        let engine = Engine::with_backend(BackendKind::Auto)?;
        let surface = engine.create_surface(native_surface_desc(window)?)?;
        let surface_info = surface.info();

        let hdr_caps = surface.hdr_caps()?;
        let device_caps = engine.caps();
        let hdr_desc = HdrPipelineDesc::select(&hdr_caps, &device_caps, HdrPreference::PreferHdr)?;

        println!(
            "rendering on {:?} using {:?}; surface {:?} at {}x{}",
            engine.adapter_name(),
            engine.backend_kind(),
            surface_info.format,
            surface_info.size.width,
            surface_info.size.height,
        );
        println!(
            "HDR mode: {:?}, tone mapping: {:?}",
            hdr_desc.mode, hdr_desc.tone_mapping,
        );
        println!("pipeline: scene (FP16) → bloom chain → HDR composite → ACES tonemap → swapchain");

        let scene_program = engine.load_shader(shader_path("shader_graph_fragment.slang"))?;
        let tonemap_program = engine.load_shader(shader_path("tonemap.slang"))?;
        let bloom_pass = BloomPass::new(&engine)?;

        Ok(Self {
            engine,
            surface,
            scene_program,
            tonemap_program,
            bloom_pass,
            bloom_config: BloomConfig::default(),
            started_at: Instant::now(),
        })
    }

    fn resize(&mut self, width: u32, height: u32) -> EngineResult<()> {
        self.surface.resize(SurfaceSize { width, height })
    }

    fn render(&mut self) -> EngineResult<()> {
        let surface_image = self.surface.acquire_image()?;
        let desc = surface_image.desc();
        let elapsed = self.started_at.elapsed().as_secs_f32();

        let frame = self.engine.begin_render_frame_for(&surface_image)?;

        // Pass 1: ray-march scene into FP16 HDR buffer
        let scene_desc =
            ImageBuilder::new_2d(Format::Rgba16Float, desc.extent.width, desc.extent.height)
                .role(ImageRole::ColorAttachment)
                .build()?;
        let scene_color = frame.image("scene_color", scene_desc)?;
        scene_color.execute_shader_with_constants(
            &self.scene_program,
            StageMask::FRAGMENT,
            &FrameGraphConstants {
                time: elapsed,
                aspect: desc.extent.width as f32 / desc.extent.height.max(1) as f32,
                resolution: [desc.extent.width as f32, desc.extent.height as f32],
            },
        )?;

        // Pass 2: bloom — bright-extract → downsample → upsample → HDR composite
        // Returns "hdr_composite" registered in the frame (linear HDR, no tonemap).
        let _hdr_composite = self.bloom_pass.execute(&scene_color, &frame, &self.bloom_config)?;

        // Pass 3: ACES tonemap — "hdr_composite" → swapchain
        let swapchain_out = frame.swapchain_image(&surface_image)?;
        swapchain_out.execute_shader(&self.tonemap_program)?;

        frame.present_image(&swapchain_out)?;
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

fn shader_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
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
