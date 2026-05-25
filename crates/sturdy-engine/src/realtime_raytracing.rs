use crate::{
    AccelerationStructure, AccelerationStructureBuildMode, AccelerationStructureDesc,
    AccelerationStructureKind, BindGroup, BlasBuildDesc, BlasGeometryDesc, Buffer, BufferDesc,
    BufferUsage, Engine, Error, Pipeline, PipelineLayout, RayTracingPipelineDesc,
    RayTracingShaderBindingTable, RayTracingStageDesc, RenderFrame, Result, RtShaderGroupDesc,
    Shader, ShaderBindingTableDesc, ShaderDesc, ShaderSource, ShaderStage, TlasBuildDesc,
    TraceRaysDesc,
};

/// Capabilities relevant to fully featured realtime hardware ray tracing.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RealtimeRayTracingSupport {
    pub ray_tracing_pipeline: bool,
    pub ray_query: bool,
    pub buffer_device_address: bool,
    pub ray_tracing_position_fetch: bool,
    pub ray_tracing_maintenance1: bool,
    pub opacity_micromap: bool,
    pub shader_execution_reordering: bool,
    pub cluster_acceleration_structure: bool,
}

impl RealtimeRayTracingSupport {
    pub fn available(self) -> bool {
        self.ray_tracing_pipeline && self.buffer_device_address
    }
}

/// Explicit application-provided shader entry points for a ray-tracing pipeline.
///
/// The engine owns pipeline/SBT construction, AS build submission, and trace dispatch; the
/// application supplies the actual raygen/miss/hit shader code.
#[derive(Clone, Debug)]
pub struct RealtimeRayTracingShaderDesc {
    pub source: ShaderSource,
    pub raygen_entry: String,
    pub miss_entries: Vec<String>,
    pub closest_hit_entries: Vec<String>,
    pub max_recursion_depth: u32,
    pub use_pipeline_libraries: bool,
    pub hit_shaders_use_ser: bool,
}

/// Runtime ray-tracing pipeline plus its default shader binding table.
pub struct RealtimeRayTracingPipeline {
    pub pipeline: Pipeline,
    pub sbt: RayTracingShaderBindingTable,
    pub layout: PipelineLayout,
    shaders: Vec<Shader>,
    miss_group_count: u32,
    hit_group_count: u32,
}

impl RealtimeRayTracingPipeline {
    pub fn new(
        engine: &Engine,
        layout: PipelineLayout,
        desc: RealtimeRayTracingShaderDesc,
    ) -> Result<Self> {
        if !engine.realtime_ray_tracing_support().available() {
            return Err(Error::Unsupported(
                "hardware ray tracing requires ray-tracing pipeline and buffer-device-address support"
                    .into(),
            ));
        }
        if desc.miss_entries.is_empty() {
            return Err(Error::InvalidInput(
                "ray-tracing pipeline requires at least one miss shader".into(),
            ));
        }
        if desc.closest_hit_entries.is_empty() {
            return Err(Error::InvalidInput(
                "ray-tracing pipeline requires at least one closest-hit shader".into(),
            ));
        }

        let mut shaders = Vec::new();
        shaders.push(engine.create_shader(ShaderDesc {
            source: desc.source.clone(),
            entry_point: desc.raygen_entry,
            stage: ShaderStage::RayGeneration,
            requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
        })?);
        for entry in &desc.miss_entries {
            shaders.push(engine.create_shader(ShaderDesc {
                source: desc.source.clone(),
                entry_point: entry.clone(),
                stage: ShaderStage::Miss,
                requires_ray_query: false,
                requires_cooperative_matrix: false,
                uses_ser: false,
            })?);
        }
        for entry in &desc.closest_hit_entries {
            shaders.push(engine.create_shader(ShaderDesc {
                source: desc.source.clone(),
                entry_point: entry.clone(),
                stage: ShaderStage::ClosestHit,
                requires_ray_query: false,
                requires_cooperative_matrix: false,
                uses_ser: desc.hit_shaders_use_ser,
            })?);
        }

        let stages = shaders
            .iter()
            .map(|shader| RayTracingStageDesc {
                shader: shader.handle(),
            })
            .collect::<Vec<_>>();
        let mut groups = Vec::new();
        groups.push(RtShaderGroupDesc::general(0));
        for miss_index in 0..desc.miss_entries.len() {
            groups.push(RtShaderGroupDesc::general(1 + miss_index as u32));
        }
        let hit_stage_base = 1 + desc.miss_entries.len() as u32;
        for hit_index in 0..desc.closest_hit_entries.len() {
            groups.push(RtShaderGroupDesc::triangles_hit(
                hit_stage_base + hit_index as u32,
                RtShaderGroupDesc::UNUSED,
            ));
        }

        let pipeline = engine.create_ray_tracing_pipeline(RayTracingPipelineDesc {
            stages,
            groups,
            max_recursion_depth: desc.max_recursion_depth.max(1),
            layout: Some(layout.handle()),
            use_pipeline_libraries: desc.use_pipeline_libraries,
        })?;
        let miss_group_count = desc.miss_entries.len() as u32;
        let hit_group_count = desc.closest_hit_entries.len() as u32;
        let sbt = engine.create_shader_binding_table(ShaderBindingTableDesc {
            pipeline: pipeline.handle(),
            raygen_group: 0,
            miss_groups: (1..=miss_group_count).collect(),
            hit_groups: ((1 + miss_group_count)..(1 + miss_group_count + hit_group_count))
                .collect(),
            callable_groups: Vec::new(),
        })?;

        Ok(Self {
            pipeline,
            sbt,
            layout,
            shaders,
            miss_group_count,
            hit_group_count,
        })
    }

