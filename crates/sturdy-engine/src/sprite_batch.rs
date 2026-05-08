// First-class 2D sprite batch renderer.
//
// Provides pixel-space, rotation/scale-aware sprite rendering backed by a
// dynamic CPU-built mesh. All sprites in a batch share one texture and one
// draw call. Multiple batches can be composed per frame.
//
// Usage:
//   let renderer = SpriteRenderer::new(&engine, 1280, 720)?;
//   let mut batch = SpriteBatch::new();
//   batch.push(Sprite { position: [64.0, 64.0], size: [32.0, 32.0], ..Default::default() });
//   renderer.draw(&batch, &output_image, &frame, &some_texture)?;
//
// Roadmap: 2D and instanced rendering — first-class 2D sprite/batch path.

use std::path::PathBuf;

use crate::{
    Engine, GraphImage, Image, Mesh, MeshProgram, MeshProgramDesc, MeshVertexKind,
    RenderFrame, Result, ShaderDesc, ShaderSource, ShaderStage, StageMask, push_constants,
    mesh::Vertex2d,
};

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

// ── Push constants ────────────────────────────────────────────────────────────

#[push_constants]
struct SpriteConstants {
    viewport_w: f32,
    viewport_h: f32,
    _pad0: f32,
    _pad1: f32,
}

// ── Sprite ────────────────────────────────────────────────────────────────────

/// A single sprite to be rendered by a `SpriteBatch`.
///
/// Coordinates are in pixels with the origin at the top-left of the viewport,
/// Y-axis pointing down (standard 2D screen convention).
#[derive(Clone, Debug)]
pub struct Sprite {
    /// Position of the sprite's anchor point (pixels, top-left origin, Y-down).
    pub position: [f32; 2],
    /// Width and height in pixels.
    pub size: [f32; 2],
    /// Rotation around `anchor` in radians, clockwise positive.
    pub rotation: f32,
    /// Pivot / anchor point within the sprite in normalized coordinates \[0, 1\].
    /// `[0, 0]` = top-left corner (default), `[0.5, 0.5]` = centre.
    pub anchor: [f32; 2],
    /// UV rectangle within the texture: `[u0, v0, u1, v1]` in normalized coordinates.
    /// Default `[0, 0, 1, 1]` = the whole texture.
    pub uv_rect: [f32; 4],
    /// RGBA tint multiplied with the texture sample. Default white `[1, 1, 1, 1]`.
    pub color: [f32; 4],
    /// Sort key for depth ordering within the batch. Higher values are drawn on top.
    /// Sprites are sorted by this value before upload. Default 0.
    pub z_order: i32,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            size: [64.0, 64.0],
            rotation: 0.0,
            anchor: [0.0, 0.0],
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
            z_order: 0,
        }
    }
}

// ── SpriteBatch ───────────────────────────────────────────────────────────────

/// Accumulates sprites in CPU memory and builds a `Mesh` for rendering.
///
/// Sprites are sorted by `z_order` ascending (lower z_order = drawn first).
/// Build a `Mesh` with `build(&engine)` then draw with `SpriteRenderer::draw()`.
///
/// `SpriteBatch` is lightweight and intended to be rebuilt each frame.
/// ```ignore
/// let mut batch = SpriteBatch::new();
/// batch.push(Sprite { position: [100.0, 200.0], size: [32.0, 32.0], ..Default::default() });
/// let mesh = batch.build(&engine)?;
/// renderer.draw_mesh(&mesh, &output, &frame, &texture)?;
/// ```
pub struct SpriteBatch {
    sprites: Vec<Sprite>,
}

impl SpriteBatch {
    pub fn new() -> Self {
        Self { sprites: Vec::new() }
    }

    /// Add a sprite to the batch.
    pub fn push(&mut self, sprite: Sprite) -> &mut Self {
        self.sprites.push(sprite);
        self
    }

    /// Add a simple axis-aligned sprite (no rotation) with defaults.
    pub fn push_simple(
        &mut self,
        position: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
    ) -> &mut Self {
        self.push(Sprite { position, size, color, ..Default::default() })
    }

    /// Add a textured sprite with a UV sub-rectangle (for sprite sheets / atlas).
    pub fn push_atlas(
        &mut self,
        position: [f32; 2],
        size: [f32; 2],
        uv_rect: [f32; 4],
        color: [f32; 4],
    ) -> &mut Self {
        self.push(Sprite { position, size, uv_rect, color, ..Default::default() })
    }

    pub fn len(&self) -> usize { self.sprites.len() }
    pub fn is_empty(&self) -> bool { self.sprites.is_empty() }
    pub fn clear(&mut self) { self.sprites.clear(); }

