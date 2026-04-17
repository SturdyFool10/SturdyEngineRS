use crate::{Error, Result, StageMask};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PushConstants {
    pub offset: u32,
    pub stages: StageMask,
    pub bytes: Vec<u8>,
}

impl PushConstants {
    pub fn validate(&self) -> Result<()> {
        if self.bytes.is_empty() {
            return Err(Error::InvalidInput(
                "push constant data must be non-empty".into(),
            ));
        }
        if self.offset % 4 != 0 || self.bytes.len() % 4 != 0 {
            return Err(Error::InvalidInput(
                "push constant offset and byte length must be 4-byte aligned".into(),
            ));
        }
        self.offset
            .checked_add(self.bytes.len() as u32)
            .ok_or_else(|| Error::InvalidInput("push constant byte range overflowed".into()))?;
        Ok(())
    }
}
