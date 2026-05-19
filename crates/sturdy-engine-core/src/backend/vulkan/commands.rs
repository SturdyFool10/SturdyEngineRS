use std::collections::HashMap;

use ash::ext::mesh_shader;
use ash::vk::TaggedStructure;
use ash::{Device, vk};

#[path = "commands/batch_pool.rs"]
mod batch_pool;

use crate::{
    AccelerationStructureBuildMode, BufferBarrier, CompiledGraph, Error, Extent3d, Format,
    ImageBarrier, IndexFormat, OpticalFlowEstimateDesc, OpticalFlowSessionHandle, PassDesc,
    PassWork, PipelineHandle, PushConstants, PushDescriptorBinding, Result, RgState, ShaderBinding,
    ShaderBindingTableRegion, ShadingRate, SubmissionHandle, SubresourceRange, VertexFormat,
};

use super::bindless::BindlessVkInfo;
use super::debug::DebugUtils;
use super::descriptors::DescriptorRegistry;
use super::pipelines::{PipelineRegistry, VulkanGraphicsPipelineState, VulkanPipeline};
use super::queues::{QueueFamilyMap, VulkanQueues, queue_family_index};
use super::resources::{ResourceRegistry, VulkanScratchBuffer};
use batch_pool::BatchPool;

/// Maximum number of render-graph passes we track per frame with GPU timestamps.
const MAX_TIMESTAMP_PASSES: u32 = 256;

struct ActiveShaderBinding {
    bind_point: vk::PipelineBindPoint,
    layout: vk::PipelineLayout,
    render_pass: vk::RenderPass,
    push_constants_bytes: u32,
    push_constant_stages: vk::ShaderStageFlags,
    uses_bindless: bool,
    uses_shader_objects: bool,
    shader_object_stages: vk::ShaderStageFlags,
    graphics_state: Option<VulkanGraphicsPipelineState>,
}

/// AMD buffer marker breadcrumb buffer: holds `MAX_TIMESTAMP_PASSES * 2` u32 values, one
/// per pass-start and one per pass-end. On device loss, the last written index identifies
/// the faulting pass. Present only in debug builds when VK_AMD_buffer_marker is available.
#[cfg(debug_assertions)]
struct BreadcrumbBuffer {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    ptr: *mut u32,
}

#[cfg(debug_assertions)]
unsafe impl Send for BreadcrumbBuffer {}
#[cfg(debug_assertions)]
unsafe impl Sync for BreadcrumbBuffer {}

pub struct CommandContext {
    /// One pool per batch slot; grows to match the largest batch count seen.
    batch_pools: Vec<BatchPool>,
    pending_semaphores: Vec<vk::Semaphore>,
    frame_fence: vk::Fence,
    frame_submitted: bool,
    submission_count: u64,
    /// Timestamp query pool — 2 queries per pass (before + after).
    timestamp_pool: vk::QueryPool,
    /// Nanoseconds per GPU timestamp tick (from physical device limits).
    timestamp_period_ns: f32,
    /// Pass names recorded during the most-recently-submitted frame.
    /// Populated during recording; used to label the readback results.
    pending_pass_names: Vec<String>,
    /// How many passes were submitted in the last frame.
    pending_pass_count: u32,
    /// Per-pass GPU timings from the previous frame (name, milliseconds).
    /// Empty until the second `submit_graph` call (first readback).
    pub pass_timings: Vec<(String, f32)>,
    acceleration_structure_scratch: Vec<VulkanScratchBuffer>,
    /// AMD buffer marker breadcrumb buffer (debug builds only).
    #[cfg(debug_assertions)]
    breadcrumb: Option<BreadcrumbBuffer>,
    /// AMD buffer marker device for writing to the breadcrumb buffer (debug builds only).
    #[cfg(debug_assertions)]
    buffer_marker_amd: Option<ash::amd::buffer_marker::Device>,
    /// GFX-1c: Timeline semaphore for inter-batch chain synchronization.
    ///
    /// When available, replaces per-frame binary chain semaphore creation/destruction.
    /// Value incremented each frame by `batch_count - 1` to synchronize N-batch submissions.
    chain_timeline: Option<vk::Semaphore>,
    chain_value: u64,
    /// GFX-1b: Framebuffers created during this frame's recording.
    /// Destroyed at the start of the *next* frame, after the fence signals.
    transient_framebuffers: Vec<vk::Framebuffer>,
    /// Track 11a: Per-frame transient buffer pool for uniforms and small staging copies.
    /// Reset to offset 0 each frame after the fence fires.
    pub transient_buffer_pool: Option<super::buffer_pool::BufferPool>,
}

