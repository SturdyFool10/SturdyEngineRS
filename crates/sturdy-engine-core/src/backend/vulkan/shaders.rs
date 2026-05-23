use std::collections::HashMap;

use ash::{Device, vk};

use crate::{Error, Result, ShaderDesc, ShaderHandle, ShaderSource, ShaderStage};

#[derive(Default)]
pub struct ShaderRegistry {
    shaders: HashMap<ShaderHandle, VulkanShader>,
}

struct VulkanShader {
    module: vk::ShaderModule,
    stage: ShaderStage,
    entry_point: String,
    /// Raw SPIR-V words, retained for VK_EXT_shader_object creation.
    spirv_words: Vec<u32>,
}

impl ShaderRegistry {
    pub fn create_shader(
        &mut self,
        device: &Device,
        handle: ShaderHandle,
        desc: &ShaderDesc,
    ) -> Result<()> {
        let ShaderSource::Spirv(words) = &desc.source else {
            return Err(Error::Unsupported(
                "Vulkan shader creation currently requires ShaderSource::Spirv",
            ));
        };
        tracing::debug!(
            stage = ?desc.stage,
            entry_point = %desc.entry_point,
            spirv_words = words.len(),
            "creating shader module"
        );
        let info = vk::ShaderModuleCreateInfo::default().code(words);
        let module = unsafe {
            device.create_shader_module(&info, None).map_err(|error| {
                tracing::error!(
                    stage = ?desc.stage,
                    entry_point = %desc.entry_point,
                    spirv_words = words.len(),
                    "vkCreateShaderModule failed — SPIR-V may be malformed, the entry point \
                     may be missing, or a required capability is unsupported: {error:?}"
                );
                Error::Backend(format!("vkCreateShaderModule failed: {error:?}"))
            })?
        };
        tracing::debug!(stage = ?desc.stage, entry_point = %desc.entry_point, "shader module ready");
        self.shaders.insert(
            handle,
            VulkanShader {
                module,
                stage: desc.stage,
                entry_point: desc.entry_point.clone(),
                spirv_words: words.clone(),
            },
        );
        Ok(())
    }

    pub fn destroy_shader(&mut self, device: &Device, handle: ShaderHandle) -> Result<()> {
        let shader = self.shaders.remove(&handle).ok_or(Error::InvalidHandle)?;
        tracing::trace!(stage = ?shader.stage, entry_point = %shader.entry_point, "destroying shader module");
        unsafe {
            device.destroy_shader_module(shader.module, None);
        }
        Ok(())
    }

    pub fn destroy_all(&mut self, device: &Device) {
        for (_, shader) in self.shaders.drain() {
            unsafe {
                device.destroy_shader_module(shader.module, None);
            }
        }
    }

    pub fn module(&self, handle: ShaderHandle) -> Result<vk::ShaderModule> {
        self.shaders
            .get(&handle)
            .map(|shader| shader.module)
            .ok_or(Error::InvalidHandle)
    }

    pub fn stage(&self, handle: ShaderHandle) -> Result<ShaderStage> {
        self.shaders
            .get(&handle)
            .map(|shader| shader.stage)
            .ok_or(Error::InvalidHandle)
    }

    pub fn entry_point(&self, handle: ShaderHandle) -> Result<&str> {
        self.shaders
            .get(&handle)
            .map(|shader| shader.entry_point.as_str())
            .ok_or(Error::InvalidHandle)
    }

    /// Returns the raw SPIR-V words for a shader.
    /// Used by VK_EXT_shader_object, which consumes the same bytes but requires 4-byte alignment.
    pub fn spirv_words(&self, handle: ShaderHandle) -> Result<&[u32]> {
        self.shaders
            .get(&handle)
            .map(|shader| shader.spirv_words.as_slice())
            .ok_or(Error::InvalidHandle)
    }
}

#[allow(dead_code)]
pub fn shader_stage_flags(stage: ShaderStage) -> vk::ShaderStageFlags {
    match stage {
        ShaderStage::Vertex => vk::ShaderStageFlags::VERTEX,
        ShaderStage::Fragment => vk::ShaderStageFlags::FRAGMENT,
        ShaderStage::Compute => vk::ShaderStageFlags::COMPUTE,
        ShaderStage::Mesh => vk::ShaderStageFlags::MESH_EXT,
        ShaderStage::Task => vk::ShaderStageFlags::TASK_EXT,
        ShaderStage::RayGeneration => vk::ShaderStageFlags::RAYGEN_KHR,
        ShaderStage::Miss => vk::ShaderStageFlags::MISS_KHR,
        ShaderStage::ClosestHit => vk::ShaderStageFlags::CLOSEST_HIT_KHR,
    }
}
