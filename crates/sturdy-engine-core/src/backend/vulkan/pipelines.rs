use std::collections::HashMap;
use std::ffi::CString;

use ash::{Device, vk};

use crate::{
    ComputePipelineDesc, ConservativeRasterMode, CullMode, Error, FrontFace, GraphicsPipelineDesc,
    PipelineHandle, PolygonMode, PrimitiveTopology, RayTracingPipelineDesc, Result,
    RtShaderGroupKind, VertexFormat, VertexInputRate,
};

use super::descriptors::DescriptorRegistry;
use super::resources::vk_format;
use super::shaders::{ShaderRegistry, shader_stage_flags};

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

/// Save the pipeline cache after this many new pipelines have been created since
/// the last save.  Keeps startup fast (no stale cache flush) while ensuring the
/// cache is written out during a long session even if the process is killed.
const PIPELINE_CACHE_CHECKPOINT_THRESHOLD: u32 = 8;

pub struct PipelineRegistry {
    pipeline_cache: vk::PipelineCache,
    pipelines: HashMap<PipelineHandle, VulkanPipeline>,
    graphics_states: HashMap<PipelineHandle, VulkanGraphicsPipelineState>,
    framebuffer_cache: FramebufferCache,
    /// Pipelines created since the last incremental cache save.
    pipelines_since_checkpoint: u32,
    /// When true, graphics pipelines are created without a VkRenderPass,
    /// using VkPipelineRenderingCreateInfoKHR instead.
    pub dynamic_rendering_enabled: bool,
    /// When true, graphics pipelines expose dynamic fragment shading rate state.
    pub vrs_pipeline_enabled: bool,
    /// When true, overestimate conservative rasterization can be requested.
    pub conservative_rasterization_overestimate_enabled: bool,
    /// When true, underestimate conservative rasterization can be requested.
    pub conservative_rasterization_underestimate_enabled: bool,
    /// When true, VK_EXT_extended_dynamic_state3 dynamic states are added to pipelines.
    pub extended_dynamic_state3_enabled: bool,
    /// When true, VK_EXT_vertex_input_dynamic_state dynamic state is added and
    /// vertex input bindings/attributes are omitted from pipeline creation.
    pub vertex_input_dynamic_state_enabled: bool,
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
            graphics_states: HashMap::new(),
            framebuffer_cache: FramebufferCache::default(),
            pipelines_since_checkpoint: 0,
            dynamic_rendering_enabled: false,
            vrs_pipeline_enabled: false,
            conservative_rasterization_overestimate_enabled: false,
            conservative_rasterization_underestimate_enabled: false,
            extended_dynamic_state3_enabled: false,
            vertex_input_dynamic_state_enabled: false,
        })
    }

    pub fn serialize_cache(&self, device: &Device) -> Result<Vec<u8>> {
        unsafe {
            device
                .get_pipeline_cache_data(self.pipeline_cache)
                .map_err(|e| Error::Backend(format!("vkGetPipelineCacheData failed: {e:?}")))
        }
    }

    /// If enough new pipelines have been created since the last save, serialize
    /// the cache and return the bytes.  The caller should write them to disk.
    /// Returns `None` if the threshold has not yet been reached.
    pub fn maybe_checkpoint(&mut self, device: &Device) -> Option<Vec<u8>> {
        if self.pipelines_since_checkpoint >= PIPELINE_CACHE_CHECKPOINT_THRESHOLD {
            self.pipelines_since_checkpoint = 0;
            self.serialize_cache(device).ok()
        } else {
            None
        }
    }

    fn note_pipeline_created(&mut self) {
        self.pipelines_since_checkpoint += 1;
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
    pub uses_bindless: bool,
}

