use crate::{Buffer, BufferDesc, BufferUsage, Engine, Error, RenderFrame, Result};

/// Typed GPU data block for shader-readable information that does not fit well
/// in push constants.
///
/// Use [`ShaderData::storage`] for `StructuredBuffer<T>` data and
/// [`ShaderData::uniform`] for `ConstantBuffer<T>`/uniform-buffer style data.
/// Bind it by reflected shader variable name with [`bind`](Self::bind):
///
/// ```ignore
/// #[repr(C)]
/// #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
/// struct SimParams { wind: [f32; 4], time: f32, _pad: [f32; 3] }
///
/// let params = ShaderData::uniform(&engine, &SimParams { /* ... */ })?;
/// params.bind(&frame, "sim_params");
/// ```
pub struct ShaderData {
    buffer: Buffer,
    usage: BufferUsage,
    len: usize,
    byte_len: u64,
    capacity_bytes: u64,
    element_stride: u64,
}

impl ShaderData {
    /// Create a shader storage buffer initialized from a typed slice.
    pub fn storage<T: bytemuck::Pod>(engine: &Engine, values: &[T]) -> Result<Self> {
        Self::with_usage(engine, BufferUsage::STORAGE, values)
    }

    /// Create a uniform buffer initialized from one typed value.
    pub fn uniform<T: bytemuck::Pod>(engine: &Engine, value: &T) -> Result<Self> {
        Self::with_usage_bytes(
            engine,
            BufferUsage::UNIFORM,
            1,
            std::mem::size_of::<T>() as u64,
            bytemuck::bytes_of(value),
        )
    }

    /// Create a shader-readable buffer with explicit usage flags.
    ///
    /// `COPY_DST` is added automatically so the data can be updated with
    /// [`write_slice`](Self::write_slice) or [`resize_and_write_slice`](Self::resize_and_write_slice).
    pub fn with_usage<T: bytemuck::Pod>(
        engine: &Engine,
        usage: BufferUsage,
        values: &[T],
    ) -> Result<Self> {
        Self::with_usage_bytes(
            engine,
            usage,
            values.len(),
            std::mem::size_of::<T>() as u64,
            bytemuck::cast_slice(values),
        )
    }

    fn with_usage_bytes(
        engine: &Engine,
        usage: BufferUsage,
        len: usize,
        element_stride: u64,
        bytes: &[u8],
    ) -> Result<Self> {
        if element_stride == 0 {
            return Err(Error::InvalidInput(
                "shader data cannot be created for zero-sized element types".into(),
            ));
        }
        if !usage.contains(BufferUsage::STORAGE) && !usage.contains(BufferUsage::UNIFORM) {
            return Err(Error::InvalidInput(
                "shader data usage must include STORAGE or UNIFORM".into(),
            ));
        }
        let usage = usage | BufferUsage::COPY_DST;
        let byte_len = bytes.len() as u64;
        let capacity_bytes = byte_len.max(1);
        let buffer = engine.create_buffer(BufferDesc {
            size: capacity_bytes,
            usage,
        })?;
        if !bytes.is_empty() {
            buffer.write(0, bytes)?;
        }
        Ok(Self {
            buffer,
            usage,
            len,
            byte_len,
            capacity_bytes,
            element_stride,
        })
    }

    /// Bind this data under a reflected shader variable name for the current frame.
    pub fn bind(&self, frame: &RenderFrame, name: impl Into<String>) {
        frame.bind_buffer(name, &self.buffer);
    }

    /// Access the underlying GPU buffer for lower-level APIs.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Number of typed elements last written to this data buffer.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of meaningful bytes last written.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }

    /// Allocated byte capacity of the underlying GPU buffer.
    pub fn capacity_bytes(&self) -> u64 {
        self.capacity_bytes
    }

    /// Size in bytes of one typed element.
    pub fn element_stride(&self) -> u64 {
        self.element_stride
    }

    pub fn usage(&self) -> BufferUsage {
        self.usage
    }

    /// Overwrite this buffer with one value. Fails if the value is larger than
    /// the current allocation; use [`resize_and_write_value`](Self::resize_and_write_value)
    /// when the shape may grow.
    pub fn write_value<T: bytemuck::Pod>(&mut self, value: &T) -> Result<()> {
        self.write_bytes(
            1,
            std::mem::size_of::<T>() as u64,
            bytemuck::bytes_of(value),
        )
    }

    /// Overwrite this buffer with a typed slice. Fails if the slice is larger
    /// than the current allocation; use [`resize_and_write_slice`](Self::resize_and_write_slice)
    /// when the shape may grow.
    pub fn write_slice<T: bytemuck::Pod>(&mut self, values: &[T]) -> Result<()> {
        self.write_bytes(
            values.len(),
            std::mem::size_of::<T>() as u64,
            bytemuck::cast_slice(values),
        )
    }

    fn write_bytes(&mut self, len: usize, element_stride: u64, bytes: &[u8]) -> Result<()> {
        if element_stride == 0 {
            return Err(Error::InvalidInput(
                "shader data cannot be written with zero-sized element types".into(),
            ));
        }
        if bytes.len() as u64 > self.capacity_bytes {
            return Err(Error::InvalidInput(format!(
                "shader data write needs {} bytes but buffer capacity is {}; use resize_and_write_* to grow it",
                bytes.len(),
                self.capacity_bytes
            )));
        }
        if !bytes.is_empty() {
            self.buffer.write(0, bytes)?;
        }
        self.len = len;
        self.byte_len = bytes.len() as u64;
        self.element_stride = element_stride;
        Ok(())
    }

    /// Resize if needed, then overwrite this uniform-style buffer with one value.
    pub fn resize_and_write_value<T: bytemuck::Pod>(
        &mut self,
        engine: &Engine,
        value: &T,
    ) -> Result<()> {
        self.resize_and_write_bytes(
            engine,
            1,
            std::mem::size_of::<T>() as u64,
            bytemuck::bytes_of(value),
        )
    }

    /// Resize if needed, then overwrite this buffer with a typed slice.
    pub fn resize_and_write_slice<T: bytemuck::Pod>(
        &mut self,
        engine: &Engine,
        values: &[T],
    ) -> Result<()> {
        self.resize_and_write_bytes(
            engine,
            values.len(),
            std::mem::size_of::<T>() as u64,
            bytemuck::cast_slice(values),
        )
    }

    fn resize_and_write_bytes(
        &mut self,
        engine: &Engine,
        len: usize,
        element_stride: u64,
        bytes: &[u8],
    ) -> Result<()> {
        if element_stride == 0 {
            return Err(Error::InvalidInput(
                "shader data cannot be written with zero-sized element types".into(),
            ));
        }
        let needed = (bytes.len() as u64).max(1);
        if needed > self.capacity_bytes {
            self.buffer = engine.create_buffer(BufferDesc {
                size: needed,
                usage: self.usage,
            })?;
            self.capacity_bytes = needed;
        }
        self.write_bytes(len, element_stride, bytes)
    }
}
