use std::collections::HashMap;
use std::ffi::CString;

use ash::{vk, Device};

use crate::{
    ComputePipelineDesc, CullMode, Error, FrontFace, GraphicsPipelineDesc, PipelineHandle,
    PrimitiveTopology, Result, VertexFormat, VertexInputRate,
};

use super::descriptors::DescriptorRegistry;
use super::resources::vk_format;
use super::shaders::{shader_stage_flags, ShaderRegistry};

#[derive(Hash, PartialEq, Eq)]
struct FramebufferKey {
    render_pass: vk::RenderPass,
    attachments: Vec<vk::ImageView>,
    width: u32,
    height: u32,
    layers: u32,
}

#[derive(Default)]
struct FramebufferCache {
    entries: HashMap<FramebufferKey, vk::Framebuffer>,
}

impl FramebufferCache {
    fn get_or_create(
        &mut self,
        device: &Device,
        render_pass: vk::RenderPass,
        attachments: &[vk::ImageView],
        width: u32,
        height: u32,
        layers: u32,
    ) -> Result<vk::Framebuffer> {
        let key = FramebufferKey {
            render_pass,
            attachments: attachments.to_vec(),
            width,
            height,
            layers,
        };
        if let Some(&fb) = self.entries.get(&key) {
            return Ok(fb);
        }
        let info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(attachments)
            .width(width)
            .height(height)
            .layers(layers);
        let fb = unsafe {
            device
                .create_framebuffer(&info, None)
                .map_err(|error| Error::Backend(format!("vkCreateFramebuffer failed: {error:?}")))?
        };
        self.entries.insert(key, fb);
        Ok(fb)
    }

    fn invalidate_render_pass(&mut self, device: &Device, render_pass: vk::RenderPass) {
        self.entries.retain(|key, fb| {
            if key.render_pass == render_pass {
                unsafe { device.destroy_framebuffer(*fb, None) };
                false
            } else {
                true
            }
        });
    }

    fn invalidate_image_view(&mut self, device: &Device, view: vk::ImageView) {
        self.entries.retain(|key, fb| {
            if key.attachments.contains(&view) {
                unsafe { device.destroy_framebuffer(*fb, None) };
                false
            } else {
                true
            }
        });
    }

    fn clear_all(&mut self, device: &Device) {
        for (_, fb) in self.entries.drain() {
            unsafe { device.destroy_framebuffer(fb, None) };
        }
    }
}

pub struct PipelineRegistry {
    pipeline_cache: vk::PipelineCache,
    pipelines: HashMap<PipelineHandle, VulkanPipeline>,
    framebuffer_cache: FramebufferCache,
}

impl PipelineRegistry {
    pub fn create(device: &Device, initial_data: Option<&[u8]>) -> Result<Self> {
        let mut info = vk::PipelineCacheCreateInfo::default();
        if let Some(data) = initial_data {
            info = info.initial_data(data);
        }
        let pipeline_cache = unsafe {
            device
                .create_pipeline_cache(&info, None)
                .map_err(|e| Error::Backend(format!("vkCreatePipelineCache failed: {e:?}")))?
        };
        Ok(Self {
            pipeline_cache,
            pipelines: HashMap::new(),
            framebuffer_cache: FramebufferCache::default(),
        })
    }

    pub fn serialize_cache(&self, device: &Device) -> Result<Vec<u8>> {
        unsafe {
            device
                .get_pipeline_cache_data(self.pipeline_cache)
                .map_err(|e| Error::Backend(format!("vkGetPipelineCacheData failed: {e:?}")))
        }
    }
}

#[derive(Copy, Clone)]
pub struct VulkanPipeline {
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
    pub bind_point: vk::PipelineBindPoint,
    pub render_pass: vk::RenderPass,
    pub push_constants_bytes: u32,
    pub push_constant_stages: vk::ShaderStageFlags,
}

impl PipelineRegistry {
    pub fn create_compute_pipeline(
        &mut self,
        device: &Device,
        handle: PipelineHandle,
        desc: ComputePipelineDesc,
        shaders: &ShaderRegistry,
        descriptors: &DescriptorRegistry,
    ) -> Result<()> {
        let module = shaders.module(desc.shader)?;
        let stage = shaders.stage(desc.shader)?;
        let layout_handle = desc
            .layout
            .expect("compute pipeline layout must be resolved before backend call");
        let layout = descriptors.pipeline_layout(layout_handle)?;
        let push_constants_bytes = descriptors.push_constants_bytes(layout_handle)?;
        let push_constant_stages = descriptors.push_constant_stages(layout_handle)?;
        let entry = CString::new(shaders.entry_point(desc.shader)?).map_err(|_| {
            Error::InvalidInput("shader entry point cannot contain interior nul bytes".into())
        })?;
        let stage_info = vk::PipelineShaderStageCreateInfo::default()
            .stage(shader_stage_flags(stage))
            .module(module)
            .name(&entry);
        let info = vk::ComputePipelineCreateInfo::default()
            .stage(stage_info)
            .layout(layout);
        let pipeline = unsafe {
            device
                .create_compute_pipelines(self.pipeline_cache, &[info], None)
                .map_err(|(_, error)| {
                    Error::Backend(format!("vkCreateComputePipelines failed: {error:?}"))
                })?
        }
        .remove(0);

        self.pipelines.insert(
            handle,
            VulkanPipeline {
                pipeline,
                layout,
                bind_point: vk::PipelineBindPoint::COMPUTE,
                render_pass: vk::RenderPass::null(),
                push_constants_bytes,
                push_constant_stages,
            },
        );
        Ok(())
    }

