use crate::{Buffer, BufferDesc, BufferUsage, Engine, Result};

/// A CPU-readable buffer used to retrieve GPU-written data without stalling the pipeline.
///
/// Create once with [`Engine::create_readback_buffer`] and reuse every frame.
/// After recording a [`RenderFrame::copy_to_readback`] pass and calling
/// [`RenderFrame::flush`] + [`RenderFrame::wait`], the buffer is safe to read.
///
/// ## One-frame-delayed readback pattern
/// ```ignore
/// // Frame N — record copy of GPU visible-count into readback buffer.
/// frame.copy_to_readback("read_visible_count", &gpu_count_buf, &readback, 0, 4)?;
/// let handle = frame.flush()?;
/// // ... continue building frame N+1 ...
///
/// // Frame N+1 start — GPU has finished frame N (engine waited on previous fence).
/// let count = readback.read_as::<u32>(&engine, 0)?;
/// diagnostics.visible_objects = count;
/// ```
pub struct ReadbackBuffer {
    pub(crate) buffer: Buffer,
    size: u64,
}

impl ReadbackBuffer {
    pub(crate) fn new(buffer: Buffer, size: u64) -> Self {
        Self { buffer, size }
    }

    /// Size of the buffer in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Read all bytes from the buffer.
    ///
    /// Call only after the GPU has finished writing (after the frame that recorded
    /// the copy has been waited on). Returns an error if the underlying map fails.
    pub fn read_bytes(&self, engine: &Engine) -> Result<Vec<u8>> {
        let mut out = vec![0u8; self.size as usize];
        engine.read_buffer(&self.buffer, 0, &mut out)?;
        Ok(out)
    }

    /// Read a single `T` value at byte `offset`.
    ///
    /// `T` must be `bytemuck::Pod` (plain-old-data). Panics if `offset + size_of::<T>()`
    /// exceeds the buffer size.
    pub fn read_as<T: bytemuck::Pod>(&self, engine: &Engine, offset: u64) -> Result<T> {
        let size = std::mem::size_of::<T>() as u64;
        assert!(
            offset + size <= self.size,
            "ReadbackBuffer::read_as offset+size ({}) exceeds buffer size ({})",
            offset + size,
            self.size
        );
        let mut bytes = vec![0u8; size as usize];
        engine.read_buffer(&self.buffer, offset, &mut bytes)?;
        Ok(*bytemuck::from_bytes(&bytes))
    }

    /// Read a slice of `T` values starting at byte `offset`.
    ///
    /// Panics if the requested range exceeds the buffer size.
    pub fn read_slice<T: bytemuck::Pod>(
        &self,
        engine: &Engine,
        offset: u64,
        count: usize,
    ) -> Result<Vec<T>> {
        let elem_size = std::mem::size_of::<T>() as u64;
        let total = elem_size * count as u64;
        assert!(
            offset + total <= self.size,
            "ReadbackBuffer::read_slice offset+size ({}) exceeds buffer size ({})",
            offset + total,
            self.size
        );
        let mut bytes = vec![0u8; total as usize];
        engine.read_buffer(&self.buffer, offset, &mut bytes)?;
        Ok(bytemuck::cast_slice(&bytes).to_vec())
    }

    pub(crate) fn buffer(&self) -> &Buffer {
        &self.buffer
    }
}

impl Engine {
    /// Create a `ReadbackBuffer` of `size` bytes.
    ///
    /// The buffer is allocated in HOST_VISIBLE memory so the CPU can read it
    /// after the GPU has written to it via [`RenderFrame::copy_to_readback`].
    /// Use `COPY_DST` semantics — the GPU writes via a buffer copy pass;
    /// the CPU reads via [`ReadbackBuffer::read_as`] or [`ReadbackBuffer::read_bytes`].
    pub fn create_readback_buffer(&self, size: u64) -> Result<ReadbackBuffer> {
        // No GPU_ONLY flag → Vulkan backend prefers HOST_VISIBLE | HOST_COHERENT memory.
        let buffer = self.create_buffer(BufferDesc {
            size,
            usage: BufferUsage::COPY_DST,
        })?;
        Ok(ReadbackBuffer::new(buffer, size))
    }

    /// Create a `ReadbackBuffer` large enough to hold `count` elements of type `T`.
    pub fn create_readback_buffer_for<T: bytemuck::Pod>(
        &self,
        count: usize,
    ) -> Result<ReadbackBuffer> {
        self.create_readback_buffer((std::mem::size_of::<T>() * count) as u64)
    }
}