impl CommandContext {
    pub fn create(
        device: &Device,
        queue_families: QueueFamilyMap,
        timestamp_period_ns: f32,
        #[cfg_attr(not(debug_assertions), allow(unused_variables))] buffer_marker_amd: Option<
            &ash::amd::buffer_marker::Device,
        >,
        #[cfg_attr(not(debug_assertions), allow(unused_variables))]
        memory_properties: vk::PhysicalDeviceMemoryProperties,
        use_timeline_chains: bool,
    ) -> Result<Self> {
        let fence_info = vk::FenceCreateInfo::default();
        let frame_fence = unsafe {
            device
                .create_fence(&fence_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateFence failed: {e:?}")))?
        };

        // Timestamp query pool: 2 entries per pass (start + end).
        let pool_info = vk::QueryPoolCreateInfo::default()
            .query_type(vk::QueryType::TIMESTAMP)
            .query_count(MAX_TIMESTAMP_PASSES * 2);
        let timestamp_pool = unsafe {
            device
                .create_query_pool(&pool_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateQueryPool failed: {e:?}")))?
        };

        // GFX-1c: timeline semaphore for inter-batch chain synchronization.
        let chain_timeline = if use_timeline_chains {
            let mut tl_info = vk::SemaphoreTypeCreateInfo::default()
                .semaphore_type(vk::SemaphoreType::TIMELINE)
                .initial_value(0);
            let sem_info = vk::SemaphoreCreateInfo::default().push(&mut tl_info);
            unsafe { device.create_semaphore(&sem_info, None).ok() }
        } else {
            None
        };

        // Pre-allocate one batch pool so there is always at least one cmd buf.
        let initial_pool = BatchPool::create(device, queue_families.graphics)?;

        // AMD breadcrumb buffer: host-visible, host-coherent, transfer-dst.
        #[cfg(debug_assertions)]
        let breadcrumb = {
            if buffer_marker_amd.is_some() {
                let buf_size = (MAX_TIMESTAMP_PASSES * 2 * 4) as u64;
                let buf_info = vk::BufferCreateInfo::default()
                    .size(buf_size)
                    .usage(vk::BufferUsageFlags::TRANSFER_DST)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE);
                let maybe: Option<BreadcrumbBuffer> = unsafe {
                    device.create_buffer(&buf_info, None).ok().and_then(|buf| {
                        let req = device.get_buffer_memory_requirements(buf);
                        // Find HOST_VISIBLE | HOST_COHERENT memory type.
                        let mut mem_type = None;
                        for i in 0..memory_properties.memory_type_count {
                            if (req.memory_type_bits & (1 << i)) != 0 {
                                let flags =
                                    memory_properties.memory_types[i as usize].property_flags;
                                if flags.contains(
                                    vk::MemoryPropertyFlags::HOST_VISIBLE
                                        | vk::MemoryPropertyFlags::HOST_COHERENT,
                                ) {
                                    mem_type = Some(i);
                                    break;
                                }
                            }
                        }
                        let mt = mem_type?;
                        let alloc_info = vk::MemoryAllocateInfo::default()
                            .allocation_size(req.size)
                            .memory_type_index(mt);
                        let memory = device.allocate_memory(&alloc_info, None).ok()?;
                        device.bind_buffer_memory(buf, memory, 0).ok()?;
                        let ptr = device
                            .map_memory(memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                            .ok()? as *mut u32;
                        // Initialize to sentinel (0xFFFF_FFFF = no pass written).
                        ptr.write_bytes(0xFF, (buf_size / 4) as usize);
                        Some(BreadcrumbBuffer {
                            buffer: buf,
                            memory,
                            ptr,
                        })
                    })
                };
                maybe
            } else {
                None
            }
        };

        Ok(Self {
            batch_pools: vec![initial_pool],
            pending_semaphores: Vec::new(),
            frame_fence,
            frame_submitted: false,
            submission_count: 0,
            timestamp_pool,
            timestamp_period_ns,
            pending_pass_names: Vec::new(),
            pending_pass_count: 0,
            pass_timings: Vec::new(),
            acceleration_structure_scratch: Vec::new(),
            #[cfg(debug_assertions)]
            breadcrumb,
            #[cfg(debug_assertions)]
            buffer_marker_amd: buffer_marker_amd.cloned(),
            chain_timeline,
            chain_value: 0,
            transient_framebuffers: Vec::new(),
            transient_buffer_pool: None,
        })
    }

    /// Track 11a: Install a pre-created `BufferPool` into this context.
    /// Called once at device creation after the pool is allocated.
    pub fn set_buffer_pool(&mut self, pool: super::buffer_pool::BufferPool) {
        self.transient_buffer_pool = Some(pool);
    }

    /// Record and submit one command buffer per graph batch, then return
    /// immediately.  The previous frame's fence is awaited first.
    pub fn submit_graph(
        &mut self,
        device: &Device,
        queues: VulkanQueues,
        queue_families: QueueFamilyMap,
        graph: &CompiledGraph,
        resources: &mut ResourceRegistry,
        descriptors: &DescriptorRegistry,
        pipelines: &mut PipelineRegistry,
        debug: &DebugUtils,
        bindless: Option<BindlessVkInfo>,
        mesh_shader_ext: Option<&mesh_shader::Device>,
        sync2: Option<&ash::khr::synchronization2::Device>,
        dynamic_rendering: Option<&ash::khr::dynamic_rendering::Device>,
        push_descriptor: Option<&ash::khr::push_descriptor::Device>,
        conditional_rendering: Option<&ash::ext::conditional_rendering::Device>,
        fragment_shading_rate: Option<&ash::khr::fragment_shading_rate::Device>,
        as_khr: Option<&ash::khr::acceleration_structure::Device>,
        rt_khr: Option<&ash::khr::ray_tracing_pipeline::Device>,
        shader_object_ext: Option<&ash::ext::shader_object::Device>,
        ray_tracing_position_fetch: bool,
        diagnostic_checkpoints_nv: Option<&ash::nv::device_diagnostic_checkpoints::Device>,
        extended_dynamic_state3: Option<&ash::ext::extended_dynamic_state3::Device>,
        vertex_input_dynamic_state: Option<&ash::ext::vertex_input_dynamic_state::Device>,
        ray_tracing_maintenance1: Option<&ash::khr::ray_tracing_maintenance1::Device>,
        optical_flow_nv: Option<&ash::nv::optical_flow::Device>,
        cluster_as_nv: Option<&ash::nv::cluster_acceleration_structure::Device>,
        optical_flow_sessions: Option<&HashMap<OpticalFlowSessionHandle, vk::OpticalFlowSessionNV>>,
        dgc_nv: Option<&ash::nv::device_generated_commands::Device>,
        indirect_command_layouts: Option<
            &HashMap<crate::IndirectCommandLayoutHandle, vk::IndirectCommandsLayoutNV>,
        >,
        wait_semaphore: Option<vk::Semaphore>,
        signal_semaphore: Option<vk::Semaphore>,
    ) -> Result<SubmissionHandle> {
        // Suppress unused warning when debug_assertions are disabled.
        #[cfg(not(debug_assertions))]
        let _ = &diagnostic_checkpoints_nv;
        // Wait for the previous frame before reusing pools / fence.
        if self.frame_submitted {
            unsafe {
                device
                    .wait_for_fences(&[self.frame_fence], true, u64::MAX)
                    .map_err(|e| {
                        if e == ash::vk::Result::ERROR_DEVICE_LOST {
                            Error::DeviceLost(
                                "vkWaitForFences returned VK_ERROR_DEVICE_LOST".into(),
                            )
                        } else {
                            Error::Backend(format!("vkWaitForFences failed: {e:?}"))
                        }
                    })?;
                device
                    .reset_fences(&[self.frame_fence])
                    .map_err(|e| Error::Backend(format!("vkResetFences failed: {e:?}")))?;
            }
            // Read back timestamps from the just-completed frame.
            if self.pending_pass_count > 0 {
                let n = self.pending_pass_count as usize;
                let mut raw = vec![0u64; n * 2];
                let result = unsafe {
                    device.get_query_pool_results(
                        self.timestamp_pool,
                        0,
                        &mut raw[..n * 2],
                        vk::QueryResultFlags::TYPE_64,
                    )
                };
                if result.is_ok() {
                    let period = self.timestamp_period_ns;
                    self.pass_timings = self
                        .pending_pass_names
                        .iter()
                        .enumerate()
                        .map(|(i, name)| {
                            let start = raw[i * 2];
                            let end = raw[i * 2 + 1];
                            let ms = (end.saturating_sub(start)) as f32 * period / 1_000_000.0;
                            (name.clone(), ms)
                        })
                        .collect();
                }
                self.pending_pass_count = 0;
                self.pending_pass_names.clear();
            }

            // GFX-1b: destroy transient framebuffers from the previous frame.
            for fb in self.transient_framebuffers.drain(..) {
                unsafe { device.destroy_framebuffer(fb, None) };
            }
            // Track 11a: reset transient buffer pool — all previous-frame allocations are safe to reuse.
            if let Some(pool) = &mut self.transient_buffer_pool {
                pool.reset();
            }
            for semaphore in self.pending_semaphores.drain(..) {
                unsafe {
                    device.destroy_semaphore(semaphore, None);
                }
            }
            for scratch in self.acceleration_structure_scratch.drain(..) {
                resources.destroy_scratch_buffer(device, scratch)?;
            }
            self.frame_submitted = false;
        }
        for scratch in self.acceleration_structure_scratch.drain(..) {
            resources.destroy_scratch_buffer(device, scratch)?;
        }

        let num_batches = graph.batches.len().max(1);

        // Grow batch pool vec to cover the number of batches in this frame.
        while self.batch_pools.len() < num_batches {
            let batch_queue = graph
                .batches
                .get(self.batch_pools.len())
                .map(|batch| batch.queue)
                .unwrap_or(crate::QueueType::Graphics);
            let bp = BatchPool::create(device, queue_families.family(batch_queue))?;
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

        // Reset the timestamp query pool for this frame's queries.
        // We reset in the first command buffer of the first batch.
        let mut ts_query_idx: u32 = 0;
        self.pending_pass_names.clear();

        // Record each batch into its own command buffer.
        if graph.batches.is_empty() {
            // Empty graph: record one empty command buffer.
            let cmd = self.batch_pools[0].command_buffer;
            self.begin_cmd(device, cmd)?;
            unsafe {
                device.cmd_reset_query_pool(cmd, self.timestamp_pool, 0, MAX_TIMESTAMP_PASSES * 2);
            }
            self.end_cmd(device, cmd)?;
        } else {
            let mut reset_done = false;
            for (batch_idx, batch) in graph.batches.iter().enumerate() {
                let cmd = self.batch_pools[batch_idx].command_buffer;
                self.begin_cmd(device, cmd)?;
                if !reset_done {
                    unsafe {
                        device.cmd_reset_query_pool(
                            cmd,
                            self.timestamp_pool,
                            0,
                            MAX_TIMESTAMP_PASSES * 2,
                        );
                    }
                    reset_done = true;
                }
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
                        queue_families,
                        sync2,
                    )?;
                    if let Some(pass) = graph.passes.get(pass_idx) {
                        // GFX-1g breadcrumbs at pass start: NV checkpoint + AMD buffer marker.
                        #[cfg(debug_assertions)]
                        if let Some(cp) = diagnostic_checkpoints_nv {
                            unsafe {
                                cp.cmd_set_checkpoint_nv(cmd, (pass_idx * 2) as usize as *const _);
                            }
                        }
                        #[cfg(debug_assertions)]
                        if let (Some(bm), Some(bc)) =
                            (self.buffer_marker_amd.as_ref(), self.breadcrumb.as_ref())
                        {
                            unsafe {
                                bm.cmd_write_buffer_marker(
                                    cmd,
                                    vk::PipelineStageFlags::TOP_OF_PIPE,
                                    bc.buffer,
                                    (pass_idx as u64) * 2 * 4,
                                    (pass_idx * 2) as u32,
                                );
                            }
                        }
                        // Write start timestamp before the pass.
                        let slot = ts_query_idx;
                        if slot < MAX_TIMESTAMP_PASSES {
                            if let Some(s2) = sync2 {
                                unsafe {
                                    s2.cmd_write_timestamp2(
                                        cmd,
                                        vk::PipelineStageFlags2::TOP_OF_PIPE,
                                        self.timestamp_pool,
                                        slot * 2,
                                    );
                                }
                            } else {
                                unsafe {
                                    device.cmd_write_timestamp(
                                        cmd,
                                        vk::PipelineStageFlags::TOP_OF_PIPE,
                                        self.timestamp_pool,
                                        slot * 2,
                                    );
                                }
                            }
                        }

                        if !pass.name.is_empty() {
                            debug.begin_region(cmd, &pass.name, [0.5, 0.5, 1.0, 1.0]);
                        }
                        self.record_pass(
                            device,
                            cmd,
                            pass,
                            resources,
                            descriptors,
                            pipelines,
                            bindless,
                            mesh_shader_ext,
                            sync2,
                            dynamic_rendering,
                            push_descriptor,
                            conditional_rendering,
                            fragment_shading_rate,
                            as_khr,
                            rt_khr,
                            shader_object_ext,
                            ray_tracing_position_fetch,
                            extended_dynamic_state3,
                            vertex_input_dynamic_state,
                            ray_tracing_maintenance1,
                            optical_flow_nv,
                            cluster_as_nv,
                            optical_flow_sessions,
                            dgc_nv,
                            indirect_command_layouts,
                        )?;
                        if !pass.name.is_empty() {
                            debug.end_region(cmd);
                        }

                        // Write end timestamp after the pass.
                        if slot < MAX_TIMESTAMP_PASSES {
                            if let Some(s2) = sync2 {
                                unsafe {
                                    s2.cmd_write_timestamp2(
                                        cmd,
                                        vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
                                        self.timestamp_pool,
                                        slot * 2 + 1,
                                    );
                                }
                            } else {
                                unsafe {
                                    device.cmd_write_timestamp(
                                        cmd,
                                        vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                                        self.timestamp_pool,
                                        slot * 2 + 1,
                                    );
                                }
                            }
                            self.pending_pass_names.push(pass.name.clone());
                            ts_query_idx += 1;
                        }
                        // GFX-1g breadcrumbs at pass end: NV checkpoint + AMD buffer marker.
                        #[cfg(debug_assertions)]
                        if let Some(cp) = diagnostic_checkpoints_nv {
                            unsafe {
                                cp.cmd_set_checkpoint_nv(
                                    cmd,
                                    (pass_idx * 2 + 1) as usize as *const _,
                                );
                            }
                        }
                        #[cfg(debug_assertions)]
                        if let (Some(bm), Some(bc)) =
                            (self.buffer_marker_amd.as_ref(), self.breadcrumb.as_ref())
                        {
                            unsafe {
                                bm.cmd_write_buffer_marker(
                                    cmd,
                                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                                    bc.buffer,
                                    (pass_idx as u64) * 2 * 4 + 4,
                                    (pass_idx * 2 + 1) as u32,
                                );
                            }
                        }
                    }
                }
                self.end_cmd(device, cmd)?;
            }
        }
        self.pending_pass_count = ts_query_idx;

        let batch_count = graph.batches.len().max(1);
        // GFX-1c: use timeline semaphore for inter-batch chains when available,
        // otherwise fall back to binary semaphores (created and destroyed each frame).
        let mut chain_semaphores = Vec::new();
        let use_timeline = self.chain_timeline.is_some();
        if !use_timeline {
            for _ in 1..batch_count {
                let info = vk::SemaphoreCreateInfo::default();
                let semaphore = unsafe {
                    device
                        .create_semaphore(&info, None)
                        .map_err(|e| Error::Backend(format!("vkCreateSemaphore failed: {e:?}")))?
                };
                chain_semaphores.push(semaphore);
            }
        }

        for batch_index in 0..batch_count {
            let batch_queue = graph
                .batches
                .get(batch_index)
                .map(|batch| batch.queue)
                .unwrap_or(crate::QueueType::Graphics);
            let mut wait_sems = Vec::new();
            if batch_index == 0 {
                wait_sems.extend(wait_semaphore);
            } else if !use_timeline {
                wait_sems.push(chain_semaphores[batch_index - 1]);
            }
            let mut signal_sems = Vec::new();
            if batch_index + 1 < batch_count && !use_timeline {
                signal_sems.push(chain_semaphores[batch_index]);
            } else if batch_index + 1 == batch_count {
                signal_sems.extend(signal_semaphore);
            }
            let fence = if batch_index + 1 == batch_count {
                self.frame_fence
            } else {
                vk::Fence::null()
            };
            if let Some(s2) = sync2 {
                // VK_KHR_synchronization2 path: use vkQueueSubmit2.
                let cmd_buf_infos = [vk::CommandBufferSubmitInfo::default()
                    .command_buffer(self.batch_pools[batch_index].command_buffer)];
                // The swapchain-acquire semaphore (batch 0) blocks at COLOR_ATTACHMENT_OUTPUT.
                // Inter-batch chain semaphores just need TOP_OF_PIPE.
                let wait_stage2 = if batch_index == 0 {
                    vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT
                } else {
                    vk::PipelineStageFlags2::TOP_OF_PIPE
                };
                let mut wait_sem_infos: Vec<vk::SemaphoreSubmitInfo<'_>> = wait_sems
                    .iter()
                    .map(|&sem| {
                        vk::SemaphoreSubmitInfo::default()
                            .semaphore(sem)
                            .stage_mask(wait_stage2)
                    })
                    .collect();
                // GFX-1c: add timeline chain wait for non-first batches.
                if use_timeline && batch_index > 0 {
                    if let Some(tl) = self.chain_timeline {
                        wait_sem_infos.push(
                            vk::SemaphoreSubmitInfo::default()
                                .semaphore(tl)
                                .value(self.chain_value + batch_index as u64)
                                .stage_mask(vk::PipelineStageFlags2::TOP_OF_PIPE),
                        );
                    }
                }
                let mut signal_sem_infos: Vec<vk::SemaphoreSubmitInfo<'_>> = signal_sems
                    .iter()
                    .map(|&sem| {
                        vk::SemaphoreSubmitInfo::default()
                            .semaphore(sem)
                            .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
                    })
                    .collect();
                // GFX-1c: add timeline chain signal for non-last batches.
                if use_timeline && batch_index + 1 < batch_count {
                    if let Some(tl) = self.chain_timeline {
                        signal_sem_infos.push(
                            vk::SemaphoreSubmitInfo::default()
                                .semaphore(tl)
                                .value(self.chain_value + batch_index as u64 + 1)
                                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
                        );
                    }
                }
                let submit2_info = vk::SubmitInfo2::default()
                    .command_buffer_infos(&cmd_buf_infos)
                    .wait_semaphore_infos(&wait_sem_infos)
                    .signal_semaphore_infos(&signal_sem_infos);
                unsafe {
                    s2.queue_submit2(queues.queue(batch_queue), &[submit2_info], fence)
                        .map_err(|e| {
                            if e == ash::vk::Result::ERROR_DEVICE_LOST {
                                Error::DeviceLost(
                                    "vkQueueSubmit2 returned VK_ERROR_DEVICE_LOST".into(),
                                )
                            } else {
                                Error::Backend(format!("vkQueueSubmit2 failed: {e:?}"))
                            }
                        })?;
                }
            } else {
                // Legacy path: vkQueueSubmit.
                // The swapchain-acquire semaphore (batch 0) only needs to block at the
                // colour-attachment-output stage — vertex shading and earlier work can
                // run freely before the image is available.  Inter-batch chain semaphores
                // (batch N+1) just need TOP_OF_PIPE; the signalling submit already
                // provides full execution + memory visibility through the semaphore.
                let wait_stage = if batch_index == 0 {
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                } else {
                    vk::PipelineStageFlags::TOP_OF_PIPE
                };
                let wait_stages = wait_sems.iter().map(|_| wait_stage).collect::<Vec<_>>();
                let cmd_bufs = [self.batch_pools[batch_index].command_buffer];
                let submit_info = vk::SubmitInfo::default()
                    .command_buffers(&cmd_bufs)
                    .wait_semaphores(&wait_sems)
                    .wait_dst_stage_mask(&wait_stages)
                    .signal_semaphores(&signal_sems);
                unsafe {
                    device
                        .queue_submit(queues.queue(batch_queue), &[submit_info], fence)
                        .map_err(|e| {
                            if e == ash::vk::Result::ERROR_DEVICE_LOST {
                                Error::DeviceLost(
                                    "vkQueueSubmit returned VK_ERROR_DEVICE_LOST".into(),
                                )
                            } else {
                                Error::Backend(format!("vkQueueSubmit failed: {e:?}"))
                            }
                        })?;
                }
            }
        }
        self.pending_semaphores.extend(chain_semaphores);
        // GFX-1c: advance timeline chain value for next frame's wait.
        if use_timeline && batch_count > 1 {
            self.chain_value += (batch_count - 1) as u64;
        }
        self.submission_count += 1;
        self.frame_submitted = true;
        Ok(SubmissionHandle(self.submission_count))
    }

    /// Block until the GPU finishes the work represented by `token`.
    #[allow(dead_code)] // used by future multi-queue submission paths
    pub fn wait_for_submission(&self, device: &Device, token: SubmissionHandle) -> Result<()> {
        if self.frame_submitted && token.0 == self.submission_count {
            unsafe {
                device
                    .wait_for_fences(&[self.frame_fence], true, u64::MAX)
                    .map_err(|e| {
                        if e == ash::vk::Result::ERROR_DEVICE_LOST {
                            Error::DeviceLost(
                                "vkWaitForFences returned VK_ERROR_DEVICE_LOST".into(),
                            )
                        } else {
                            Error::Backend(format!("vkWaitForFences failed: {e:?}"))
                        }
                    })?;
            }
        }
        Ok(())
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            // device_wait_idle is called first in VulkanBackend::Drop.
            device.destroy_fence(self.frame_fence, None);
            device.destroy_query_pool(self.timestamp_pool, None);
            if let Some(pool) = &self.transient_buffer_pool {
                pool.destroy();
            }
            for fb in &self.transient_framebuffers {
                device.destroy_framebuffer(*fb, None);
            }
            for semaphore in &self.pending_semaphores {
                device.destroy_semaphore(*semaphore, None);
            }
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
        &mut self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        pass: &PassDesc,
        resources: &mut ResourceRegistry,
        descriptors: &DescriptorRegistry,
        pipelines: &mut PipelineRegistry,
        bindless: Option<BindlessVkInfo>,
        mesh_shader_ext: Option<&mesh_shader::Device>,
        sync2: Option<&ash::khr::synchronization2::Device>,
        dynamic_rendering: Option<&ash::khr::dynamic_rendering::Device>,
        push_descriptor: Option<&ash::khr::push_descriptor::Device>,
        conditional_rendering: Option<&ash::ext::conditional_rendering::Device>,
        fragment_shading_rate: Option<&ash::khr::fragment_shading_rate::Device>,
        as_khr: Option<&ash::khr::acceleration_structure::Device>,
        rt_khr: Option<&ash::khr::ray_tracing_pipeline::Device>,
        shader_object_ext: Option<&ash::ext::shader_object::Device>,
        ray_tracing_position_fetch: bool,
        extended_dynamic_state3: Option<&ash::ext::extended_dynamic_state3::Device>,
        vertex_input_dynamic_state: Option<&ash::ext::vertex_input_dynamic_state::Device>,
        ray_tracing_maintenance1: Option<&ash::khr::ray_tracing_maintenance1::Device>,
        optical_flow_nv: Option<&ash::nv::optical_flow::Device>,
        cluster_as_nv: Option<&ash::nv::cluster_acceleration_structure::Device>,
        optical_flow_sessions: Option<&HashMap<OpticalFlowSessionHandle, vk::OpticalFlowSessionNV>>,
        dgc_nv: Option<&ash::nv::device_generated_commands::Device>,
        indirect_command_layouts: Option<
            &HashMap<crate::IndirectCommandLayoutHandle, vk::IndirectCommandsLayoutNV>,
        >,
    ) -> Result<()> {
        if let Some(predicate) = pass.predicate {
            let conditional_rendering = conditional_rendering.ok_or_else(|| {
                Error::Unsupported(
                    "conditional rendering requires VK_EXT_conditional_rendering".into(),
                )
            })?;
            let buffer = resources.buffer(predicate.buffer)?;
            let flags = if predicate.inverted {
                vk::ConditionalRenderingFlagsEXT::INVERTED
            } else {
                vk::ConditionalRenderingFlagsEXT::empty()
            };
            let begin_info = vk::ConditionalRenderingBeginInfoEXT::default()
                .buffer(buffer)
                .offset(predicate.offset)
                .flags(flags);
            unsafe {
                (conditional_rendering
                    .fp()
                    .cmd_begin_conditional_rendering_ext)(
                    command_buffer, &begin_info
                );
            }
        }

        let bound_binding = bind_pass_shader_binding(
            device,
            command_buffer,
            pass,
            resources,
            descriptors,
            pipelines,
            bindless,
            push_descriptor,
            shader_object_ext,
        )?;

        // GFX-2d: Extended dynamic state 3 — record polygon mode and depth clamp when the
        // pipeline was created with these as dynamic states.
        if let (Some(eds3), Some(binding)) = (extended_dynamic_state3, bound_binding.as_ref()) {
            if binding.bind_point == vk::PipelineBindPoint::GRAPHICS && !binding.uses_shader_objects
            {
                if let Some(state) = &binding.graphics_state {
                    unsafe {
                        eds3.cmd_set_polygon_mode(command_buffer, state.polygon_mode);
                        eds3.cmd_set_depth_clamp_enable(command_buffer, state.depth_clamp);
                    }
                }
            }
        }

        // GFX-2d: Record rasterizer discard enable — core Vulkan 1.3 / VK_EXT_extended_dynamic_state.
        // RASTERIZER_DISCARD_ENABLE is always in the pipeline dynamic state list.
        if let Some(binding) = bound_binding.as_ref() {
            if binding.bind_point == vk::PipelineBindPoint::GRAPHICS && !binding.uses_shader_objects
            {
                if let Some(state) = &binding.graphics_state {
                    unsafe {
                        device.cmd_set_rasterizer_discard_enable(
                            command_buffer,
                            state.rasterizer_discard,
                        );
                    }
                }
            }
        }

        // GFX-2e: Vertex input dynamic state — record vertex format when it differs per-draw.
        if let (Some(vids), Some(binding)) = (vertex_input_dynamic_state, bound_binding.as_ref()) {
            if binding.bind_point == vk::PipelineBindPoint::GRAPHICS && !binding.uses_shader_objects
            {
                if let Some(state) = &binding.graphics_state {
                    if !state.vertex_bindings.is_empty() || !state.vertex_attributes.is_empty() {
                        unsafe {
                            vids.cmd_set_vertex_input(
                                command_buffer,
                                &state.vertex_bindings,
                                &state.vertex_attributes,
                            );
                        }
                    }
                }
            }
        }

        if let Some(rate) = pass.pipeline_shading_rate {
            let fragment_shading_rate = fragment_shading_rate.ok_or_else(|| {
                Error::Unsupported(
                    "pipeline shading rate requires VK_KHR_fragment_shading_rate".into(),
                )
            })?;
            let extent = vk_fragment_shading_rate(rate);
            let combiner_ops = [
                vk::FragmentShadingRateCombinerOpKHR::REPLACE,
                vk::FragmentShadingRateCombinerOpKHR::KEEP,
            ];
            unsafe {
                (fragment_shading_rate.fp().cmd_set_fragment_shading_rate_khr)(
                    command_buffer,
                    &extent,
                    &combiner_ops,
                );
            }
        }

        match pass.work {
            PassWork::None => {}
            PassWork::Dispatch(dispatch) => {
                let binding = bound_binding.as_ref().ok_or_else(|| {
                    Error::InvalidInput("dispatch pass requires a compute shader binding".into())
                })?;
                if binding.bind_point != vk::PipelineBindPoint::COMPUTE {
                    return Err(Error::InvalidInput(
                        "dispatch pass shader binding must use the compute bind point".into(),
                    ));
                }
                unsafe {
                    device.cmd_dispatch(command_buffer, dispatch.x, dispatch.y, dispatch.z);
                }
            }
            PassWork::Draw(draw) => {
                let binding = bound_binding.as_ref().ok_or_else(|| {
                    Error::InvalidInput("draw pass requires a graphics shader binding".into())
                })?;
                if binding.bind_point != vk::PipelineBindPoint::GRAPHICS {
                    return Err(Error::InvalidInput(
                        "draw pass shader binding must use the graphics bind point".into(),
                    ));
                }
                let shader_object_draw_ext =
                    prepare_shader_object_draw(command_buffer, binding, shader_object_ext)?;
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
                    binding.render_pass,
                    resources,
                    pipelines,
                    draw.viewport,
                    dynamic_rendering,
                    shader_object_draw_ext,
                    fragment_shading_rate,
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
            PassWork::DispatchIndirect(desc) => {
                let binding = bound_binding.as_ref().ok_or_else(|| {
                    Error::InvalidInput(
                        "dispatch-indirect pass requires a compute shader binding".into(),
                    )
                })?;
                if binding.bind_point != vk::PipelineBindPoint::COMPUTE {
                    return Err(Error::InvalidInput(
                        "dispatch-indirect pass shader binding must use the compute bind point"
                            .into(),
                    ));
                }
                let indirect_buf = resources.buffer(desc.indirect_buffer)?;
                unsafe {
                    device.cmd_dispatch_indirect(command_buffer, indirect_buf, desc.offset);
                }
            }
            PassWork::DrawIndirect(desc) => {
                let binding = bound_binding.as_ref().ok_or_else(|| {
                    Error::InvalidInput(
                        "draw-indirect pass requires a graphics shader binding".into(),
                    )
                })?;
                if binding.bind_point != vk::PipelineBindPoint::GRAPHICS {
                    return Err(Error::InvalidInput(
                        "draw-indirect pass shader binding must use the graphics bind point".into(),
                    ));
                }
                let shader_object_draw_ext =
                    prepare_shader_object_draw(command_buffer, binding, shader_object_ext)?;
                let indirect_buf = resources.buffer(desc.indirect_buffer)?;
                let vertex_buffer = desc
                    .vertex_buffer
                    .map(|b| {
                        resources
                            .buffer(b.buffer)
                            .map(|vb| (b.binding, vb, b.offset))
                    })
                    .transpose()?;
                let index_buffer = desc
                    .index_buffer
                    .map(|b| {
                        resources
                            .buffer(b.buffer)
                            .map(|ib| (ib, b.offset, vk_index_type(b.format)))
                    })
                    .transpose()?;
                self.record_draw_pass(
                    device,
                    command_buffer,
                    pass,
                    binding.render_pass,
                    resources,
                    pipelines,
                    None, // DrawIndirect has no per-tile viewport yet
                    dynamic_rendering,
                    shader_object_draw_ext,
                    fragment_shading_rate,
                    || unsafe {
                        if let Some((binding, vb, offset)) = vertex_buffer {
                            device.cmd_bind_vertex_buffers(
                                command_buffer,
                                binding,
                                &[vb],
                                &[offset],
                            );
                        }
                        if let Some((ib, offset, index_type)) = index_buffer {
                            device.cmd_bind_index_buffer(command_buffer, ib, offset, index_type);
                            device.cmd_draw_indexed_indirect(
                                command_buffer,
                                indirect_buf,
                                desc.offset,
                                desc.draw_count,
                                desc.stride,
                            );
                        } else {
                            device.cmd_draw_indirect(
                                command_buffer,
                                indirect_buf,
                                desc.offset,
                                desc.draw_count,
                                desc.stride,
                            );
                        }
                    },
                )?;
            }
            PassWork::DrawIndirectCount(desc) => {
                let binding = bound_binding.as_ref().ok_or_else(|| {
                    Error::InvalidInput(
                        "draw-indirect-count pass requires a graphics shader binding".into(),
                    )
                })?;
                if binding.bind_point != vk::PipelineBindPoint::GRAPHICS {
                    return Err(Error::InvalidInput(
                        "draw-indirect-count pass shader binding must use the graphics bind point"
                            .into(),
                    ));
                }
                let shader_object_draw_ext =
                    prepare_shader_object_draw(command_buffer, binding, shader_object_ext)?;
                let indirect_buf = resources.buffer(desc.indirect_buffer)?;
                let count_buf = resources.buffer(desc.count_buffer)?;
                let vertex_buffer = desc
                    .vertex_buffer
                    .map(|b| {
                        resources
                            .buffer(b.buffer)
                            .map(|vb| (b.binding, vb, b.offset))
                    })
                    .transpose()?;
                let index_buffer = desc
                    .index_buffer
                    .map(|b| {
                        resources
                            .buffer(b.buffer)
                            .map(|ib| (ib, b.offset, vk_index_type(b.format)))
                    })
                    .transpose()?;
                self.record_draw_pass(
                    device,
                    command_buffer,
                    pass,
                    binding.render_pass,
                    resources,
                    pipelines,
                    None,
                    dynamic_rendering,
                    shader_object_draw_ext,
                    fragment_shading_rate,
                    || unsafe {
                        if let Some((binding, vb, offset)) = vertex_buffer {
                            device.cmd_bind_vertex_buffers(
                                command_buffer,
                                binding,
                                &[vb],
                                &[offset],
                            );
                        }
                        if let Some((ib, offset, index_type)) = index_buffer {
                            device.cmd_bind_index_buffer(command_buffer, ib, offset, index_type);
                            device.cmd_draw_indexed_indirect_count(
                                command_buffer,
                                indirect_buf,
                                desc.indirect_offset,
                                count_buf,
                                desc.count_offset,
                                desc.max_draw_count,
                                desc.stride,
                            );
                        } else {
                            device.cmd_draw_indirect_count(
                                command_buffer,
                                indirect_buf,
                                desc.indirect_offset,
                                count_buf,
                                desc.count_offset,
                                desc.max_draw_count,
                                desc.stride,
                            );
                        }
                    },
                )?;
            }
            PassWork::DrawMeshShader(desc) => {
                let binding = bound_binding.as_ref().ok_or_else(|| {
                    Error::InvalidInput(
                        "mesh shader draw pass requires a graphics shader binding".into(),
                    )
                })?;
                if binding.bind_point != vk::PipelineBindPoint::GRAPHICS {
                    return Err(Error::InvalidInput(
                        "mesh shader draw pass shader binding must use the graphics bind point"
                            .into(),
                    ));
                }
                let shader_object_draw_ext =
                    prepare_shader_object_mesh_draw(command_buffer, binding, shader_object_ext)?;
                let mesh_shader_ext = mesh_shader_ext.ok_or_else(|| {
                    Error::Unsupported(
                        "mesh shader draw pass requires VK_EXT_mesh_shader to be enabled".into(),
                    )
                })?;
                self.record_draw_pass(
                    device,
                    command_buffer,
                    pass,
                    binding.render_pass,
                    resources,
                    pipelines,
                    None,
                    dynamic_rendering,
                    shader_object_draw_ext,
                    fragment_shading_rate,
                    || unsafe {
                        mesh_shader_ext.cmd_draw_mesh_tasks(
                            command_buffer,
                            desc.group_count_x,
                            desc.group_count_y,
                            desc.group_count_z,
                        );
                    },
                )?;
            }
            PassWork::DrawMeshShaderIndirect(desc) => {
                let binding = bound_binding.as_ref().ok_or_else(|| {
                    Error::InvalidInput(
                        "indirect mesh shader draw pass requires a graphics shader binding".into(),
                    )
                })?;
                if binding.bind_point != vk::PipelineBindPoint::GRAPHICS {
                    return Err(Error::InvalidInput(
                        "indirect mesh shader draw pass shader binding must use the graphics bind point"
                            .into(),
                    ));
                }
                if desc.stride < std::mem::size_of::<vk::DrawMeshTasksIndirectCommandEXT>() as u32
                    || desc.stride % 4 != 0
                {
                    return Err(Error::InvalidInput(
                        "indirect mesh shader draw stride must be a multiple of 4 and at least the size of VkDrawMeshTasksIndirectCommandEXT"
                            .into(),
                    ));
                }
                let mesh_shader_ext = mesh_shader_ext.ok_or_else(|| {
                    Error::Unsupported(
                        "indirect mesh shader draw pass requires VK_EXT_mesh_shader to be enabled"
                            .into(),
                    )
                })?;
                let shader_object_draw_ext =
                    prepare_shader_object_mesh_draw(command_buffer, binding, shader_object_ext)?;
                let indirect_buf = resources.buffer(desc.indirect_buffer)?;
                self.record_draw_pass(
                    device,
                    command_buffer,
                    pass,
                    binding.render_pass,
                    resources,
                    pipelines,
                    None,
                    dynamic_rendering,
                    shader_object_draw_ext,
                    fragment_shading_rate,
                    || unsafe {
                        mesh_shader_ext.cmd_draw_mesh_tasks_indirect(
                            command_buffer,
                            indirect_buf,
                            desc.offset,
                            desc.draw_count,
                            desc.stride,
                        );
                    },
                )?;
            }
            PassWork::GenerateMipmaps {
                image: img_handle,
                mip_count,
            } => unsafe {
                let vk_image = resources.image(img_handle)?;
                let img_desc = resources.image_desc(img_handle)?;
                let aspect = image_aspect_mask(img_desc.format);
                let mips = mip_count.min(img_desc.mip_levels as u32);

                if let Some(s2) = sync2 {
                    // Sync2 path: use vk::ImageMemoryBarrier2 for each transition.
                    // (Named variables for barriers so the slice borrows are valid.)

                    // Transition mip 0: SHADER_READ → TRANSFER_SRC
                    let src_barrier2 = [vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
                        .src_access_mask(vk::AccessFlags2::SHADER_READ)
                        .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
                        .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .image(vk_image)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: aspect,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        })];
                    let dep_info =
                        vk::DependencyInfo::default().image_memory_barriers(&src_barrier2);
                    s2.cmd_pipeline_barrier2(command_buffer, &dep_info);

                    for mip in 1..mips {
                        let src_w = (img_desc.extent.width >> (mip - 1)).max(1) as i32;
                        let src_h = (img_desc.extent.height >> (mip - 1)).max(1) as i32;
                        let dst_w = (img_desc.extent.width >> mip).max(1) as i32;
                        let dst_h = (img_desc.extent.height >> mip).max(1) as i32;

                        // Transition dest mip: UNDEFINED → TRANSFER_DST
                        let dst_barrier2 = [vk::ImageMemoryBarrier2::default()
                            .src_stage_mask(vk::PipelineStageFlags2::TOP_OF_PIPE)
                            .src_access_mask(vk::AccessFlags2::empty())
                            .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                            .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                            .old_layout(vk::ImageLayout::UNDEFINED)
                            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                            .image(vk_image)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: aspect,
                                base_mip_level: mip,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            })];
                        let dep_info =
                            vk::DependencyInfo::default().image_memory_barriers(&dst_barrier2);
                        s2.cmd_pipeline_barrier2(command_buffer, &dep_info);

                        let blit = vk::ImageBlit::default()
                            .src_subresource(vk::ImageSubresourceLayers {
                                aspect_mask: aspect,
                                mip_level: mip - 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            })
                            .src_offsets([
                                vk::Offset3D { x: 0, y: 0, z: 0 },
                                vk::Offset3D {
                                    x: src_w,
                                    y: src_h,
                                    z: 1,
                                },
                            ])
                            .dst_subresource(vk::ImageSubresourceLayers {
                                aspect_mask: aspect,
                                mip_level: mip,
                                base_array_layer: 0,
                                layer_count: 1,
                            })
                            .dst_offsets([
                                vk::Offset3D { x: 0, y: 0, z: 0 },
                                vk::Offset3D {
                                    x: dst_w,
                                    y: dst_h,
                                    z: 1,
                                },
                            ]);
                        device.cmd_blit_image(
                            command_buffer,
                            vk_image,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            vk_image,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            &[blit],
                            vk::Filter::LINEAR,
                        );

                        // Transition this mip: TRANSFER_DST → TRANSFER_SRC
                        let to_src2 = [vk::ImageMemoryBarrier2::default()
                            .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                            .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                            .dst_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                            .dst_access_mask(vk::AccessFlags2::TRANSFER_READ)
                            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                            .image(vk_image)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: aspect,
                                base_mip_level: mip,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            })];
                        let dep_info =
                            vk::DependencyInfo::default().image_memory_barriers(&to_src2);
                        s2.cmd_pipeline_barrier2(command_buffer, &dep_info);
                    }