#[derive(Clone)]
pub struct VulkanGraphicsPipelineState {
    pub topology: vk::PrimitiveTopology,
    pub cull_mode: vk::CullModeFlags,
    pub front_face: vk::FrontFace,
    pub rasterization_samples: vk::SampleCountFlags,
    pub depth_test_enable: bool,
    pub depth_write_enable: bool,
    pub depth_compare_op: vk::CompareOp,
    pub conservative_rasterization_mode: Option<vk::ConservativeRasterizationModeEXT>,
    pub vertex_bindings: Vec<vk::VertexInputBindingDescription2EXT<'static>>,
    pub vertex_attributes: Vec<vk::VertexInputAttributeDescription2EXT<'static>>,
    pub color_blend_enables: Vec<vk::Bool32>,
    pub color_blend_equations: Vec<vk::ColorBlendEquationEXT>,
    pub color_write_masks: Vec<vk::ColorComponentFlags>,
    /// Polygon fill mode, recorded dynamically when extended_dynamic_state3 is available.
    pub polygon_mode: vk::PolygonMode,
    /// Depth clamp, recorded dynamically when extended_dynamic_state3 is available.
    pub depth_clamp: bool,
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
        //panic allowed, reason = "layout is resolved by the caller before the backend; absence is a caller defect"
        let layout_handle = desc
            .layout
            .expect("compute pipeline layout must be resolved before backend call");
        let layout = descriptors.pipeline_layout(layout_handle)?;
        let uses_bindless = descriptors.pipeline_uses_bindless(layout_handle)?;
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
                uses_bindless,
            },
        );
        self.note_pipeline_created();
        Ok(())
    }

    pub fn create_ray_tracing_pipeline(
        &mut self,
        _device: &Device,
        handle: PipelineHandle,
        desc: &RayTracingPipelineDesc,
        shaders: &ShaderRegistry,
        descriptors: &DescriptorRegistry,
        rt_ext: &ash::khr::ray_tracing_pipeline::Device,
    ) -> Result<()> {
        //panic allowed, reason = "layout is resolved by the caller before the backend; absence is a caller defect"
        let layout_handle = desc
            .layout
            .expect("RT pipeline layout must be resolved before backend call");
        let layout = descriptors.pipeline_layout(layout_handle)?;
        let uses_bindless = descriptors.pipeline_uses_bindless(layout_handle)?;
        let push_constants_bytes = descriptors.push_constants_bytes(layout_handle)?;
        let push_constant_stages = descriptors.push_constant_stages(layout_handle)?;

        // Build stage CStrings first so they outlive the stage infos.
        let entry_cstrings = desc
            .stages
            .iter()
            .map(|s| {
                CString::new(shaders.entry_point(s.shader)?).map_err(|_| {
                    Error::InvalidInput(
                        "shader entry point cannot contain interior nul bytes".into(),
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let stage_infos = desc
            .stages
            .iter()
            .zip(entry_cstrings.iter())
            .map(|(s, entry)| {
                let stage = shaders.stage(s.shader)?;
                Ok(vk::PipelineShaderStageCreateInfo::default()
                    .stage(shader_stage_flags(stage))
                    .module(shaders.module(s.shader)?)
                    .name(entry))
            })
            .collect::<Result<Vec<_>>>()?;

        let group_infos = desc
            .groups
            .iter()
            .map(|g| {
                vk::RayTracingShaderGroupCreateInfoKHR::default()
                    .ty(match g.kind {
                        RtShaderGroupKind::General => vk::RayTracingShaderGroupTypeKHR::GENERAL,
                        RtShaderGroupKind::TrianglesHit => {
                            vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP
                        }
                        RtShaderGroupKind::ProceduralHit => {
                            vk::RayTracingShaderGroupTypeKHR::PROCEDURAL_HIT_GROUP
                        }
                    })
                    .general_shader(g.general_shader)
                    .closest_hit_shader(g.closest_hit_shader)
                    .any_hit_shader(g.any_hit_shader)
                    .intersection_shader(g.intersection_shader)
            })
            .collect::<Vec<_>>();

        let create_info = vk::RayTracingPipelineCreateInfoKHR::default()
            .stages(&stage_infos)
            .groups(&group_infos)
            .max_pipeline_ray_recursion_depth(desc.max_recursion_depth)
            .layout(layout);

        let pipeline = unsafe {
            rt_ext
                .create_ray_tracing_pipelines(
                    vk::DeferredOperationKHR::null(),
                    self.pipeline_cache,
                    &[create_info],
                    None,
                )
                .map_err(|(_, e)| {
                    Error::Backend(format!("vkCreateRayTracingPipelinesKHR failed: {e:?}"))
                })?
        }
        .remove(0);

        self.pipelines.insert(
            handle,
            VulkanPipeline {
                pipeline,
                layout,
                bind_point: vk::PipelineBindPoint::RAY_TRACING_KHR,
                render_pass: vk::RenderPass::null(),
                push_constants_bytes,
                push_constant_stages,
                uses_bindless,
            },
        );
        self.note_pipeline_created();
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
        //panic allowed, reason = "layout is resolved by the caller before the backend; absence is a caller defect"
        let layout_handle = desc
            .layout
            .expect("graphics pipeline layout must be resolved before backend call");
        let layout = descriptors.pipeline_layout(layout_handle)?;
        let uses_bindless = descriptors.pipeline_uses_bindless(layout_handle)?;
        let push_constants_bytes = descriptors.push_constants_bytes(layout_handle)?;
        let push_constant_stages = descriptors.push_constant_stages(layout_handle)?;

        if self.dynamic_rendering_enabled {
            // Dynamic rendering path: no render pass needed.
            return self.create_graphics_pipeline_inner(
                device,
                handle,
                desc,
                shaders,
                layout,
                vk::RenderPass::null(),
                push_constants_bytes,
                push_constant_stages,
                uses_bindless,
            );
        }

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
            uses_bindless,
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
        uses_bindless: bool,
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
        //panic allowed, reason = "fragment_entry is Some whenever desc.fragment_shader is Some; enforced by the outer if-let"
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

        // When vertex_input_dynamic_state is enabled, vertex format is set per-draw via
        // vkCmdSetVertexInputEXT, so the pipeline can use empty binding/attribute slices.
        let vertex_bindings;
        let vertex_attributes;
        let vertex_input;
        if self.vertex_input_dynamic_state_enabled {
            vertex_bindings = Vec::new();
            vertex_attributes = Vec::new();
            vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        } else {
            vertex_bindings = desc
                .vertex_buffers
                .iter()
                .map(|binding| {
                    vk::VertexInputBindingDescription::default()
                        .binding(binding.binding)
                        .stride(binding.stride)
                        .input_rate(vk_vertex_input_rate(binding.input_rate))
                })
                .collect::<Vec<_>>();
            vertex_attributes = desc
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
            vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
                .vertex_binding_descriptions(&vertex_bindings)
                .vertex_attribute_descriptions(&vertex_attributes);
        }
        let _ = &vertex_bindings;
        let _ = &vertex_attributes;
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk_topology(desc.topology))
            .primitive_restart_enable(false);
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let conservative_mode = match desc.conservative_raster {
            ConservativeRasterMode::Off => None,
            ConservativeRasterMode::Overestimate => {
                if !self.conservative_rasterization_overestimate_enabled {
                    return Err(Error::Unsupported(
                        "conservative rasterization overestimate requires VK_EXT_conservative_rasterization"
                            .into(),
                    ));
                }
                Some(vk::ConservativeRasterizationModeEXT::OVERESTIMATE)
            }
            ConservativeRasterMode::Underestimate => {
                if !self.conservative_rasterization_underestimate_enabled {
                    return Err(Error::Unsupported(
                        "conservative rasterization underestimate requires primitiveUnderestimation support"
                            .into(),
                    ));
                }
                Some(vk::ConservativeRasterizationModeEXT::UNDERESTIMATE)
            }
        };
        let mut conservative_rasterization =
            vk::PipelineRasterizationConservativeStateCreateInfoEXT::default();
        if let Some(mode) = conservative_mode {
            conservative_rasterization =
                conservative_rasterization.conservative_rasterization_mode(mode);
        }
        let mut rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk_polygon_mode(desc.raster.polygon_mode))
            .cull_mode(vk_cull_mode(desc.raster.cull_mode))
            .front_face(vk_front_face(desc.raster.front_face))
            .depth_clamp_enable(desc.raster.depth_clamp)
            .line_width(1.0);
        if let Some(_mode) = conservative_mode {
            rasterization = rasterization.push_next(&mut conservative_rasterization);
        }
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk_samples(desc.samples)?);
        let color_blend_attachments = desc
            .color_targets
            .iter()
            .map(|target| {
                let attachment = vk::PipelineColorBlendAttachmentState::default()
                    .color_write_mask(vk::ColorComponentFlags::RGBA);
                match target.blend {
                    crate::BlendMode::Opaque => attachment.blend_enable(false),
                    crate::BlendMode::Alpha => attachment
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ONE)
                        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                        .alpha_blend_op(vk::BlendOp::ADD),
                }
            })
            .collect::<Vec<_>>();
        let graphics_state = shader_object_graphics_state(desc)?;
        let color_blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_blend_attachments);
        let mut dynamic_states = vec![vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        if self.vrs_pipeline_enabled {
            dynamic_states.push(vk::DynamicState::FRAGMENT_SHADING_RATE_KHR);
        }
        if self.extended_dynamic_state3_enabled {
            // Polygon mode and depth clamp are set dynamically per-pass when EDS3 is available.
            dynamic_states.push(vk::DynamicState::POLYGON_MODE_EXT);
            dynamic_states.push(vk::DynamicState::DEPTH_CLAMP_ENABLE_EXT);
        }
        if self.vertex_input_dynamic_state_enabled {
            // Vertex input layout is set dynamically per-draw.
            dynamic_states.push(vk::DynamicState::VERTEX_INPUT_EXT);
        }
        let dynamic_state =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let base_info = vk::GraphicsPipelineCreateInfo::default()
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

        // When using dynamic rendering (render_pass == null), chain
        // VkPipelineRenderingCreateInfo to specify attachment formats.
        let color_formats: Vec<vk::Format> = desc
            .color_targets
            .iter()
            .map(|t| vk_format(t.format))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| {
                Error::InvalidInput(
                    "graphics pipeline color target has an unsupported format".into(),
                )
            })?;
        let depth_vk_format = desc
            .depth_format
            .map(vk_format)
            .transpose()
            .map_err(|_| {
                Error::InvalidInput("graphics pipeline depth format is unsupported".into())
            })?
            .unwrap_or(vk::Format::UNDEFINED);
        let mut pipeline_rendering_info = vk::PipelineRenderingCreateInfo::default()
            .color_attachment_formats(&color_formats)
            .depth_attachment_format(depth_vk_format);

        let mut info = base_info;
        if render_pass == vk::RenderPass::null() {
            info = info.push_next(&mut pipeline_rendering_info);
        }

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
                uses_bindless,
            },
        );
        self.graphics_states.insert(handle, graphics_state);
        self.note_pipeline_created();
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
        self.graphics_states.remove(&handle);
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
        self.graphics_states.clear();
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

    pub fn graphics_state(&self, handle: PipelineHandle) -> Result<VulkanGraphicsPipelineState> {
        self.graphics_states
            .get(&handle)
            .cloned()
            .ok_or(Error::InvalidHandle)
    }
}

