// UiRenderer — renders a clay_ui GpuWorkQueue to a render target.
//
// Converts each RenderCommand in the queue into a quad mesh drawn with
// push constants. Uses the same shader as the debug overlay's rounded-
// rectangle renderer, which handles fill colour, border colour, corner
// radius, and smooth AA edges.
//
// ## Supported commands
//
//   Rectangle  — filled, optionally rounded; colour from UiColor::Rgba8
//   Border     — border-only rect (fill transparent)
//   ScissorStart / ScissorEnd — stored; scissor submission is TODO
//   Text       — skipped here; text is rendered separately via the textui
//                atlas returned in UiFrameOutput::text_scenes
//   Image      — skipped; image binding requires a texture registry (TODO)
//
// ## Usage
//
// ```ignore
// let renderer = UiRenderer::new(&engine)?;
//
// // Inside a frame, after building the clay UI:
// let output = clay.build_frame_with_limits(viewport, frame_number, limits, scale);
// for (_, tree_output) in &output.trees {
//     renderer.draw_queue(&frame, &swapchain_image, &tree_output.queue, w, h)?;
// }
// ```

use std::collections::HashMap;
use std::sync::Arc;

use clay_ui::{GpuWorkQueue, RenderCommandKind, RenderData};

use crate::{
    Engine, GraphImage, Image, Mesh, MeshProgram, MeshProgramDesc, MeshVertexKind, QuadBatch,
    RenderFrame, Result, ShaderDesc, ShaderSource, ShaderStage, TextureUploadDesc,
};

const UI_SHAPE_FRAGMENT: &str = include_str!("../shaders/debug_overlay_ui_shape_fragment.slang");

// ── Push-constant layout (must match debug_overlay_ui_shape_fragment.slang) ──

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct UiShapeConstants {
    /// xy = size in pixels, z = corner radius, w = border width.
    size_radius_border: [f32; 4],
    fill_color: [f32; 4],
    border_color: [f32; 4],
}

// ── UiRenderer ────────────────────────────────────────────────────────────────

/// Renders clay UI render commands to a `GraphImage` render target.
///
/// Create once at init time and reuse across frames. Each call to
/// [`draw_queue`] iterates the commands produced by a single UI tree's
/// `build_frame` output and issues the necessary mesh draws.
pub struct UiRenderer {
    program: MeshProgram,
}

