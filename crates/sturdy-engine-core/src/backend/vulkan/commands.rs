use ash::{Device, vk};

use crate::{
    BufferBarrier, CompiledGraph, Error, ImageBarrier, IndexFormat, PassDesc, PassWork, Result,
    RgState, SubresourceRange,
};

use super::descriptors::DescriptorRegistry;
use super::pipelines::PipelineRegistry;
use super::resources::ResourceRegistry;

pub struct CommandContext {
    pool: vk::CommandPool,
}

impl CommandContext {
    pub fn create(device: &Device, queue_family: u32) -> Result<Self> {
        let info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);
        let pool = unsafe {
            device
                .create_command_pool(&info, None)
                .map_err(|error| Error::Backend(format!("vkCreateCommandPool failed: {error:?}")))?
        };
        Ok(Self { pool })
    }

    pub fn record_submit_and_wait(
        &mut self,
        device: &Device,
        queue: vk::Queue,
        graph: &CompiledGraph,
        resources: &ResourceRegistry,
        descriptors: &DescriptorRegistry,
        pipelines: &mut PipelineRegistry,
    ) -> Result<()> {
        let command_buffer = self.allocate_command_buffer(device)?;
        let fence = self.create_fence(device)?;

        let result = self.record_graph(
            device,
            command_buffer,
            graph,
            resources,
            descriptors,
            pipelines,
        );
        let submit_result = result.and_then(|()| {
            self.submit_and_wait(device, queue, command_buffer, fence)
        });

        unsafe {
            device.destroy_fence(fence, None);
            device.free_command_buffers(self.pool, &[command_buffer]);
        }

        submit_result
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            device.destroy_command_pool(self.pool, None);
        }
    }

    fn allocate_command_buffer(&self, device: &Device) -> Result<vk::CommandBuffer> {
        let info = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let buffers = unsafe {
            device.allocate_command_buffers(&info).map_err(|error| {
                Error::Backend(format!("vkAllocateCommandBuffers failed: {error:?}"))
            })?
        };
        Ok(buffers[0])
    }

    fn create_fence(&self, device: &Device) -> Result<vk::Fence> {
        let info = vk::FenceCreateInfo::default();
        unsafe {
            device
                .create_fence(&info, None)
                .map_err(|error| Error::Backend(format!("vkCreateFence failed: {error:?}")))
        }
    }

    fn record_graph(
        &self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        graph: &CompiledGraph,
        resources: &ResourceRegistry,
        descriptors: &DescriptorRegistry,
        pipelines: &mut PipelineRegistry,
    ) -> Result<()> {
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            device
                .begin_command_buffer(command_buffer, &begin)
                .map_err(|error| {
                    Error::Backend(format!("vkBeginCommandBuffer failed: {error:?}"))
                })?;
        }

        for (pass_index, image_barriers) in graph.barriers_per_pass.iter().enumerate() {
            let buffer_barriers = graph
                .buffer_barriers_per_pass
                .get(pass_index)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            self.record_pass_barriers(
                device,
                command_buffer,
                image_barriers,
                buffer_barriers,
                resources,
            )?;
            if let Some(pass) = graph.passes.get(pass_index) {
                let mut bound_pipeline = None;
                if let Some(pipeline) = pass.pipeline {
                    let pipeline = pipelines.pipeline(pipeline)?;
                    unsafe {
                        device.cmd_bind_pipeline(
                            command_buffer,
                            pipeline.bind_point,
                            pipeline.pipeline,
                        );
                    }
                    let mut sets = Vec::new();
                    for bind_group in &pass.bind_groups {
                        sets.extend_from_slice(descriptors.descriptor_sets(*bind_group)?);
                    }
                    if !sets.is_empty() {
                        unsafe {
                            device.cmd_bind_descriptor_sets(
                                command_buffer,
                                pipeline.bind_point,
                                pipeline.layout,
                                0,
                                &sets,
                                &[],
                            );
                        }
                    }
                    bound_pipeline = Some(pipeline);
                }

                match pass.work {
                    PassWork::None => {}
                    PassWork::Dispatch(dispatch) => {
                        let pipeline = bound_pipeline.ok_or_else(|| {
                            Error::InvalidInput("dispatch pass requires a compute pipeline".into())
                        })?;
                        if pipeline.bind_point != vk::PipelineBindPoint::COMPUTE {
                            return Err(Error::InvalidInput(
                                "dispatch pass pipeline must bind to the compute pipeline bind point"
                                    .into(),
                            ));
                        }
                        unsafe {
                            device.cmd_dispatch(command_buffer, dispatch.x, dispatch.y, dispatch.z);
                        }
                    }
                    PassWork::Draw(draw) => {
                        let pipeline = bound_pipeline.ok_or_else(|| {
                            Error::InvalidInput("draw pass requires a graphics pipeline".into())
                        })?;
                        if pipeline.bind_point != vk::PipelineBindPoint::GRAPHICS {
                            return Err(Error::InvalidInput(
                                "draw pass pipeline must bind to the graphics pipeline bind point"
                                    .into(),
                            ));
                        }
                        let vertex_buffer = draw
                            .vertex_buffer
                            .map(|binding| {
                                Ok((
                                    binding.binding,
                                    resources.buffer(binding.buffer)?,
                                    binding.offset,
                                ))
                            })
                            .transpose()?;
                        let index_buffer = draw
                            .index_buffer
                            .map(|binding| {
                                Ok((
                                    resources.buffer(binding.buffer)?,
                                    binding.offset,
                                    vk_index_type(binding.format),
                                ))
                            })
                            .transpose()?;
                        self.record_draw_pass(
                            device,
                            command_buffer,
                            pass,
                            pipeline.render_pass,
                            resources,
                            pipelines,
                            || unsafe {
                                if let Some((binding, buffer, offset)) = vertex_buffer {
                                    let buffers = [buffer];
                                    let offsets = [offset];
                                    device.cmd_bind_vertex_buffers(
                                        command_buffer,
                                        binding,
                                        &buffers,
                                        &offsets,
                                    );
                                }
                                if let Some((buffer, offset, index_type)) = index_buffer {
                                    device.cmd_bind_index_buffer(
                                        command_buffer,
                                        buffer,
                                        offset,
                                        index_type,
                                    );
                                    device.cmd_draw_indexed(
                                        command_buffer,
                                        draw.vertex_count,
                                        draw.instance_count,
                                        draw.first_vertex,
                                        0,
                                        draw.first_instance,
                                    );
                                } else {
                                    device.cmd_draw(
                                        command_buffer,
                                        draw.vertex_count,
                                        draw.instance_count,
                                        draw.first_vertex,
                                        draw.first_instance,
                                    );
                                }
                            },
                        )?;
                    }
                    PassWork::CopyImageToBuffer(copy) => unsafe {
                        device.cmd_copy_image_to_buffer(
                            command_buffer,
                            resources.image(copy.image)?,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            resources.buffer(copy.buffer)?,
                            &[vk::BufferImageCopy::default()
                                .buffer_offset(copy.buffer_offset)
                                .buffer_row_length(0)
                                .buffer_image_height(0)
                                .image_subresource(vk::ImageSubresourceLayers {
                                    aspect_mask: vk::ImageAspectFlags::COLOR,
                                    mip_level: 0,
                                    base_array_layer: 0,
                                    layer_count: 1,
                                })
                                .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                                .image_extent(vk::Extent3D {
                                    width: copy.width,
                                    height: copy.height,
                                    depth: copy.depth,
                                })],
                        );
                    },
                }
            }
        }

        unsafe {
            device
                .end_command_buffer(command_buffer)
                .map_err(|error| Error::Backend(format!("vkEndCommandBuffer failed: {error:?}")))?;
        }
        Ok(())
    }

    fn record_draw_pass(
        &self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        pass: &PassDesc,
        render_pass: vk::RenderPass,
        resources: &ResourceRegistry,
        pipelines: &mut PipelineRegistry,
        record_draw: impl FnOnce(),
    ) -> Result<()> {
        let color_uses = pass
            .writes
            .iter()
            .filter(|usage| usage.state == RgState::RenderTarget)
            .collect::<Vec<_>>();
        if color_uses.is_empty() {
            return Err(Error::InvalidInput(
                "draw pass requires at least one RenderTarget image write".into(),
            ));
        }

        let attachments = color_uses
            .iter()
            .map(|usage| resources.image_view(usage.image))
            .collect::<Result<Vec<_>>>()?;
        let first_desc = resources.image_desc(color_uses[0].image)?;
        let framebuffer = pipelines.get_or_create_framebuffer(
            device,
            render_pass,
            &attachments,
            first_desc.extent.width,
            first_desc.extent.height,
            first_desc.layers as u32,
        )?;

        let clear_values = attachments
            .iter()
            .map(|_| vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.05, 0.07, 0.10, 1.0],
                },
            })
            .collect::<Vec<_>>();
        let render_area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: first_desc.extent.width,
                height: first_desc.extent.height,
            },
        };
        let begin = vk::RenderPassBeginInfo::default()
            .render_pass(render_pass)
            .framebuffer(framebuffer)
            .render_area(render_area)
            .clear_values(&clear_values);
        unsafe {
            device.cmd_begin_render_pass(command_buffer, &begin, vk::SubpassContents::INLINE);
            device.cmd_set_viewport(
                command_buffer,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: first_desc.extent.width as f32,
                    height: first_desc.extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            device.cmd_set_scissor(command_buffer, 0, &[render_area]);
        }

        record_draw();

        unsafe {
            device.cmd_end_render_pass(command_buffer);
        }
        Ok(())
    }

    fn record_pass_barriers(
        &self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        image_barriers: &[ImageBarrier],
        buffer_barriers: &[BufferBarrier],
        resources: &ResourceRegistry,
    ) -> Result<()> {
        if image_barriers.is_empty() && buffer_barriers.is_empty() {
            return Ok(());
        }

        let vk_image_barriers = image_barriers
            .iter()
            .map(|barrier| {
                Ok(vk::ImageMemoryBarrier::default()
                    .src_access_mask(access_mask(barrier.before))
                    .dst_access_mask(access_mask(barrier.after))
                    .old_layout(image_layout(barrier.before))
                    .new_layout(image_layout(barrier.after))
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(resources.image(barrier.image)?)
                    .subresource_range(subresource_range(barrier.after, barrier.subresource)))
            })
            .collect::<Result<Vec<_>>>()?;
        let vk_buffer_barriers = buffer_barriers
            .iter()
            .map(|barrier| {
                Ok(vk::BufferMemoryBarrier::default()
                    .src_access_mask(access_mask(barrier.before))
                    .dst_access_mask(access_mask(barrier.after))
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .buffer(resources.buffer(barrier.buffer)?)
                    .offset(barrier.offset)
                    .size(if barrier.size == 0 {
                        vk::WHOLE_SIZE
                    } else {
                        barrier.size
                    }))
            })
            .collect::<Result<Vec<_>>>()?;

        unsafe {
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &vk_buffer_barriers,
                &vk_image_barriers,
            );
        }
        Ok(())
    }

    fn submit_and_wait(
        &self,
        device: &Device,
        queue: vk::Queue,
        command_buffer: vk::CommandBuffer,
        fence: vk::Fence,
    ) -> Result<()> {
        let command_buffers = [command_buffer];
        let submit = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
        unsafe {
            device
                .queue_submit(queue, &submit, fence)
                .map_err(|error| Error::Backend(format!("vkQueueSubmit failed: {error:?}")))?;
            device
                .wait_for_fences(&[fence], true, u64::MAX)
                .map_err(|error| Error::Backend(format!("vkWaitForFences failed: {error:?}")))?;
        }
        Ok(())
    }
}

