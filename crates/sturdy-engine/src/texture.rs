use crate::{
    Access, Buffer, CopyBufferToImageDesc, CopyImageToBufferDesc, Error, Extent3d, Format, Frame,
    Image, ImageDesc, ImageRef, ImageUsage, ImageUse, PassDesc, PassWork, QueueType, Result,
    RgState, SubresourceRange,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct ImageCopyRegion {
    pub buffer_offset: u64,
    pub mip_level: u32,
    pub base_layer: u32,
    pub layer_count: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

impl ImageCopyRegion {
    pub const fn whole_2d(width: u32, height: u32) -> Self {
        Self {
            buffer_offset: 0,
            mip_level: 0,
            base_layer: 0,
            layer_count: 1,
            width,
            height,
            depth: 1,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct TextureUploadDesc {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub usage: ImageUsage,
}

impl TextureUploadDesc {
    pub const fn sampled_rgba8(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED,
        }
    }
}

impl Frame {
    pub fn copy_buffer_to_image(
        &mut self,
        name: impl Into<String>,
        buffer: &Buffer,
        image: &impl ImageRef,
        region: ImageCopyRegion,
    ) -> Result<()> {
        self.inner
            .graph_mut(|g| g.import_buffer(buffer.handle(), buffer.desc()))?;
        self.inner
            .graph_mut(|g| g.import_image(image.image_handle(), image.image_desc()))?;

        let base_mip = u16::try_from(region.mip_level)
            .map_err(|_| Error::InvalidInput("copy region mip_level exceeds u16 range".into()))?;
        let base_layer = u16::try_from(region.base_layer)
            .map_err(|_| Error::InvalidInput("copy region base_layer exceeds u16 range".into()))?;
        let layer_count = u16::try_from(region.layer_count)
            .map_err(|_| Error::InvalidInput("copy region layer_count exceeds u16 range".into()))?;

        self.add_pass(PassDesc {
            name: name.into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                buffer: buffer.handle(),
                image: image.image_handle(),
                buffer_offset: region.buffer_offset,
                mip_level: region.mip_level,
                base_layer: region.base_layer,
                layer_count: region.layer_count,
                width: region.width,
                height: region.height,
                depth: region.depth,
            }),
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: image.image_handle(),
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: SubresourceRange {
                    base_mip,
                    mip_count: 1,
                    base_layer,
                    layer_count,
                },
            }],
            buffer_reads: vec![crate::BufferUse {
                buffer: buffer.handle(),
                access: Access::Read,
                state: RgState::CopySrc,
                offset: region.buffer_offset,
                size: 0,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    pub fn copy_image_to_buffer(
        &mut self,
        name: impl Into<String>,
        image: &impl ImageRef,
        buffer: &Buffer,
        region: ImageCopyRegion,
    ) -> Result<()> {
        self.inner
            .graph_mut(|g| g.import_image(image.image_handle(), image.image_desc()))?;
        self.inner
            .graph_mut(|g| g.import_buffer(buffer.handle(), buffer.desc()))?;

        let base_mip = u16::try_from(region.mip_level)
            .map_err(|_| Error::InvalidInput("copy region mip_level exceeds u16 range".into()))?;
        let base_layer = u16::try_from(region.base_layer)
            .map_err(|_| Error::InvalidInput("copy region base_layer exceeds u16 range".into()))?;
        let layer_count = u16::try_from(region.layer_count)
            .map_err(|_| Error::InvalidInput("copy region layer_count exceeds u16 range".into()))?;

        self.add_pass(PassDesc {
            name: name.into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::CopyImageToBuffer(CopyImageToBufferDesc {
                image: image.image_handle(),
                buffer: buffer.handle(),
                buffer_offset: region.buffer_offset,
                mip_level: region.mip_level,
                base_layer: region.base_layer,
                layer_count: region.layer_count,
                width: region.width,
                height: region.height,
                depth: region.depth,
            }),
            reads: vec![ImageUse {
                image: image.image_handle(),
                access: Access::Read,
                state: RgState::CopySrc,
                subresource: SubresourceRange {
                    base_mip,
                    mip_count: 1,
                    base_layer,
                    layer_count,
                },
            }],
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: vec![crate::BufferUse {
                buffer: buffer.handle(),
                access: Access::Write,
                state: RgState::CopyDst,
                offset: region.buffer_offset,
                size: 0,
            }],
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }

    pub fn upload_texture_2d(
        &mut self,
        name: impl Into<String>,
        desc: TextureUploadDesc,
        data: &[u8],
    ) -> Result<Image> {
        if desc.width == 0 || desc.height == 0 {
            return Err(Error::InvalidInput(
                "texture upload dimensions must be non-zero".into(),
            ));
        }
        let expected_len = texture_upload_byte_count(desc)?;
        if data.len() as u64 != expected_len {
            return Err(Error::InvalidInput(format!(
                "texture upload data length {} does not match expected byte count {expected_len}",
                data.len()
            )));
        }

        let image = self.engine.create_image(ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: Extent3d {
                width: desc.width,
                height: desc.height,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: desc.format,
            usage: desc.usage | ImageUsage::COPY_DST,
            transient: false,
            clear_value: None,
            debug_name: Some("uploaded texture"),
        })?;
        let allocation = self.upload_arena.upload(&self.engine, data)?;
        let (staging_handle, staging_desc) = {
            let staging = self.upload_arena.buffer(allocation);
            (staging.handle(), staging.desc())
        };

        let name = name.into();
        self.inner
            .graph_mut(|g| g.import_buffer(staging_handle, staging_desc))?;
        self.inner
            .graph_mut(|g| g.import_image(image.handle(), image.desc()))?;
        self.add_pass(PassDesc {
            name: format!("{name}-copy"),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                buffer: staging_handle,
                image: image.handle(),
                buffer_offset: allocation.offset(),
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: desc.width,
                height: desc.height,
                depth: 1,
            }),
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: image.handle(),
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            buffer_reads: vec![crate::BufferUse {
                buffer: staging_handle,
                access: Access::Read,
                state: RgState::CopySrc,
                offset: allocation.offset(),
                size: expected_len,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })?;
        self.add_pass(PassDesc {
            name: format!("{name}-shader-read"),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads: vec![ImageUse {
                image: image.handle(),
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })?;
        Ok(image)
    }
}

fn texture_upload_byte_count(desc: TextureUploadDesc) -> Result<u64> {
    let texel_size = match desc.format {
        Format::Unknown => {
            return Err(Error::InvalidInput(
                "texture upload format must be specified".into(),
            ));
        }
        Format::Rgba8Unorm | Format::Bgra8Unorm => 4,
        Format::Rgba16Float => 8,
        Format::Rgba32Float => 16,
        Format::Depth32Float | Format::Depth24Stencil8 => 4,
    };
    [desc.width as u64, desc.height as u64, texel_size]
        .into_iter()
        .try_fold(1u64, |acc, value| {
            acc.checked_mul(value)
                .ok_or_else(|| Error::InvalidInput("texture upload byte count overflowed".into()))
        })
}
