use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, IndexFormat, Result, VertexAttributeDesc, VertexFormat,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct Vertex2d {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct Vertex3d {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

pub struct Mesh {
    pub(crate) vertex_buffer: Buffer,
    pub(crate) index_buffer: Option<Buffer>,
    pub(crate) vertex_count: u32,
    pub(crate) index_count: u32,
    pub(crate) index_format: IndexFormat,
}

impl Mesh {
    pub fn new_2d(engine: &Engine, vertices: &[Vertex2d]) -> Result<Self> {
        let vertex_buffer = upload_slice(engine, vertices, BufferUsage::VERTEX)?;
        Ok(Self {
            vertex_buffer,
            index_buffer: None,
            vertex_count: vertices.len() as u32,
            index_count: 0,
            index_format: IndexFormat::Uint32,
        })
    }

    pub fn new_3d(engine: &Engine, vertices: &[Vertex3d]) -> Result<Self> {
        let vertex_buffer = upload_slice(engine, vertices, BufferUsage::VERTEX)?;
        Ok(Self {
            vertex_buffer,
            index_buffer: None,
            vertex_count: vertices.len() as u32,
            index_count: 0,
            index_format: IndexFormat::Uint32,
        })
    }

    pub fn indexed_2d(engine: &Engine, vertices: &[Vertex2d], indices: &[u32]) -> Result<Self> {
        let vertex_buffer = upload_slice(engine, vertices, BufferUsage::VERTEX)?;
        let index_buffer = upload_slice(engine, indices, BufferUsage::INDEX)?;
        Ok(Self {
            vertex_buffer,
            index_buffer: Some(index_buffer),
            vertex_count: vertices.len() as u32,
            index_count: indices.len() as u32,
            index_format: IndexFormat::Uint32,
        })
    }

    pub fn indexed_3d(engine: &Engine, vertices: &[Vertex3d], indices: &[u32]) -> Result<Self> {
        let vertex_buffer = upload_slice(engine, vertices, BufferUsage::VERTEX)?;
        let index_buffer = upload_slice(engine, indices, BufferUsage::INDEX)?;
        Ok(Self {
            vertex_buffer,
            index_buffer: Some(index_buffer),
            vertex_count: vertices.len() as u32,
            index_count: indices.len() as u32,
            index_format: IndexFormat::Uint32,
        })
    }

    pub fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub fn is_indexed(&self) -> bool {
        self.index_buffer.is_some()
    }
}

impl Mesh {
    /// Unit cube centered at the origin with hard per-face normals and per-face UVs.
    ///
    /// `size` is the side length (half-extent = `size / 2`). Each face is a
    /// separate quad so normals are sharp at edges. UVs tile 0→1 per face.
    pub fn cube(engine: &Engine, size: f32) -> Result<Self> {
        let s = size * 0.5;
        let verts: &[Vertex3d] = &[
            // +Z front
            Vertex3d {
                position: [-s, -s, s],
                normal: [0., 0., 1.],
                uv: [0., 1.],
            },
            Vertex3d {
                position: [s, -s, s],
                normal: [0., 0., 1.],
                uv: [1., 1.],
            },
            Vertex3d {
                position: [s, s, s],
                normal: [0., 0., 1.],
                uv: [1., 0.],
            },
            Vertex3d {
                position: [-s, s, s],
                normal: [0., 0., 1.],
                uv: [0., 0.],
            },
            // -Z back
            Vertex3d {
                position: [s, -s, -s],
                normal: [0., 0., -1.],
                uv: [0., 1.],
            },
            Vertex3d {
                position: [-s, -s, -s],
                normal: [0., 0., -1.],
                uv: [1., 1.],
            },
            Vertex3d {
                position: [-s, s, -s],
                normal: [0., 0., -1.],
                uv: [1., 0.],
            },
            Vertex3d {
                position: [s, s, -s],
                normal: [0., 0., -1.],
                uv: [0., 0.],
            },
            // +X right
            Vertex3d {
                position: [s, -s, -s],
                normal: [1., 0., 0.],
                uv: [0., 1.],
            },
            Vertex3d {
                position: [s, -s, s],
                normal: [1., 0., 0.],
                uv: [1., 1.],
            },
            Vertex3d {
                position: [s, s, s],
                normal: [1., 0., 0.],
                uv: [1., 0.],
            },
            Vertex3d {
                position: [s, s, -s],
                normal: [1., 0., 0.],
                uv: [0., 0.],
            },
            // -X left
            Vertex3d {
                position: [-s, -s, s],
                normal: [-1., 0., 0.],
                uv: [0., 1.],
            },
            Vertex3d {
                position: [-s, -s, -s],
                normal: [-1., 0., 0.],
                uv: [1., 1.],
            },
            Vertex3d {
                position: [-s, s, -s],
                normal: [-1., 0., 0.],
                uv: [1., 0.],
            },
            Vertex3d {
                position: [-s, s, s],
                normal: [-1., 0., 0.],
                uv: [0., 0.],
            },
            // +Y top
            Vertex3d {
                position: [-s, s, -s],
                normal: [0., 1., 0.],
                uv: [0., 0.],
            },
            Vertex3d {
                position: [s, s, -s],
                normal: [0., 1., 0.],
                uv: [1., 0.],
            },
            Vertex3d {
                position: [s, s, s],
                normal: [0., 1., 0.],
                uv: [1., 1.],
            },
            Vertex3d {
                position: [-s, s, s],
                normal: [0., 1., 0.],
                uv: [0., 1.],
            },
            // -Y bottom
            Vertex3d {
                position: [-s, -s, s],
                normal: [0., -1., 0.],
                uv: [0., 0.],
            },
            Vertex3d {
                position: [s, -s, s],
                normal: [0., -1., 0.],
                uv: [1., 0.],
            },
            Vertex3d {
                position: [s, -s, -s],
                normal: [0., -1., 0.],
                uv: [1., 1.],
            },
            Vertex3d {
                position: [-s, -s, -s],
                normal: [0., -1., 0.],
                uv: [0., 1.],
            },
        ];
        #[rustfmt::skip]
        let idx: &[u32] = &[
             0, 1, 2,  0, 2, 3,
             4, 5, 6,  4, 6, 7,
             8, 9,10,  8,10,11,
            12,13,14, 12,14,15,
            16,17,18, 16,18,19,
            20,21,22, 20,22,23,
        ];
        Self::indexed_3d(engine, verts, idx)
    }

    /// Flat horizontal plane (in the XZ plane, normal pointing +Y) centered at the origin.
    ///
    /// `width` extends along X, `depth` along Z. UVs run 0→1 across the full quad.
    pub fn plane(engine: &Engine, width: f32, depth: f32) -> Result<Self> {
        let hw = width * 0.5;
        let hd = depth * 0.5;
        let verts: &[Vertex3d] = &[
            Vertex3d {
                position: [-hw, 0., -hd],
                normal: [0., 1., 0.],
                uv: [0., 0.],
            },
            Vertex3d {
                position: [hw, 0., -hd],
                normal: [0., 1., 0.],
                uv: [1., 0.],
            },
            Vertex3d {
                position: [hw, 0., hd],
                normal: [0., 1., 0.],
                uv: [1., 1.],
            },
            Vertex3d {
                position: [-hw, 0., hd],
                normal: [0., 1., 0.],
                uv: [0., 1.],
            },
        ];
        let idx: &[u32] = &[0, 1, 2, 0, 2, 3];
        Self::indexed_3d(engine, verts, idx)
    }

    /// UV sphere centered at the origin.
    ///
    /// `rings` controls horizontal latitude bands (minimum 2), `segments` controls
    /// longitudinal slices (minimum 3). Normals point outward. UVs wrap once around.
    pub fn uv_sphere(engine: &Engine, radius: f32, rings: u32, segments: u32) -> Result<Self> {
        let rings = rings.max(2);
        let segments = segments.max(3);
        let mut verts: Vec<Vertex3d> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();

        for ring in 0..=rings {
            let phi = std::f32::consts::PI * ring as f32 / rings as f32;
            let (sin_phi, cos_phi) = phi.sin_cos();
            for seg in 0..=segments {
                let theta = 2.0 * std::f32::consts::PI * seg as f32 / segments as f32;
                let (sin_theta, cos_theta) = theta.sin_cos();
                let nx = sin_phi * cos_theta;
                let ny = cos_phi;
                let nz = sin_phi * sin_theta;
                verts.push(Vertex3d {
                    position: [nx * radius, ny * radius, nz * radius],
                    normal: [nx, ny, nz],
                    uv: [seg as f32 / segments as f32, ring as f32 / rings as f32],
                });
            }
        }

        let stride = segments + 1;
        for ring in 0..rings {
            for seg in 0..segments {
                let a = ring * stride + seg;
                let b = a + stride;
                idx.extend_from_slice(&[a, b, a + 1, b, b + 1, a + 1]);
            }
        }

        Self::indexed_3d(engine, &verts, &idx)
    }
}

fn upload_slice<T>(engine: &Engine, data: &[T], usage: BufferUsage) -> Result<Buffer> {
    let buffer = engine.create_buffer(BufferDesc {
        size: std::mem::size_of_val(data) as u64,
        usage,
    })?;
    buffer.write(0, bytes_of_slice(data))?;
    Ok(buffer)
}

fn bytes_of_slice<T>(data: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), std::mem::size_of_val(data)) }
}

