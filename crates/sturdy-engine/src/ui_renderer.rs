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
//   ScissorStart / ScissorEnd — CPU-clips generated quads to the active clip
//   Text       — rendered separately via draw_ui_text using the textui atlas
//                returned in UiFrameOutput::text_scenes
//   Image      — sampled from the renderer image registry by image_key
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
use std::sync::{Arc, RwLock};

use clay_ui::{GpuWorkQueue, RenderCommandKind, RenderData};

use crate::{
    Engine, GraphImage, Image, Mesh, MeshProgram, MeshProgramDesc, MeshVertexKind, QuadBatch,
    RenderFrame, Result, ShaderDesc, ShaderSource, ShaderStage, TextureUploadDesc,
};

const UI_SHAPE_FRAGMENT: &str = include_str!("../shaders/debug_overlay_ui_shape_fragment.slang");
const UI_IMAGE_FRAGMENT: &str = r#"
Texture2D<float4> ui_image;
SamplerState ui_image_sampler;

struct FSInput {
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
    float4 color : COLOR0;
};

float4 main(FSInput input) : SV_TARGET {
    return ui_image.SampleLevel(ui_image_sampler, input.uv, 0.0) * input.color;
}
"#;

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
    image_program: MeshProgram,
    images: RwLock<HashMap<String, Arc<Image>>>,
}

