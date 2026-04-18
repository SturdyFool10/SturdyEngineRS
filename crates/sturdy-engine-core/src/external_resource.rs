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
mod tests {
    use super::*;
    use crate::{BufferUsage, Extent3d, Format, ImageUsage};

    #[test]
    fn external_vulkan_image_requires_nonzero_image_and_view() {
        let desc = ExternalImageDesc {
            desc: image_desc(),
            handle: ExternalImageHandle::Vulkan(VulkanExternalImage {
                image: 0,
                image_view: 1,
            }),
        };

        assert!(matches!(desc.validate(), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn external_vulkan_buffer_requires_nonzero_buffer() {
        let desc = ExternalBufferDesc {
            desc: BufferDesc {
                size: 64,
                usage: BufferUsage::STORAGE,
            },
            handle: ExternalBufferHandle::Vulkan(VulkanExternalBuffer { buffer: 0 }),
        };

        assert!(matches!(desc.validate(), Err(Error::InvalidInput(_))));
    }

    fn image_desc() -> ImageDesc {
        ImageDesc {
            extent: Extent3d {
                width: 1,
                height: 1,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED,
        }
    }
}
