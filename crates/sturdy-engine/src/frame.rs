use sturdy_engine_core as core;

use crate::{
    Access, BindGroup, Buffer, BufferDesc, BufferUse, DispatchDesc, DrawDesc, Engine, Error,
    FrameSyncReason, FrameSyncReport, Image, ImageDesc, ImageUse, IndexBufferBinding, IndexFormat,
    PassDesc, PassWork, Pipeline, PushConstants, Result, RgState, ShadingRate, StageMask,
    SubmissionHandle, SubresourceRange, Surface, SurfaceImage, VertexBufferBinding,
    upload_arena::UploadArena,
};

pub trait ImageRef {
    fn image_handle(&self) -> core::ImageHandle;
    fn image_desc(&self) -> ImageDesc;
}

impl ImageRef for Image {
    fn image_handle(&self) -> core::ImageHandle {
        self.handle
    }
    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

impl ImageRef for SurfaceImage {
    fn image_handle(&self) -> core::ImageHandle {
        self.handle
    }
    fn image_desc(&self) -> ImageDesc {
        self.desc
    }
}

pub struct DrawPassBuilder<'f> {
    frame: &'f mut Frame,
    name: String,
    pipeline: Option<core::PipelineHandle>,
    bind_groups: Vec<core::BindGroupHandle>,
    color_writes: Vec<(core::ImageHandle, ImageDesc)>,
    depth_write: Option<(core::ImageHandle, ImageDesc)>,
    image_reads: Vec<(core::ImageHandle, ImageDesc)>,
    extra_buffer_reads: Vec<(core::BufferHandle, BufferDesc)>,
    vertex_buf: Option<(core::BufferHandle, BufferDesc, u32, u64)>,
    index_buf: Option<(core::BufferHandle, BufferDesc, IndexFormat, u64)>,
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
    push_constants: Option<PushConstants>,
    pipeline_shading_rate: Option<ShadingRate>,
    /// Clear color per image handle (stored as f32 bit-patterns).
    clear_colors: Vec<(core::ImageHandle, [u32; 4])>,
    clear_depth: Option<(core::ImageHandle, u32, u8)>,
}

impl<'f> DrawPassBuilder<'f> {
    pub fn color(mut self, image: &impl ImageRef) -> Self {
        self.color_writes
            .push((image.image_handle(), image.image_desc()));
        self
    }

    /// Clear the last added color attachment to `rgba` before this pass executes.
    /// Must be called after the `.color()` call for the image to clear.
    pub fn clear_color(mut self, rgba: [f32; 4]) -> Self {
        if let Some((handle, _)) = self.color_writes.last() {
            let bits = rgba.map(f32::to_bits);
            self.clear_colors.push((*handle, bits));
        }
        self
    }

    /// Clear the depth attachment to `depth` (and `stencil`) before this pass executes.
    pub fn clear_depth(mut self, depth: f32, stencil: u8) -> Self {
        if let Some((handle, _)) = self.depth_write {
            self.clear_depth = Some((handle, depth.to_bits(), stencil));
        }
        self
    }

    pub fn depth(mut self, image: &impl ImageRef) -> Self {
        self.depth_write = Some((image.image_handle(), image.image_desc()));
        self
    }

    pub fn sample(mut self, image: &impl ImageRef) -> Self {
        self.image_reads
            .push((image.image_handle(), image.image_desc()));
        self
    }

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