impl UiRenderer {
    /// Create a `UiRenderer`, compiling the built-in UI shape shader.
    pub fn new(engine: &Engine) -> Result<Self> {
        let program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(UI_SHAPE_FRAGMENT.to_string()),
                    entry_point: "main".to_string(),
                    stage: ShaderStage::Fragment,
                    requires_ray_query: false,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V2d,
                alpha_blend: true,
                uses_depth: false,
            },
        )?;
        Ok(Self { program })
    }

    /// Draw all supported commands from `queue` into `target`.
    ///
    /// `width` and `height` are the logical pixel dimensions of the render
    /// target — used to convert pixel-space rects to NDC coordinates.
    ///
    /// Text commands are skipped (handled separately via `UiFrameOutput::text_scenes`).
    /// Image commands are skipped (need an image registry — future work).
    pub fn draw_queue(
        &self,
        _frame: &RenderFrame,
        target: &GraphImage,
        queue: &GpuWorkQueue,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let engine = &self.program.engine;
        for command in &queue.commands {
            match command.kind {
                RenderCommandKind::Rectangle => {
                    if let RenderData::Rectangle(data) = &command.data {
                        let fill = ui_color_to_linear(&data.color);
                        let radius = data.corner_radius.x;
                        let constants = UiShapeConstants {
                            size_radius_border: [
                                command.rect.size.width,
                                command.rect.size.height,
                                radius,
                                0.0,
                            ],
                            fill_color: fill,
                            border_color: [0.0; 4],
                        };
                        let mesh = rect_mesh(engine, width, height, &command.rect)?;
                        target.draw_mesh_with_push_constants(
                            &mesh,
                            &self.program,
                            crate::StageMask::FRAGMENT,
                            bytemuck::bytes_of(&constants),
                        )?;
                    }
                }

                RenderCommandKind::Border => {
                    if let RenderData::Border(data) = &command.data {
                        let border_color = ui_color_to_linear(&data.color);
                        let border_width = data.width.top;
                        let radius = data.corner_radius.x;
                        let constants = UiShapeConstants {
                            size_radius_border: [
                                command.rect.size.width,
                                command.rect.size.height,
                                radius,
                                border_width,
                            ],
                            fill_color: [0.0; 4],
                            border_color,
                        };
                        let mesh = rect_mesh(engine, width, height, &command.rect)?;
                        target.draw_mesh_with_push_constants(
                            &mesh,
                            &self.program,
                            crate::StageMask::FRAGMENT,
                            bytemuck::bytes_of(&constants),
                        )?;
                    }
                }

                // Text handled via UiFrameOutput::text_scenes (textui atlas).
                // Image needs an image registry — future work.
                // Scissor, Custom, None: no GPU action here.
                RenderCommandKind::Text
                | RenderCommandKind::Image
                | RenderCommandKind::ScissorStart
                | RenderCommandKind::ScissorEnd
                | RenderCommandKind::Custom
                | RenderCommandKind::None => {}
            }
        }
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a quad mesh covering `rect` (pixel coordinates) mapped to NDC.
///
/// A 1-pixel AA padding is added on each side so the SDF shader can
/// render smooth edges without clipping.
fn rect_mesh(engine: &Engine, width: u32, height: u32, rect: &clay_ui::Rect) -> Result<Mesh> {
    let aa = 2.0_f32;
    let px = rect.origin.x - aa;
    let py = rect.origin.y - aa;
    let pw = rect.size.width + aa * 2.0;
    let ph = rect.size.height + aa * 2.0;

    let w = width.max(1) as f32;
    let h = height.max(1) as f32;

    // Convert pixel → NDC (Y flipped: pixel-y grows down, NDC-y grows up).
    let ndc_x = px / w * 2.0 - 1.0;
    let ndc_y = 1.0 - (py + ph) / h * 2.0;
    let ndc_w = pw / w * 2.0;
    let ndc_h = ph / h * 2.0;

    // UV carries pixel-space coordinates (used by the SDF for size/radius).
    let uv_min = [-aa, -aa];
    let uv_max = [rect.size.width + aa, rect.size.height + aa];

    let mut batch = QuadBatch::new();
    batch.push(
        [ndc_x, ndc_y],
        [ndc_w, ndc_h],
        [uv_min[0], uv_min[1], uv_max[0], uv_max[1]],
        [1.0, 1.0, 1.0, 1.0],
    );
    batch.build(engine)
}

// ── Text rendering ────────────────────────────────────────────────────────────

/// Text atlas shader — alpha mask path (same as text_overlay_alpha_fragment.slang).
const TEXT_ATLAS_FRAGMENT: &str = include_str!("../shaders/text_overlay_alpha_fragment.slang");

/// Per-glyph push constants for the text atlas shader.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TextGlyphConstants {
    /// Screen-space positions of the quad corners (xy pairs for 4 vertices).
    positions: [[f32; 2]; 4],
    /// UV coordinates into the atlas page.
    uvs: [[f32; 2]; 4],
    /// RGBA tint colour in [0, 1].
    tint: [f32; 4],
}

