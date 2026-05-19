use crate::{Error, Result};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum AccelerationStructureKind {
    BottomLevel,
    TopLevel,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct AccelerationStructureDesc {
    pub kind: AccelerationStructureKind,
    pub size: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct AccelerationStructureBuildSizes {
    pub acceleration_structure_size: u64,
    pub build_scratch_size: u64,
    pub update_scratch_size: u64,
}

impl AccelerationStructureDesc {
    pub fn validate(&self) -> Result<()> {
        if self.size == 0 {
            return Err(Error::InvalidInput(
                "acceleration structure size must be non-zero".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "acceleration_structure_tests.rs"]
mod tests;