    pub fn push_constants_at(mut self, offset: u32, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn pipeline_shading_rate(mut self, rate: ShadingRate) -> Self {
        self.pipeline_shading_rate = Some(rate);
        self
    }

    pub fn vertex_buffer(mut self, buffer: &Buffer, binding: u32, offset: u64) -> Self {
        self.vertex_buf = Some((buffer.handle(), buffer.desc(), binding, offset));
        self
    }

    pub fn index_buffer(mut self, buffer: &Buffer, format: IndexFormat, offset: u64) -> Self {
        self.index_buf = Some((buffer.handle(), buffer.desc(), format, offset));
        self
    }

    pub fn draw(mut self, vertex_count: u32) -> Self {
        self.vertex_count = vertex_count;
        self
    }

    pub fn draw_instanced(mut self, vertex_count: u32, instance_count: u32) -> Self {
        self.vertex_count = vertex_count;
        self.instance_count = instance_count;
        self
    }

    pub fn submit(self) -> Result<()> {
        let Self {
            frame,
            name,
            pipeline,
            bind_groups,
            color_writes,
            depth_write,
            image_reads,
            extra_buffer_reads,
            vertex_buf,
            index_buf,
            vertex_count,
            instance_count,
            first_vertex,
            first_instance,
            push_constants,
            pipeline_shading_rate,
            clear_colors,
            clear_depth,
        } = self;

        for (handle, desc) in &color_writes {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        if let Some((handle, desc)) = &depth_write {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        for (handle, desc) in &image_reads {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        if let Some((handle, desc, _, _)) = &vertex_buf {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }
        if let Some((handle, desc, _, _)) = &index_buf {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }
        for (handle, desc) in &extra_buffer_reads {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }

        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };

        let writes: Vec<ImageUse> = color_writes
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource,
            })
            .chain(depth_write.iter().map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::DepthWrite,
                subresource,
            }))
            .collect();

        let reads: Vec<ImageUse> = image_reads
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            })
            .collect();

        let mut buffer_reads: Vec<BufferUse> = Vec::new();
        if let Some((handle, desc, _, _)) = &vertex_buf {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::VertexRead,
                offset: 0,
                size: desc.size,
            });
        }
        if let Some((handle, desc, _, _)) = &index_buf {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::IndexRead,
                offset: 0,
                size: desc.size,
            });
        }
        for (handle, desc) in &extra_buffer_reads {
            buffer_reads.push(BufferUse {
                buffer: *handle,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: desc.size,
            });
        }

        let vertex_buffer = vertex_buf.map(|(handle, _, binding, offset)| VertexBufferBinding {
            buffer: handle,
            binding,
            offset,
        });
        let index_buffer = index_buf.map(|(handle, _, format, offset)| IndexBufferBinding {
            buffer: handle,
            offset,
            format,
        });

        frame.add_pass(PassDesc {
            pipeline,
            bind_groups,
            push_constants,
            pipeline_shading_rate,
            work: PassWork::Draw(DrawDesc {
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
                vertex_buffer,
                index_buffer,
                viewport: None,
            }),
            reads,
            writes,
            buffer_reads,
            clear_colors,
            clear_depth,
            ..PassDesc::default_graphics(name)
        })
    }
}

pub struct ComputePassBuilder<'f> {
    frame: &'f mut Frame,
    name: String,
    pipeline: Option<core::PipelineHandle>,
    bind_groups: Vec<core::BindGroupHandle>,
    push_constants: Option<PushConstants>,
    image_reads: Vec<(core::ImageHandle, ImageDesc)>,
    image_writes: Vec<(core::ImageHandle, ImageDesc)>,
    buffer_reads: Vec<(core::BufferHandle, BufferDesc)>,
    buffer_writes: Vec<(core::BufferHandle, BufferDesc)>,
    dispatch: Option<DispatchDesc>,
}