    pub fn trace(
        &self,
        frame: &RenderFrame,
        name: impl Into<String>,
        width: u32,
        height: u32,
        bind_groups: &[&BindGroup],
    ) -> Result<()> {
        frame.trace_rays(
            name,
            &self.pipeline,
            &self.sbt,
            width,
            height,
            1,
            bind_groups,
        )
    }

    pub fn trace_desc(&self, width: u32, height: u32) -> TraceRaysDesc {
        TraceRaysDesc {
            pipeline: self.pipeline.handle(),
            sbt: self.sbt.table(),
            width,
            height,
            depth: 1,
            indirect: None,
        }
    }

    pub fn miss_group_count(&self) -> u32 {
        self.miss_group_count
    }

    pub fn hit_group_count(&self) -> u32 {
        self.hit_group_count
    }
}

/// Owned bottom-level AS with optional scratch allocation retained for update/refit workloads.
pub struct RealtimeBlas {
    pub acceleration_structure: AccelerationStructure,
    pub scratch: Buffer,
    pub geometries: Vec<BlasGeometryDesc>,
}

impl RealtimeBlas {
    pub fn new(engine: &Engine, geometries: Vec<BlasGeometryDesc>) -> Result<Self> {
        let sizing_desc = BlasBuildDesc {
            dst: Default::default(),
            src: None,
            scratch_buffer: None,
            geometries: geometries.clone(),
            mode: AccelerationStructureBuildMode::Build,
            opacity_micromap: None,
        };
        let sizes = engine.blas_build_sizes(&sizing_desc)?;
        let acceleration_structure =
            engine.create_acceleration_structure(AccelerationStructureDesc {
                kind: AccelerationStructureKind::BottomLevel,
                size: sizes.acceleration_structure_size,
            })?;
        let scratch = engine.create_buffer(BufferDesc {
            size: sizes.build_scratch_size.max(1).saturating_add(255),
            usage: BufferUsage::STORAGE | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        Ok(Self {
            acceleration_structure,
            scratch,
            geometries,
        })
    }

    pub fn build_desc(&self) -> BlasBuildDesc {
        BlasBuildDesc {
            dst: self.acceleration_structure.handle(),
            src: None,
            scratch_buffer: Some(self.scratch.handle()),
            geometries: self.geometries.clone(),
            mode: AccelerationStructureBuildMode::Build,
            opacity_micromap: None,
        }
    }

    pub fn device_address(&self) -> Result<u64> {
        self.acceleration_structure.device_address()
    }
}

/// Packed Vulkan-compatible TLAS instance data.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RealtimeTlasInstance {
    pub transform: [f32; 12],
    pub instance_custom_index_and_mask: u32,
    pub instance_shader_binding_table_record_offset_and_flags: u32,
    pub acceleration_structure_reference: u64,
}

impl RealtimeTlasInstance {
    pub fn new(transform: [f32; 12], blas_device_address: u64) -> Self {
        Self {
            transform,
            instance_custom_index_and_mask: 0xFF << 24,
            instance_shader_binding_table_record_offset_and_flags: 0,
            acceleration_structure_reference: blas_device_address,
        }
    }

