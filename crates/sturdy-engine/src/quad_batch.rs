use crate::{mesh::Vertex2d, Engine, Mesh, Result};

/// Builds a batched mesh of textured, tinted 2D quads.
///
/// Each quad maps to 4 vertices and 2 triangles. Positions are in NDC unless you
/// scale them in the vertex or fragment shader via push constants.
///
/// ```rust,ignore
/// let mut batch = QuadBatch::new();
/// batch.push([-0.5, -0.5], [1.0, 1.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
/// let mesh = batch.build(&engine)?;
/// ```
#[derive(Default)]
pub struct QuadBatch {
    vertices: Vec<Vertex2d>,
    indices: Vec<u32>,
}

impl QuadBatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a quad.
    ///
    /// - `pos`: NDC position of the quad's top-left corner `[x, y]`.
    /// - `size`: Width and height in NDC `[w, h]`.
    /// - `uv_rect`: UV region `[u0, v0, u1, v1]`.
    /// - `color`: Per-quad tint as `[r, g, b, a]`.
    pub fn push(
        &mut self,
        pos: [f32; 2],
        size: [f32; 2],
        uv_rect: [f32; 4],
        color: [f32; 4],
    ) -> &mut Self {
        let base = self.vertices.len() as u32;
        let [x, y] = pos;
        let [w, h] = size;
        let [u0, v0, u1, v1] = uv_rect;

        self.vertices.extend_from_slice(&[
            Vertex2d { position: [x, y],         uv: [u0, v0], color },
            Vertex2d { position: [x + w, y],     uv: [u1, v0], color },
            Vertex2d { position: [x + w, y + h], uv: [u1, v1], color },
            Vertex2d { position: [x, y + h],     uv: [u0, v1], color },
        ]);
        self.indices.extend_from_slice(&[
            base, base + 1, base + 2,
            base, base + 2, base + 3,
        ]);
        self
    }

    pub fn quad_count(&self) -> usize {
        self.vertices.len() / 4
    }

    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Upload the current batch to GPU memory as an indexed mesh.
    pub fn build(&self, engine: &Engine) -> Result<Mesh> {
        Mesh::indexed_2d(engine, &self.vertices, &self.indices)
    }
}