                    // Transition all mips: TRANSFER_SRC → SHADER_READ_ONLY
                    let final_barrier2 = [vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(vk::PipelineStageFlags2::ALL_TRANSFER)
                        .src_access_mask(vk::AccessFlags2::TRANSFER_READ)
                        .dst_stage_mask(
                            vk::PipelineStageFlags2::FRAGMENT_SHADER
                                | vk::PipelineStageFlags2::COMPUTE_SHADER,
                        )
                        .dst_access_mask(vk::AccessFlags2::SHADER_READ)
                        .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .image(vk_image)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: aspect,
                            base_mip_level: 0,
                            level_count: mips,
                            base_array_layer: 0,
                            layer_count: 1,
                        })];
                    let dep_info =
                        vk::DependencyInfo::default().image_memory_barriers(&final_barrier2);
                    s2.cmd_pipeline_barrier2(command_buffer, &dep_info);
                } else {
                    // Legacy path.

                    // Transition mip 0 from SHADER_READ_ONLY (or whatever) to TRANSFER_SRC.
                    let src_barrier = vk::ImageMemoryBarrier::default()
                        .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .src_access_mask(vk::AccessFlags::SHADER_READ)
                        .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                        .image(vk_image)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: aspect,
                            base_mip_level: 0,
                            level_count: 1,
                            base_array_layer: 0,
                            layer_count: 1,
                        });
                    device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::FRAGMENT_SHADER,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[src_barrier],
                    );

                    for mip in 1..mips {
                        let src_w = (img_desc.extent.width >> (mip - 1)).max(1) as i32;
                        let src_h = (img_desc.extent.height >> (mip - 1)).max(1) as i32;
                        let dst_w = (img_desc.extent.width >> mip).max(1) as i32;
                        let dst_h = (img_desc.extent.height >> mip).max(1) as i32;

                        // Transition destination mip to TRANSFER_DST.
                        let dst_barrier = vk::ImageMemoryBarrier::default()
                            .old_layout(vk::ImageLayout::UNDEFINED)
                            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                            .src_access_mask(vk::AccessFlags::empty())
                            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                            .image(vk_image)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: aspect,
                                base_mip_level: mip,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            });
                        device.cmd_pipeline_barrier(
                            command_buffer,
                            vk::PipelineStageFlags::TRANSFER,
                            vk::PipelineStageFlags::TRANSFER,
                            vk::DependencyFlags::empty(),
                            &[],
                            &[],
                            &[dst_barrier],
                        );

                        // Blit mip-1 → mip.
                        let blit = vk::ImageBlit::default()
                            .src_subresource(vk::ImageSubresourceLayers {
                                aspect_mask: aspect,
                                mip_level: mip - 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            })
                            .src_offsets([
                                vk::Offset3D { x: 0, y: 0, z: 0 },
                                vk::Offset3D {
                                    x: src_w,
                                    y: src_h,
                                    z: 1,
                                },
                            ])
                            .dst_subresource(vk::ImageSubresourceLayers {
                                aspect_mask: aspect,
                                mip_level: mip,
                                base_array_layer: 0,
                                layer_count: 1,
                            })
                            .dst_offsets([
                                vk::Offset3D { x: 0, y: 0, z: 0 },
                                vk::Offset3D {
                                    x: dst_w,
                                    y: dst_h,
                                    z: 1,
                                },
                            ]);
                        device.cmd_blit_image(
                            command_buffer,
                            vk_image,
                            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                            vk_image,
                            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                            &[blit],
                            vk::Filter::LINEAR,
                        );

                        // Transition this mip to TRANSFER_SRC so the next iteration can read it.
                        let to_src = vk::ImageMemoryBarrier::default()
                            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
                            .image(vk_image)
                            .subresource_range(vk::ImageSubresourceRange {
                                aspect_mask: aspect,
                                base_mip_level: mip,
                                level_count: 1,
                                base_array_layer: 0,
                                layer_count: 1,
                            });
                        device.cmd_pipeline_barrier(
                            command_buffer,
                            vk::PipelineStageFlags::TRANSFER,
                            vk::PipelineStageFlags::TRANSFER,
                            vk::DependencyFlags::empty(),
                            &[],
                            &[],
                            &[to_src],
                        );
                    }

                    // Transition all mips from TRANSFER_SRC to SHADER_READ_ONLY.
                    let final_barrier = vk::ImageMemoryBarrier::default()
                        .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                        .dst_access_mask(vk::AccessFlags::SHADER_READ)
                        .image(vk_image)
                        .subresource_range(vk::ImageSubresourceRange {
                            aspect_mask: aspect,
                            base_mip_level: 0,
                            level_count: mips,
                            base_array_layer: 0,
                            layer_count: 1,
                        });
                    device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::FRAGMENT_SHADER
                            | vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[final_barrier],
                    );
                }
            },

            PassWork::BlitMip {
                image,
                src_mip,
                dst_mip,
                src_width,
                src_height,
                dst_width,
                dst_height,
                ..
            } => unsafe {
                let vk_image = resources.image(image)?;
                let img_desc = resources.image_desc(image)?;
                let aspect = image_aspect_mask(img_desc.format);
                let blit = vk::ImageBlit::default()
                    .src_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: aspect,
                        mip_level: src_mip,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .src_offsets([
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D {
                            x: src_width as i32,
                            y: src_height as i32,
                            z: 1,
                        },
                    ])
                    .dst_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: aspect,
                        mip_level: dst_mip,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .dst_offsets([
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D {
                            x: dst_width as i32,
                            y: dst_height as i32,
                            z: 1,
                        },
                    ]);
                device.cmd_blit_image(
                    command_buffer,
                    vk_image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk_image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[blit],
                    vk::Filter::LINEAR,
                );
            },

            PassWork::ResolveImage(resolve) => unsafe {
                let src_desc = resources.image_desc(resolve.src)?;
                device.cmd_resolve_image(
                    command_buffer,
                    resources.image(resolve.src)?,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    resources.image(resolve.dst)?,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::ImageResolve::default()
                        .src_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: image_aspect_mask(src_desc.format),
                            mip_level: resolve.src_mip_level,
                            base_array_layer: resolve.src_base_layer,
                            layer_count: resolve.layer_count,
                        })
                        .src_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                        .dst_subresource(vk::ImageSubresourceLayers {
                            aspect_mask: image_aspect_mask(src_desc.format),
                            mip_level: resolve.dst_mip_level,
                            base_array_layer: resolve.dst_base_layer,
                            layer_count: resolve.layer_count,
                        })
                        .dst_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                        .extent(vk::Extent3D {
                            width: resolve.width,
                            height: resolve.height,
                            depth: 1,
                        })],
                );
            },

            PassWork::BuildBlas(ref build) => {
                let as_ext = as_khr.ok_or_else(|| {
                    Error::Unsupported("BLAS build requires VK_KHR_acceleration_structure".into())
                })?;
                let dst_as = resources.acceleration_structure(build.dst)?;
                let src_as = build
                    .src
                    .map(|h| resources.acceleration_structure(h))
                    .transpose()?;

                if build.mode == AccelerationStructureBuildMode::Compact {
                    let src_as = src_as.ok_or_else(|| {
                        Error::InvalidInput(
                            "BLAS compaction requires a source acceleration structure".into(),
                        )
                    })?;
                    let info = vk::CopyAccelerationStructureInfoKHR::default()
                        .src(src_as)
                        .dst(dst_as)
                        .mode(vk::CopyAccelerationStructureModeKHR::COMPACT);
                    unsafe {
                        as_ext.cmd_copy_acceleration_structure(command_buffer, &info);
                    }
                } else {
                    let geometries: Vec<vk::AccelerationStructureGeometryKHR> = build
                        .geometries
                        .iter()
                        .map(|g| {
                            let vertex_addr = resources.buffer_device_address_at(
                                device,
                                g.vertex_buffer,
                                g.vertex_offset,
                            )?;
                            let index_data = if let Some(idx_buf) = g.index_buffer {
                                vk::DeviceOrHostAddressConstKHR {
                                    device_address: resources.buffer_device_address_at(
                                        device,
                                        idx_buf,
                                        g.index_offset,
                                    )?,
                                }
                            } else {
                                vk::DeviceOrHostAddressConstKHR { device_address: 0 }
                            };
                            let transform_data = if let Some(tf_buf) = g.transform_buffer {
                                vk::DeviceOrHostAddressConstKHR {
                                    device_address: resources.buffer_device_address_at(
                                        device,
                                        tf_buf,
                                        g.transform_offset,
                                    )?,
                                }
                            } else {
                                vk::DeviceOrHostAddressConstKHR { device_address: 0 }
                            };
                            let triangles =
                                vk::AccelerationStructureGeometryTrianglesDataKHR::default()
                                    .vertex_format(vk_vertex_format_for_as(g.vertex_format)?)
                                    .vertex_data(vk::DeviceOrHostAddressConstKHR {
                                        device_address: vertex_addr,
                                    })
                                    .vertex_stride(g.vertex_stride as u64)
                                    .max_vertex(g.vertex_count.saturating_sub(1))
                                    .index_type(
                                        g.index_format
                                            .map(vk_index_type)
                                            .unwrap_or(vk::IndexType::NONE_KHR),
                                    )
                                    .index_data(index_data)
                                    .transform_data(transform_data);
                            Ok(vk::AccelerationStructureGeometryKHR::default()
                                .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
                                .geometry(vk::AccelerationStructureGeometryDataKHR { triangles }))
                        })
                        .collect::<Result<Vec<_>>>()?;

                    let build_range_infos: Vec<vk::AccelerationStructureBuildRangeInfoKHR> = build
                        .geometries
                        .iter()
                        .map(|g| {
                            let primitive_count = if g.index_buffer.is_some() {
                                g.index_count / 3
                            } else {
                                g.vertex_count / 3
                            };
                            vk::AccelerationStructureBuildRangeInfoKHR {
                                primitive_count,
                                primitive_offset: 0,
                                first_vertex: 0,
                                transform_offset: 0,
                            }
                        })
                        .collect();

                    let mut build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
                        .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
                        .flags(vk_blas_build_flags(ray_tracing_position_fetch))
                        .mode(match build.mode {
                            AccelerationStructureBuildMode::Build => {
                                vk::BuildAccelerationStructureModeKHR::BUILD
                            }
                            AccelerationStructureBuildMode::Update => {
                                vk::BuildAccelerationStructureModeKHR::UPDATE
                            }
                            AccelerationStructureBuildMode::Compact => unreachable!(
                                "compact BLAS builds are handled by cmd_copy_acceleration_structure"
                            ),
                        })
                        .src_acceleration_structure(
                            src_as.unwrap_or(vk::AccelerationStructureKHR::null()),
                        )
                        .dst_acceleration_structure(dst_as)
                        .geometries(&geometries);

                    let scratch_addr = if let Some(scratch) = build.scratch_buffer {
                        resources.buffer_device_address_raw(device, scratch)?
                    } else {
                        let scratch_size =
                            build_scratch_size(as_ext, &build_info, &build_range_infos)?;
                        let scratch = resources.create_scratch_buffer(device, scratch_size)?;
                        let address =
                            ResourceRegistry::scratch_buffer_device_address(device, &scratch)?;
                        self.acceleration_structure_scratch.push(scratch);
                        address
                    };
                    build_info = build_info.scratch_data(vk::DeviceOrHostAddressKHR {
                        device_address: scratch_addr,
                    });

                    let range_slices: Vec<&[vk::AccelerationStructureBuildRangeInfoKHR]> =
                        vec![build_range_infos.as_slice()];
                    unsafe {
                        as_ext.cmd_build_acceleration_structures(
                            command_buffer,
                            &[build_info],
                            &range_slices,
                        );
                    }
                }
            }

            PassWork::BuildTlas(ref build) => {
                let as_ext = as_khr.ok_or_else(|| {
                    Error::Unsupported("TLAS build requires VK_KHR_acceleration_structure".into())
                })?;
                let dst_as = resources.acceleration_structure(build.dst)?;
                let src_as = build
                    .src
                    .map(|h| resources.acceleration_structure(h))
                    .transpose()?;
                if build.mode == AccelerationStructureBuildMode::Compact {
                    let src_as = src_as.ok_or_else(|| {
                        Error::InvalidInput(
                            "TLAS compaction requires a source acceleration structure".into(),
                        )
                    })?;
                    let info = vk::CopyAccelerationStructureInfoKHR::default()
                        .src(src_as)
                        .dst(dst_as)
                        .mode(vk::CopyAccelerationStructureModeKHR::COMPACT);
                    unsafe {
                        as_ext.cmd_copy_acceleration_structure(command_buffer, &info);
                    }
                } else {
                    let instance_addr = resources.buffer_device_address_at(
                        device,
                        build.instance_buffer,
                        build.instance_offset,
                    )?;

                    let instances = vk::AccelerationStructureGeometryInstancesDataKHR::default()
                        .data(vk::DeviceOrHostAddressConstKHR {
                            device_address: instance_addr,
                        });
                    let geometry = vk::AccelerationStructureGeometryKHR::default()
                        .geometry_type(vk::GeometryTypeKHR::INSTANCES)
                        .geometry(vk::AccelerationStructureGeometryDataKHR { instances });

                    let mut build_info = vk::AccelerationStructureBuildGeometryInfoKHR::default()
                        .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
                        .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
                        .mode(match build.mode {
                            AccelerationStructureBuildMode::Build => {
                                vk::BuildAccelerationStructureModeKHR::BUILD
                            }
                            AccelerationStructureBuildMode::Update => {
                                vk::BuildAccelerationStructureModeKHR::UPDATE
                            }
                            AccelerationStructureBuildMode::Compact => unreachable!(
                                "compact TLAS builds are handled by cmd_copy_acceleration_structure"
                            ),
                        })
                        .src_acceleration_structure(
                            src_as.unwrap_or(vk::AccelerationStructureKHR::null()),
                        )
                        .dst_acceleration_structure(dst_as)
                        .geometries(std::slice::from_ref(&geometry));

                    let range_info = [vk::AccelerationStructureBuildRangeInfoKHR {
                        primitive_count: build.instance_count,
                        primitive_offset: 0,
                        first_vertex: 0,
                        transform_offset: 0,
                    }];
                    let scratch_addr = if let Some(scratch) = build.scratch_buffer {
                        resources.buffer_device_address_raw(device, scratch)?
                    } else {
                        let scratch_size = build_scratch_size(as_ext, &build_info, &range_info)?;
                        let scratch = resources.create_scratch_buffer(device, scratch_size)?;
                        let address =
                            ResourceRegistry::scratch_buffer_device_address(device, &scratch)?;
                        self.acceleration_structure_scratch.push(scratch);
                        address
                    };
                    build_info = build_info.scratch_data(vk::DeviceOrHostAddressKHR {
                        device_address: scratch_addr,
                    });
                    let range_slices: Vec<&[vk::AccelerationStructureBuildRangeInfoKHR]> =
                        vec![range_info.as_slice()];
                    unsafe {
                        as_ext.cmd_build_acceleration_structures(
                            command_buffer,
                            &[build_info],
                            &range_slices,
                        );
                    }
                }
            }

            PassWork::TraceRays(ref trace) => {
                let rt_ext = rt_khr.ok_or_else(|| {
                    Error::Unsupported("TraceRays requires VK_KHR_ray_tracing_pipeline".into())
                })?;
                let pipeline = pipelines.pipeline(trace.pipeline)?;
                if pipeline.bind_point != vk::PipelineBindPoint::RAY_TRACING_KHR {
                    return Err(Error::InvalidInput(
                        "TraceRays requires a ray-tracing pipeline".into(),
                    ));
                }
                unsafe {
                    device.cmd_bind_pipeline(
                        command_buffer,
                        vk::PipelineBindPoint::RAY_TRACING_KHR,
                        pipeline.pipeline,
                    );
                }
                let region = |sbt: ShaderBindingTableRegion| -> Result<_> {
                    let addr =
                        resources.buffer_device_address_at(device, sbt.buffer, sbt.offset)?;
                    Ok(vk::StridedDeviceAddressRegionKHR {
                        device_address: addr,
                        stride: sbt.stride,
                        size: sbt.size,
                    })
                };
                let raygen = region(trace.sbt.raygen)?;
                let miss = region(trace.sbt.miss)?;
                let hit = region(trace.sbt.hit)?;
                let call = trace
                    .sbt
                    .callable
                    .map(region)
                    .transpose()?
                    .unwrap_or_default();
                // GFX-3c: vkCmdTraceRaysIndirectKHR2 when ray_tracing_maintenance1 is available
                // and an indirect buffer is provided. All SBT regions and dimensions come from GPU.
                if let (Some((indirect_buf, indirect_offset)), Some(rt_m1)) =
                    (trace.indirect, ray_tracing_maintenance1)
                {
                    let indirect_addr = resources.buffer_device_address_at(
                        device,
                        indirect_buf,
                        indirect_offset,
                    )?;
                    unsafe {
                        rt_m1.cmd_trace_rays_indirect2(command_buffer, indirect_addr);
                    }
                } else {
                    unsafe {
                        rt_ext.cmd_trace_rays(
                            command_buffer,
                            &raygen,
                            &miss,
                            &hit,
                            &call,
                            trace.width,
                            trace.height,
                            trace.depth,
                        );
                    }
                }
            }

            PassWork::DecodeVideoFrame(_) | PassWork::EncodeVideoFrame(_) => {
                // GFX-4: recording requires per-frame codec-specific parameters
                // (H.264/H.265 slice offsets, picture info, reference DPB slots) that
                // are not yet exposed in DecodeFrameDesc / EncodeFrameDesc. Use the
                // high-level VideoDecodeSession / VideoEncodeSession API instead.
                return Err(Error::Unsupported(
                    "video pass recording requires codec-specific per-frame parameters; use VideoDecodeSession/VideoEncodeSession high-level API".into(),
                ));
            }

            PassWork::ExecuteGeneratedCommands(ref exec) => {
                let dgc = dgc_nv.ok_or_else(|| {
                    Error::Unsupported(
                        "ExecuteGeneratedCommands requires VK_NV_device_generated_commands".into(),
                    )
                })?;
                let layouts = indirect_command_layouts.ok_or_else(|| {
                    Error::Unsupported("no indirect command layout registry available".into())
                })?;
                let layout = *layouts.get(&exec.layout).ok_or(Error::InvalidHandle)?;
                let pipeline = exec
                    .state_pipeline
                    .map(|h| pipelines.pipeline(h).map(|p| p.pipeline))
                    .transpose()?
                    .unwrap_or(vk::Pipeline::null());
                let cmd_buf = resources.buffer(exec.commands_buffer)?;
                let stream = ash::vk::IndirectCommandsStreamNV::default()
                    .buffer(cmd_buf)
                    .offset(exec.commands_offset);
                let gen_info = ash::vk::GeneratedCommandsInfoNV::default()
                    .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                    .pipeline(pipeline)
                    .indirect_commands_layout(layout)
                    .streams(std::slice::from_ref(&stream))
                    .sequences_count(exec.max_command_count)
                    .preprocess_buffer(vk::Buffer::null())
                    .preprocess_offset(0)
                    .preprocess_size(0);
                unsafe {
                    (dgc.fp().cmd_execute_generated_commands_nv)(
                        command_buffer,
                        vk::FALSE, // not preprocessed
                        &gen_info,
                    );
                }
            }

            PassWork::PreprocessGeneratedCommands(ref prep) => {
                let dgc = dgc_nv.ok_or_else(|| {
                    Error::Unsupported(
                        "PreprocessGeneratedCommands requires VK_NV_device_generated_commands"
                            .into(),
                    )
                })?;
                let layouts = indirect_command_layouts.ok_or_else(|| {
                    Error::Unsupported("no indirect command layout registry available".into())
                })?;
                let layout = *layouts.get(&prep.layout).ok_or(Error::InvalidHandle)?;
                let in_buf = resources.buffer(prep.input_buffer)?;
                let out_buf = resources.buffer(prep.output_buffer)?;
                let stream = ash::vk::IndirectCommandsStreamNV::default()
                    .buffer(in_buf)
                    .offset(prep.input_offset);
                let out_size: u64 = (prep.max_command_count as u64) * 64; // conservative estimate
                let gen_info = ash::vk::GeneratedCommandsInfoNV::default()
                    .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                    .pipeline(vk::Pipeline::null())
                    .indirect_commands_layout(layout)
                    .streams(std::slice::from_ref(&stream))
                    .sequences_count(prep.max_command_count)
                    .preprocess_buffer(out_buf)
                    .preprocess_offset(prep.output_offset)
                    .preprocess_size(out_size);
                unsafe {
                    (dgc.fp().cmd_preprocess_generated_commands_nv)(command_buffer, &gen_info);
                }
            }

            PassWork::EstimateOpticalFlow(desc) => {
                record_optical_flow_estimate(
                    device,
                    command_buffer,
                    desc,
                    resources,
                    optical_flow_nv,
                    optical_flow_sessions,
                )?;
            }
            PassWork::BuildClusterAccelerationStructure(desc) => {
                let ext = cluster_as_nv.ok_or_else(|| {
                    Error::Unsupported(
                        "cluster AS build requires VK_NV_cluster_acceleration_structure",
                    )
                })?;
                let input_info = ash::vk::ClusterAccelerationStructureInputInfoNV::default()
                    .max_acceleration_structure_count(desc.max_as_count)
                    .flags(ash::vk::BuildAccelerationStructureFlagsKHR::from_raw(
                        desc.build_flags,
                    ))
                    .op_type(ash::vk::ClusterAccelerationStructureOpTypeNV::from_raw(
                        desc.op_type as i32,
                    ))
                    .op_mode(ash::vk::ClusterAccelerationStructureOpModeNV::from_raw(
                        desc.op_mode as i32,
                    ));
                let commands_info = ash::vk::ClusterAccelerationStructureCommandsInfoNV {
                    input: input_info,
                    dst_implicit_data: desc.dst_implicit_data_address,
                    scratch_data: desc.scratch_address,
                    dst_addresses_array: ash::vk::StridedDeviceAddressRegionKHR {
                        device_address: desc.dst_addresses[0],
                        size: desc.dst_addresses[1],
                        stride: desc.dst_addresses[2],
                    },
                    dst_sizes_array: ash::vk::StridedDeviceAddressRegionKHR {
                        device_address: desc.dst_sizes[0],
                        size: desc.dst_sizes[1],
                        stride: desc.dst_sizes[2],
                    },
                    src_infos_array: ash::vk::StridedDeviceAddressRegionKHR {
                        device_address: desc.src_infos[0],
                        size: desc.src_infos[1],
                        stride: desc.src_infos[2],
                    },
                    src_infos_count: desc.src_infos_count_address,
                    ..Default::default()
                };
                unsafe {
                    ext.cmd_build_cluster_acceleration_structure_indirect(
                        command_buffer,
                        &commands_info,
                    );
                }
            }
        }
        if let Some(conditional_rendering) =
            conditional_rendering.filter(|_| pass.predicate.is_some())
        {
            unsafe {
                (conditional_rendering.fp().cmd_end_conditional_rendering_ext)(command_buffer);
            }
        }
        Ok(())
    }

    fn record_draw_pass(
        &mut self,
        device: &Device,
        command_buffer: vk::CommandBuffer,
        pass: &PassDesc,
        render_pass: vk::RenderPass,
        resources: &mut ResourceRegistry,
        _pipelines: &mut PipelineRegistry,
        viewport_override: Option<[u32; 4]>,
        dynamic_rendering: Option<&ash::khr::dynamic_rendering::Device>,
        shader_object_ext: Option<&ash::ext::shader_object::Device>,
        fragment_shading_rate: Option<&ash::khr::fragment_shading_rate::Device>,
        record_draw: impl FnOnce(),
    ) -> Result<()> {
        let color_uses = pass
            .writes
            .iter()
            .filter(|usage| usage.state == RgState::RenderTarget)
            .collect::<Vec<_>>();

        // Depth-only passes (e.g. shadow maps) are valid — zero color attachments allowed.
        let depth_use = pass.writes.iter().find(|u| u.state == RgState::DepthWrite);
        if color_uses.is_empty() && depth_use.is_none() {
            return Err(Error::InvalidInput(
                "draw pass has neither a RenderTarget nor a DepthWrite attachment".into(),
            ));
        }

        // Derive framebuffer dimensions from color targets if present, otherwise from depth.
        let (first_extent, framebuffer_layers) = if !color_uses.is_empty() {
            let first_desc = resources.image_desc(color_uses[0].image)?;
            let ext = mip_extent(first_desc.extent, color_uses[0].subresource.base_mip);
            let layers = subresource_layer_count(first_desc.layers, color_uses[0].subresource);
            (ext, layers)
        } else {
            //panic allowed, reason = "empty color target path is entered only after depth_use was proven Some"
            let du = depth_use.unwrap(); // safe: checked above
            let desc = resources.image_desc(du.image)?;
            let ext = mip_extent(desc.extent, du.subresource.base_mip);
            let layers = subresource_layer_count(desc.layers, du.subresource);
            (ext, layers)
        };

        let render_area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: first_extent.width,
                height: first_extent.height,
            },
        };
        let (vp, scissor) = match viewport_override {
            Some([x, y, w, h]) => {
                let vp = vk::Viewport {
                    x: x as f32,
                    y: y as f32,
                    width: w as f32,
                    height: h as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                };
                let sc = vk::Rect2D {
                    offset: vk::Offset2D {
                        x: x as i32,
                        y: y as i32,
                    },
                    extent: vk::Extent2D {
                        width: w,
                        height: h,
                    },
                };
                (vp, sc)
            }
            None => {
                let vp = vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: first_extent.width as f32,
                    height: first_extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                };
                (vp, render_area)
            }
        };

        // Choose between dynamic rendering and legacy render passes.
        if let Some(dr) = dynamic_rendering {
            if render_pass == vk::RenderPass::null() {
                // VK_KHR_dynamic_rendering path.
                let color_attachments = color_uses
                    .iter()
                    .map(|usage| {
                        let image_view = resources.image_view_for_subresource(
                            device,
                            usage.image,
                            usage.subresource,
                        )?;
                        Ok(vk::RenderingAttachmentInfo::default()
                            .image_view(image_view)
                            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                            .load_op(vk::AttachmentLoadOp::CLEAR)
                            .store_op(vk::AttachmentStoreOp::STORE)
                            .clear_value(vk::ClearValue {
                                color: vk::ClearColorValue {
                                    float32: [0.05, 0.07, 0.10, 1.0],
                                },
                            }))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let depth_attachment_info = if let Some(du) = depth_use {
                    let depth_view =
                        resources.image_view_for_subresource(device, du.image, du.subresource)?;
                    Some(
                        vk::RenderingAttachmentInfo::default()
                            .image_view(depth_view)
                            .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                            .load_op(vk::AttachmentLoadOp::CLEAR)
                            .store_op(vk::AttachmentStoreOp::DONT_CARE)
                            .clear_value(vk::ClearValue {
                                depth_stencil: vk::ClearDepthStencilValue {
                                    depth: 1.0,
                                    stencil: 0,
                                },
                            }),
                    )
                } else {
                    None
                };

                let mut rendering_info = vk::RenderingInfo::default()
                    .render_area(render_area)
                    .layer_count(framebuffer_layers)
                    .color_attachments(&color_attachments);
                if let Some(ref dai) = depth_attachment_info {
                    rendering_info = rendering_info.depth_attachment(dai);
                }
                // GFX-2a: attachment-tier VRS image.
                let mut vrs_attachment_info;
                if let (Some(vrs_handle), Some(fsr)) =
                    (pass.shading_rate_image, fragment_shading_rate)
                {
                    if let Ok(vrs_view) = resources.image_view(vrs_handle) {
                        vrs_attachment_info =
                            vk::RenderingFragmentShadingRateAttachmentInfoKHR::default()
                                .image_view(vrs_view)
                                .image_layout(
                                    vk::ImageLayout::FRAGMENT_SHADING_RATE_ATTACHMENT_OPTIMAL_KHR,
                                )
                                .shading_rate_attachment_texel_size(vk::Extent2D {
                                    width: 16,
                                    height: 16,
                                });
                        let _ = fsr; // extension loaded, chain into rendering info
                        rendering_info = rendering_info.push(&mut vrs_attachment_info);
                    }
                }

                unsafe {
                    dr.cmd_begin_rendering(command_buffer, &rendering_info);
                    set_viewport_and_scissor(
                        device,
                        shader_object_ext,
                        command_buffer,
                        vp,
                        scissor,
                    );
                }

                record_draw();

                unsafe {
                    dr.cmd_end_rendering(command_buffer);
                }
                return Ok(());
            }
        }

        // Legacy render pass path.
        let mut attachments = color_uses
            .iter()
            .map(|usage| {
                resources.image_view_for_subresource(device, usage.image, usage.subresource)
            })
            .collect::<Result<Vec<_>>>()?;

        // Depth attachment — appended after colour views to match render-pass order.
        if let Some(du) = depth_use {
            let depth_view =
                resources.image_view_for_subresource(device, du.image, du.subresource)?;
            attachments.push(depth_view);
        }

        // GFX-1b: create a transient framebuffer; it will be destroyed after
        // the frame fence fires (at the start of the next frame's recording).
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(first_extent.width)
            .height(first_extent.height)
            .layers(framebuffer_layers);
        let framebuffer = unsafe {
            device
                .create_framebuffer(&fb_info, None)
                .map_err(|e| Error::Backend(format!("vkCreateFramebuffer failed: {e:?}")))?
        };
        self.transient_framebuffers.push(framebuffer);

        let mut clear_values: Vec<vk::ClearValue> = color_uses
            .iter()
            .map(|_| vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.05, 0.07, 0.10, 1.0],
                },
            })
            .collect();
        if depth_use.is_some() {
            clear_values.push(vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            });
        }
        let begin = vk::RenderPassBeginInfo::default()
            .render_pass(render_pass)
            .framebuffer(framebuffer)
            .render_area(render_area)
            .clear_values(&clear_values);
        unsafe {
            device.cmd_begin_render_pass(command_buffer, &begin, vk::SubpassContents::INLINE);
            set_viewport_and_scissor(device, shader_object_ext, command_buffer, vp, scissor);
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
        resources: &mut ResourceRegistry,
        queue_families: QueueFamilyMap,
        sync2: Option<&ash::khr::synchronization2::Device>,
    ) -> Result<()> {
        if image_barriers.is_empty() && buffer_barriers.is_empty() {
            return Ok(());
        }

        if let Some(s2) = sync2 {
            // VK_KHR_synchronization2 path: per-barrier stage masks, no global union needed.
            let vk_image_barriers2 = image_barriers
                .iter()
                .map(|barrier| {
                    let (src_queue_family, dst_queue_family) = queue_family_index(
                        queue_families,
                        barrier.before_queue,
                        barrier.after_queue,
                        barrier.queue,
                    );
                    Ok(vk::ImageMemoryBarrier2::default()
                        .src_stage_mask(stage_mask2(barrier.before))
                        .src_access_mask(access_mask2(barrier.before))
                        .dst_stage_mask(stage_mask2(barrier.after))
                        .dst_access_mask(access_mask2(barrier.after))
                        .old_layout(image_layout(barrier.before))
                        .new_layout(image_layout(barrier.after))
                        .src_queue_family_index(src_queue_family)
                        .dst_queue_family_index(dst_queue_family)
                        .image(resources.image(barrier.image)?)
                        .subresource_range(subresource_range(barrier.after, barrier.subresource)))
                })
                .collect::<Result<Vec<_>>>()?;
            let vk_buffer_barriers2 = buffer_barriers
                .iter()
                .map(|barrier| {
                    let (src_queue_family, dst_queue_family) = queue_family_index(
                        queue_families,
                        barrier.before_queue,
                        barrier.after_queue,
                        barrier.queue,
                    );
                    Ok(vk::BufferMemoryBarrier2::default()
                        .src_stage_mask(stage_mask2(barrier.before))
                        .src_access_mask(access_mask2(barrier.before))
                        .dst_stage_mask(stage_mask2(barrier.after))
                        .dst_access_mask(access_mask2(barrier.after))
                        .src_queue_family_index(src_queue_family)
                        .dst_queue_family_index(dst_queue_family)
                        .buffer(resources.buffer(barrier.buffer)?)
                        .offset(barrier.offset)
                        .size(if barrier.size == 0 {
                            vk::WHOLE_SIZE
                        } else {
                            barrier.size
                        }))
                })
                .collect::<Result<Vec<_>>>()?;
            let dep_info = vk::DependencyInfo::default()
                .image_memory_barriers(&vk_image_barriers2)
                .buffer_memory_barriers(&vk_buffer_barriers2);
            unsafe {
                s2.cmd_pipeline_barrier2(command_buffer, &dep_info);
            }
        } else {
            // Legacy path: single vkCmdPipelineBarrier with unioned stage masks.
            let vk_image_barriers = image_barriers
                .iter()
                .map(|barrier| {
                    let (src_queue_family, dst_queue_family) = queue_family_index(
                        queue_families,
                        barrier.before_queue,
                        barrier.after_queue,
                        barrier.queue,
                    );
                    Ok(vk::ImageMemoryBarrier::default()
                        .src_access_mask(access_mask(barrier.before))
                        .dst_access_mask(access_mask(barrier.after))
                        .old_layout(image_layout(barrier.before))
                        .new_layout(image_layout(barrier.after))
                        .src_queue_family_index(src_queue_family)
                        .dst_queue_family_index(dst_queue_family)
                        .image(resources.image(barrier.image)?)
                        .subresource_range(subresource_range(barrier.after, barrier.subresource)))
                })
                .collect::<Result<Vec<_>>>()?;
            let vk_buffer_barriers = buffer_barriers
                .iter()
                .map(|barrier| {
                    let (src_queue_family, dst_queue_family) = queue_family_index(
                        queue_families,
                        barrier.before_queue,
                        barrier.after_queue,
                        barrier.queue,
                    );
                    Ok(vk::BufferMemoryBarrier::default()
                        .src_access_mask(access_mask(barrier.before))
                        .dst_access_mask(access_mask(barrier.after))
                        .src_queue_family_index(src_queue_family)
                        .dst_queue_family_index(dst_queue_family)
                        .buffer(resources.buffer(barrier.buffer)?)
                        .offset(barrier.offset)
                        .size(if barrier.size == 0 {
                            vk::WHOLE_SIZE
                        } else {
                            barrier.size
                        }))
                })
                .collect::<Result<Vec<_>>>()?;

            // Compute the tightest src/dst stage masks across all barriers in this batch.
            // Taking the union means every barrier in the call is covered while avoiding
            // the ALL_COMMANDS over-synchronisation that would otherwise stall the whole GPU.
            let src_stages = image_barriers
                .iter()
                .map(|b| stage_mask(b.before))
                .chain(buffer_barriers.iter().map(|b| stage_mask(b.before)))
                .fold(vk::PipelineStageFlags::empty(), |acc, s| acc | s);
            let dst_stages = image_barriers
                .iter()
                .map(|b| stage_mask(b.after))
                .chain(buffer_barriers.iter().map(|b| stage_mask(b.after)))
                .fold(vk::PipelineStageFlags::empty(), |acc, s| acc | s);

            // Fall back to TOP_OF_PIPE / BOTTOM_OF_PIPE if somehow empty (defensive only —
            // the early return above ensures at least one barrier exists at this point).
            let src_stages = if src_stages.is_empty() {
                vk::PipelineStageFlags::TOP_OF_PIPE
            } else {
                src_stages
            };
            let dst_stages = if dst_stages.is_empty() {
                vk::PipelineStageFlags::BOTTOM_OF_PIPE
            } else {
                dst_stages
            };

            unsafe {
                device.cmd_pipeline_barrier(
                    command_buffer,
                    src_stages,
                    dst_stages,
                    vk::DependencyFlags::empty(),
                    &[],
                    &vk_buffer_barriers,
                    &vk_image_barriers,
                );
            }
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
        // Present uses semaphore-based GPU sync; the access mask is empty because
        // the presentation engine's visibility is guaranteed by the semaphore chain.
        RgState::Present => vk::AccessFlags::empty(),
        RgState::UniformRead => vk::AccessFlags::UNIFORM_READ,
        RgState::VertexRead => vk::AccessFlags::VERTEX_ATTRIBUTE_READ,
        RgState::IndexRead => vk::AccessFlags::INDEX_READ,
        RgState::IndirectRead => vk::AccessFlags::INDIRECT_COMMAND_READ,
        RgState::AccelerationStructureBuild => vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_KHR,
        RgState::AccelerationStructureRead => vk::AccessFlags::ACCELERATION_STRUCTURE_READ_KHR,
        RgState::ShadingRateAttachment => {
            vk::AccessFlags::FRAGMENT_SHADING_RATE_ATTACHMENT_READ_KHR
        }
    }
}