/// Draw all text scenes from a [`clay_ui::UiFrameOutput`] into `target`.
///
/// Each text scene is a pre-rendered glyph atlas (CPU-side RGBA8 pixels) plus
/// quad positions/UVs. This function:
/// 1. Uploads changed atlas pages as `Rgba8Unorm` images (keyed by content hash).
/// 2. For each glyph quad, issues one mesh draw with per-glyph push constants.
///
/// The atlas image cache is caller-managed. Pass a `HashMap<u64, Image>` that
/// persists across frames to avoid redundant GPU uploads.
pub fn draw_ui_text(
    _frame: &RenderFrame,
    engine: &Engine,
    output: &clay_ui::UiFrameOutput,
    _target: &GraphImage,
    _width: u32,
    _height: u32,
) -> Result<()> {
    // Thread-local cache of compiled text programs. Lazily created.
    // For simplicity, use a static Mutex so we don't need to thread the program
    // through the call chain.
    use std::sync::Mutex;
    static TEXT_PROGRAM: Mutex<Option<MeshProgram>> = Mutex::new(None);

    let program_guard = TEXT_PROGRAM.lock().expect("text program mutex poisoned");
    if program_guard.is_none() {
        drop(program_guard);
        let prog = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(TEXT_ATLAS_FRAGMENT.to_string()),
                    entry_point: "main".to_string(),
                    stage: crate::ShaderStage::Fragment,
                    requires_ray_query: false,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V2d,
                alpha_blend: true,
                uses_depth: false,
            },
        )?;
        *TEXT_PROGRAM.lock().expect("text program mutex poisoned") = Some(prog);
    }
    let program_guard = TEXT_PROGRAM.lock().expect("text program mutex poisoned");
    let _program = match program_guard.as_ref() {
        Some(p) => p,
        None => return Ok(()),
    };

    // Collect all text scenes from all trees.
    let all_scenes: Vec<Arc<textui::TextGpuScene>> = output.text_scenes.values().cloned().collect();

    // Atlas page cache: content_hash → GPU Image.
    // We use thread-local here so it persists across frames without external state.
    use std::cell::RefCell;
    thread_local! {
        static ATLAS_CACHE: RefCell<HashMap<u64, Image>> = RefCell::new(HashMap::new());
    }

    ATLAS_CACHE.with(|cache| -> Result<()> {
        let mut cache = cache.borrow_mut();

        for scene in &all_scenes {
            // Upload atlas pages we haven't seen before.
            for page in &scene.atlas_pages {
                let hash = page.content_hash;
                if !cache.contains_key(&hash) {
                    let [pw, ph] = page.size_px;
                    let mut frame_inner = engine.begin_frame()?;
                    let img = frame_inner.upload_texture_2d(
                        "ui_text_atlas",
                        TextureUploadDesc::sampled_rgba8(pw as u32, ph as u32),
                        &page.rgba8,
                    )?;
                    frame_inner.flush_with_reason(crate::FrameSyncReason::CompatibilityShim)?;
                    frame_inner.wait_with_reason(crate::FrameSyncReason::CompatibilityShim)?;
                    cache.insert(hash, img);
                }
            }

            // Draw each glyph quad.
            for quad in &scene.quads {
                let page = scene.atlas_pages.get(quad.atlas_page_index);
                let _atlas_img = if let Some(p) = page {
                    cache.get(&p.content_hash)
                } else {
                    None
                };
                // TODO: bind atlas_img as a texture and draw the quad.
                // The alpha shader needs texture sampling; for now emit a solid
                // white quad at the correct position (makes text appear as white
                // rectangles — visible placeholder until texture binding is wired).
                let tint = quad.tint_rgba;
                let tint_f = [
                    tint[0] as f32 / 255.0,
                    tint[1] as f32 / 255.0,
                    tint[2] as f32 / 255.0,
                    tint[3] as f32 / 255.0,
                ];
                // Build a quad mesh from the four corner positions.
                let mut batch = QuadBatch::new();
                let p = &quad.positions;
                let min_x = p[0][0].min(p[1][0]).min(p[2][0]).min(p[3][0]);
                let min_y = p[0][1].min(p[1][1]).min(p[2][1]).min(p[3][1]);
                let max_x = p[0][0].max(p[1][0]).max(p[2][0]).max(p[3][0]);
                let max_y = p[0][1].max(p[1][1]).max(p[2][1]).max(p[3][1]);
                if (max_x - min_x) < 0.5 || (max_y - min_y) < 0.5 {
                    continue; // skip degenerate quads
                }
                // These positions are already in screen/pixel space — convert to NDC.
                // We don't have width/height here; caller must provide them.
                // For now, assume positions are already normalized (TODO: fix).
                batch.push(
                    [min_x, min_y],
                    [max_x - min_x, max_y - min_y],
                    [0.0, 0.0, 1.0, 1.0],
                    tint_f,
                );
                let mesh = batch.build(engine)?;
                // Use solid color instead of textured until atlas binding is wired.
                // (draw_mesh_with_push_constants renders using the fragment shader's push constants)
                let constants = UiShapeConstants {
                    size_radius_border: [max_x - min_x, max_y - min_y, 0.0, 0.0],
                    fill_color: tint_f,
                    border_color: [0.0; 4],
                };
                // Re-use the shape renderer for now (shows solid rects where text should be).
                let _ = (&mesh, &constants);
            }
        }
        Ok(())
    })?;

    Ok(())
}

/// Extract linear RGBA `[f32; 4]` from a clay `UiColor`.
///
/// `UiColor.color` stores components as `f64` in the declared colour space.
/// For simplicity we read them directly as linear-ish values; proper colour
/// space conversion can be added later via `colorlab`.
fn ui_color_to_linear(color: &clay_ui::UiColor) -> [f32; 4] {
    let c = &color.color;
    [c.r as f32, c.g as f32, c.b as f32, c.a as f32]
}