    pub fn with_custom_index_and_mask(mut self, custom_index: u32, mask: u8) -> Self {
        self.instance_custom_index_and_mask = (custom_index & 0x00FF_FFFF) | ((mask as u32) << 24);
        self
    }

    pub fn with_hit_group_offset_and_flags(mut self, hit_group_offset: u32, flags: u8) -> Self {
        self.instance_shader_binding_table_record_offset_and_flags =
            (hit_group_offset & 0x00FF_FFFF) | ((flags as u32) << 24);
        self
    }
}

/// Owned top-level AS and upload buffer for hardware ray-tracing instances.
pub struct RealtimeTlas {
    pub acceleration_structure: AccelerationStructure,
    pub instance_buffer: Buffer,
    pub scratch: Buffer,
    pub instance_count: u32,
}

impl RealtimeTlas {
    pub fn new(engine: &Engine, instances: &[RealtimeTlasInstance]) -> Result<Self> {
        if instances.is_empty() {
            return Err(Error::InvalidInput(
                "TLAS requires at least one instance".into(),
            ));
        }
        let instance_bytes = std::mem::size_of_val(instances) as u64;
        let instance_buffer = engine.create_buffer(BufferDesc {
            size: instance_bytes.max(1),
            usage: BufferUsage::COPY_DST
                | BufferUsage::STORAGE
                | BufferUsage::ACCELERATION_STRUCTURE
                | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        instance_buffer.write(0, bytemuck::cast_slice(instances))?;
        let sizing_desc = TlasBuildDesc {
            dst: Default::default(),
            src: None,
            scratch_buffer: None,
            instance_buffer: instance_buffer.handle(),
            instance_offset: 0,
            instance_count: instances.len() as u32,
            mode: AccelerationStructureBuildMode::Build,
        };
        let sizes = engine.tlas_build_sizes(&sizing_desc)?;
        let acceleration_structure =
            engine.create_acceleration_structure(AccelerationStructureDesc {
                kind: AccelerationStructureKind::TopLevel,
                size: sizes.acceleration_structure_size,
            })?;
        let scratch = engine.create_buffer(BufferDesc {
            size: sizes.build_scratch_size.max(1).saturating_add(255),
            usage: BufferUsage::STORAGE | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        Ok(Self {
            acceleration_structure,
            instance_buffer,
            scratch,
            instance_count: instances.len() as u32,
        })
    }

    pub fn build_desc(&self) -> TlasBuildDesc {
        TlasBuildDesc {
            dst: self.acceleration_structure.handle(),
            src: None,
            scratch_buffer: Some(self.scratch.handle()),
            instance_buffer: self.instance_buffer.handle(),
            instance_offset: 0,
            instance_count: self.instance_count,
            mode: AccelerationStructureBuildMode::Build,
        }
    }
}

impl Engine {
    pub fn realtime_ray_tracing_support(&self) -> RealtimeRayTracingSupport {
        let caps = self.caps();
        RealtimeRayTracingSupport {
            ray_tracing_pipeline: caps.features.ray_tracing,
            ray_query: caps.features.ray_query,
            buffer_device_address: caps.features.buffer_device_address,
            ray_tracing_position_fetch: caps.features.ray_tracing_position_fetch,
            ray_tracing_maintenance1: caps.features.ray_tracing_maintenance1,
            opacity_micromap: caps.features.opacity_micromap,
            shader_execution_reordering: caps.features.shader_execution_reordering,
            cluster_acceleration_structure: caps.features.cluster_acceleration_structure,
        }
    }
}

impl AccelerationStructure {
    pub fn device_address(&self) -> Result<u64> {
        self.device
            .acceleration_structure_device_address(self.handle)
    }
}