    /// Build a GPU mesh from the current sprites, sorted by `z_order`.
    ///
    /// Call once per frame after all sprites are pushed.
    pub fn build(&self, engine: &Engine) -> Result<Option<Mesh>> {
        if self.sprites.is_empty() {
            return Ok(None);
        }

        // Sort a copy by z_order ascending (painter's algorithm).
        let mut sorted: Vec<&Sprite> = self.sprites.iter().collect();
        sorted.sort_by_key(|s| s.z_order);

        let mut vertices: Vec<Vertex2d> = Vec::with_capacity(sorted.len() * 4);
        let mut indices:  Vec<u32>      = Vec::with_capacity(sorted.len() * 6);

        for sprite in &sorted {
            let base = vertices.len() as u32;
            let [u0, v0, u1, v1] = sprite.uv_rect;
            let [ax, ay] = sprite.anchor;
            let [w, h]   = sprite.size;
            let [px, py] = sprite.position;
            let color    = sprite.color;

            // Compute the four corners relative to the anchor, pre-rotated.
            let corners = [
                [-ax * w,        -ay * h       ],  // top-left
                [(1.0 - ax) * w, -ay * h       ],  // top-right
                [(1.0 - ax) * w, (1.0 - ay) * h], // bottom-right
                [-ax * w,        (1.0 - ay) * h],  // bottom-left
            ];
            let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];

            let (sin_r, cos_r) = sprite.rotation.sin_cos();

            for (corner, uv) in corners.iter().zip(uvs.iter()) {
                // Rotate around anchor then translate to world position.
                let rx = corner[0] * cos_r - corner[1] * sin_r;
                let ry = corner[0] * sin_r + corner[1] * cos_r;
                vertices.push(Vertex2d {
                    position: [px + rx, py + ry],
                    uv: *uv,
                    color,
                });
            }

            indices.extend_from_slice(&[
                base, base + 1, base + 2,
                base, base + 2, base + 3,
            ]);
        }

        Ok(Some(Mesh::indexed_2d(engine, &vertices, &indices)?))
    }
}

impl Default for SpriteBatch { fn default() -> Self { Self::new() } }

// ── SpriteRenderer ────────────────────────────────────────────────────────────

/// Renders `SpriteBatch` meshes into a `GraphImage` using the sprite shaders.
///
/// Uses alpha blending for transparency. Maintains the viewport size for the
/// pixel → NDC push constant.
///
/// # Zero config
/// `SpriteRenderer::new(&engine, width, height)?` is all the setup needed.
///
/// # Example
/// ```ignore
/// let renderer = SpriteRenderer::new(&engine, 1280, 720)?;
/// // Each frame:
/// let mut batch = SpriteBatch::new();
/// batch.push(Sprite { position: [100.0, 200.0], size: [64.0, 64.0], ..Default::default() });
/// let mesh = batch.build(&engine)?.unwrap();
/// frame.bind_image("sprite_texture", &my_texture);
/// renderer.draw_mesh(&mesh, &output, &frame)?;
/// ```
pub struct SpriteRenderer {
    program: MeshProgram,
    /// Viewport width and height in pixels. Update with `set_viewport` on resize.
    pub viewport_width:  f32,
    pub viewport_height: f32,
    /// 1×1 white pixel texture used when no sprite_texture is bound.
    white_pixel: Image,
}

impl SpriteRenderer {
    pub fn new(engine: &Engine, viewport_width: u32, viewport_height: u32) -> Result<Self> {
        let program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                vertex: Some(ShaderDesc {
                    source: ShaderSource::File(engine_shader("sprite_vertex.slang")),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Vertex,
                }),
                fragment: ShaderDesc {
                    source: ShaderSource::File(engine_shader("sprite_fragment.slang")),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex_kind: MeshVertexKind::V2d,
                alpha_blend: true,
                uses_depth: false,
            },
        )?;

        let white_pixel = engine.generate_texture_2d("sprite_white_pixel", 1, 1, |_, _| {
            [255, 255, 255, 255]
        })?;

        Ok(Self {
            program,
            viewport_width: viewport_width as f32,
            viewport_height: viewport_height as f32,
            white_pixel,
        })
    }

    /// Update the viewport size (call on window resize).
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.viewport_width  = width  as f32;
        self.viewport_height = height as f32;
    }

    /// Build and draw a `SpriteBatch` in one call.
    ///
    /// Binds `sprite_texture` automatically from the pre-bound frame binding,
    /// or falls back to the internal 1×1 white pixel if none is bound.
    ///
    /// Call `frame.bind_image("sprite_texture", &tex)` before this to set the texture.
    pub fn draw(
        &self,
        batch: &SpriteBatch,
        output: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> Result<()> {
        let mesh = match batch.build(engine)? {
            Some(m) => m,
            None => return Ok(()),  // empty batch — nothing to draw
        };
        self.draw_mesh(&mesh, output, frame)
    }

    /// Draw a pre-built `Mesh` (built via `SpriteBatch::build`).
    ///
    /// Bind `sprite_texture` on `frame` before calling. Falls back to white pixel.
    pub fn draw_mesh(
        &self,
        mesh: &Mesh,
        output: &GraphImage,
        frame: &RenderFrame,
    ) -> Result<()> {
        // If the caller hasn't bound a texture, use the white pixel fallback.
        if frame.find_image_by_name("sprite_texture").is_none() {
            frame.bind_image("sprite_texture", &self.white_pixel);
        }

        let constants = SpriteConstants {
            viewport_w: self.viewport_width,
            viewport_h: self.viewport_height,
            _pad0: 0.0,
            _pad1: 0.0,
        };

        output.draw_mesh_with_push_constants(
            mesh,
            &self.program,
            StageMask::VERTEX,
            bytemuck::bytes_of(&constants),
        )
    }
}
