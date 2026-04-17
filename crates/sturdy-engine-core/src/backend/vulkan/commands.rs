use ash::{Device, vk};

use crate::{
    BufferBarrier, CompiledGraph, Error, Format, ImageBarrier, IndexFormat, PassDesc, PassWork,
    PushConstants, Result, RgState, SubresourceRange, SubmissionHandle,
};

use super::descriptors::DescriptorRegistry;
use super::pipelines::PipelineRegistry;
use super::resources::ResourceRegistry;

/// One command pool + command buffer per batch slot.
/// Pools are reset at the start of each frame; command buffers are reused.
struct BatchPool {
    pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
}

impl BatchPool {
    fn create(device: &Device, queue_family: u32) -> Result<Self> {
        let pool_info = vk::CommandPoolCreateInfo::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);
        let pool = unsafe {
            device
                .create_command_pool(&pool_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateCommandPool failed: {e:?}")))?
        };
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe {
            match device.allocate_command_buffers(&alloc_info) {
                Ok(bufs) => bufs[0],
                Err(e) => {
                    device.destroy_command_pool(pool, None);
                    return Err(Error::Backend(format!(
                        "vkAllocateCommandBuffers failed: {e:?}"
                    )));
                }
            }
        };
        Ok(Self { pool, command_buffer })
    }

    fn destroy(&self, device: &Device) {
        unsafe {
            device.destroy_command_pool(self.pool, None);
        }
    }
}

pub struct CommandContext {
    queue_family: u32,
    /// One pool per batch slot; grows to match the largest batch count seen.
    batch_pools: Vec<BatchPool>,
    frame_fence: vk::Fence,
    frame_submitted: bool,
    submission_count: u64,
}

impl CommandContext {
    pub fn create(device: &Device, queue_family: u32) -> Result<Self> {
        let fence_info = vk::FenceCreateInfo::default();
        let frame_fence = unsafe {
            device
                .create_fence(&fence_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateFence failed: {e:?}")))?
        };

        // Pre-allocate one batch pool so there is always at least one cmd buf.
        let initial_pool = BatchPool::create(device, queue_family)?;

        Ok(Self {
            queue_family,
            batch_pools: vec![initial_pool],
            frame_fence,
            frame_submitted: false,
            submission_count: 0,
        })
    }

    /// Record and submit one command buffer per graph batch, then return
    /// immediately.  The previous frame's fence is awaited first.
    pub fn submit_graph(
        &mut self,
        device: &Device,
        queue: vk::Queue,
        graph: &CompiledGraph,
        resources: &ResourceRegistry,
        descriptors: &DescriptorRegistry,
        pipelines: &mut PipelineRegistry,
        wait_semaphore: Option<vk::Semaphore>,
        signal_semaphore: Option<vk::Semaphore>,
    ) -> Result<SubmissionHandle> {
        // Wait for the previous frame before reusing pools / fence.
        if self.frame_submitted {
            unsafe {
                device
                    .wait_for_fences(&[self.frame_fence], true, u64::MAX)
                    .map_err(|e| Error::Backend(format!("vkWaitForFences failed: {e:?}")))?;
                device
                    .reset_fences(&[self.frame_fence])
                    .map_err(|e| Error::Backend(format!("vkResetFences failed: {e:?}")))?;
            }
            self.frame_submitted = false;
        }

        let num_batches = graph.batches.len().max(1);

        // Grow batch pool vec to cover the number of batches in this frame.
        while self.batch_pools.len() < num_batches {
            let bp = BatchPool::create(device, self.queue_family)?;
            self.batch_pools.push(bp);
        }

        // Reset all pools that will be used this frame.
        for bp in &self.batch_pools[..num_batches] {
            unsafe {
                device
                    .reset_command_pool(bp.pool, vk::CommandPoolResetFlags::empty())
                    .map_err(|e| Error::Backend(format!("vkResetCommandPool failed: {e:?}")))?;
            }
        }

        // Record each batch into its own command buffer.
        if graph.batches.is_empty() {
            // Empty graph: record one empty command buffer.
            let cmd = self.batch_pools[0].command_buffer;
            self.begin_cmd(device, cmd)?;
            self.end_cmd(device, cmd)?;
        } else {
            for (batch_idx, batch) in graph.batches.iter().enumerate() {
                let cmd = self.batch_pools[batch_idx].command_buffer;
                self.begin_cmd(device, cmd)?;
                for &pass_idx in &batch.pass_indices {
                    let pass_idx = pass_idx as usize;
                    let image_barriers = graph
                        .barriers_per_pass
                        .get(pass_idx)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let buffer_barriers = graph
                        .buffer_barriers_per_pass
                        .get(pass_idx)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    self.record_pass_barriers(
                        device,
                        cmd,
                        image_barriers,
                        buffer_barriers,
                        resources,
                    )?;
                    if let Some(pass) = graph.passes.get(pass_idx) {
                        self.record_pass(device, cmd, pass, resources, descriptors, pipelines)?;
                    }
                }
                self.end_cmd(device, cmd)?;
            }
        }

        // Collect command buffers in batch order.
        let cmd_bufs: Vec<vk::CommandBuffer> = self.batch_pools[..num_batches]
            .iter()
            .map(|bp| bp.command_buffer)
            .collect();

        let wait_sems: Vec<vk::Semaphore> = wait_semaphore.into_iter().collect();
        let signal_sems: Vec<vk::Semaphore> = signal_semaphore.into_iter().collect();
        let wait_stages: Vec<vk::PipelineStageFlags> = wait_sems
            .iter()
            .map(|_| vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .collect();

        let submit_info = vk::SubmitInfo::default()
            .command_buffers(&cmd_bufs)
            .wait_semaphores(&wait_sems)
            .wait_dst_stage_mask(&wait_stages)
            .signal_semaphores(&signal_sems);

        unsafe {
            device
                .queue_submit(queue, &[submit_info], self.frame_fence)
                .map_err(|e| Error::Backend(format!("vkQueueSubmit failed: {e:?}")))?;
        }
        self.submission_count += 1;
        self.frame_submitted = true;
        Ok(SubmissionHandle(self.submission_count))
    }

    /// Block until the GPU finishes the work represented by `token`.
    pub fn wait_for_submission(&self, device: &Device, token: SubmissionHandle) -> Result<()> {
        if self.frame_submitted && token.0 == self.submission_count {
            unsafe {
                device
                    .wait_for_fences(&[self.frame_fence], true, u64::MAX)
                    .map_err(|e| Error::Backend(format!("vkWaitForFences failed: {e:?}")))?;
            }
        }
        Ok(())
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            // device_wait_idle is called first in VulkanBackend::Drop.
            device.destroy_fence(self.frame_fence, None);
            for bp in &self.batch_pools {
                bp.destroy(device);
            }
        }
    }

    // ── private helpers ──────────────────────────────────────────────────────

    fn begin_cmd(&self, device: &Device, cmd: vk::CommandBuffer) -> Result<()> {
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            device
                .begin_command_buffer(cmd, &begin)
                .map_err(|e| Error::Backend(format!("vkBeginCommandBuffer failed: {e:?}")))
        }
    }

