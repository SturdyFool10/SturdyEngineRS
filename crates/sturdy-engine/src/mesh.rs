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
