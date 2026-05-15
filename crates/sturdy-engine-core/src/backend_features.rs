#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct BackendFeatures {
    pub ray_tracing: bool,
    pub mesh_shading: bool,
    pub bindless: bool,
    pub descriptor_indexing: bool,
    pub timeline_semaphores: bool,
    pub dynamic_rendering: bool,
    pub synchronization2: bool,
    /// VK_KHR_buffer_device_address (or Vulkan 1.2 core) — expose GPU virtual
    /// addresses for buffers created with SHADER_DEVICE_ADDRESS usage.
    pub buffer_device_address: bool,
    pub hdr_output: bool,
    pub shader_fp16: bool,
    pub shader_fp64: bool,
    pub image_fp16_render: bool,
    pub image_fp32_render: bool,
    pub variable_rate_shading: bool,
    /// Multiple indirect draw commands can be submitted by one API call.
    pub multi_draw_indirect: bool,
    /// GPU-visible count buffers can limit indirect draw command consumption.
    pub draw_indirect_count: bool,
    /// Backend can create sparse/tiled resources with explicit page binding.
    pub sparse_binding: bool,
    /// Sparse buffers are supported.
    pub sparse_residency_buffer: bool,
    /// Sparse 2D images are supported.
    pub sparse_residency_image_2d: bool,
    /// Sparse 3D images are supported.
    pub sparse_residency_image_3d: bool,
    /// Multiple sparse resources can alias the same memory pages.
    pub sparse_residency_aliased: bool,
    /// VK_EXT_memory_budget — query per-heap budget and usage.
    pub memory_budget: bool,
    /// VK_EXT_memory_priority — assign allocation priority hints.
    pub memory_priority: bool,
    /// VK_EXT_pageable_device_local_memory — allow eviction of device-local pages.
    pub pageable_device_local_memory: bool,
    /// VK_EXT_device_fault — retrieve breadcrumbs after VK_ERROR_DEVICE_LOST.
    pub device_fault: bool,
    /// VK_EXT_host_image_copy (or Vulkan 1.4 core) — CPU→GPU image uploads
    /// without a staging buffer or explicit transfer command.
    pub host_image_copy: bool,
    /// VK_KHR_push_descriptor — inline descriptor updates pushed directly into
    /// a command buffer without a descriptor pool.
    pub push_descriptors: bool,
    /// VK_EXT_conditional_rendering — GPU-side predicate that skips draw/dispatch
    /// commands based on a buffer value.
    pub conditional_rendering: bool,
    /// VK_EXT_extended_dynamic_state3 — additional pipeline state settable dynamically.
    pub extended_dynamic_state3: bool,
    /// Polygon mode can be set dynamically (part of extended_dynamic_state3).
    pub extended_dynamic_state3_polygon_mode: bool,
    /// Color blend enable can be set dynamically (part of extended_dynamic_state3).
    pub extended_dynamic_state3_color_blend: bool,
    /// VK_EXT_vertex_input_dynamic_state — vertex input state settable dynamically.
    pub vertex_input_dynamic_state: bool,
    /// VK_EXT_conservative_rasterization overestimation mode.
    pub conservative_rasterization_overestimate: bool,
    /// VK_EXT_conservative_rasterization underestimation mode.
    pub conservative_rasterization_underestimate: bool,
    /// VK_KHR_global_priority — assign global GPU scheduler priority to queues.
    pub global_queue_priority: bool,
    /// VK_EXT_sampler_filter_minmax (core 1.2) — min/max reduction sampler filter.
    pub sampler_filter_minmax: bool,
    /// VK_EXT_custom_border_color — arbitrary border color for samplers.
    pub custom_border_color: bool,
    /// VK_EXT_filter_cubic — cubic filtering for sampled images.
    pub filter_cubic: bool,
    /// VK_EXT_image_view_min_lod — clamp minimum LOD in an image view.
    pub image_view_min_lod: bool,
    /// VK_EXT_image_compression_control — explicit lossy/lossless compression hints.
    pub image_compression_control: bool,
    /// VK_EXT_multisampled_render_to_single_sampled — render MSAA into single-sample storage.
    pub msaa_render_to_single_sampled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_features_are_conservative() {
        let features = BackendFeatures::default();

        assert!(!features.mesh_shading);
        assert!(!features.ray_tracing);
        assert!(!features.bindless);
        assert!(!features.hdr_output);
        assert!(!features.shader_fp16);
        assert!(!features.shader_fp64);
        assert!(!features.image_fp16_render);
        assert!(!features.image_fp32_render);
        assert!(!features.dynamic_rendering);
        assert!(!features.timeline_semaphores);
        assert!(!features.multi_draw_indirect);
        assert!(!features.draw_indirect_count);
        assert!(!features.sparse_binding);
        assert!(!features.sparse_residency_buffer);
        assert!(!features.sparse_residency_image_2d);
        assert!(!features.sparse_residency_image_3d);
        assert!(!features.sparse_residency_aliased);
    }

    #[test]
    fn sparse_texture_ready_requires_binding_and_image_residency() {
        let features = BackendFeatures {
            sparse_binding: true,
            sparse_residency_image_2d: true,
            ..BackendFeatures::default()
        };

        assert!(features.supports_sparse_2d_textures());
        assert!(!features.supports_sparse_buffers());
    }

    #[test]
    fn gpu_draw_compaction_requires_multi_draw_and_indirect_count() {
        let features = BackendFeatures {
            multi_draw_indirect: true,
            draw_indirect_count: true,
            ..BackendFeatures::default()
        };

        assert!(features.supports_gpu_draw_compaction());
    }
}

impl BackendFeatures {
    /// True when the backend can support sparse/tiled 2D texture residency.
    pub fn supports_sparse_2d_textures(&self) -> bool {
        self.sparse_binding && self.sparse_residency_image_2d
    }

    /// True when the backend can support sparse/tiled 3D texture residency.
    pub fn supports_sparse_3d_textures(&self) -> bool {
        self.sparse_binding && self.sparse_residency_image_3d
    }

    /// True when the backend can support sparse/tiled buffer residency.
    pub fn supports_sparse_buffers(&self) -> bool {
        self.sparse_binding && self.sparse_residency_buffer
    }

    /// True when the backend can consume GPU-compacted indirect draw streams
    /// through a count buffer.
    pub fn supports_gpu_draw_compaction(&self) -> bool {
        self.multi_draw_indirect && self.draw_indirect_count
    }
}
