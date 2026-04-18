use crate::{Buffer, BufferDesc, BufferUsage, Engine, Error, Result};

const DEFAULT_UPLOAD_BLOCK_SIZE: u64 = 1024 * 1024;
const DEFAULT_UPLOAD_ALIGNMENT: u64 = 4;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct UploadAllocation {
    block_index: usize,
    offset: u64,
}

impl UploadAllocation {
    pub(crate) fn offset(self) -> u64 {
        self.offset
    }
}

struct UploadBlock {
    buffer: Buffer,
    used: u64,
}

#[derive(Default)]
pub(crate) struct UploadArena {
    blocks: Vec<UploadBlock>,
}

impl UploadArena {
    pub(crate) fn upload(&mut self, engine: &Engine, data: &[u8]) -> Result<UploadAllocation> {
        if data.is_empty() {
            return Err(Error::InvalidInput(
                "upload arena allocations must not be empty".into(),
            ));
        }
        let size = data.len() as u64;
        let block_index = self.ensure_block(engine, size)?;
        let block = &mut self.blocks[block_index];
        let offset = align_up(block.used, DEFAULT_UPLOAD_ALIGNMENT)?;
        block.buffer.write(offset, data)?;
        block.used = offset
            .checked_add(size)
            .ok_or_else(|| Error::InvalidInput("upload arena offset overflowed".into()))?;
        Ok(UploadAllocation {
            block_index,
            offset,
        })
    }

    pub(crate) fn buffer(&self, allocation: UploadAllocation) -> &Buffer {
        &self.blocks[allocation.block_index].buffer
    }

    #[cfg(test)]
    pub(crate) fn block_count(&self) -> usize {
        self.blocks.len()
    }

    fn ensure_block(&mut self, engine: &Engine, size: u64) -> Result<usize> {
        for (index, block) in self.blocks.iter().enumerate() {
            let offset = align_up(block.used, DEFAULT_UPLOAD_ALIGNMENT)?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::InvalidInput("upload arena offset overflowed".into()))?;
            if end <= block.buffer.desc().size {
                return Ok(index);
            }
        }

        let block_size = DEFAULT_UPLOAD_BLOCK_SIZE.max(size);
        let buffer = engine.create_buffer(BufferDesc {
            size: block_size,
            usage: BufferUsage::COPY_SRC,
        })?;
        self.blocks.push(UploadBlock { buffer, used: 0 });
        Ok(self.blocks.len() - 1)
    }
}

fn align_up(value: u64, alignment: u64) -> Result<u64> {
    let mask = alignment
        .checked_sub(1)
        .ok_or_else(|| Error::InvalidInput("upload arena alignment must be non-zero".into()))?;
    value
        .checked_add(mask)
        .map(|value| value & !mask)
        .ok_or_else(|| Error::InvalidInput("upload arena alignment overflowed".into()))
}
