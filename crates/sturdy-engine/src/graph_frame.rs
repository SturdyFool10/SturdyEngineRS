use sturdy_engine_core as core;

use crate::{
    Access, BindGroup, Buffer, BufferDesc, BufferUse, DrawDesc, Engine, Error, Format, Frame,
    Image, ImageDesc, ImageHandle, ImageRef, ImageUse, PassDesc, PassWork, Pipeline, PushConstants,
    QueueType, Result, RgState, StageMask, SubresourceRange, SurfaceImage,
};

/// An image operand for [`GraphFrame`] operations.
///
/// Obtained via [`GraphFrame::image`] (transient) or
/// [`GraphFrame::swapchain_image`] (borrowed swapchain image).
pub struct ImageNode {
    handle: ImageHandle,
    desc: ImageDesc,
}

impl ImageNode {
    pub fn handle(&self) -> ImageHandle {
        self.handle
    }

    pub fn desc(&self) -> ImageDesc {
        self.desc
    }
}

impl ImageRef for ImageNode {
    fn image_handle(&self) -> core::ImageHandle {
        self.handle
    }

    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

/// Image-centric frame builder built on top of [`Frame`].
///
/// Provides high-level operations (`clear`, `copy_image`, `fullscreen_pass`,
/// `present`) in addition to the underlying `draw_pass` / `compute_pass`
/// builders.  All operations are deferred until `flush()` is called.
pub struct GraphFrame {
    engine: Engine,
    frame: Frame,
    owned_images: Vec<Image>,
    owned_buffers: Vec<Buffer>,
}

impl GraphFrame {
    pub(crate) fn new(engine: Engine, frame: Frame) -> Self {
        Self {
            engine,
            frame,
            owned_images: Vec::new(),
            owned_buffers: Vec::new(),
        }
    }

    /// Allocate a transient image for use within this frame.
    pub fn image(&mut self, desc: ImageDesc) -> Result<ImageNode> {
        let image = self.engine.create_image(desc)?;
        let node = ImageNode {
            handle: image.handle(),
            desc: image.desc(),
        };
        self.frame.import_image(&image)?;
        self.owned_images.push(image);
        Ok(node)
    }

    /// Import a swapchain image as an [`ImageNode`].
    pub fn swapchain_image(&mut self, surface_image: &SurfaceImage) -> Result<ImageNode> {
        self.frame.import_surface_image(surface_image)?;
        Ok(ImageNode {
            handle: surface_image.handle(),
            desc: surface_image.desc(),
        })
    }

    /// Import a plain `Image` as a graph node (useful for headless targets).
    pub fn swapchain_image_from_image(&mut self, image: &Image) -> Result<ImageNode> {
        self.frame.import_image(image)?;
        Ok(ImageNode {
            handle: image.handle(),
            desc: image.desc(),
        })
    }