/// Map a resource state to the tightest pipeline stage(s) that produce or consume it.
///
/// Used to build precise `srcStageMask` / `dstStageMask` for pipeline barriers
/// instead of blanket `ALL_COMMANDS`, which unnecessarily serialises the whole GPU.
fn stage_mask(state: RgState) -> vk::PipelineStageFlags {
    match state {
        // No real predecessor — barrier is just an initialisation transition.
        RgState::Undefined => vk::PipelineStageFlags::TOP_OF_PIPE,
        // Sampled images and storage reads can occur in any shader stage.
        RgState::ShaderRead => {
            vk::PipelineStageFlags::VERTEX_SHADER
                | vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COMPUTE_SHADER
        }
        // Storage writes happen exclusively in compute shaders in this engine.
        RgState::ShaderWrite => vk::PipelineStageFlags::COMPUTE_SHADER,
        RgState::RenderTarget => vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
        RgState::DepthRead | RgState::DepthWrite => {
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
        }
        RgState::CopySrc | RgState::CopyDst => vk::PipelineStageFlags::TRANSFER,
        // Presentation: semaphore handles the actual memory visibility.
        // BOTTOM_OF_PIPE ensures the transition is recorded before present.
        RgState::Present => vk::PipelineStageFlags::BOTTOM_OF_PIPE,
        // Uniform buffers bound in any shader stage.
        RgState::UniformRead => {
            vk::PipelineStageFlags::VERTEX_SHADER
                | vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COMPUTE_SHADER
        }
        RgState::VertexRead | RgState::IndexRead => vk::PipelineStageFlags::VERTEX_INPUT,
        RgState::IndirectRead => vk::PipelineStageFlags::DRAW_INDIRECT,
        RgState::AccelerationStructureBuild => {
            vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR
        }
        RgState::AccelerationStructureRead => {
            vk::PipelineStageFlags::RAY_TRACING_SHADER_KHR
                | vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR
        }
        RgState::ShadingRateAttachment => {
            vk::PipelineStageFlags::FRAGMENT_SHADING_RATE_ATTACHMENT_KHR
        }
    }
}

