use crate::{BufferDesc, Error, ImageDesc, Result};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VulkanExternalImage {
    pub image: u64,
    pub image_view: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VulkanExternalBuffer {
    pub buffer: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ExternalImageHandle {
    Vulkan(VulkanExternalImage),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ExternalBufferHandle {
    Vulkan(VulkanExternalBuffer),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ExternalImageDesc {
    pub desc: ImageDesc,
    pub handle: ExternalImageHandle,
}

impl ExternalImageDesc {
    pub fn validate(&self) -> Result<()> {
        self.desc.validate()?;
        match self.handle {
            ExternalImageHandle::Vulkan(handle) => {
                if handle.image == 0 {
                    return Err(Error::InvalidInput(
                        "external Vulkan image handle must be non-zero".into(),
                    ));
                }
                if handle.image_view == 0 {
                    return Err(Error::InvalidInput(
                        "external Vulkan image view handle must be non-zero".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ExternalBufferDesc {
    pub desc: BufferDesc,
    pub handle: ExternalBufferHandle,
}

impl ExternalBufferDesc {
    pub fn validate(&self) -> Result<()> {
        self.desc.validate()?;
        match self.handle {
            ExternalBufferHandle::Vulkan(handle) => {
                if handle.buffer == 0 {
                    return Err(Error::InvalidInput(
                        "external Vulkan buffer handle must be non-zero".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "external_resource_tests.rs"]
mod tests;
