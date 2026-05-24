use std::collections::HashMap;
use std::ffi::CString;
use std::hash::{DefaultHasher, Hash, Hasher};

use ash::vk::TaggedStructure;
use ash::{Device, vk};

use crate::{
    ComputePipelineDesc, ConservativeRasterMode, CullMode, Error, FrontFace, GraphicsPipelineDesc,
    PipelineHandle, PolygonMode, PrimitiveTopology, RayTracingPipelineDesc, Result,
    RtShaderGroupKind, VertexFormat, VertexInputRate,
};

use super::descriptors::DescriptorRegistry;
use super::resources::vk_format;
use super::shaders::{ShaderRegistry, shader_stage_flags};

// GFX-1b: FramebufferCache removed. Framebuffers are now created transiently per-pass
// in CommandContext::record_draw_pass and destroyed after the frame fence fires.

/// Save the pipeline cache after this many new pipelines have been created since
/// the last save.  Keeps startup fast (no stale cache flush) while ensuring the
/// cache is written out during a long session even if the process is killed.
const PIPELINE_CACHE_CHECKPOINT_THRESHOLD: u32 = 8;

pub struct PipelineRegistry {
    pipeline_cache: vk::PipelineCache,
    pipelines: HashMap<PipelineHandle, VulkanPipeline>,
    graphics_states: HashMap<PipelineHandle, VulkanGraphicsPipelineState>,
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
    /// GFX-7b: When true, pipelines carry `VK_PIPELINE_CREATE_2_DESCRIPTOR_HEAP_BIT_EXT`.
    pub descriptor_heap_enabled: bool,
    /// GFX-7a: When true, non-bindless pipelines carry `VK_PIPELINE_CREATE_DESCRIPTOR_BUFFER_BIT_EXT`.
    pub descriptor_buffer_enabled: bool,
    /// GFX-2c: VK_EXT_graphics_pipeline_library enabled; use 4-stage library linkage.
    pub graphics_pipeline_library_enabled: bool,
    /// GFX-2c: Cached VertexInput libraries keyed by (vertex format + topology) hash.
    /// These are material-independent and rarely change.
    pub vertex_input_libs: HashMap<u64, vk::Pipeline>,
    /// GFX-2c: Cached FragmentOutput libraries keyed by (attachment formats + blend + samples) hash.
    pub fragment_output_libs: HashMap<u64, vk::Pipeline>,
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
            pipelines_since_checkpoint: 0,
            dynamic_rendering_enabled: false,
            vrs_pipeline_enabled: false,
            conservative_rasterization_overestimate_enabled: false,
            conservative_rasterization_underestimate_enabled: false,
            extended_dynamic_state3_enabled: false,
            vertex_input_dynamic_state_enabled: false,
            descriptor_heap_enabled: false,
            graphics_pipeline_library_enabled: false,
            vertex_input_libs: HashMap::new(),
            fragment_output_libs: HashMap::new(),
            descriptor_buffer_enabled: false,
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
            tracing::debug!(
                new_pipelines = self.pipelines_since_checkpoint,
                total_pipelines = self.pipelines.len(),
                "pipeline cache checkpoint: serializing to disk"
            );
            self.pipelines_since_checkpoint = 0;
            match self.serialize_cache(device) {
                Ok(data) => {
                    tracing::debug!(bytes = data.len(), "pipeline cache serialized");
                    Some(data)
                }
                Err(e) => {
                    tracing::warn!(
                        "pipeline cache serialization failed — cache will not be saved this checkpoint: {e:?}"
                    );
                    None
                }
            }
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
    /// Rasterizer discard, recorded dynamically when EDS3 or basic dynamic state is available.
    pub rasterizer_discard: bool,
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
        let layout_handle = desc.layout.ok_or_else(|| {
            Error::InvalidInput(
                "compute pipeline layout must be resolved before backend call".into(),
            )
        })?;
        let layout = descriptors.pipeline_layout(layout_handle)?;
        let uses_bindless = descriptors.pipeline_uses_bindless(layout_handle)?;
        let push_constants_bytes = descriptors.push_constants_bytes(layout_handle)?;
        let push_constant_stages = descriptors.push_constant_stages(layout_handle)?;
        let entry_str = shaders.entry_point(desc.shader)?;
        tracing::debug!(
            ?handle,
            entry_point = %entry_str,
            uses_bindless,
            push_constants_bytes,
            "compiling compute pipeline"
        );
        let entry = CString::new(entry_str).map_err(|_| {
            Error::InvalidInput("shader entry point cannot contain interior nul bytes".into())
        })?;
        let stage_info = vk::PipelineShaderStageCreateInfo::default()
            .stage(shader_stage_flags(stage))
            .module(module)
            .name(&entry);
        let mut info = vk::ComputePipelineCreateInfo::default()
            .stage(stage_info)
            .layout(layout);
        // GFX-7b: only pipelines whose layout actually uses the bindless heap should carry
        // VK_PIPELINE_CREATE_2_DESCRIPTOR_HEAP_BIT_EXT. Advertising heap access on ordinary
        // descriptor-set/descriptor-buffer pipelines can trigger driver crashes on some stacks.
        let mut heap_flags_info;
        if self.descriptor_heap_enabled && uses_bindless {
            heap_flags_info = vk::PipelineCreateFlags2CreateInfo::default()
                .flags(vk::PipelineCreateFlags2::DESCRIPTOR_HEAP_EXT);
            info = info.push(&mut heap_flags_info);
        }
        // GFX-7a: non-bindless compute pipelines use descriptor buffer when available.
        if self.descriptor_buffer_enabled && !uses_bindless {
            info = info.flags(info.flags | vk::PipelineCreateFlags::DESCRIPTOR_BUFFER_EXT);
        }
        let pipeline = unsafe {
            device
                .create_compute_pipelines(self.pipeline_cache, &[info], None)
                .map_err(|(_, error)| {
                    tracing::error!(
                        ?handle,
                        "vkCreateComputePipelines failed — check shader compilation errors \
                         above, push constant size limits, or descriptor layout mismatch: {error:?}"
                    );
                    Error::Backend(format!("vkCreateComputePipelines failed: {error:?}"))
                })?
        }
        .remove(0);
        tracing::debug!(?handle, "compute pipeline compiled");

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
        let layout_handle = desc.layout.ok_or_else(|| {
            Error::InvalidInput("RT pipeline layout must be resolved before backend call".into())
        })?;
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