/// Map a resource state to `vk::PipelineStageFlags2` for VK_KHR_synchronization2 barriers.
fn stage_mask2(state: RgState) -> vk::PipelineStageFlags2 {
    match state {
        RgState::Undefined => vk::PipelineStageFlags2::TOP_OF_PIPE,
        RgState::ShaderRead | RgState::UniformRead => {
            vk::PipelineStageFlags2::VERTEX_SHADER
                | vk::PipelineStageFlags2::FRAGMENT_SHADER
                | vk::PipelineStageFlags2::COMPUTE_SHADER
        }
        RgState::ShaderWrite => vk::PipelineStageFlags2::COMPUTE_SHADER,
        RgState::RenderTarget => vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
        RgState::DepthRead | RgState::DepthWrite => {
            vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS
        }
        RgState::CopySrc | RgState::CopyDst => vk::PipelineStageFlags2::ALL_TRANSFER,
        RgState::Present => vk::PipelineStageFlags2::BOTTOM_OF_PIPE,
        RgState::VertexRead => vk::PipelineStageFlags2::VERTEX_ATTRIBUTE_INPUT,
        RgState::IndexRead => vk::PipelineStageFlags2::INDEX_INPUT,
        RgState::IndirectRead => vk::PipelineStageFlags2::DRAW_INDIRECT,
        RgState::AccelerationStructureBuild => {
            vk::PipelineStageFlags2::ACCELERATION_STRUCTURE_BUILD_KHR
        }
        RgState::AccelerationStructureRead => {
            vk::PipelineStageFlags2::RAY_TRACING_SHADER_KHR
                | vk::PipelineStageFlags2::ACCELERATION_STRUCTURE_BUILD_KHR
        }
        RgState::ShadingRateAttachment => {
            vk::PipelineStageFlags2::FRAGMENT_SHADING_RATE_ATTACHMENT_KHR
        }
    }
}

