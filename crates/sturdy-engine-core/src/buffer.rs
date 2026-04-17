use crate::{Error, Result};

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BufferUsage(pub u32);

impl BufferUsage {
    pub const COPY_SRC: Self = Self(1 << 0);
    pub const COPY_DST: Self = Self(1 << 1);
    pub const UNIFORM: Self = Self(1 << 2);
    pub const STORAGE: Self = Self(1 << 3);
    pub const VERTEX: Self = Self(1 << 4);
    pub const INDEX: Self = Self(1 << 5);
    pub const INDIRECT: Self = Self(1 << 6);
    pub const ACCELERATION_STRUCTURE: Self = Self(1 << 7);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

impl std::ops::BitOr for BufferUsage {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct BufferDesc {
    pub size: u64,
    pub usage: BufferUsage,
}

impl BufferDesc {
    pub fn validate(&self) -> Result<()> {
        if self.size == 0 {
            return Err(Error::InvalidInput("buffer size must be non-zero".into()));
        }
        if self.usage == BufferUsage::empty() {
            return Err(Error::InvalidInput("buffer usage must be non-empty".into()));
        }
        Ok(())
    }
}