impl<'f> ComputePassBuilder<'f> {
    pub fn read_image(mut self, image: &impl ImageRef) -> Self {
        self.image_reads
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn write_image(mut self, image: &impl ImageRef) -> Self {
        self.image_writes
            .push((image.image_handle(), image.image_desc()));
        self
    }

    pub fn read_buffer(mut self, buffer: &Buffer) -> Self {
        self.buffer_reads.push((buffer.handle(), buffer.desc()));
        self
    }

    pub fn write_buffer(mut self, buffer: &Buffer) -> Self {
        self.buffer_writes.push((buffer.handle(), buffer.desc()));
        self
    }

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

    pub fn push_constants_at(mut self, offset: u32, stages: StageMask, bytes: &[u8]) -> Self {
        self.push_constants = Some(PushConstants {
            offset,
            stages,
            bytes: bytes.to_vec(),
        });
        self
    }

    pub fn dispatch(mut self, x: u32, y: u32, z: u32) -> Self {
        self.dispatch = Some(DispatchDesc { x, y, z });
        self
    }

    pub fn submit(self) -> Result<()> {
        let Self {
            frame,
            name,
            pipeline,
            bind_groups,
            push_constants,
            image_reads,
            image_writes,
            buffer_reads,
            buffer_writes,
            dispatch,
        } = self;

        let dispatch = dispatch.ok_or_else(|| {
            Error::InvalidInput("compute pass requires a dispatch call before submit".into())
        })?;

        for (handle, desc) in image_reads.iter().chain(image_writes.iter()) {
            frame.inner.graph_mut(|g| g.import_image(*handle, *desc))?;
        }
        for (handle, desc) in buffer_reads.iter().chain(buffer_writes.iter()) {
            frame.inner.graph_mut(|g| g.import_buffer(*handle, *desc))?;
        }

        let subresource = SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        };

        let reads: Vec<ImageUse> = image_reads
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            })
            .collect();

        let writes: Vec<ImageUse> = image_writes
            .iter()
            .map(|(h, _)| ImageUse {
                image: *h,
                access: Access::Write,
                state: RgState::ShaderWrite,
                subresource,
            })
            .collect();

        let buf_reads: Vec<BufferUse> = buffer_reads
            .iter()
            .map(|(h, desc)| BufferUse {
                buffer: *h,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: desc.size,
            })
            .collect();

        let buf_writes: Vec<BufferUse> = buffer_writes
            .iter()
            .map(|(h, desc)| BufferUse {
                buffer: *h,
                access: Access::Write,
                state: RgState::ShaderWrite,
                offset: 0,
                size: desc.size,
            })
            .collect();

        frame.add_pass(PassDesc {
            pipeline,
            bind_groups,
            push_constants,
            work: PassWork::Dispatch(dispatch),
            reads,
            writes,
            buffer_reads: buf_reads,
            buffer_writes: buf_writes,
            ..PassDesc::default_compute(name)
        })
    }
}

pub struct Frame {
    pub(crate) engine: Engine,
    pub(crate) inner: core::Frame,
    pub(crate) upload_arena: UploadArena,
}