/// Map a resource state to `vk::AccessFlags2` for VK_KHR_synchronization2 barriers.
fn access_mask2(state: RgState) -> vk::AccessFlags2 {
    match state {
        RgState::Undefined => vk::AccessFlags2::empty(),
        RgState::ShaderRead => vk::AccessFlags2::SHADER_READ,
        RgState::ShaderWrite => vk::AccessFlags2::SHADER_WRITE,
        RgState::RenderTarget => vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
        RgState::DepthRead => vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ,
        RgState::DepthWrite => vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
        RgState::CopySrc => vk::AccessFlags2::TRANSFER_READ,
        RgState::CopyDst => vk::AccessFlags2::TRANSFER_WRITE,
        RgState::Present => vk::AccessFlags2::empty(),
        RgState::UniformRead => vk::AccessFlags2::UNIFORM_READ,
        RgState::VertexRead => vk::AccessFlags2::VERTEX_ATTRIBUTE_READ,
        RgState::IndexRead => vk::AccessFlags2::INDEX_READ,
        RgState::IndirectRead => vk::AccessFlags2::INDIRECT_COMMAND_READ,
        RgState::AccelerationStructureBuild => vk::AccessFlags2::ACCELERATION_STRUCTURE_WRITE_KHR,
        RgState::AccelerationStructureRead => vk::AccessFlags2::ACCELERATION_STRUCTURE_READ_KHR,
        RgState::ShadingRateAttachment => {
            vk::AccessFlags2::FRAGMENT_SHADING_RATE_ATTACHMENT_READ_KHR
        }
    }
}

fn vk_index_type(format: IndexFormat) -> vk::IndexType {
    match format {
        IndexFormat::Uint16 => vk::IndexType::UINT16,
        IndexFormat::Uint32 => vk::IndexType::UINT32,
    }
}

fn vk_vertex_format_for_as(format: VertexFormat) -> Result<vk::Format> {
    match format {
        VertexFormat::Float32x2 => Ok(vk::Format::R32G32_SFLOAT),
        VertexFormat::Float32x3 => Ok(vk::Format::R32G32B32_SFLOAT),
        VertexFormat::Float32x4 => Ok(vk::Format::R32G32B32A32_SFLOAT),
    }
}

fn vk_blas_build_flags(ray_tracing_position_fetch: bool) -> vk::BuildAccelerationStructureFlagsKHR {
    let mut flags = vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE;
    if ray_tracing_position_fetch {
        flags |= vk::BuildAccelerationStructureFlagsKHR::ALLOW_DATA_ACCESS;
    }
    flags
}

fn build_scratch_size(
    as_ext: &ash::khr::acceleration_structure::Device,
    build_info: &vk::AccelerationStructureBuildGeometryInfoKHR<'_>,
    ranges: &[vk::AccelerationStructureBuildRangeInfoKHR],
) -> Result<u64> {
    let primitive_counts = ranges
        .iter()
        .map(|range| range.primitive_count)
        .collect::<Vec<_>>();
    let mut sizes = vk::AccelerationStructureBuildSizesInfoKHR::default();
    unsafe {
        as_ext.get_acceleration_structure_build_sizes(
            vk::AccelerationStructureBuildTypeKHR::DEVICE,
            build_info,
            &primitive_counts,
            &mut sizes,
        );
    }
    let size = match build_info.mode {
        vk::BuildAccelerationStructureModeKHR::UPDATE => sizes.update_scratch_size,
        _ => sizes.build_scratch_size,
    };
    if size == 0 {
        return Err(Error::Backend(
            "Vulkan returned zero AS scratch size".into(),
        ));
    }
    Ok(size)
}

fn bind_pass_shader_binding(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    pass: &PassDesc,
    resources: &ResourceRegistry,
    descriptors: &DescriptorRegistry,
    pipelines: &PipelineRegistry,
    bindless: Option<BindlessVkInfo>,
    push_descriptor: Option<&ash::khr::push_descriptor::Device>,
    shader_object_ext: Option<&ash::ext::shader_object::Device>,
) -> Result<Option<ActiveShaderBinding>> {
    let binding = match &pass.shader_binding {
        Some(ShaderBinding::Pipeline(handle)) => Some(bind_pipeline_shader_binding(
            device,
            command_buffer,
            pipelines,
            *handle,
        )?),
        Some(ShaderBinding::ShaderObjects(handles)) => Some(bind_shader_object_binding(
            device,
            command_buffer,
            pass,
            resources,
            descriptors,
            pipelines,
            shader_object_ext,
            handles,
        )?),
        None => pass
            .pipeline
            .map(|handle| bind_pipeline_shader_binding(device, command_buffer, pipelines, handle))
            .transpose()?,
    };

    if let Some(binding) = binding.as_ref() {
        record_bound_resources(
            device,
            command_buffer,
            pass,
            resources,
            descriptors,
            bindless,
            push_descriptor,
            binding,
        )?;
    } else if pass.push_constants.is_some() {
        return Err(Error::InvalidInput(
            "push constants require a bound shader binding".into(),
        ));
    } else if pass.push_descriptor_set.is_some() {
        return Err(Error::InvalidInput(
            "push descriptors require a bound shader binding".into(),
        ));
    }

    Ok(binding)
}

fn bind_pipeline_shader_binding(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    pipelines: &PipelineRegistry,
    handle: PipelineHandle,
) -> Result<ActiveShaderBinding> {
    let pipeline = pipelines.pipeline(handle)?;
    unsafe {
        device.cmd_bind_pipeline(command_buffer, pipeline.bind_point, pipeline.pipeline);
    }
    Ok(active_binding_from_pipeline(pipeline))
}