    pub fn create_graphics_pipeline(
        &mut self,
        device: &Device,
        handle: PipelineHandle,
        desc: &GraphicsPipelineDesc,
        shaders: &ShaderRegistry,
        descriptors: &DescriptorRegistry,
    ) -> Result<()> {
        let layout_handle = desc
            .layout
            .expect("graphics pipeline layout must be resolved before backend call");
        let layout = descriptors.pipeline_layout(layout_handle)?;
        let push_constants_bytes = descriptors.push_constants_bytes(layout_handle)?;
        let push_constant_stages = descriptors.push_constant_stages(layout_handle)?;
        let render_pass = create_render_pass(device, desc)?;
        let result = self.create_graphics_pipeline_inner(
            device,
            handle,
            desc,
            shaders,
            layout,
            render_pass,
            push_constants_bytes,
            push_constant_stages,
        );
        if result.is_err() {
            unsafe {
                device.destroy_render_pass(render_pass, None);
            }
        }
        result
    }

    fn create_graphics_pipeline_inner(
        &mut self,
        device: &Device,
        handle: PipelineHandle,
        desc: &GraphicsPipelineDesc,
        shaders: &ShaderRegistry,
        layout: vk::PipelineLayout,
        render_pass: vk::RenderPass,
        push_constants_bytes: u32,
        push_constant_stages: vk::ShaderStageFlags,
    ) -> Result<()> {
        let vertex_entry =
            CString::new(shaders.entry_point(desc.vertex_shader)?).map_err(|_| {
                Error::InvalidInput(
                    "vertex shader entry point cannot contain interior nul bytes".into(),
                )
            })?;
        let vertex_stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(shader_stage_flags(shaders.stage(desc.vertex_shader)?))
            .module(shaders.module(desc.vertex_shader)?)
            .name(&vertex_entry);

        let fragment_entry = if let Some(shader) = desc.fragment_shader {
            Some(CString::new(shaders.entry_point(shader)?).map_err(|_| {
                Error::InvalidInput(
                    "fragment shader entry point cannot contain interior nul bytes".into(),
                )
            })?)
        } else {
            None
        };
        let fragment_stage = if let Some(shader) = desc.fragment_shader {
            Some(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(shader_stage_flags(shaders.stage(shader)?))
                    .module(shaders.module(shader)?)
                    .name(fragment_entry.as_ref().expect("fragment entry exists")),
            )
        } else {
            None
        };
        let mut stages = vec![vertex_stage];
        if let Some(fragment_stage) = fragment_stage {
            stages.push(fragment_stage);
        }
        let _ = &fragment_entry;

        let vertex_bindings = desc
            .vertex_buffers
            .iter()
            .map(|binding| {
                vk::VertexInputBindingDescription::default()
                    .binding(binding.binding)
                    .stride(binding.stride)
                    .input_rate(vk_vertex_input_rate(binding.input_rate))
            })
            .collect::<Vec<_>>();
        let vertex_attributes = desc
            .vertex_attributes
            .iter()
            .map(|attribute| {
                Ok(vk::VertexInputAttributeDescription::default()
                    .location(attribute.location)
                    .binding(attribute.binding)
                    .format(vk_vertex_format(attribute.format)?)
                    .offset(attribute.offset))
            })
            .collect::<Result<Vec<_>>>()?;
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&vertex_bindings)
            .vertex_attribute_descriptions(&vertex_attributes);
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk_topology(desc.topology))
            .primitive_restart_enable(false);
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk_cull_mode(desc.raster.cull_mode))
            .front_face(vk_front_face(desc.raster.front_face))
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let color_blend_attachments = desc
            .color_targets
            .iter()
            .map(|_| {
                vk::PipelineColorBlendAttachmentState::default()
                    .color_write_mask(vk::ColorComponentFlags::RGBA)
                    .blend_enable(false)
            })
            .collect::<Vec<_>>();
        let color_blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_blend_attachments);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .color_blend_state(&color_blend)
            .dynamic_state(&dynamic_state)
            .layout(layout)
            .render_pass(render_pass)
            .subpass(0);

        let pipeline = unsafe {
            device
                .create_graphics_pipelines(self.pipeline_cache, &[info], None)
                .map_err(|(_, error)| {
                    Error::Backend(format!("vkCreateGraphicsPipelines failed: {error:?}"))
                })?
        }
        .remove(0);

        self.pipelines.insert(
            handle,
            VulkanPipeline {
                pipeline,
                layout,
                bind_point: vk::PipelineBindPoint::GRAPHICS,
                render_pass,
                push_constants_bytes,
                push_constant_stages,
            },
        );
        Ok(())
    }

    pub fn get_or_create_framebuffer(
        &mut self,
        device: &Device,
        render_pass: vk::RenderPass,
        attachments: &[vk::ImageView],
        width: u32,
        height: u32,
        layers: u32,
    ) -> Result<vk::Framebuffer> {
        self.framebuffer_cache.get_or_create(
            device,
            render_pass,
            attachments,
            width,
            height,
            layers,
        )
    }

    pub fn invalidate_framebuffers_for_view(&mut self, device: &Device, view: vk::ImageView) {
        self.framebuffer_cache.invalidate_image_view(device, view);
    }

    pub fn clear_all_framebuffers(&mut self, device: &Device) {
        self.framebuffer_cache.clear_all(device);
    }

    pub fn destroy_pipeline(&mut self, device: &Device, handle: PipelineHandle) -> Result<()> {
        let pipeline = self.pipelines.remove(&handle).ok_or(Error::InvalidHandle)?;
        if pipeline.render_pass != vk::RenderPass::null() {
            self.framebuffer_cache
                .invalidate_render_pass(device, pipeline.render_pass);
        }
        unsafe {
            device.destroy_pipeline(pipeline.pipeline, None);
            if pipeline.render_pass != vk::RenderPass::null() {
                device.destroy_render_pass(pipeline.render_pass, None);
            }
        }
        Ok(())
    }

    pub fn destroy_all(&mut self, device: &Device) {
        self.framebuffer_cache.clear_all(device);
        for (_, pipeline) in self.pipelines.drain() {
            unsafe {
                device.destroy_pipeline(pipeline.pipeline, None);
                if pipeline.render_pass != vk::RenderPass::null() {
                    device.destroy_render_pass(pipeline.render_pass, None);
                }
            }
        }
        unsafe { device.destroy_pipeline_cache(self.pipeline_cache, None) };
    }

    pub fn pipeline(&self, handle: PipelineHandle) -> Result<VulkanPipeline> {
        self.pipelines
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }
}