fn access_mask(state: RgState) -> vk::AccessFlags {
    match state {
        RgState::Undefined => vk::AccessFlags::empty(),
        RgState::ShaderRead => vk::AccessFlags::SHADER_READ,
        RgState::ShaderWrite => vk::AccessFlags::SHADER_WRITE,
        RgState::RenderTarget => vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        RgState::DepthRead => vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ,
        RgState::DepthWrite => vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        RgState::CopySrc => vk::AccessFlags::TRANSFER_READ,
        RgState::CopyDst => vk::AccessFlags::TRANSFER_WRITE,
        RgState::Present => vk::AccessFlags::MEMORY_READ,
        RgState::UniformRead => vk::AccessFlags::UNIFORM_READ,
        RgState::VertexRead => vk::AccessFlags::VERTEX_ATTRIBUTE_READ,
        RgState::IndexRead => vk::AccessFlags::INDEX_READ,
        RgState::IndirectRead => vk::AccessFlags::INDIRECT_COMMAND_READ,
    }
}

fn vk_index_type(format: IndexFormat) -> vk::IndexType {
    match format {
        IndexFormat::Uint16 => vk::IndexType::UINT16,
        IndexFormat::Uint32 => vk::IndexType::UINT32,
    }
}

fn image_layout(state: RgState) -> vk::ImageLayout {
    match state {
        RgState::Undefined => vk::ImageLayout::UNDEFINED,
        RgState::ShaderRead => vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        RgState::ShaderWrite => vk::ImageLayout::GENERAL,
        RgState::RenderTarget => vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        RgState::DepthRead => vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL,
        RgState::DepthWrite => vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        RgState::CopySrc => vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        RgState::CopyDst => vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        RgState::Present => vk::ImageLayout::PRESENT_SRC_KHR,
        RgState::UniformRead | RgState::VertexRead | RgState::IndexRead | RgState::IndirectRead => {
            vk::ImageLayout::GENERAL
        }
    }
}

fn subresource_range(state: RgState, subresource: SubresourceRange) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(aspect_mask(state))
        .base_mip_level(subresource.base_mip as u32)
        .level_count(subresource_count(subresource.mip_count))
        .base_array_layer(subresource.base_layer as u32)
        .layer_count(subresource_count(subresource.layer_count))
}

fn aspect_mask(state: RgState) -> vk::ImageAspectFlags {
    match state {
        RgState::DepthRead | RgState::DepthWrite => vk::ImageAspectFlags::DEPTH,
        _ => vk::ImageAspectFlags::COLOR,
    }
}

fn subresource_count(count: u16) -> u32 {
    if count == u16::MAX {
        vk::REMAINING_MIP_LEVELS
    } else {
        count as u32
    }
}