fn active_binding_from_pipeline(pipeline: VulkanPipeline) -> ActiveShaderBinding {
    ActiveShaderBinding {
        bind_point: pipeline.bind_point,
        layout: pipeline.layout,
        render_pass: pipeline.render_pass,
        push_constants_bytes: pipeline.push_constants_bytes,
        push_constant_stages: pipeline.push_constant_stages,
        uses_bindless: pipeline.uses_bindless,
        uses_shader_objects: false,
        shader_object_stages: vk::ShaderStageFlags::empty(),
        graphics_state: None,
    }
}

fn bind_shader_object_binding(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    pass: &PassDesc,
    resources: &ResourceRegistry,
    descriptors: &DescriptorRegistry,
    pipelines: &PipelineRegistry,
    shader_object_ext: Option<&ash::ext::shader_object::Device>,
    handles: &[crate::shader_object::ShaderObjectHandle],
) -> Result<ActiveShaderBinding> {
    let Some(shader_object_ext) = shader_object_ext else {
        let fallback = pass.pipeline.ok_or_else(|| {
            Error::Unsupported(
                "shader object binding requires VK_EXT_shader_object or a fallback pipeline".into(),
            )
        })?;
        return bind_pipeline_shader_binding(device, command_buffer, pipelines, fallback);
    };
    if handles.is_empty() {
        return Err(Error::InvalidInput(
            "shader object binding requires at least one shader object".into(),
        ));
    }

    let mut stages = Vec::with_capacity(handles.len());
    let mut shaders = Vec::with_capacity(handles.len());
    let mut seen_stages = vk::ShaderStageFlags::empty();
    let mut layout_handle: Option<crate::PipelineLayoutHandle> = None;
    let mut bind_point = None;

    for handle in handles {
        let shader_object = resources.shader_object(*handle)?;
        if seen_stages.intersects(shader_object.stage) {
            return Err(Error::InvalidInput(
                "shader object binding cannot bind the same stage more than once".into(),
            ));
        }
        seen_stages |= shader_object.stage;

        let object_bind_point = shader_object_bind_point(shader_object.stage)?;
        if let Some(existing) = bind_point {
            if existing != object_bind_point {
                return Err(Error::InvalidInput(
                    "shader object binding cannot mix compute and graphics stages".into(),
                ));
            }
        } else {
            bind_point = Some(object_bind_point);
        }

        if let Some(shader_layout) = shader_object.layout {
            if let Some(current) = layout_handle {
                if current != shader_layout {
                    return Err(Error::InvalidInput(
                        "all shader objects in a binding must use the same pipeline layout".into(),
                    ));
                }
            } else {
                layout_handle = Some(shader_layout);
            }
        }

        stages.push(shader_object.stage);
        shaders.push(shader_object.shader);
    }

    let bind_point = bind_point.ok_or_else(|| {
        Error::InvalidInput("shader object binding requires at least one stage".into())
    })?;
    validate_shader_object_stage_set(bind_point, seen_stages)?;
    if bind_point == vk::PipelineBindPoint::GRAPHICS
        && !seen_stages.contains(vk::ShaderStageFlags::FRAGMENT)
    {
        stages.push(vk::ShaderStageFlags::FRAGMENT);
        shaders.push(vk::ShaderEXT::null());
    }
    unsafe {
        shader_object_ext.cmd_bind_shaders(command_buffer, &stages, &shaders);
    }

    let mut binding = if let Some(layout_handle) = layout_handle {
        ActiveShaderBinding {
            bind_point,
            layout: descriptors.pipeline_layout(layout_handle)?,
            render_pass: vk::RenderPass::null(),
            push_constants_bytes: descriptors.push_constants_bytes(layout_handle)?,
            push_constant_stages: descriptors.push_constant_stages(layout_handle)?,
            uses_bindless: descriptors.pipeline_uses_bindless(layout_handle)?,
            uses_shader_objects: true,
            shader_object_stages: seen_stages,
            graphics_state: None,
        }
    } else {
        ActiveShaderBinding {
            bind_point,
            layout: vk::PipelineLayout::null(),
            render_pass: vk::RenderPass::null(),
            push_constants_bytes: 0,
            push_constant_stages: vk::ShaderStageFlags::empty(),
            uses_bindless: false,
            uses_shader_objects: true,
            shader_object_stages: seen_stages,
            graphics_state: None,
        }
    };

    if bind_point == vk::PipelineBindPoint::GRAPHICS {
        let anchor = pass.pipeline.ok_or_else(|| {
            Error::InvalidInput(
                "graphics shader-object passes require pass.pipeline as render-state/fallback anchor"
                    .into(),
            )
        })?;
        let anchor_pipeline = pipelines.pipeline(anchor)?;
        if anchor_pipeline.bind_point != vk::PipelineBindPoint::GRAPHICS {
            return Err(Error::InvalidInput(
                "graphics shader-object fallback pipeline must use the graphics bind point".into(),
            ));
        }
        if binding.layout != vk::PipelineLayout::null() && binding.layout != anchor_pipeline.layout
        {
            return Err(Error::InvalidInput(
                "graphics shader-object layout must match the fallback pipeline layout".into(),
            ));
        }
        binding.render_pass = anchor_pipeline.render_pass;
        binding.graphics_state = Some(pipelines.graphics_state(anchor)?);
    }

    Ok(binding)
}

fn shader_object_bind_point(stage: vk::ShaderStageFlags) -> Result<vk::PipelineBindPoint> {
    if stage == vk::ShaderStageFlags::COMPUTE {
        return Ok(vk::PipelineBindPoint::COMPUTE);
    }
    let graphics_stages = vk::ShaderStageFlags::VERTEX
        | vk::ShaderStageFlags::TESSELLATION_CONTROL
        | vk::ShaderStageFlags::TESSELLATION_EVALUATION
        | vk::ShaderStageFlags::GEOMETRY
        | vk::ShaderStageFlags::FRAGMENT
        | vk::ShaderStageFlags::TASK_EXT
        | vk::ShaderStageFlags::MESH_EXT;
    if stage.intersects(graphics_stages) && (stage.as_raw() & !graphics_stages.as_raw()) == 0 {
        Ok(vk::PipelineBindPoint::GRAPHICS)
    } else {
        Err(Error::Unsupported(
            "render-graph shader object binding supports compute and graphics stages only".into(),
        ))
    }
}

fn validate_shader_object_stage_set(
    bind_point: vk::PipelineBindPoint,
    stages: vk::ShaderStageFlags,
) -> Result<()> {
    if bind_point != vk::PipelineBindPoint::GRAPHICS {
        return Ok(());
    }
    if stages.contains(vk::ShaderStageFlags::TASK_EXT)
        && !stages.contains(vk::ShaderStageFlags::MESH_EXT)
    {
        return Err(Error::InvalidInput(
            "task shader objects require a mesh shader object in the same binding".into(),
        ));
    }
    if stages.contains(vk::ShaderStageFlags::VERTEX)
        && stages.contains(vk::ShaderStageFlags::MESH_EXT)
    {
        return Err(Error::InvalidInput(
            "shader object binding cannot mix vertex and mesh shader stages".into(),
        ));
    }
    if !stages.contains(vk::ShaderStageFlags::VERTEX)
        && !stages.contains(vk::ShaderStageFlags::MESH_EXT)
    {
        return Err(Error::InvalidInput(
            "graphics shader object binding requires a vertex or mesh shader object".into(),
        ));
    }
    Ok(())
}

fn record_bound_resources(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    pass: &PassDesc,
    resources: &ResourceRegistry,
    descriptors: &DescriptorRegistry,
    bindless: Option<BindlessVkInfo>,
    push_descriptor: Option<&ash::khr::push_descriptor::Device>,
    binding: &ActiveShaderBinding,
) -> Result<()> {
    let requires_layout = !pass.bind_groups.is_empty()
        || pass.push_descriptor_set.is_some()
        || pass.push_constants.is_some()
        || binding.uses_bindless;
    if requires_layout && binding.layout == vk::PipelineLayout::null() {
        return Err(Error::InvalidInput(
            "descriptor sets, push descriptors, bindless descriptors, and push constants require a shader-object layout"
                .into(),
        ));
    }

    if binding.uses_bindless {
        let bindless = bindless.ok_or_else(|| {
            Error::Backend(
                "shader binding requires bindless descriptors but no bindless heap is available"
                    .into(),
            )
        })?;
        unsafe {
            device.cmd_bind_descriptor_sets(
                command_buffer,
                binding.bind_point,
                binding.layout,
                0,
                &[bindless.set],
                &[],
            );
        }
    }

    // GFX-7a: use descriptor buffer binding when all groups have buffer backing,
    // otherwise fall back to pool-based descriptor set binding.
    descriptors.bind_descriptor_buffer_or_sets(
        device,
        command_buffer,
        binding.bind_point,
        binding.layout,
        &pass.bind_groups,
        binding.uses_bindless,
    )?;

    if let Some(push_descriptor_set) = &pass.push_descriptor_set {
        let push_descriptor = push_descriptor.ok_or_else(|| {
            Error::Unsupported("push descriptors require VK_KHR_push_descriptor".into())
        })?;
        for push_binding in &push_descriptor_set.bindings {
            record_push_descriptor_binding(
                push_descriptor,
                command_buffer,
                binding,
                push_descriptor_set.set,
                push_binding,
                resources,
            )?;
        }
    }
    if let Some(push_constants) = &pass.push_constants {
        record_push_constants(device, command_buffer, binding, push_constants)?;
    }

    Ok(())
}

fn prepare_shader_object_draw<'a>(
    command_buffer: vk::CommandBuffer,
    binding: &ActiveShaderBinding,
    shader_object_ext: Option<&'a ash::ext::shader_object::Device>,
) -> Result<Option<&'a ash::ext::shader_object::Device>> {
    if !binding.uses_shader_objects {
        return Ok(None);
    }
    if binding
        .shader_object_stages
        .intersects(vk::ShaderStageFlags::TASK_EXT | vk::ShaderStageFlags::MESH_EXT)
    {
        return Err(Error::InvalidInput(
            "vertex/index draw passes require vertex shader objects, not mesh shader objects"
                .into(),
        ));
    }
    if !binding
        .shader_object_stages
        .contains(vk::ShaderStageFlags::VERTEX)
    {
        return Err(Error::InvalidInput(
            "vertex/index draw passes require a vertex shader object".into(),
        ));
    }
    let shader_object_ext = shader_object_ext.ok_or_else(|| {
        Error::Unsupported("graphics shader-object draws require VK_EXT_shader_object".into())
    })?;
    let state = binding.graphics_state.as_ref().ok_or_else(|| {
        Error::InvalidInput("graphics shader-object draws require graphics dynamic state".into())
    })?;
    record_shader_object_graphics_state(shader_object_ext, command_buffer, state);
    Ok(Some(shader_object_ext))
}

fn prepare_shader_object_mesh_draw<'a>(
    command_buffer: vk::CommandBuffer,
    binding: &ActiveShaderBinding,
    shader_object_ext: Option<&'a ash::ext::shader_object::Device>,
) -> Result<Option<&'a ash::ext::shader_object::Device>> {
    if !binding.uses_shader_objects {
        return Ok(None);
    }
    if binding
        .shader_object_stages
        .contains(vk::ShaderStageFlags::VERTEX)
    {
        return Err(Error::InvalidInput(
            "mesh draw passes require mesh shader objects, not vertex shader objects".into(),
        ));
    }
    if !binding
        .shader_object_stages
        .contains(vk::ShaderStageFlags::MESH_EXT)
    {
        return Err(Error::InvalidInput(
            "mesh draw passes require a mesh shader object".into(),
        ));
    }
    let shader_object_ext = shader_object_ext.ok_or_else(|| {
        Error::Unsupported("mesh shader-object draws require VK_EXT_shader_object".into())
    })?;
    let state = binding.graphics_state.as_ref().ok_or_else(|| {
        Error::InvalidInput("mesh shader-object draws require graphics dynamic state".into())
    })?;
    record_shader_object_graphics_state(shader_object_ext, command_buffer, state);
    Ok(Some(shader_object_ext))
}

fn record_shader_object_graphics_state(
    shader_object_ext: &ash::ext::shader_object::Device,
    command_buffer: vk::CommandBuffer,
    state: &VulkanGraphicsPipelineState,
) {
    let sample_mask_len = ((state.rasterization_samples.as_raw() + 31) / 32) as usize;
    let sample_mask = vec![u32::MAX; sample_mask_len];
    unsafe {
        shader_object_ext.cmd_set_vertex_input(
            command_buffer,
            &state.vertex_bindings,
            &state.vertex_attributes,
        );
        shader_object_ext.cmd_set_primitive_topology(command_buffer, state.topology);
        shader_object_ext.cmd_set_primitive_restart_enable(command_buffer, false);
        shader_object_ext.cmd_set_cull_mode(command_buffer, state.cull_mode);
        shader_object_ext.cmd_set_front_face(command_buffer, state.front_face);
        shader_object_ext
            .cmd_set_rasterizer_discard_enable(command_buffer, state.rasterizer_discard);
        shader_object_ext.cmd_set_depth_bias_enable(command_buffer, false);
        shader_object_ext.cmd_set_polygon_mode(command_buffer, state.polygon_mode);
        shader_object_ext.cmd_set_depth_clamp_enable(command_buffer, state.depth_clamp);
        shader_object_ext
            .cmd_set_rasterization_samples(command_buffer, state.rasterization_samples);
        shader_object_ext.cmd_set_sample_mask(
            command_buffer,
            state.rasterization_samples,
            &sample_mask,
        );
        shader_object_ext.cmd_set_alpha_to_coverage_enable(command_buffer, false);
        shader_object_ext.cmd_set_alpha_to_one_enable(command_buffer, false);
        shader_object_ext.cmd_set_depth_test_enable(command_buffer, state.depth_test_enable);
        shader_object_ext.cmd_set_depth_write_enable(command_buffer, state.depth_write_enable);
        shader_object_ext.cmd_set_depth_compare_op(command_buffer, state.depth_compare_op);
        shader_object_ext.cmd_set_depth_bounds_test_enable(command_buffer, false);
        shader_object_ext.cmd_set_stencil_test_enable(command_buffer, false);
        shader_object_ext.cmd_set_logic_op_enable(command_buffer, false);
        if let Some(mode) = state.conservative_rasterization_mode {
            shader_object_ext.cmd_set_conservative_rasterization_mode(command_buffer, mode);
            shader_object_ext.cmd_set_extra_primitive_overestimation_size(command_buffer, 0.0);
        }
        if !state.color_blend_enables.is_empty() {
            shader_object_ext.cmd_set_color_blend_enable(
                command_buffer,
                0,
                &state.color_blend_enables,
            );
            shader_object_ext.cmd_set_color_blend_equation(
                command_buffer,
                0,
                &state.color_blend_equations,
            );
            shader_object_ext.cmd_set_color_write_mask(command_buffer, 0, &state.color_write_masks);
        }
    }
}