fn create_render_pass(device: &Device, desc: &GraphicsPipelineDesc) -> Result<vk::RenderPass> {
    let color_attachments = desc
        .color_targets
        .iter()
        .map(|target| {
            Ok(vk::AttachmentDescription::default()
                .format(vk_format(target.format)?)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL))
        })
        .collect::<Result<Vec<_>>>()?;
    let color_refs = (0..desc.color_targets.len())
        .map(|index| {
            vk::AttachmentReference::default()
                .attachment(index as u32)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        })
        .collect::<Vec<_>>();
    let subpass = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_refs)];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&color_attachments)
        .subpasses(&subpass);
    unsafe {
        device
            .create_render_pass(&info, None)
            .map_err(|error| Error::Backend(format!("vkCreateRenderPass failed: {error:?}")))
    }
}

fn vk_topology(topology: PrimitiveTopology) -> vk::PrimitiveTopology {
    match topology {
        PrimitiveTopology::TriangleList => vk::PrimitiveTopology::TRIANGLE_LIST,
        PrimitiveTopology::TriangleStrip => vk::PrimitiveTopology::TRIANGLE_STRIP,
        PrimitiveTopology::LineList => vk::PrimitiveTopology::LINE_LIST,
        PrimitiveTopology::LineStrip => vk::PrimitiveTopology::LINE_STRIP,
        PrimitiveTopology::PointList => vk::PrimitiveTopology::POINT_LIST,
    }
}

fn vk_vertex_input_rate(input_rate: VertexInputRate) -> vk::VertexInputRate {
    match input_rate {
        VertexInputRate::Vertex => vk::VertexInputRate::VERTEX,
        VertexInputRate::Instance => vk::VertexInputRate::INSTANCE,
    }
}

fn vk_vertex_format(format: VertexFormat) -> Result<vk::Format> {
    match format {
        VertexFormat::Float32x2 => Ok(vk::Format::R32G32_SFLOAT),
        VertexFormat::Float32x3 => Ok(vk::Format::R32G32B32_SFLOAT),
        VertexFormat::Float32x4 => Ok(vk::Format::R32G32B32A32_SFLOAT),
    }
}

fn vk_cull_mode(cull_mode: CullMode) -> vk::CullModeFlags {
    match cull_mode {
        CullMode::None => vk::CullModeFlags::NONE,
        CullMode::Front => vk::CullModeFlags::FRONT,
        CullMode::Back => vk::CullModeFlags::BACK,
    }
}

fn vk_front_face(front_face: FrontFace) -> vk::FrontFace {
    match front_face {
        FrontFace::CounterClockwise => vk::FrontFace::COUNTER_CLOCKWISE,
        FrontFace::Clockwise => vk::FrontFace::CLOCKWISE,
    }
}
