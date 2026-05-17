// Headless (windowless) application shell for GPU compute and offline rendering.
//
// Use this when you need GPU acceleration without opening a window:
//   - Data visualisation pipelines (render frames → save PNG sequences)
//   - Scientific / ML compute workloads (Vulkan compute shaders)
//   - Procedural content generation (render thumbnails, bake lightmaps)
//   - Test fixtures (render reference images for comparison)
//
// The three entry points:
//   run_headless<App>()         — compute loop: run until App::render returns false
//   render_to_rgba8()           — single frame → RGBA8 pixel buffer
//   render_to_rgba8_with_engine — same, but reuse an existing Engine

use crate::{
    Engine, Extent3d, Format, FrameSyncReason, GraphImage, ImageDesc, ImageDimension, ImageUsage,
    RenderFrame, Result, ScreenshotCapture,
};

// ── HeadlessApp trait ─────────────────────────────────────────────────────────

/// Application trait for windowless GPU compute or offline rendering.
///
/// Implement this and call `run_headless::<MyApp>()` to start the loop.
///
/// # Example
/// ```ignore
/// use sturdy_engine::{Engine, RenderFrame, HeadlessApp, run_headless};
///
/// struct MyCompute { frame_count: u32 }
///
/// impl HeadlessApp for MyCompute {
///     type Error = sturdy_engine::Error;
///
///     fn init(engine: &Engine) -> Result<Self, Self::Error> {
///         Ok(Self { frame_count: 0 })
///     }
///
///     fn render(&mut self, frame: &RenderFrame, _engine: &Engine)
///         -> Result<bool, Self::Error>
///     {
///         self.frame_count += 1;
///         // Record GPU compute passes into `frame` here.
///         Ok(self.frame_count < 100)  // stop after 100 frames
///     }
/// }
///
/// fn main() { run_headless::<MyCompute>().unwrap(); }
/// ```
pub trait HeadlessApp: Sized {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Called once to initialise the application and allocate GPU resources.
    fn init(engine: &Engine) -> std::result::Result<Self, Self::Error>;

    /// Called once per frame. Record GPU work into `frame`.
    ///
    /// Return `true` to keep running; `false` to stop after this frame.
    /// The frame is flushed and GPU-waited before the next call.
    fn render(
        &mut self,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> std::result::Result<bool, Self::Error>;
}

// ── run_headless ──────────────────────────────────────────────────────────────

/// Run a headless application without opening a window.
///
/// - Creates a `Vulkan` (or best available) engine.
/// - Calls `App::init`.
/// - Loops calling `App::render`, flushing and GPU-waiting each frame.
/// - Stops when `render` returns `false` or returns an error.
///
/// Each frame is synchronous: `render` does not return until the GPU has
/// finished all work recorded in that frame. This is intentional for compute
/// workloads — use `Engine::begin_render_frame()` directly if you need async
/// frame pipelining.
pub fn run_headless<App: HeadlessApp>() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let engine = Engine::new()?;
    Engine::set_global(&engine);
    let mut app = App::init(&engine).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
    loop {
        // Upload any textures that background workers finished decoding.
        let _ = engine.drain_pending_uploads();
        let frame = engine.begin_render_frame()?;
        let keep_going = app
            .render(&frame, &engine)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        frame.flush_with_reason(FrameSyncReason::ExplicitUserRequest)?;
        frame.wait()?;
        if !keep_going {
            break;
        }
    }
    Ok(())
}

// ── render_to_rgba8 ───────────────────────────────────────────────────────────

/// Render one frame offscreen and return the result as a flat RGBA8 byte buffer.
///
/// Creates a new engine, allocates an `Rgba8Unorm` image of `width × height`,
/// calls `render_fn` to record GPU work into it, and downloads the pixels to
/// CPU before returning. The GPU is waited on before returning.
///
/// # Example
/// ```ignore
/// let pixels = render_to_rgba8(256, 256, |frame, output, engine| {
///     output.clear([0.2, 0.5, 0.8, 1.0])?;  // solid blue
///     Ok(())
/// })?;
/// // `pixels` is 256 × 256 × 4 bytes, row-major, RGBA
/// ```
pub fn render_to_rgba8(
    width: u32,
    height: u32,
    render_fn: impl FnOnce(&RenderFrame, &GraphImage, &Engine) -> Result<()>,
) -> Result<Vec<u8>> {
    let engine = Engine::new()?;
    Engine::set_global(&engine);
    render_to_rgba8_with_engine(&engine, width, height, render_fn)
}

/// Same as `render_to_rgba8` but reuses an existing `Engine`.
///
/// Prefer this when rendering multiple images in sequence to avoid the
/// engine-creation overhead for each frame.
pub fn render_to_rgba8_with_engine(
    engine: &Engine,
    width: u32,
    height: u32,
    render_fn: impl FnOnce(&RenderFrame, &GraphImage, &Engine) -> Result<()>,
) -> Result<Vec<u8>> {
    let image = engine.create_image(ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width,
            height,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::RENDER_TARGET | ImageUsage::COPY_SRC | ImageUsage::SAMPLED,
        transient: false,
        clear_value: None,
        debug_name: Some("headless_output"),
                compression: Default::default(), min_lod_bits: None, msaa_resolve_to_single_sampled: false,
    })?;

    let capture = ScreenshotCapture::new(engine, width, height, Format::Rgba8Unorm)?;

    let frame = engine.begin_render_frame()?;
    let graph_img = frame.import_image("headless_output", &image)?;
    render_fn(&frame, &graph_img, engine)?;
    capture.record_render_frame_readback(&frame, &graph_img)?;
    frame.flush_with_reason(FrameSyncReason::ReadbackCompletion)?;
    frame.wait()?;

    capture.read_rgba8_pixels()
}