fn set_viewport_and_scissor(
    device: &Device,
    shader_object_ext: Option<&ash::ext::shader_object::Device>,
    command_buffer: vk::CommandBuffer,
    viewport: vk::Viewport,
    scissor: vk::Rect2D,
) {
    unsafe {
        if let Some(shader_object_ext) = shader_object_ext {
            shader_object_ext.cmd_set_viewport_with_count(command_buffer, &[viewport]);
            shader_object_ext.cmd_set_scissor_with_count(command_buffer, &[scissor]);
        } else {
            device.cmd_set_viewport(command_buffer, 0, &[viewport]);
            device.cmd_set_scissor(command_buffer, 0, &[scissor]);
        }
    }
}

fn record_push_constants(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    binding: &ActiveShaderBinding,
    push_constants: &PushConstants,
) -> Result<()> {
    let end = push_constants
        .offset
        .checked_add(push_constants.bytes.len() as u32)
        .ok_or_else(|| Error::InvalidInput("push constant byte range overflowed".into()))?;
    if end > binding.push_constants_bytes {
        return Err(Error::InvalidInput(format!(
            "push constant byte range [{}, {}) exceeds pipeline layout push constant size {}",
            push_constants.offset, end, binding.push_constants_bytes
        )));
    }
    unsafe {
        device.cmd_push_constants(
            command_buffer,
            binding.layout,
            binding.push_constant_stages,
            push_constants.offset,
            &push_constants.bytes,
        );
    }
    Ok(())
}

fn record_push_descriptor_binding(
    push_descriptor: &ash::khr::push_descriptor::Device,
    command_buffer: vk::CommandBuffer,
    binding_state: &ActiveShaderBinding,
    set: u32,
    binding: &PushDescriptorBinding,
    resources: &ResourceRegistry,
) -> Result<()> {
    match binding {
        PushDescriptorBinding::SampledImage {
            binding,
            image_view,
            layout,
        } => {
            let info = [vk::DescriptorImageInfo::default()
                .image_view(resources.image_view(*image_view)?)
                .image_layout(image_layout(*layout))];
            let write = [vk::WriteDescriptorSet::default()
                .dst_binding(*binding)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(&info)];
            unsafe {
                push_descriptor.cmd_push_descriptor_set(
                    command_buffer,
                    binding_state.bind_point,
                    binding_state.layout,
                    set,
                    &write,
                );
            }
        }
        PushDescriptorBinding::Sampler { binding, sampler } => {
            let info = [vk::DescriptorImageInfo::default().sampler(resources.sampler(*sampler)?)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_binding(*binding)
                .descriptor_type(vk::DescriptorType::SAMPLER)
                .image_info(&info)];
            unsafe {
                push_descriptor.cmd_push_descriptor_set(
                    command_buffer,
                    binding_state.bind_point,
                    binding_state.layout,
                    set,
                    &write,
                );
            }
        }
        PushDescriptorBinding::StorageBuffer {
            binding,
            buffer,
            offset,
            range,
        } => {
            let info = [vk::DescriptorBufferInfo::default()
                .buffer(resources.buffer(*buffer)?)
                .offset(*offset)
                .range(*range)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_binding(*binding)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&info)];
            unsafe {
                push_descriptor.cmd_push_descriptor_set(
                    command_buffer,
                    binding_state.bind_point,
                    binding_state.layout,
                    set,
                    &write,
                );
            }
        }
    }
    Ok(())
}

fn vk_fragment_shading_rate(rate: ShadingRate) -> vk::Extent2D {
    let (width, height) = match rate {
        ShadingRate::Rate1x1 => (1, 1),
        ShadingRate::Rate1x2 => (1, 2),
        ShadingRate::Rate2x1 => (2, 1),
        ShadingRate::Rate2x2 => (2, 2),
        ShadingRate::Rate2x4 => (2, 4),
        ShadingRate::Rate4x2 => (4, 2),
        ShadingRate::Rate4x4 => (4, 4),
    };
    vk::Extent2D { width, height }
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
        // Acceleration structure states don't apply to images, use GENERAL as a safe fallback.
        RgState::AccelerationStructureBuild | RgState::AccelerationStructureRead => {
            vk::ImageLayout::GENERAL
        }
        RgState::ShadingRateAttachment => {
            vk::ImageLayout::FRAGMENT_SHADING_RATE_ATTACHMENT_OPTIMAL_KHR
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

fn subresource_layer_count(image_layers: u16, subresource: SubresourceRange) -> u32 {
    if subresource.layer_count == u16::MAX {
        u32::from(image_layers.saturating_sub(subresource.base_layer))
    } else {
        u32::from(subresource.layer_count)
    }
}

fn mip_extent(extent: Extent3d, base_mip: u16) -> Extent3d {
    let shift = u32::from(base_mip);
    Extent3d {
        width: (extent.width >> shift).max(1),
        height: (extent.height >> shift).max(1),
        depth: (extent.depth >> shift).max(1),
    }
}

fn record_optical_flow_estimate(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    desc: OpticalFlowEstimateDesc,
    resources: &ResourceRegistry,
    optical_flow: Option<&ash::nv::optical_flow::Device>,
    sessions: Option<&HashMap<OpticalFlowSessionHandle, vk::OpticalFlowSessionNV>>,
) -> Result<()> {
    let optical_flow = optical_flow
        .ok_or_else(|| Error::Unsupported("VK_NV_optical_flow is not enabled".into()))?;
    let session = sessions
        .and_then(|sessions| sessions.get(&desc.session).copied())
        .ok_or(Error::InvalidHandle)?;
    bind_optical_flow_image(
        device,
        optical_flow,
        session,
        vk::OpticalFlowSessionBindingPointNV::INPUT,
        resources.image_view(desc.input_current)?,
    )?;
    bind_optical_flow_image(
        device,
        optical_flow,
        session,
        vk::OpticalFlowSessionBindingPointNV::REFERENCE,
        resources.image_view(desc.input_previous)?,
    )?;
    bind_optical_flow_image(
        device,
        optical_flow,
        session,
        vk::OpticalFlowSessionBindingPointNV::FLOW_VECTOR,
        resources.image_view(desc.output_motion_vectors)?,
    )?;
    if let Some(hint) = desc.input_hint {
        bind_optical_flow_image(
            device,
            optical_flow,
            session,
            vk::OpticalFlowSessionBindingPointNV::HINT,
            resources.image_view(hint)?,
        )?;
    }
    let flags = if desc.input_hint.is_some() {
        vk::OpticalFlowExecuteFlagsNV::empty()
    } else {
        vk::OpticalFlowExecuteFlagsNV::DISABLE_TEMPORAL_HINTS
    };
    let execute_info = vk::OpticalFlowExecuteInfoNV::default().flags(flags);
    unsafe {
        (optical_flow.fp().cmd_optical_flow_execute_nv)(command_buffer, session, &execute_info);
    }
    Ok(())
}

fn bind_optical_flow_image(
    device: &Device,
    optical_flow: &ash::nv::optical_flow::Device,
    session: vk::OpticalFlowSessionNV,
    binding_point: vk::OpticalFlowSessionBindingPointNV,
    view: vk::ImageView,
) -> Result<()> {
    unsafe {
        (optical_flow.fp().bind_optical_flow_session_image_nv)(
            device.handle(),
            session,
            binding_point,
            view,
            vk::ImageLayout::GENERAL,
        )
        .result()
        .map_err(|error| {
            Error::Backend(format!("vkBindOpticalFlowSessionImageNV failed: {error:?}"))
        })
    }
}

// ── Multi-frame command context ───────────────────────────────────────────────

/// Number of independent frame slots.  Each slot has its own command pools and
/// fence so the CPU can record frame N+1 while the GPU is still executing frame N.
pub const FRAMES_IN_FLIGHT: usize = 2;

/// Manages `FRAMES_IN_FLIGHT` independent [`CommandContext`]s.
///
/// Consecutive flushes rotate through the slots.  A slot's fence is only waited
/// when that slot is selected again, meaning the CPU is never blocked for the
/// current frame's GPU work — only for work that is `FRAMES_IN_FLIGHT` frames old.
pub struct FramedCommands {
    contexts: Vec<CommandContext>,
    /// Slot that will be used on the next submission (cycles 0..FRAMES_IN_FLIGHT).
    next_slot: usize,
    /// Monotonically-increasing counter.  The `n`-th submission used slot `(n-1) % N`.
    total_submissions: u64,
}

impl FramedCommands {
    pub fn create(
        device: &Device,
        queue_families: QueueFamilyMap,
        timestamp_period_ns: f32,
        buffer_marker_amd: Option<&ash::amd::buffer_marker::Device>,
        memory_properties: vk::PhysicalDeviceMemoryProperties,
        use_timeline_chains: bool,
    ) -> Result<Self> {
        let mut contexts = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for _ in 0..FRAMES_IN_FLIGHT {
            contexts.push(CommandContext::create(
                device,
                queue_families,
                timestamp_period_ns,
                buffer_marker_amd,
                memory_properties,
                use_timeline_chains,
            )?);
        }
        Ok(Self {
            contexts,
            next_slot: 0,
            total_submissions: 0,
        })
    }

    /// Record and submit one frame of GPU work, cycling to the next slot.
    ///
    /// Blocks only when the slot's previous submission (up to `FRAMES_IN_FLIGHT`
    /// frames ago) has not yet completed — never for the immediately preceding frame.
    pub fn submit(
        &mut self,
        device: &Device,
        queues: VulkanQueues,
        queue_families: QueueFamilyMap,
        graph: &CompiledGraph,
        resources: &mut ResourceRegistry,
        descriptors: &DescriptorRegistry,
        pipelines: &mut PipelineRegistry,
        debug: &DebugUtils,
        bindless: Option<BindlessVkInfo>,
        mesh_shader_ext: Option<&mesh_shader::Device>,
        sync2: Option<&ash::khr::synchronization2::Device>,
        dynamic_rendering: Option<&ash::khr::dynamic_rendering::Device>,
        push_descriptor: Option<&ash::khr::push_descriptor::Device>,
        conditional_rendering: Option<&ash::ext::conditional_rendering::Device>,
        fragment_shading_rate: Option<&ash::khr::fragment_shading_rate::Device>,
        as_khr: Option<&ash::khr::acceleration_structure::Device>,
        rt_khr: Option<&ash::khr::ray_tracing_pipeline::Device>,
        shader_object_ext: Option<&ash::ext::shader_object::Device>,
        ray_tracing_position_fetch: bool,
        diagnostic_checkpoints_nv: Option<&ash::nv::device_diagnostic_checkpoints::Device>,
        extended_dynamic_state3: Option<&ash::ext::extended_dynamic_state3::Device>,
        vertex_input_dynamic_state: Option<&ash::ext::vertex_input_dynamic_state::Device>,
        ray_tracing_maintenance1: Option<&ash::khr::ray_tracing_maintenance1::Device>,
        optical_flow_nv: Option<&ash::nv::optical_flow::Device>,
        cluster_as_nv: Option<&ash::nv::cluster_acceleration_structure::Device>,
        optical_flow_sessions: Option<&HashMap<OpticalFlowSessionHandle, vk::OpticalFlowSessionNV>>,
        dgc_nv: Option<&ash::nv::device_generated_commands::Device>,
        indirect_command_layouts: Option<
            &HashMap<crate::IndirectCommandLayoutHandle, vk::IndirectCommandsLayoutNV>,
        >,
        wait_semaphore: Option<vk::Semaphore>,
        signal_semaphore: Option<vk::Semaphore>,
    ) -> Result<SubmissionHandle> {
        let slot = self.next_slot;
        self.total_submissions += 1;
        let _handle = self.contexts[slot].submit_graph(
            device,
            queues,
            queue_families,
            graph,
            resources,
            descriptors,
            pipelines,
            debug,
            bindless,
            mesh_shader_ext,
            sync2,
            dynamic_rendering,
            push_descriptor,
            conditional_rendering,
            fragment_shading_rate,
            as_khr,
            rt_khr,
            shader_object_ext,
            ray_tracing_position_fetch,
            diagnostic_checkpoints_nv,
            extended_dynamic_state3,
            vertex_input_dynamic_state,
            ray_tracing_maintenance1,
            optical_flow_nv,
            cluster_as_nv,
            optical_flow_sessions,
            dgc_nv,
            indirect_command_layouts,
            wait_semaphore,
            signal_semaphore,
        )?;
        self.next_slot = (slot + 1) % self.contexts.len();
        // Override the per-context submission handle with the global counter so
        // callers can correlate handles across slots.
        Ok(SubmissionHandle(self.total_submissions))
    }

    /// Wait for a specific submission.  Uses `handle % N` to identify the slot,
    /// then waits on that slot's fence if it holds that submission.
    /// Falls back to waiting all submitted slots, which is always safe.
    pub fn wait_for_submission(&self, device: &Device, handle: SubmissionHandle) -> Result<()> {
        // Identify the slot: submission N used slot (N-1) % FRAMES_IN_FLIGHT.
        let n = self.contexts.len();
        let slot = if handle.0 > 0 {
            ((handle.0 - 1) as usize) % n
        } else {
            return Ok(());
        };
        let ctx = &self.contexts[slot];
        if ctx.frame_submitted {
            unsafe {
                device
                    .wait_for_fences(&[ctx.frame_fence], true, u64::MAX)
                    .map_err(|e| {
                        if e == ash::vk::Result::ERROR_DEVICE_LOST {
                            Error::DeviceLost(
                                "vkWaitForFences returned VK_ERROR_DEVICE_LOST".into(),
                            )
                        } else {
                            Error::Backend(format!("vkWaitForFences failed: {e:?}"))
                        }
                    })?;
            }
        }
        Ok(())
    }

    /// Wait on every slot that has submitted but not yet been waited on.
    /// Used before surface recreation or shutdown to drain the GPU safely
    /// without resorting to `vkDeviceWaitIdle`.
    /// Return mutable access to all contexts — used during setup (e.g. installing buffer pools).
    pub fn contexts_mut(&mut self) -> &mut [CommandContext] {
        &mut self.contexts
    }

    /// Return mutable access to the context that will be used on the NEXT submission.
    pub fn current_context_mut(&mut self) -> &mut CommandContext {
        &mut self.contexts[self.next_slot]
    }

    pub fn wait_all(&self, device: &Device) -> Result<()> {
        let fences: Vec<vk::Fence> = self
            .contexts
            .iter()
            .filter(|c| c.frame_submitted)
            .map(|c| c.frame_fence)
            .collect();
        if !fences.is_empty() {
            unsafe {
                device
                    .wait_for_fences(&fences, true, u64::MAX)
                    .map_err(|e| {
                        if e == ash::vk::Result::ERROR_DEVICE_LOST {
                            Error::DeviceLost(
                                "vkWaitForFences returned VK_ERROR_DEVICE_LOST".into(),
                            )
                        } else {
                            Error::Backend(format!("vkWaitForFences failed: {e:?}"))
                        }
                    })?;
            }
        }
        Ok(())
    }

    /// Per-pass GPU timings from the most recently completed frame.
    ///
    /// Each entry is `(pass_name, gpu_ms)`. Empty until the second frame.
    pub fn pass_timings(&self) -> &[(String, f32)] {
        // Return timings from the context that last completed (previous slot).
        let n = self.contexts.len();
        let prev_slot = self.next_slot.wrapping_sub(1).min(n - 1);
        &self.contexts[prev_slot].pass_timings
    }

    pub fn destroy(&self, device: &Device) {
        for ctx in &self.contexts {
            ctx.destroy(device);
        }
    }
}
