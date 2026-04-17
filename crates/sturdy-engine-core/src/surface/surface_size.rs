use crate::{Error, Result};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SurfaceSize {
    pub width: u32,
    pub height: u32,
}

impl SurfaceSize {
    pub fn validate(self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Err(Error::InvalidInput("surface size must be non-zero".into()));
        }
        Ok(())
    }
}