pub(crate) fn vertex2d_attributes() -> Vec<VertexAttributeDesc> {
    vec![
        VertexAttributeDesc {
            location: 0,
            binding: 0,
            format: VertexFormat::Float32x2,
            offset: std::mem::offset_of!(Vertex2d, position) as u32,
        },
        VertexAttributeDesc {
            location: 1,
            binding: 0,
            format: VertexFormat::Float32x2,
            offset: std::mem::offset_of!(Vertex2d, uv) as u32,
        },
        VertexAttributeDesc {
            location: 2,
            binding: 0,
            format: VertexFormat::Float32x4,
            offset: std::mem::offset_of!(Vertex2d, color) as u32,
        },
    ]
}

pub(crate) fn vertex3d_attributes() -> Vec<VertexAttributeDesc> {
    vec![
        VertexAttributeDesc {
            location: 0,
            binding: 0,
            format: VertexFormat::Float32x3,
            offset: std::mem::offset_of!(Vertex3d, position) as u32,
        },
        VertexAttributeDesc {
            location: 1,
            binding: 0,
            format: VertexFormat::Float32x3,
            offset: std::mem::offset_of!(Vertex3d, normal) as u32,
        },
        VertexAttributeDesc {
            location: 2,
            binding: 0,
            format: VertexFormat::Float32x2,
            offset: std::mem::offset_of!(Vertex3d, uv) as u32,
        },
    ]
}
