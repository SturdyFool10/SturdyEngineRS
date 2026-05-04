use crate::{
    BoundingSphere, Buffer, BufferDesc, BufferUsage, Engine, IndexFormat, Result,
    VertexAttributeDesc, VertexFormat,
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
    /// Tangent vector in object space. `w` encodes the handedness of the
    /// bitangent: `bitangent = cross(normal, tangent.xyz) * tangent.w`.
    /// Set to `[1, 0, 0, 1]` when tangents are unavailable (e.g. meshes
    /// with no UV mapping). Correct tangents are required for normal mapping.
    pub tangent: [f32; 4],
}

pub struct Mesh {
    pub(crate) vertex_buffer: Buffer,
    pub(crate) index_buffer: Option<Buffer>,
    pub(crate) vertex_count: u32,
    pub(crate) index_count: u32,
    pub(crate) index_format: IndexFormat,
    /// Conservative bounding sphere in object (local) space.
    /// Used by CPU frustum culling (`ComputeIndirect` backend) and the
    /// task shader bounds buffer (`MeshShader` / `VirtualizedRaster` backends).
    pub bounding_sphere: BoundingSphere,
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
            bounding_sphere: BoundingSphere::EMPTY,
        })
    }

    pub fn new_3d(engine: &Engine, vertices: &[Vertex3d]) -> Result<Self> {
        let vertex_buffer = upload_slice(engine, vertices, BufferUsage::VERTEX)?;
        let positions: Vec<[f32; 3]> = vertices.iter().map(|v| v.position).collect();
        Ok(Self {
            vertex_buffer,
            index_buffer: None,
            vertex_count: vertices.len() as u32,
            index_count: 0,
            index_format: IndexFormat::Uint32,
            bounding_sphere: BoundingSphere::from_positions(&positions),
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
            bounding_sphere: BoundingSphere::EMPTY,
        })
    }

    pub fn indexed_3d(engine: &Engine, vertices: &[Vertex3d], indices: &[u32]) -> Result<Self> {
        let vertex_buffer = upload_slice(engine, vertices, BufferUsage::VERTEX)?;
        let index_buffer = upload_slice(engine, indices, BufferUsage::INDEX)?;
        let positions: Vec<[f32; 3]> = vertices.iter().map(|v| v.position).collect();
        Ok(Self {
            vertex_buffer,
            index_buffer: Some(index_buffer),
            vertex_count: vertices.len() as u32,
            index_count: indices.len() as u32,
            index_format: IndexFormat::Uint32,
            bounding_sphere: BoundingSphere::from_positions(&positions),
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
        let t = [1.0f32, 0.0, 0.0, 1.0]; // placeholder; overwritten by compute_tangents
        let mut verts: Vec<Vertex3d> = vec![
            // +Z front
            Vertex3d { position: [-s, -s, s], normal: [0., 0., 1.], uv: [0., 1.], tangent: t },
            Vertex3d { position: [s, -s, s],  normal: [0., 0., 1.], uv: [1., 1.], tangent: t },
            Vertex3d { position: [s, s, s],   normal: [0., 0., 1.], uv: [1., 0.], tangent: t },
            Vertex3d { position: [-s, s, s],  normal: [0., 0., 1.], uv: [0., 0.], tangent: t },
            // -Z back
            Vertex3d { position: [s, -s, -s],  normal: [0., 0., -1.], uv: [0., 1.], tangent: t },
            Vertex3d { position: [-s, -s, -s], normal: [0., 0., -1.], uv: [1., 1.], tangent: t },
            Vertex3d { position: [-s, s, -s],  normal: [0., 0., -1.], uv: [1., 0.], tangent: t },
            Vertex3d { position: [s, s, -s],   normal: [0., 0., -1.], uv: [0., 0.], tangent: t },
            // +X right
            Vertex3d { position: [s, -s, -s], normal: [1., 0., 0.], uv: [0., 1.], tangent: t },
            Vertex3d { position: [s, -s, s],  normal: [1., 0., 0.], uv: [1., 1.], tangent: t },
            Vertex3d { position: [s, s, s],   normal: [1., 0., 0.], uv: [1., 0.], tangent: t },
            Vertex3d { position: [s, s, -s],  normal: [1., 0., 0.], uv: [0., 0.], tangent: t },
            // -X left
            Vertex3d { position: [-s, -s, s],  normal: [-1., 0., 0.], uv: [0., 1.], tangent: t },
            Vertex3d { position: [-s, -s, -s], normal: [-1., 0., 0.], uv: [1., 1.], tangent: t },
            Vertex3d { position: [-s, s, -s],  normal: [-1., 0., 0.], uv: [1., 0.], tangent: t },
            Vertex3d { position: [-s, s, s],   normal: [-1., 0., 0.], uv: [0., 0.], tangent: t },
            // +Y top
            Vertex3d { position: [-s, s, -s], normal: [0., 1., 0.], uv: [0., 0.], tangent: t },
            Vertex3d { position: [s, s, -s],  normal: [0., 1., 0.], uv: [1., 0.], tangent: t },
            Vertex3d { position: [s, s, s],   normal: [0., 1., 0.], uv: [1., 1.], tangent: t },
            Vertex3d { position: [-s, s, s],  normal: [0., 1., 0.], uv: [0., 1.], tangent: t },
            // -Y bottom
            Vertex3d { position: [-s, -s, s],  normal: [0., -1., 0.], uv: [0., 0.], tangent: t },
            Vertex3d { position: [s, -s, s],   normal: [0., -1., 0.], uv: [1., 0.], tangent: t },
            Vertex3d { position: [s, -s, -s],  normal: [0., -1., 0.], uv: [1., 1.], tangent: t },
            Vertex3d { position: [-s, -s, -s], normal: [0., -1., 0.], uv: [0., 1.], tangent: t },
        ];
        #[rustfmt::skip]
        let idx: Vec<u32> = vec![
             0, 1, 2,  0, 2, 3,
             4, 5, 6,  4, 6, 7,
             8, 9,10,  8,10,11,
            12,13,14, 12,14,15,
            16,17,18, 16,18,19,
            20,21,22, 20,22,23,
        ];
        compute_tangents(&mut verts, &idx);
        Self::indexed_3d(engine, &verts, &idx)
    }

    /// Flat horizontal plane (in the XZ plane, normal pointing +Y) centered at the origin.
    ///
    /// `width` extends along X, `depth` along Z. UVs run 0→1 across the full quad.
    pub fn plane(engine: &Engine, width: f32, depth: f32) -> Result<Self> {
        let hw = width * 0.5;
        let hd = depth * 0.5;
        let t = [1.0f32, 0.0, 0.0, 1.0];
        let mut verts: Vec<Vertex3d> = vec![
            Vertex3d { position: [-hw, 0., -hd], normal: [0., 1., 0.], uv: [0., 0.], tangent: t },
            Vertex3d { position: [hw, 0., -hd],  normal: [0., 1., 0.], uv: [1., 0.], tangent: t },
            Vertex3d { position: [hw, 0., hd],   normal: [0., 1., 0.], uv: [1., 1.], tangent: t },
            Vertex3d { position: [-hw, 0., hd],  normal: [0., 1., 0.], uv: [0., 1.], tangent: t },
        ];
        let idx: Vec<u32> = vec![0, 1, 2, 0, 2, 3];
        compute_tangents(&mut verts, &idx);
        Self::indexed_3d(engine, &verts, &idx)
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
                    tangent: [1.0, 0.0, 0.0, 1.0],
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

        compute_tangents(&mut verts, &idx);
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
        VertexAttributeDesc {
            location: 3,
            binding: 0,
            format: VertexFormat::Float32x4,
            offset: std::mem::offset_of!(Vertex3d, tangent) as u32,
        },
    ]
}

/// Compute per-vertex tangents for an indexed triangle list using the
/// UV-derivative method (surface-gradient tangent frame).
///
/// `vertices` must be indexed by `indices` in triangle-list order. The
/// result is Gram-Schmidt orthogonalised against the vertex normal and the
/// handedness sign is stored in `tangent.w`.
///
/// Call this after building a mesh from positions/normals/UVs whenever
/// the source format doesn't supply tangents (OBJ, STL, procedural meshes).
/// GLTF should extract tangents from the `TANGENT` attribute instead.
pub fn compute_tangents(vertices: &mut Vec<Vertex3d>, indices: &[u32]) {
    let n = vertices.len();
    let mut tan_accum = vec![[0.0f32; 3]; n]; // sum of tangent contributions
    let mut bitan_accum = vec![[0.0f32; 3]; n]; // sum of bitangent contributions

    let tri_count = indices.len() / 3;
    for t in 0..tri_count {
        let i0 = indices[t * 3    ] as usize;
        let i1 = indices[t * 3 + 1] as usize;
        let i2 = indices[t * 3 + 2] as usize;

        let p0 = glam::Vec3::from_array(vertices[i0].position);
        let p1 = glam::Vec3::from_array(vertices[i1].position);
        let p2 = glam::Vec3::from_array(vertices[i2].position);

        let uv0 = glam::Vec2::from_array(vertices[i0].uv);
        let uv1 = glam::Vec2::from_array(vertices[i1].uv);
        let uv2 = glam::Vec2::from_array(vertices[i2].uv);

        let dp1 = p1 - p0;
        let dp2 = p2 - p0;
        let duv1 = uv1 - uv0;
        let duv2 = uv2 - uv0;

        let denom = duv1.x * duv2.y - duv2.x * duv1.y;
        if denom.abs() < 1e-8 {
            continue;
        }
        let r = 1.0 / denom;
        let tan   = (dp1 * duv2.y - dp2 * duv1.y) * r;
        let bitan = (dp2 * duv1.x - dp1 * duv2.x) * r;

        for &idx in &[i0, i1, i2] {
            for j in 0..3 {
                tan_accum[idx][j]   += tan[j];
                bitan_accum[idx][j] += bitan[j];
            }
        }
    }

    // Gram-Schmidt orthogonalise and store handedness.
    for (i, v) in vertices.iter_mut().enumerate() {
        let n = glam::Vec3::from_array(v.normal);
        let t = glam::Vec3::from_array(tan_accum[i]);
        let bt = glam::Vec3::from_array(bitan_accum[i]);
        if t.length_squared() < 1e-8 {
            v.tangent = [1.0, 0.0, 0.0, 1.0];
            continue;
        }
        let t_ortho = (t - n * n.dot(t)).normalize();
        let handedness = if n.cross(t_ortho).dot(bt) < 0.0 { -1.0_f32 } else { 1.0_f32 };
        v.tangent = [t_ortho.x, t_ortho.y, t_ortho.z, handedness];
    }
}