impl UiRenderer {
    /// Create a `UiRenderer`, compiling the built-in UI shape and image shaders.
    pub fn new(engine: &Engine) -> Result<Self> {
        let program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(UI_SHAPE_FRAGMENT.to_string()),
                    entry_point: "main".to_string(),
                    stage: ShaderStage::Fragment,
                    requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V2d,
                alpha_blend: true,
                uses_depth: false,
            },
        )?;
        let image_program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::Inline(UI_IMAGE_FRAGMENT.to_string()),
                    entry_point: "main".to_string(),
                    stage: ShaderStage::Fragment,
                    requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
                },
                vertex: None,
                vertex_kind: MeshVertexKind::V2d,
                alpha_blend: true,
                uses_depth: false,
            },
        )?;
        Ok(Self {
            program,
            image_program,
            images: RwLock::new(HashMap::new()),
        })
    }

    /// Register an image for Clay `Image` commands with the matching `image_key`.
    pub fn set_image(&self, image_key: impl Into<String>, image: Arc<Image>) {
        self.images
            .write()
            .expect("ui image registry rwlock poisoned")
            .insert(image_key.into(), image);
    }

    /// Remove a registered Clay image by key.
    pub fn remove_image(&self, image_key: &str) -> Option<Arc<Image>> {
        self.images
            .write()
            .expect("ui image registry rwlock poisoned")
            .remove(image_key)
    }

    /// Draw all supported commands from `queue` into `target`.
    ///
    /// `width` and `height` are the logical pixel dimensions of the render
    /// target — used to convert pixel-space rects to NDC coordinates.
    ///
    /// Text commands are handled separately via `draw_ui_text` and
    /// `UiFrameOutput::text_scenes`; image commands use this renderer's image
    /// registry.
    pub fn draw_queue(
        &self,
        frame: &RenderFrame,
        target: &GraphImage,
        queue: &GpuWorkQueue,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let engine = &self.program.engine;
        let mut clip_stack = vec![ClipRect::viewport(width, height)];
        for command in &queue.commands {
            let active_clip = *clip_stack
                .last()
                .expect("ui clip stack always has viewport root");
            match command.kind {
                RenderCommandKind::Rectangle => {
                    if let RenderData::Rectangle(data) = &command.data {
                        let Some(rect) = active_clip.clip_rect(command.rect) else {
                            continue;
                        };
                        let fill = ui_color_to_linear(&data.color);
                        let radius = data.corner_radius.x;
                        let constants = UiShapeConstants {
                            size_radius_border: [rect.size.width, rect.size.height, radius, 0.0],
                            fill_color: fill,
                            border_color: [0.0; 4],
                        };
                        let mesh = rect_mesh(engine, width, height, &rect)?;
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
                        let Some(rect) = active_clip.clip_rect(command.rect) else {
                            continue;
                        };
                        let border_color = ui_color_to_linear(&data.color);
                        let border_width = data.width.top;
                        let radius = data.corner_radius.x;
                        let constants = UiShapeConstants {
                            size_radius_border: [
                                rect.size.width,
                                rect.size.height,
                                radius,
                                border_width,
                            ],
                            fill_color: [0.0; 4],
                            border_color,
                        };
                        let mesh = rect_mesh(engine, width, height, &rect)?;
                        target.draw_mesh_with_push_constants(
                            &mesh,
                            &self.program,
                            crate::StageMask::FRAGMENT,
                            bytemuck::bytes_of(&constants),
                        )?;
                    }
                }

                RenderCommandKind::Image => {
                    if let RenderData::Image(data) = &command.data {
                        let image = {
                            let images = self
                                .images
                                .read()
                                .expect("ui image registry rwlock poisoned");
                            images.get(&data.image_key).cloned()
                        };
                        let Some(image) = image else {
                            continue;
                        };
                        let natural = clay_ui::Size::new(
                            image.desc().extent.width as f32,
                            image.desc().extent.height as f32,
                        );
                        let image_rect =
                            data.options
                                .fit
                                .fitted_rect(command.rect, natural, data.options.align);
                        let Some(clipped) = active_clip.clip_rect(image_rect) else {
                            continue;
                        };
                        let uv = clipped_uv(image_rect, clipped);
                        let tint = ui_color_to_linear(&data.tint);
                        let mesh = rect_mesh_with_uv(engine, width, height, &clipped, uv, tint)?;
                        frame.bind_image("ui_image", image.as_ref());
                        target.draw_mesh(&mesh, &self.image_program)?;
                    }
                }

                RenderCommandKind::ScissorStart => {
                    if let RenderData::Clip(data) = &command.data {
                        clip_stack.push(active_clip.intersect_axes(
                            command.rect,
                            data.horizontal,
                            data.vertical,
                        ));
                    } else {
                        clip_stack.push(active_clip);
                    }
                }

                RenderCommandKind::ScissorEnd => {
                    if clip_stack.len() > 1 {
                        clip_stack.pop();
                    }
                }

                // Text handled via UiFrameOutput::text_scenes (textui atlas).
                RenderCommandKind::Text | RenderCommandKind::Custom | RenderCommandKind::None => {}
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

fn rect_mesh_with_uv(
    engine: &Engine,
    width: u32,
    height: u32,
    rect: &clay_ui::Rect,
    uv_rect: [f32; 4],
    color: [f32; 4],
) -> Result<Mesh> {
    let (pos, size) = pixel_rect_to_ndc(
        rect.origin.x,
        rect.origin.y,
        rect.right(),
        rect.bottom(),
        width,
        height,
    );
    let mut batch = QuadBatch::new();
    batch.push(pos, size, uv_rect, color);
    batch.build(engine)
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct ClipRect {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl ClipRect {
    fn viewport(width: u32, height: u32) -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: width as f32,
            max_y: height as f32,
        }
    }

    fn intersect_axes(self, rect: clay_ui::Rect, horizontal: bool, vertical: bool) -> Self {
        let mut next = self;
        if horizontal {
            next.min_x = next.min_x.max(rect.origin.x);
            next.max_x = next.max_x.min(rect.right());
        }
        if vertical {
            next.min_y = next.min_y.max(rect.origin.y);
            next.max_y = next.max_y.min(rect.bottom());
        }
        next
    }

    fn clip_rect(self, rect: clay_ui::Rect) -> Option<clay_ui::Rect> {
        let min_x = rect.origin.x.max(self.min_x);
        let min_y = rect.origin.y.max(self.min_y);
        let max_x = rect.right().min(self.max_x);
        let max_y = rect.bottom().min(self.max_y);
        (max_x > min_x && max_y > min_y)
            .then(|| clay_ui::Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
    }
}

fn clipped_uv(original: clay_ui::Rect, clipped: clay_ui::Rect) -> [f32; 4] {
    let inv_w = 1.0 / original.size.width.max(1.0);
    let inv_h = 1.0 / original.size.height.max(1.0);
    let u0 = (clipped.origin.x - original.origin.x) * inv_w;
    let v0 = (clipped.origin.y - original.origin.y) * inv_h;
    let u1 = (clipped.right() - original.origin.x) * inv_w;
    let v1 = (clipped.bottom() - original.origin.y) * inv_h;
    [u0, v0, u1, v1]
}

// ── Text rendering ────────────────────────────────────────────────────────────

/// Text atlas shader — alpha mask path (same as text_overlay_alpha_fragment.slang).
const TEXT_ATLAS_FRAGMENT: &str = include_str!("../shaders/text_overlay_alpha_fragment.slang");

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
    frame: &RenderFrame,
    engine: &Engine,
    output: &clay_ui::UiFrameOutput,
    target: &GraphImage,
    width: u32,
    height: u32,
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
            requires_cooperative_matrix: false,
            uses_ser: false,
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
    let program = match program_guard.as_ref() {
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
                let atlas_img = if let Some(p) = page {
                    cache.get(&p.content_hash)
                } else {
                    None
                };
                let Some(atlas_img) = atlas_img else {
                    continue;
                };

                let tint = quad.tint_rgba;
                let tint_f = [
                    tint[0] as f32 / 255.0,
                    tint[1] as f32 / 255.0,
                    tint[2] as f32 / 255.0,
                    tint[3] as f32 / 255.0,
                ];

                let ([min_x, min_y], [max_x, max_y]) = quad_bounds(&quad.positions);
                if (max_x - min_x) < 0.5 || (max_y - min_y) < 0.5 {
                    continue; // skip degenerate quads
                }

                let ([u0, v0], [u1, v1]) = quad_bounds(&quad.uvs);
                let (pos, size) = pixel_rect_to_ndc(min_x, min_y, max_x, max_y, width, height);
                let mut batch = QuadBatch::new();
                batch.push(pos, size, [u0, v0, u1, v1], tint_f);
                let mesh = batch.build(engine)?;

                frame.bind_image("text_atlas", atlas_img);
                target.draw_mesh(&mesh, program)?;
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

fn quad_bounds(points: &[[f32; 2]; 4]) -> ([f32; 2], [f32; 2]) {
    let mut min_x = points[0][0];
    let mut min_y = points[0][1];
    let mut max_x = points[0][0];
    let mut max_y = points[0][1];
    for point in points.iter().skip(1) {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    ([min_x, min_y], [max_x, max_y])
}

fn pixel_rect_to_ndc(
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    width: u32,
    height: u32,
) -> ([f32; 2], [f32; 2]) {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let ndc_x = min_x / w * 2.0 - 1.0;
    let ndc_y = 1.0 - max_y / h * 2.0;
    let ndc_w = (max_x - min_x) / w * 2.0;
    let ndc_h = (max_y - min_y) / h * 2.0;
    ([ndc_x, ndc_y], [ndc_w, ndc_h])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_rect_to_ndc_uses_viewport_dimensions() {
        let (pos, size) = pixel_rect_to_ndc(100.0, 50.0, 300.0, 250.0, 800, 600);

        assert_eq!(pos, [-0.75, 1.0 - 250.0 / 600.0 * 2.0]);
        assert_eq!(size, [0.5, 200.0 / 600.0 * 2.0]);
    }

    #[test]
    fn quad_bounds_handles_unordered_points() {
        let (min, max) = quad_bounds(&[[4.0, 1.0], [2.0, 9.0], [7.0, 3.0], [5.0, -1.0]]);

        assert_eq!(min, [2.0, -1.0]);
        assert_eq!(max, [7.0, 9.0]);
    }

    #[test]
    fn clip_rect_intersects_enabled_axes_only() {
        let clip = ClipRect::viewport(100, 100);
        let horizontal_only =
            clip.intersect_axes(clay_ui::Rect::new(25.0, 30.0, 50.0, 10.0), true, false);

        assert_eq!(
            horizontal_only,
            ClipRect {
                min_x: 25.0,
                min_y: 0.0,
                max_x: 75.0,
                max_y: 100.0,
            }
        );
    }

    #[test]
    fn clipped_uv_preserves_sample_region_after_scissor() {
        let original = clay_ui::Rect::new(100.0, 50.0, 200.0, 100.0);
        let clipped = clay_ui::Rect::new(150.0, 75.0, 100.0, 50.0);

        assert_eq!(clipped_uv(original, clipped), [0.25, 0.25, 0.75, 0.75]);
    }
}