fn create_render_pass(device: &Device, desc: &GraphicsPipelineDesc) -> Result<vk::RenderPass> {
    let mut all_attachments: Vec<vk::AttachmentDescription> = desc
        .color_targets
        .iter()
        .map(|target| {
            Ok(vk::AttachmentDescription::default()
                .format(vk_format(target.format)?)
                .samples(vk_samples(desc.samples)?)
                .load_op(match target.blend {
                    crate::BlendMode::Opaque => vk::AttachmentLoadOp::CLEAR,
                    crate::BlendMode::Alpha => vk::AttachmentLoadOp::LOAD,
                })
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL))
        })
        .collect::<Result<Vec<_>>>()?;

    let color_refs: Vec<vk::AttachmentReference> = (0..desc.color_targets.len())
        .map(|i| {
            vk::AttachmentReference::default()
                .attachment(i as u32)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        })
        .collect();

    // Depth attachment, if requested — appended after all colour attachments.
    let depth_ref = if let Some(depth_fmt) = desc.depth_format {
        let depth_idx = all_attachments.len() as u32;
        all_attachments.push(
            vk::AttachmentDescription::default()
                .format(vk_format(depth_fmt)?)
                .samples(vk_samples(desc.samples)?)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
        );
        Some(
            vk::AttachmentReference::default()
                .attachment(depth_idx)
                .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
        )
    } else {
        None
    };

    let mut subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_refs);
    if let Some(ref d) = depth_ref {
        subpass = subpass.depth_stencil_attachment(d);
    }
    let subpasses = [subpass];

    let info = vk::RenderPassCreateInfo::default()
        .attachments(&all_attachments)
        .subpasses(&subpasses);
    unsafe {
        device
            .create_render_pass(&info, None)
            .map_err(|error| Error::Backend(format!("vkCreateRenderPass failed: {error:?}")))
    }
}