    fn end_cmd(&self, device: &Device, cmd: vk::CommandBuffer) -> Result<()> {
        unsafe {
            device
                .end_command_buffer(cmd)
                .map_err(|e| Error::Backend(format!("vkEndCommandBuffer failed: {e:?}")))
        }
    }

    fn record_pass(
        &self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        pass: &PassDesc,
        resources: &ResourceRegistry,
        descriptors: &DescriptorRegistry,
        pipelines: &mut PipelineRegistry,
    ) -> Result<()> {
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
            if let Some(push_constants) = &pass.push_constants {
                record_push_constants(device, command_buffer, pipeline, push_constants)?;
            }
            bound_pipeline = Some(pipeline);
        } else if pass.push_constants.is_some() {
            return Err(Error::InvalidInput(
                "push constants require a bound pipeline".into(),
            ));
        }

        match pass.work {
            PassWork::None => {}
            PassWork::Dispatch(dispatch) => {
                let pipeline = bound_pipeline.ok_or_else(|| {
                    Error::InvalidInput("dispatch pass requires a compute pipeline".into())
                })?;
                if pipeline.bind_point != vk::PipelineBindPoint::COMPUTE {
                    return Err(Error::InvalidInput(
                        "dispatch pass pipeline must bind to the compute pipeline bind point".into(),
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
                        "draw pass pipeline must bind to the graphics pipeline bind point".into(),
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
                let image_desc = resources.image_desc(copy.image)?;
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
                            aspect_mask: image_aspect_mask(image_desc.format),
                            mip_level: copy.mip_level,
                            base_array_layer: copy.base_layer,
                            layer_count: copy.layer_count,
                        })
                        .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                        .image_extent(vk::Extent3D {
                            width: copy.width,
                            height: copy.height,
                            depth: copy.depth,
                        })],
                );
            },
            PassWork::CopyBufferToImage(copy) => unsafe {
                let image_desc = resources.image_desc(copy.image)?;
                device.cmd_copy_buffer_to_image(
                    command_buffer,
                    resources.buffer(copy.buffer)?,
                    resources.image(copy.image)?,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy::default()
                        .buffer_offset(copy.buffer_offset)
                        .buffer_row_length(0)
                        .buffer_image_height(0)
                        .image_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: image_aspect_mask(image_desc.format),
                            mip_level: copy.mip_level,
                            base_array_layer: copy.base_layer,
                            layer_count: copy.layer_count,
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

fn record_push_constants(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    pipeline: super::pipelines::VulkanPipeline,
    push_constants: &PushConstants,
) -> Result<()> {
    let end = push_constants
        .offset
        .checked_add(push_constants.bytes.len() as u32)
        .ok_or_else(|| Error::InvalidInput("push constant byte range overflowed".into()))?;
    if end > pipeline.push_constants_bytes {
        return Err(Error::InvalidInput(format!(
            "push constant byte range [{}, {}) exceeds pipeline layout push constant size {}",
            push_constants.offset, end, pipeline.push_constants_bytes
        )));
    }
    unsafe {
        device.cmd_push_constants(
            command_buffer,
            pipeline.layout,
            pipeline.push_constant_stages,
            push_constants.offset,
            &push_constants.bytes,
        );
    }
    Ok(())
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

fn image_aspect_mask(format: Format) -> vk::ImageAspectFlags {
    match format {
        Format::Depth32Float => vk::ImageAspectFlags::DEPTH,
        Format::Depth24Stencil8 => vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL,
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