    /// Add a clear-color pass for `image`.
    pub fn clear(&mut self, image: &ImageNode, color: [f32; 4]) -> Result<()> {
        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };
        self.frame.add_pass(PassDesc {
            name: format!("clear-{}", image.handle().0),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: image.handle(),
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource,
            }],
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: vec![(image.handle(), color.map(f32::to_bits))],
            clear_depth: None,
        })
    }

    /// Copy `src` into `dst` via a staging buffer.
    ///
    /// Copies the mip-0 layer-0 region, clamped to the smaller of the two
    /// images.  Both images must have the same format.
    pub fn copy_image(&mut self, src: &ImageNode, dst: &ImageNode) -> Result<()> {
        let src_desc = src.desc();
        let dst_desc = dst.desc();

        if src_desc.format != dst_desc.format {
            return Err(Error::InvalidInput(
                "copy_image requires src and dst to have the same format".into(),
            ));
        }

        let copy_width = src_desc.extent.width.min(dst_desc.extent.width);
        let copy_height = src_desc.extent.height.min(dst_desc.extent.height);
        let bpt = format_bytes_per_texel(src_desc.format)?;
        let stride = copy_width as u64 * copy_height as u64 * bpt as u64;

        let staging = self.engine.create_buffer(BufferDesc {
            size: stride,
            usage: crate::BufferUsage::COPY_SRC | crate::BufferUsage::COPY_DST,
        })?;
        let staging_handle = staging.handle();
        let staging_desc = staging.desc();

        self.frame
            .inner
            .graph_mut(|g| g.import_buffer(staging_handle, staging_desc))?;

        let src_sub = SubresourceRange::WHOLE;
        let dst_sub = SubresourceRange::WHOLE;

        self.frame.add_pass(PassDesc {
            name: "copy-image-to-staging".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::CopyImageToBuffer(crate::CopyImageToBufferDesc {
                image: src.handle(),
                buffer: staging_handle,
                buffer_offset: 0,
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: copy_width,
                height: copy_height,
                depth: 1,
            }),
            reads: vec![ImageUse {
                image: src.handle(),
                access: Access::Read,
                state: RgState::CopySrc,
                subresource: src_sub,
            }],
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: vec![BufferUse {
                buffer: staging_handle,
                access: Access::Write,
                state: RgState::CopyDst,
                offset: 0,
                size: stride,
            }],
            clear_colors: Vec::new(),
            clear_depth: None,
        })?;

        self.frame.add_pass(PassDesc {
            name: "copy-staging-to-image".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            work: PassWork::CopyBufferToImage(crate::CopyBufferToImageDesc {
                buffer: staging_handle,
                image: dst.handle(),
                buffer_offset: 0,
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: copy_width,
                height: copy_height,
                depth: 1,
            }),
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: dst.handle(),
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: dst_sub,
            }],
            buffer_reads: vec![BufferUse {
                buffer: staging_handle,
                access: Access::Read,
                state: RgState::CopySrc,
                offset: 0,
                size: stride,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })?;

        self.owned_buffers.push(staging);
        Ok(())
    }

    /// Transition `image` to the `Present` state.
    pub fn present(&mut self, image: &ImageNode) -> Result<()> {
        self.frame.present_image(image)
    }

    /// Begin building a fullscreen-triangle pass targeting `target`.
    ///
    /// Uses a 3-vertex, no-buffer draw call — the vertex shader must generate
    /// the fullscreen triangle from `gl_VertexIndex`.
    pub fn fullscreen_pass(
        &mut self,
        name: impl Into<String>,
        target: &ImageNode,
    ) -> FullscreenPassBuilder<'_> {
        FullscreenPassBuilder {
            frame: &mut self.frame,
            name: name.into(),
            target_handle: target.handle(),
            target_desc: target.desc(),
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            input_images: Vec::new(),
        }
    }

    /// Blend `src` over `dst` using a caller-supplied fullscreen pipeline.
    ///
    /// The pipeline is expected to sample from `src` and write to `dst`.
    /// Uses a 3-vertex fullscreen triangle (no vertex buffer).
    pub fn blend_over(
        &mut self,
        src: &ImageNode,
        dst: &ImageNode,
        pipeline: &Pipeline,
    ) -> Result<()> {
        self.fullscreen_pass("blend-over", dst)
            .pipeline(pipeline)
            .input(src)
            .submit()
    }

    /// Delegate to the underlying `Frame::draw_pass`.
    pub fn draw_pass(&mut self, name: impl Into<String>) -> crate::DrawPassBuilder<'_> {
        self.frame.draw_pass(name)
    }

    /// Delegate to the underlying `Frame::compute_pass`.
    pub fn compute_pass(&mut self, name: impl Into<String>) -> crate::ComputePassBuilder<'_> {
        self.frame.compute_pass(name)
    }

    pub fn flush(&mut self) -> Result<crate::SubmissionHandle> {
        self.frame.flush()
    }

    pub fn flush_with_reason(
        &mut self,
        reason: crate::FrameSyncReason,
    ) -> Result<crate::FrameSyncReport> {
        self.frame.flush_with_reason(reason)
    }

    pub fn wait(&self) -> Result<()> {
        self.frame.wait()
    }

    pub fn wait_with_reason(
        &self,
        reason: crate::FrameSyncReason,
    ) -> Result<crate::FrameSyncReport> {
        self.frame.wait_with_reason(reason)
    }
}

/// Builds a fullscreen-triangle pass that renders into one color attachment.
pub struct FullscreenPassBuilder<'f> {
    frame: &'f mut Frame,
    name: String,
    target_handle: ImageHandle,
    target_desc: ImageDesc,
    pipeline: Option<core::PipelineHandle>,
    bind_groups: Vec<core::BindGroupHandle>,
    push_constants: Option<PushConstants>,
    input_images: Vec<(core::ImageHandle, ImageDesc)>,
}

impl<'f> FullscreenPassBuilder<'f> {
    pub fn pipeline(mut self, pipeline: &Pipeline) -> Self {
        self.pipeline = Some(pipeline.handle());
        self
    }

    pub fn bind(mut self, bind_group: &BindGroup) -> Self {
        self.bind_groups.push(bind_group.handle());
        self
    }

    pub fn push_constants(mut self, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset: 0,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    /// Declare an image to be sampled by the fullscreen shader.
    pub fn input(mut self, image: &ImageNode) -> Self {
        self.input_images.push((image.handle(), image.desc()));
        self
    }

    pub fn submit(self) -> Result<()> {
        let Self {
            frame,
            name,
            target_handle,
            target_desc,
            pipeline,
            bind_groups,
            push_constants,
            input_images,
        } = self;

        frame
            .inner
            .graph_mut(|g| g.import_image(target_handle, target_desc))?;
        for (h, desc) in &input_images {
            frame.inner.graph_mut(|g| g.import_image(*h, *desc))?;
        }

        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };

        let reads: Vec<ImageUse> = input_images
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            })
            .collect();

        frame.add_pass(PassDesc {
            name,
            queue: QueueType::Graphics,
            shader: None,
            pipeline,
            bind_groups,
            push_constants,
            work: PassWork::Draw(DrawDesc {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer: None,
                index_buffer: None,
            }),
            reads,
            writes: vec![ImageUse {
                image: target_handle,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource,
            }],
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
        })
    }
}

fn format_bytes_per_texel(format: Format) -> Result<u32> {
    Ok(match format {
        Format::Rgba8Unorm | Format::Bgra8Unorm => 4,
        Format::Rgba16Float => 8,
        Format::Rgba32Float => 16,
        Format::Depth32Float => 4,
        Format::Depth24Stencil8 => 4,
        Format::Unknown => {
            return Err(Error::InvalidInput(
                "cannot copy image with unknown format".into(),
            ));
        }
    })
}