impl Frame {
    pub fn import_image(&mut self, image: &Image) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_image(image.handle(), image.desc()))
    }

    pub fn import_surface_image(&mut self, image: &SurfaceImage) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_image(image.handle(), image.desc()))
    }

    pub fn import_buffer(&mut self, buffer: &Buffer) -> Result<()> {
        self.inner
            .graph_mut(|graph| graph.import_buffer(buffer.handle(), buffer.desc()))
    }

    pub fn add_pass(&mut self, pass: PassDesc) -> Result<()> {
        self.inner.graph_mut(|graph| graph.add_pass(pass))
    }

    /// Generate a full mip chain for `image` using linear-filtered blits.
    ///
    /// The image must have been created with `mip_levels > 1` and
    /// `ImageUsage::COPY_SRC | ImageUsage::COPY_DST`. Call after any write to
    /// mip 0 that you want propagated to all deeper mips.
    ///
    /// The pass is recorded immediately into the current frame; the GPU executes
    /// it when `flush()` is called.
    ///
    /// # Example
    /// ```ignore
    /// let mut tex = engine.create_image(ImageDesc {
    ///     mip_levels: 8,
    ///     usage: ImageUsage::SAMPLED | ImageUsage::COPY_SRC | ImageUsage::COPY_DST | ImageUsage::RENDER_TARGET,
    ///     ..ImageDesc::d2(512, 512, Format::Rgba8Unorm)
    /// })?;
    /// // ... upload data into mip 0 ...
    /// frame.generate_mipmaps(&tex)?;
    /// ```
    pub fn generate_mipmaps(&mut self, image: &Image) -> Result<()> {
        self.import_image(image)?;
        self.add_pass(PassDesc {
            work: PassWork::GenerateMipmaps {
                image: image.handle(),
                mip_count: image.desc().mip_levels as u32,
            },
            ..PassDesc::default_graphics(format!(
                "generate_mipmaps({})",
                image.desc().debug_name.unwrap_or("image")
            ))
        })
    }

    pub fn debug_marker(&mut self, name: impl Into<String>) -> Result<()> {
        self.add_pass(PassDesc::default_graphics(name))
    }

    pub fn draw_pass(&mut self, name: impl Into<String>) -> DrawPassBuilder<'_> {
        DrawPassBuilder {
            frame: self,
            name: name.into(),
            pipeline: None,
            bind_groups: Vec::new(),
            color_writes: Vec::new(),
            depth_write: None,
            image_reads: Vec::new(),
            extra_buffer_reads: Vec::new(),
            vertex_buf: None,
            index_buf: None,
            vertex_count: 0,
            instance_count: 1,
            first_vertex: 0,
            first_instance: 0,
            push_constants: None,
            pipeline_shading_rate: None,
            clear_colors: Vec::new(),
            clear_depth: None,
        }
    }

    pub fn compute_pass(&mut self, name: impl Into<String>) -> ComputePassBuilder<'_> {
        ComputePassBuilder {
            frame: self,
            name: name.into(),
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            image_reads: Vec::new(),
            image_writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            dispatch: None,
        }
    }

    pub fn present_image(&mut self, image: &impl ImageRef) -> Result<()> {
        self.inner
            .graph_mut(|g| g.import_image(image.image_handle(), image.image_desc()))?;
        self.add_pass(PassDesc {
            reads: vec![ImageUse {
                image: image.image_handle(),
                access: Access::Read,
                state: RgState::Present,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            ..PassDesc::default_graphics("present")
        })
    }

    pub fn flush(&mut self) -> Result<SubmissionHandle> {
        self.inner.flush()
    }

    /// Explicitly submit queued frame work and report why synchronization is allowed here.
    pub fn flush_with_reason(&mut self, reason: FrameSyncReason) -> Result<FrameSyncReport> {
        let submission = self.flush()?;
        Ok(FrameSyncReport::submitted(reason, submission))
    }

    pub fn present(&mut self) -> Result<()> {
        self.inner.present()
    }

    pub fn wait(&self) -> Result<()> {
        self.inner.wait()
    }

    /// Explicitly wait for this frame's submission and report why blocking is allowed here.
    pub fn wait_with_reason(&self, reason: FrameSyncReason) -> Result<FrameSyncReport> {
        let submission = self.inner.last_submission();
        self.wait()?;
        Ok(FrameSyncReport::waited(
            reason,
            submission.is_some(),
            submission,
        ))
    }

    pub(crate) fn last_submission(&self) -> Option<SubmissionHandle> {
        self.inner.last_submission()
    }

    /// Finish rendering this frame and present to the given surface in a single call.
    ///
    /// This is a convenience method that calls `flush()`, `wait()`, and
    /// `surface.present()` in sequence, returning the first error if any step fails.
    ///
    /// It is the replacement for the common three-call pattern:
    /// ```ignore
    /// frame.flush()?;
    /// frame.wait()?;
    /// self.surface.present()?;
    /// ```
    ///
    /// **Note**: The caller must have already called [`present_image`](Self::present_image)
    /// with the surface image that will be presented.
    pub fn finish_and_present(&mut self, surface: &Surface) -> Result<()> {
        self.finish_and_present_with_reason(surface, FrameSyncReason::FrameBoundaryPresent)?;
        Ok(())
    }

    pub fn finish_and_present_with_reason(
        &mut self,
        surface: &Surface,
        reason: FrameSyncReason,
    ) -> Result<FrameSyncReport> {
        let submission = self.flush()?;
        self.wait()?;
        surface.present()?;
        Ok(FrameSyncReport::frame_boundary_present(reason, submission))
    }
}