fn shader_object_graphics_state(
    desc: &GraphicsPipelineDesc,
) -> Result<VulkanGraphicsPipelineState> {
    let vertex_bindings = desc
        .vertex_buffers
        .iter()
        .map(|binding| {
            vk::VertexInputBindingDescription2EXT::default()
                .binding(binding.binding)
                .stride(binding.stride)
                .input_rate(vk_vertex_input_rate(binding.input_rate))
                .divisor(1)
        })
        .collect::<Vec<_>>();
    let vertex_attributes = desc
        .vertex_attributes
        .iter()
        .map(|attribute| {
            Ok(vk::VertexInputAttributeDescription2EXT::default()
                .location(attribute.location)
                .binding(attribute.binding)
                .format(vk_vertex_format(attribute.format)?)
                .offset(attribute.offset))
        })
        .collect::<Result<Vec<_>>>()?;
    let color_blend_enables = desc
        .color_targets
        .iter()
        .map(|target| match target.blend {
            crate::BlendMode::Opaque => vk::FALSE,
            crate::BlendMode::Alpha => vk::TRUE,
        })
        .collect::<Vec<_>>();
    let color_blend_equations = desc
        .color_targets
        .iter()
        .map(|target| match target.blend {
            crate::BlendMode::Opaque => vk::ColorBlendEquationEXT::default()
                .src_color_blend_factor(vk::BlendFactor::ONE)
                .dst_color_blend_factor(vk::BlendFactor::ZERO)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ZERO)
                .alpha_blend_op(vk::BlendOp::ADD),
            crate::BlendMode::Alpha => vk::ColorBlendEquationEXT::default()
                .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ONE)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
                .alpha_blend_op(vk::BlendOp::ADD),
        })
        .collect::<Vec<_>>();
    let color_write_masks = desc
        .color_targets
        .iter()
        .map(|_| vk::ColorComponentFlags::RGBA)
        .collect::<Vec<_>>();
    let depth_enable = desc.depth_format.is_some();

    Ok(VulkanGraphicsPipelineState {
        topology: vk_topology(desc.topology),
        cull_mode: vk_cull_mode(desc.raster.cull_mode),
        front_face: vk_front_face(desc.raster.front_face),
        rasterization_samples: vk_samples(desc.samples)?,
        depth_test_enable: depth_enable,
        depth_write_enable: depth_enable,
        depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
        conservative_rasterization_mode: match desc.conservative_raster {
            ConservativeRasterMode::Off => None,
            ConservativeRasterMode::Overestimate => {
                Some(vk::ConservativeRasterizationModeEXT::OVERESTIMATE)
            }
            ConservativeRasterMode::Underestimate => {
                Some(vk::ConservativeRasterizationModeEXT::UNDERESTIMATE)
            }
        },
        vertex_bindings,
        vertex_attributes,
        color_blend_enables,
        color_blend_equations,
        color_write_masks,
        polygon_mode: vk_polygon_mode(desc.raster.polygon_mode),
        depth_clamp: desc.raster.depth_clamp,
    })
}

fn vk_samples(samples: u8) -> Result<vk::SampleCountFlags> {
    match samples {
        1 => Ok(vk::SampleCountFlags::TYPE_1),
        2 => Ok(vk::SampleCountFlags::TYPE_2),
        4 => Ok(vk::SampleCountFlags::TYPE_4),
        8 => Ok(vk::SampleCountFlags::TYPE_8),
        16 => Ok(vk::SampleCountFlags::TYPE_16),
        32 => Ok(vk::SampleCountFlags::TYPE_32),
        64 => Ok(vk::SampleCountFlags::TYPE_64),
        _ => Err(Error::InvalidInput(format!(
            "unsupported Vulkan graphics sample count: {samples}"
        ))),
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

fn vk_polygon_mode(mode: PolygonMode) -> vk::PolygonMode {
    match mode {
        PolygonMode::Fill => vk::PolygonMode::FILL,
        PolygonMode::Line => vk::PolygonMode::LINE,
        PolygonMode::Point => vk::PolygonMode::POINT,
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
