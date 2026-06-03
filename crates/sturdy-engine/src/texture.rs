use crate::{
    Access, Buffer, CopyBufferDesc, CopyBufferToImageDesc, CopyImageToBufferDesc, Error,
    Extent3d, Format, Frame, Image, ImageDesc, ImageRef, ImageUsage, ImageUse, PassDesc, PassWork,
    Result, RgState, SubresourceRange,
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
    /// Track 11d: when `true` (default), asset loading may transcode the texture to a
    /// block-compressed format (BC3/BC4/BC5) before GPU upload. Set to `false` for
    /// render targets, UAVs, and any image that must stay uncompressed.
    pub prefer_compressed: bool,
    /// Generate the full mip chain after upload via `vkCmdBlitImage`.
    ///
    /// When `true` (default for sampled textures), the engine computes
    /// `floor(log2(max(w,h))) + 1` mip levels and automatically records
    /// a `GenerateMipmaps` pass after the staging copy.  This improves
    /// rendering quality for distant surfaces and reduces GPU bandwidth.
    /// Set to `false` for textures that will always be sampled at full
    /// resolution (e.g. UI images, compute output textures).
    pub generate_mips: bool,
}

impl TextureUploadDesc {
    pub const fn sampled_rgba8(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// 16-bit float RGBA — suitable for HDR/EXR textures, environment maps, and
    /// any image where values exceed [0, 1]. Uses half the memory of Rgba32Float.
    pub const fn sampled_rgba16f(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Rgba16Float,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// 32-bit float RGBA — full precision HDR. Use when f16 precision is insufficient
    /// (e.g. very bright environment maps, scientific imagery).
    pub const fn sampled_rgba32f(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Rgba32Float,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// BC4 — single-channel, 0.5 bytes/texel (roughness, AO, metallic, height).
    ///
    /// Width and height must be multiples of 4.
    pub const fn sampled_bc4(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Bc4Unorm,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// BC5 — two-channel XY normal map, 1 byte/texel.
    ///
    /// Reconstruct Z in the shader: `z = sqrt(1.0 - dot(n.xy, n.xy))`.
    /// Width and height must be multiples of 4.
    pub const fn sampled_bc5(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Bc5Unorm,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// BC3 (DXT5) — RGBA, 1 byte/texel (colour + alpha). Use when BC7 is unavailable.
    ///
    /// Width and height must be multiples of 4.
    pub const fn sampled_bc3(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Bc3Unorm,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// BC3 sRGB (DXT5) — sRGB-encoded RGBA, 1 byte/texel. Default albedo format.
    ///
    /// Width and height must be multiples of 4.
    pub const fn sampled_bc3_srgb(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Bc3UnormSrgb,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// BC7 — four-channel RGBA, 1 byte/texel (linear-space albedo, emissive).
    ///
    /// Width and height must be multiples of 4.
    pub const fn sampled_bc7(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Bc7Unorm,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// BC7 sRGB — four-channel RGBA, 1 byte/texel (sRGB-encoded albedo textures).
    ///
    /// The GPU decodes RGB from sRGB to linear on sample. Use for most
    /// game-art albedo/diffuse textures. Width and height must be multiples of 4.
    pub const fn sampled_bc7_srgb(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Bc7UnormSrgb,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
        }
    }

    /// BC6H unsigned float — three-channel HDR, 1 byte/texel (emissive, env maps).
    ///
    /// Width and height must be multiples of 4.
    pub const fn sampled_bc6h(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            format: Format::Bc6hUfloat,
            usage: ImageUsage::SAMPLED,
            prefer_compressed: true,
            generate_mips: true,
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
            ..PassDesc::default_transfer(name)
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
            buffer_writes: vec![crate::BufferUse {
                buffer: buffer.handle(),
                access: Access::Write,
                state: RgState::CopyDst,
                offset: region.buffer_offset,
                size: 0,
            }],
            ..PassDesc::default_transfer(name)
        })
    }

    /// Record a buffer-to-buffer copy pass.  Typically used to transfer data
    /// from a HOST_VISIBLE staging buffer to a `GPU_ONLY` / DEVICE_LOCAL buffer.
    ///
    /// Both `src` and `dst` must already be registered in the graph (via
    /// `RenderGraph::import_buffer`); this method handles the import itself.
    pub fn copy_buffer(
        &mut self,
        name: impl Into<String>,
        src: &Buffer,
        dst: &Buffer,
        src_offset: u64,
        dst_offset: u64,
        size: u64,
    ) -> Result<()> {
        self.inner.graph_mut(|g| g.import_buffer(src.handle(), src.desc()))?;
        self.inner.graph_mut(|g| g.import_buffer(dst.handle(), dst.desc()))?;
        self.add_pass(PassDesc {
            buffer_reads: vec![crate::BufferUse {
                buffer: src.handle(),
                access: Access::Read,
                state: RgState::CopySrc,
                offset: src_offset,
                size,
            }],
            buffer_writes: vec![crate::BufferUse {
                buffer: dst.handle(),
                access: Access::Write,
                state: RgState::CopyDst,
                offset: dst_offset,
                size,
            }],
            work: PassWork::CopyBuffer(CopyBufferDesc {
                src: src.handle(),
                src_offset,
                dst: dst.handle(),
                dst_offset,
                size,
            }),
            ..PassDesc::default_transfer(name)
        })
    }

    /// Upload raw bytes to a DEVICE_LOCAL GPU buffer via internal staging.
    ///
    /// Allocates a staging region from the frame's upload arena, copies `data`
    /// into it, and records a `vkCmdCopyBuffer` to transfer the data into `dst`
    /// (which must have `COPY_DST` and `GPU_ONLY` usage).  The copy completes
    /// before any subsequent shader access.
    ///
    /// # When to use
    ///
    /// Call this for one-time uploads of static GPU data (vertex buffers, index
    /// buffers, large lookup tables) where DEVICE_LOCAL memory gives better
    /// GPU bandwidth than HOST_VISIBLE.  For per-frame dynamic data use the
    /// transient buffer pool or `upload_uniform` instead.
    pub fn upload_buffer_data(
        &mut self,
        name: impl Into<String>,
        dst: &Buffer,
        data: &[u8],
    ) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        if !dst.desc().usage.contains(crate::BufferUsage::COPY_DST) {
            return Err(Error::InvalidInput(
                "upload_buffer_data: dst buffer must have COPY_DST usage".into(),
            ));
        }
        let name = name.into();
        let allocation = self.upload_arena.upload(&self.engine, data)?;
        let staging = self.upload_arena.buffer(allocation);
        let staging_handle = staging.handle();
        let staging_desc = staging.desc();
        let data_size = data.len() as u64;
        self.inner.graph_mut(|g| g.import_buffer(staging_handle, staging_desc))?;
        self.inner.graph_mut(|g| g.import_buffer(dst.handle(), dst.desc()))?;
        self.add_pass(PassDesc {
            buffer_reads: vec![crate::BufferUse {
                buffer: staging_handle,
                access: Access::Read,
                state: RgState::CopySrc,
                offset: allocation.offset(),
                size: data_size,
            }],
            buffer_writes: vec![crate::BufferUse {
                buffer: dst.handle(),
                access: Access::Write,
                state: RgState::CopyDst,
                offset: 0,
                size: data_size,
            }],
            work: PassWork::CopyBuffer(CopyBufferDesc {
                src: staging_handle,
                src_offset: allocation.offset(),
                dst: dst.handle(),
                dst_offset: 0,
                size: data_size,
            }),
            ..PassDesc::default_transfer(name)
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

        // Compute mip count: full chain when mips are requested and format supports blitting.
        let can_blit = !matches!(
            desc.format,
            Format::Bc3Unorm | Format::Bc3UnormSrgb | Format::Bc4Unorm
                | Format::Bc5Unorm | Format::Bc6hUfloat | Format::Bc7Unorm | Format::Bc7UnormSrgb
        );
        let mip_levels: u32 = if desc.generate_mips && can_blit {
            let max_dim = desc.width.max(desc.height);
            (u32::BITS - max_dim.leading_zeros()).max(1)  // floor(log2(max_dim)) + 1
        } else {
            1
        };
        let generate_mips = mip_levels > 1;
        let extra_usage = if generate_mips {
            // GenerateMipmaps requires COPY_SRC (src of each blit) + COPY_DST (dst of each blit).
            ImageUsage::COPY_DST | ImageUsage::COPY_SRC
        } else {
            ImageUsage::COPY_DST
        };

        let image = self.engine.create_image(ImageDesc {
            dimension: crate::ImageDimension::D2,
            extent: Extent3d {
                width: desc.width,
                height: desc.height,
                depth: 1,
            },
            mip_levels: mip_levels as u16,
            layers: 1,
            samples: 1,
            format: desc.format,
            usage: desc.usage | extra_usage,
            transient: false,
            clear_value: None,
            debug_name: Some("uploaded texture"),
            compression: Default::default(),
            min_lod_bits: None,
            msaa_resolve_to_single_sampled: false,
            drm_format_modifier: None,
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
            ..PassDesc::default_transfer(format!("{name}-copy"))
        })?;

        // Optional: generate the full mip chain from mip 0 via blit.
        // The GenerateMipmaps pass transitions each mip to SHADER_READ_ONLY at the end.
        if generate_mips {
            self.inner.graph_mut(|g| g.import_image(image.handle(), image.desc()))?;
            self.add_pass(PassDesc {
                work: PassWork::GenerateMipmaps {
                    image: image.handle(),
                    mip_count: mip_levels,
                },
                reads: vec![ImageUse {
                    image: image.handle(),
                    access: Access::Read,
                    state: RgState::CopySrc,
                    subresource: SubresourceRange { base_mip: 0, mip_count: 1, base_layer: 0, layer_count: 1 },
                }],
                writes: vec![ImageUse {
                    image: image.handle(),
                    access: Access::Write,
                    state: RgState::CopyDst,
                    subresource: SubresourceRange { base_mip: 0, mip_count: u16::MAX, base_layer: 0, layer_count: 1 },
                }],
                ..PassDesc::default_transfer(format!("{name}-gen-mips"))
            })?;
        } else {
            self.add_pass(PassDesc {
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
                ..PassDesc::default_graphics(format!("{name}-shader-read"))
            })?;
        }
        Ok(image)
    }

    pub fn upload_pixels_to_image(
        &mut self,
        name: impl Into<String>,
        image: &Image,
        data: &[u8],
    ) -> Result<()> {
        let desc = image.desc();
        let expected_len = texture_upload_byte_count(TextureUploadDesc {
            width: desc.extent.width,
            height: desc.extent.height,
            format: desc.format,
            usage: desc.usage,
            prefer_compressed: false,
            generate_mips: false,
        })?;
        if data.len() as u64 != expected_len {
            return Err(Error::InvalidInput(format!(
                "texture upload data length {} does not match expected byte count {expected_len}",
                data.len()
            )));
        }

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
            work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                buffer: staging_handle,
                image: image.handle(),
                buffer_offset: allocation.offset(),
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: desc.extent.width,
                height: desc.extent.height,
                depth: 1,
            }),
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
            ..PassDesc::default_transfer(format!("{name}-copy"))
        })?;
        self.add_pass(PassDesc {
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
            ..PassDesc::default_graphics(format!("{name}-shader-read"))
        })
    }
}

pub(crate) fn texture_upload_byte_count(desc: TextureUploadDesc) -> Result<u64> {
    if desc.format.is_block_compressed() {
        // BC formats: tightly-packed 4×4 blocks. Width/height are padded to
        // the next multiple of 4 for the block count calculation.
        let block_bytes = desc.format.bc_block_bytes();
        let blocks_x = (desc.width as u64).div_ceil(4);
        let blocks_y = (desc.height as u64).div_ceil(4);
        return blocks_x
            .checked_mul(blocks_y)
            .and_then(|b| b.checked_mul(block_bytes))
            .ok_or_else(|| Error::InvalidInput("BC texture byte count overflowed".into()));
    }

    let texel_size: u64 = match desc.format {
        Format::Unknown => {
            return Err(Error::InvalidInput(
                "texture upload format must be specified".into(),
            ));
        }
        Format::Rgba8Unorm | Format::Bgra8Unorm => 4,
        Format::R8Unorm => 1,
        Format::Rg8Unorm => 2,
        Format::Rgba16Float => 8,
        Format::Rgba32Float => 16,
        Format::Depth32Float | Format::Depth24Stencil8 => 4,
        Format::G8_B8R8_2PLANE_420_UNORM => 1,
        // BC formats handled above.
        Format::Bc3Unorm
        | Format::Bc3UnormSrgb
        | Format::Bc4Unorm
        | Format::Bc5Unorm
        | Format::Bc7Unorm
        | Format::Bc7UnormSrgb
        | Format::Bc6hUfloat => {
            return Err(Error::InvalidInput(
                "BC-compressed upload byte counts must be computed from 4x4 blocks".into(),
            ));
        }
    };
    [desc.width as u64, desc.height as u64, texel_size]
        .into_iter()
        .try_fold(1u64, |acc, value| {
            acc.checked_mul(value)
                .ok_or_else(|| Error::InvalidInput("texture upload byte count overflowed".into()))
        })
}