        // RT pipelines must not share the on-disk graphics pipeline cache.
        // The graphics cache contains type-specific blobs; passing it to the RT
        // compiler causes it to deserialize graphics blobs as RT blobs and crash.
        // Null disables caching for this call — this is correct Vulkan behaviour.
        tracing::debug!(
            stages = desc.stages.len(),
            groups = desc.groups.len(),
            max_recursion = desc.max_recursion_depth,
            "compiling RT pipeline (no shared cache — RT uses its own cache space)"
        );
        let pipeline = unsafe {
            rt_ext
                .create_ray_tracing_pipelines(
                    vk::DeferredOperationKHR::null(),
                    vk::PipelineCache::null(),
                    &[create_info],
                    None,
                )
                .map_err(|(_, e)| {
                    tracing::error!("vkCreateRayTracingPipelinesKHR failed: {e:?}");
                    Error::Backend(format!("vkCreateRayTracingPipelinesKHR failed: {e:?}"))
                })?
        }
        .remove(0);
        tracing::debug!("RT pipeline compiled successfully");

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
        tracing::debug!(
            ?handle,
            color_targets = desc.color_targets.len(),
            has_depth = desc.depth_format.is_some(),
            samples = desc.samples,
            topology = ?desc.topology,
            cull_mode = ?desc.raster.cull_mode,
            dynamic_rendering = self.dynamic_rendering_enabled,
            pipeline_library = self.graphics_pipeline_library_enabled,
            "creating graphics pipeline"
        );
        let layout_handle = desc.layout.ok_or_else(|| {
            Error::InvalidInput(
                "graphics pipeline layout must be resolved before backend call".into(),
            )
        })?;
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
        let fragment_stage = if let Some(shader) = desc.fragment_shader {
            let Some(fragment_entry) = fragment_entry.as_ref() else {
                return Err(Error::ResourceStateCorruption(
                    "fragment shader entry string was not created".into(),
                ));
            };
            Some(
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(shader_stage_flags(shaders.stage(shader)?))
                    .module(shaders.module(shader)?)
                    .name(fragment_entry),
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
            .rasterizer_discard_enable(desc.raster.rasterizer_discard)
            .line_width(1.0);
        if let Some(_mode) = conservative_mode {
            rasterization = rasterization.push(&mut conservative_rasterization);
        }
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk_samples(desc.samples)?);
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(desc.depth_format.is_some())
            .depth_write_enable(desc.depth_format.is_some())
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);
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
                    crate::BlendMode::Negative => attachment
                        .blend_enable(true)
                        .src_color_blend_factor(vk::BlendFactor::ONE_MINUS_DST_COLOR)
                        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_COLOR)
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(vk::BlendFactor::ZERO)
                        .dst_alpha_blend_factor(vk::BlendFactor::ONE)
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
            .depth_stencil_state(&depth_stencil)
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

        // GFX-2c: When graphics_pipeline_library is available and dynamic rendering is active,
        // split into 4 stage-specialised library pipelines and link them. This allows the
        // material-independent VertexInput and FragmentOutput libraries to be cached and
        // reused across materials, and lets VS + FS compilation happen independently.
        if self.graphics_pipeline_library_enabled && render_pass == vk::RenderPass::null() {
            let pipeline = self.create_with_pipeline_library(
                device,
                desc,
                &stages,
                &vertex_input,
                &input_assembly,
                &viewport_state,
                &rasterization,
                &multisample,
                &color_blend,
                &color_blend_attachments,
                &dynamic_state,
                &mut pipeline_rendering_info,
                &color_formats,
                depth_vk_format,
                layout,
                push_constants_bytes,
                push_constant_stages,
                uses_bindless,
                handle,
                graphics_state.clone(),
            )?;
            if pipeline {
                return Ok(());
            }
            // Fall through to monolithic path on error.
        }

        let mut info = base_info;
        if render_pass == vk::RenderPass::null() {
            info = info.push(&mut pipeline_rendering_info);
        }
        // GFX-7b: only pipelines whose layout actually uses the bindless heap should carry
        // VK_PIPELINE_CREATE_2_DESCRIPTOR_HEAP_BIT_EXT. Advertising heap access on ordinary
        // descriptor-set/descriptor-buffer pipelines can trigger driver crashes on some stacks.
        let mut heap_flags_info;
        if self.descriptor_heap_enabled && uses_bindless {
            heap_flags_info = vk::PipelineCreateFlags2CreateInfo::default()
                .flags(vk::PipelineCreateFlags2::DESCRIPTOR_HEAP_EXT);
            info = info.push(&mut heap_flags_info);
        }
        // GFX-7a: non-bindless pipelines use descriptor buffer when available.
        if self.descriptor_buffer_enabled && !uses_bindless {
            info = info.flags(info.flags | vk::PipelineCreateFlags::DESCRIPTOR_BUFFER_EXT);
        }

        tracing::debug!(
            ?handle,
            stages = stages.len(),
            color_targets = desc.color_targets.len(),
            has_depth = desc.depth_format.is_some(),
            push_constants_bytes,
            uses_bindless,
            descriptor_heap_enabled = self.descriptor_heap_enabled,
            descriptor_heap_pipeline_flag = self.descriptor_heap_enabled && uses_bindless,
            descriptor_buffer_enabled = self.descriptor_buffer_enabled,
            descriptor_buffer_pipeline_flag = self.descriptor_buffer_enabled && !uses_bindless,
            "vkCreateGraphicsPipelines (monolithic)"
        );
        let pipeline = unsafe {
            device
                .create_graphics_pipelines(self.pipeline_cache, &[info], None)
                .map_err(|(_, error)| {
                    tracing::error!(
                        ?handle,
                        stages = stages.len(),
                        color_targets = desc.color_targets.len(),
                        has_depth = desc.depth_format.is_some(),
                        "vkCreateGraphicsPipelines failed — likely causes: shader interface \
                         mismatch between vertex/fragment stages, color/depth format incompatible \
                         with render pass, push constant size exceeds device limit, or \
                         unsupported feature combination: {error:?}"
                    );
                    Error::Backend(format!("vkCreateGraphicsPipelines failed: {error:?}"))
                })?
        }
        .remove(0);
        tracing::debug!(?handle, "graphics pipeline compiled");

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

    /// GFX-2c: 4-stage pipeline library path.
    ///
    /// Creates VertexInput + FragmentOutput libraries (cached by hash) and
    /// per-material PreRasterization + FragmentShader libraries, then links them.
    /// Returns `true` when the linked pipeline was successfully stored; `false`
    /// to fall back to the monolithic path.
    #[allow(clippy::too_many_arguments)]
    fn create_with_pipeline_library(
        &mut self,
        device: &Device,
        desc: &crate::GraphicsPipelineDesc,
        stages: &[vk::PipelineShaderStageCreateInfo<'_>],
        vertex_input: &vk::PipelineVertexInputStateCreateInfo<'_>,
        input_assembly: &vk::PipelineInputAssemblyStateCreateInfo<'_>,
        viewport_state: &vk::PipelineViewportStateCreateInfo<'_>,
        rasterization: &vk::PipelineRasterizationStateCreateInfo<'_>,
        multisample: &vk::PipelineMultisampleStateCreateInfo<'_>,
        color_blend: &vk::PipelineColorBlendStateCreateInfo<'_>,
        _color_blend_attachments: &[vk::PipelineColorBlendAttachmentState],
        dynamic_state: &vk::PipelineDynamicStateCreateInfo<'_>,
        pipeline_rendering_info: &mut vk::PipelineRenderingCreateInfo<'_>,
        color_formats: &[vk::Format],
        depth_vk_format: vk::Format,
        layout: vk::PipelineLayout,
        push_constants_bytes: u32,
        push_constant_stages: vk::ShaderStageFlags,
        uses_bindless: bool,
        handle: crate::PipelineHandle,
        graphics_state: VulkanGraphicsPipelineState,
    ) -> Result<bool> {
        // ── 1. VertexInput library (cache by vertex format + topology hash) ────
        let vi_key = {
            let mut h = DefaultHasher::new();
            desc.vertex_buffers.iter().for_each(|b| {
                b.binding.hash(&mut h);
                b.stride.hash(&mut h);
                (b.input_rate as u32).hash(&mut h);
            });
            desc.vertex_attributes.iter().for_each(|a| {
                a.location.hash(&mut h);
                a.binding.hash(&mut h);
                (a.format as u32).hash(&mut h);
                a.offset.hash(&mut h);
            });
            (desc.topology as u32).hash(&mut h);
            h.finish()
        };
        let vi_lib = if let Some(&lib) = self.vertex_input_libs.get(&vi_key) {
            lib
        } else {
            let mut lib_stage_info = vk::GraphicsPipelineLibraryCreateInfoEXT::default()
                .flags(vk::GraphicsPipelineLibraryFlagsEXT::VERTEX_INPUT_INTERFACE);
            let vi_info = vk::GraphicsPipelineCreateInfo::default()
                .flags(vk::PipelineCreateFlags::LIBRARY_KHR)
                .vertex_input_state(vertex_input)
                .input_assembly_state(input_assembly)
                .dynamic_state(dynamic_state)
                .push(&mut lib_stage_info);
            let lib = unsafe {
                device
                    .create_graphics_pipelines(self.pipeline_cache, &[vi_info], None)
                    .map_err(|(_, e)| {
                        Error::Backend(format!("VertexInput library failed: {e:?}"))
                    })?
            }
            .remove(0);
            self.vertex_input_libs.insert(vi_key, lib);
            lib
        };

        // ── 2. FragmentOutput library (cache by attachment formats + blend + samples) ──
        let fo_key = {
            let mut h = DefaultHasher::new();
            color_formats.iter().for_each(|f| f.as_raw().hash(&mut h));
            depth_vk_format.as_raw().hash(&mut h);
            desc.color_targets
                .iter()
                .for_each(|t| (t.blend as u32).hash(&mut h));
            (desc.samples as u32).hash(&mut h);
            h.finish()
        };
        let fo_lib = if let Some(&lib) = self.fragment_output_libs.get(&fo_key) {
            lib
        } else {
            let mut lib_stage_info = vk::GraphicsPipelineLibraryCreateInfoEXT::default()
                .flags(vk::GraphicsPipelineLibraryFlagsEXT::FRAGMENT_OUTPUT_INTERFACE);
            let fo_info = vk::GraphicsPipelineCreateInfo::default()
                .flags(vk::PipelineCreateFlags::LIBRARY_KHR)
                .multisample_state(multisample)
                .color_blend_state(color_blend)
                .dynamic_state(dynamic_state)
                .push(&mut lib_stage_info)
                .push(pipeline_rendering_info);
            let lib = unsafe {
                device
                    .create_graphics_pipelines(self.pipeline_cache, &[fo_info], None)
                    .map_err(|(_, e)| {
                        Error::Backend(format!("FragmentOutput library failed: {e:?}"))
                    })?
            }
            .remove(0);
            self.fragment_output_libs.insert(fo_key, lib);
            lib
        };

        // ── 3. PreRasterization library (VS + rasterization; per-material) ────
        // Spec: PRE_RASTERIZATION_SHADERS must NOT include pVertexInputState or
        // pInputAssemblyState — those belong to VERTEX_INPUT_INTERFACE (vi_lib).
        let vs_stages: Vec<_> = stages
            .iter()
            .filter(|s| s.stage.contains(vk::ShaderStageFlags::VERTEX))
            .copied()
            .collect();
        let mut pre_raster_lib_info = vk::GraphicsPipelineLibraryCreateInfoEXT::default()
            .flags(vk::GraphicsPipelineLibraryFlagsEXT::PRE_RASTERIZATION_SHADERS);
        let pre_raster_info = vk::GraphicsPipelineCreateInfo::default()
            .flags(vk::PipelineCreateFlags::LIBRARY_KHR)
            .stages(&vs_stages)
            .viewport_state(viewport_state)
            .rasterization_state(rasterization)
            .dynamic_state(dynamic_state)
            .layout(layout)
            .push(&mut pre_raster_lib_info)
            .push(pipeline_rendering_info);
        let pre_raster_lib = unsafe {
            device
                .create_graphics_pipelines(self.pipeline_cache, &[pre_raster_info], None)
                .map_err(|(_, e)| {
                    Error::Backend(format!("PreRasterization library failed: {e:?}"))
                })?
        }
        .remove(0);

        // ── 4. FragmentShader library (FS; per-material) ─────────────────────
        let fs_stages: Vec<_> = stages
            .iter()
            .filter(|s| s.stage.contains(vk::ShaderStageFlags::FRAGMENT))
            .copied()
            .collect();
        let frag_lib = if !fs_stages.is_empty() {
            let mut frag_lib_info = vk::GraphicsPipelineLibraryCreateInfoEXT::default()
                .flags(vk::GraphicsPipelineLibraryFlagsEXT::FRAGMENT_SHADER);
            let frag_info = vk::GraphicsPipelineCreateInfo::default()
                .flags(vk::PipelineCreateFlags::LIBRARY_KHR)
                .stages(&fs_stages)
                .multisample_state(multisample)
                .dynamic_state(dynamic_state)
                .layout(layout)
                .push(&mut frag_lib_info)
                .push(pipeline_rendering_info);
            let lib = unsafe {
                device
                    .create_graphics_pipelines(self.pipeline_cache, &[frag_info], None)
                    .map_err(|(_, e)| {
                        device.destroy_pipeline(pre_raster_lib, None);
                        Error::Backend(format!("FragmentShader library failed: {e:?}"))
                    })?
            }
            .remove(0);
            Some(lib)
        } else {
            None
        };

        // ── 5. Link into final pipeline ───────────────────────────────────────
        let mut all_libs = vec![vi_lib, fo_lib, pre_raster_lib];
        if let Some(fl) = frag_lib {
            all_libs.push(fl);
        }
        let mut lib_create_info = vk::PipelineLibraryCreateInfoKHR::default().libraries(&all_libs);
        let link_info = vk::GraphicsPipelineCreateInfo::default()
            .flags(vk::PipelineCreateFlags::LINK_TIME_OPTIMIZATION_EXT)
            .layout(layout)
            .push(&mut lib_create_info);
        let linked =
            unsafe { device.create_graphics_pipelines(self.pipeline_cache, &[link_info], None) };

        // Destroy the per-material intermediate libs (vi + fo stay in the cache).
        unsafe { device.destroy_pipeline(pre_raster_lib, None) };
        if let Some(fl) = frag_lib {
            unsafe { device.destroy_pipeline(fl, None) };
        }

        let pipeline = linked
            .map_err(|(_, e)| Error::Backend(format!("pipeline library link failed: {e:?}")))?
            .remove(0);

        self.pipelines.insert(
            handle,
            VulkanPipeline {
                pipeline,
                layout,
                bind_point: vk::PipelineBindPoint::GRAPHICS,
                render_pass: vk::RenderPass::null(),
                push_constants_bytes,
                push_constant_stages,
                uses_bindless,
            },
        );
        self.graphics_states.insert(handle, graphics_state);
        self.note_pipeline_created();
        Ok(true)
    }

    pub fn destroy_pipeline(&mut self, device: &Device, handle: PipelineHandle) -> Result<()> {
        let pipeline = self.pipelines.remove(&handle).ok_or(Error::InvalidHandle)?;
        self.graphics_states.remove(&handle);
        unsafe {
            device.destroy_pipeline(pipeline.pipeline, None);
            if pipeline.render_pass != vk::RenderPass::null() {
                device.destroy_render_pass(pipeline.render_pass, None);
            }
        }
        Ok(())
    }

    pub fn destroy_all(&mut self, device: &Device) {
        self.graphics_states.clear();
        for (_, pipeline) in self.pipelines.drain() {
            unsafe {
                device.destroy_pipeline(pipeline.pipeline, None);
                if pipeline.render_pass != vk::RenderPass::null() {
                    device.destroy_render_pass(pipeline.render_pass, None);
                }
            }
        }
        // GFX-2c: destroy cached pipeline library objects.
        for (_, lib) in self.vertex_input_libs.drain() {
            unsafe { device.destroy_pipeline(lib, None) };
        }
        for (_, lib) in self.fragment_output_libs.drain() {
            unsafe { device.destroy_pipeline(lib, None) };
        }
        unsafe { device.destroy_pipeline_cache(self.pipeline_cache, None) };
    }

    pub fn pipeline(&self, handle: PipelineHandle) -> Result<VulkanPipeline> {
        self.pipelines
            .get(&handle)
            .copied()
            .ok_or(Error::InvalidHandle)
    }

    /// Return the raw VkPipeline handle for pipeline executable stats queries.
    pub fn vk_pipeline(&self, handle: PipelineHandle) -> Result<vk::Pipeline> {
        self.pipeline(handle).map(|p| p.pipeline)
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
                    crate::BlendMode::Alpha | crate::BlendMode::Negative => {
                        vk::AttachmentLoadOp::LOAD
                    }
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
            crate::BlendMode::Alpha | crate::BlendMode::Negative => vk::TRUE,
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
            crate::BlendMode::Negative => vk::ColorBlendEquationEXT::default()
                .src_color_blend_factor(vk::BlendFactor::ONE_MINUS_DST_COLOR)
                .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_COLOR)
                .color_blend_op(vk::BlendOp::ADD)
                .src_alpha_blend_factor(vk::BlendFactor::ZERO)
                .dst_alpha_blend_factor(vk::BlendFactor::ONE)
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
        rasterizer_discard: desc.raster.rasterizer_discard,
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
