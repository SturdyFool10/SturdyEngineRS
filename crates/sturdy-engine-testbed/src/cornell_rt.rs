use std::{env, path::PathBuf};

use bytemuck::{Pod, Zeroable};
use sturdy_engine::{
    BindingKind, BlasGeometryDesc, Buffer, BufferDesc, BufferUsage, Engine, Extent3d, Format,
    GraphImage, ImageDesc, ImageDimension, ImageUsage, IndexFormat, RealtimeBlas,
    RealtimeRayTracingPipeline, RealtimeRayTracingShaderDesc, RealtimeTlas, RealtimeTlasInstance,
    RenderFrame, ResourceBinding, Result, ShaderSource, StageMask, UpdateRate, VertexFormat,
};

use crate::path_tracer_camera::PathTracerCameraGpu;
use crate::path_tracer_subscene::{PathTracerSubscene, PathTracerVertex, geometry_for_subscene};

pub struct CornellRtScene {
    constants_buffer: Buffer,
    cornell: PathTracerSubsceneGpu,
    outdoor: PathTracerSubsceneGpu,
    pipeline: RealtimeRayTracingPipeline,
}

struct PathTracerSubsceneGpu {
    subscene: PathTracerSubscene,
    scene_id: u32,
    _vertex_buffer: Buffer,
    _index_buffer: Buffer,
    triangle_info_buffer: Buffer,
    blas: RealtimeBlas,
    tlas: RealtimeTlas,
}

pub struct CornellRtFrame {
    pub color: GraphImage,
    pub guide: GraphImage,
    pub material_guide: GraphImage,
    pub motion_vectors: GraphImage,
    pub normals: GraphImage,
    pub depth: GraphImage,
}

impl CornellRtScene {
    pub fn output_desc(width: u32, height: u32) -> ImageDesc {
        path_tracer_image_desc(width, height, "cornell_rt_sample", Format::Rgba32Float)
    }

    pub fn guide_desc(width: u32, height: u32) -> ImageDesc {
        path_tracer_image_desc(width, height, "cornell_rt_guide", Format::Rgba16Float)
    }

    pub fn material_guide_desc(width: u32, height: u32) -> ImageDesc {
        path_tracer_image_desc(
            width,
            height,
            "cornell_rt_material_guide",
            Format::Rgba16Float,
        )
    }

    pub fn motion_vectors_desc(width: u32, height: u32) -> ImageDesc {
        path_tracer_image_desc(
            width,
            height,
            "cornell_rt_motion_vectors",
            Format::Rgba16Float,
        )
    }

    pub fn normals_desc(width: u32, height: u32) -> ImageDesc {
        path_tracer_image_desc(width, height, "cornell_rt_normals", Format::Rgba16Float)
    }

    pub fn depth_desc(width: u32, height: u32) -> ImageDesc {
        path_tracer_image_desc(width, height, "cornell_rt_depth", Format::Rgba32Float)
    }

    pub fn new(engine: &Engine, shader_path: PathBuf) -> Result<Option<Self>> {
        if !engine.realtime_ray_tracing_support().available() {
            return Ok(None);
        }
        if env::var_os("STURDY_TESTBED_DISABLE_CORNELL_RT").is_some() {
            tracing::info!(
                "Cornell box: hardware ray tracing disabled by STURDY_TESTBED_DISABLE_CORNELL_RT"
            );
            return Ok(None);
        }

        let cornell = PathTracerSubsceneGpu::new(engine, PathTracerSubscene::CornellBox)?;
        let outdoor = PathTracerSubsceneGpu::new(engine, PathTracerSubscene::Outdoor)?;

        let constants_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<CornellRtConstants>() as u64,
            usage: BufferUsage::COPY_DST | BufferUsage::UNIFORM,
        })?;

        tracing::debug!("TLAS OK, building pipeline layout");
        let layout = engine
            .pipeline_layout()
            .binding(
                "cornell_rt",
                "output_image",
                BindingKind::StorageImage,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "scene_tlas",
                BindingKind::AccelerationStructure,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "rt_constants",
                BindingKind::UniformBuffer,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "guide_image",
                BindingKind::StorageImage,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "material_guide_image",
                BindingKind::StorageImage,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "triangle_infos",
                BindingKind::StorageBuffer,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "motion_vector_image",
                BindingKind::StorageImage,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "normal_image",
                BindingKind::StorageImage,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .binding(
                "cornell_rt",
                "depth_image",
                BindingKind::StorageImage,
                StageMask::RAY_TRACING,
                UpdateRate::Frame,
            )
            .build(engine)?;
        tracing::debug!("layout OK, compiling RT pipeline (cornell_rt.slang)");

        let pipeline = RealtimeRayTracingPipeline::new(
            engine,
            layout,
            RealtimeRayTracingShaderDesc {
                source: ShaderSource::File(shader_path),
                raygen_entry: "raygen".into(),
                miss_entries: vec!["miss".into()],
                closest_hit_entries: vec!["closest_hit".into()],
                max_recursion_depth: 2,
                use_pipeline_libraries: false,
                hit_shaders_use_ser: false,
            },
        )?;
        tracing::debug!("RT pipeline OK");

        Ok(Some(Self {
            constants_buffer,
            cornell,
            outdoor,
            pipeline,
        }))
    }

    pub fn draw(
        &self,
        frame: &RenderFrame,
        engine: &Engine,
        subscene: PathTracerSubscene,
        width: u32,
        height: u32,
        aspect: f32,
        frame_index: u32,
        camera: PathTracerCameraGpu,
        previous_camera: PathTracerCameraGpu,
    ) -> Result<CornellRtFrame> {
        let active = self.subscene_gpu(subscene);
        let cache_key = active.subscene.cache_key();
        frame.build_blas(
            format!("path_tracer_{cache_key}_static_blas"),
            active.blas.build_desc(),
        )?;
        frame.build_tlas(
            format!("path_tracer_{cache_key}_static_tlas"),
            active.tlas.build_desc(),
        )?;

        let output = frame.image("cornell_rt_sample", Self::output_desc(width, height))?;
        let guide = frame.image("cornell_rt_guide", Self::guide_desc(width, height))?;
        let material_guide = frame.image(
            "cornell_rt_material_guide",
            Self::material_guide_desc(width, height),
        )?;
        let motion_vectors = frame.image(
            "cornell_rt_motion_vectors",
            Self::motion_vectors_desc(width, height),
        )?;
        let normals = frame.image("cornell_rt_normals", Self::normals_desc(width, height))?;
        let depth = frame.image("cornell_rt_depth", Self::depth_desc(width, height))?;

        self.constants_buffer.write(
            0,
            bytemuck::bytes_of(&CornellRtConstants {
                frame_index,
                width: width.max(1),
                height: height.max(1),
                scene_id: active.scene_id,
                aspect,
                tan_half_fov_y: camera.origin[3],
                _pad0: 0,
                _pad1: 0,
                camera_origin: camera.origin,
                camera_forward: camera.forward,
                camera_right: camera.right,
                camera_up: camera.up,
                previous_camera_origin: previous_camera.origin,
                previous_camera_forward: previous_camera.forward,
                previous_camera_right: previous_camera.right,
                previous_camera_up: previous_camera.up,
            }),
        )?;
        let bind_group = engine
            .bind_group(&self.pipeline.layout)
            .entry_binding(0, 0, ResourceBinding::Image(output.handle()))?
            .acceleration_structure_binding(0, 1, &active.tlas.acceleration_structure)?
            .buffer_binding(0, 2, &self.constants_buffer)?
            .entry_binding(0, 3, ResourceBinding::Image(guide.handle()))?
            .entry_binding(0, 4, ResourceBinding::Image(material_guide.handle()))?
            .buffer_binding(0, 5, &active.triangle_info_buffer)?
            .entry_binding(0, 6, ResourceBinding::Image(motion_vectors.handle()))?
            .entry_binding(0, 7, ResourceBinding::Image(normals.handle()))?
            .entry_binding(0, 8, ResourceBinding::Image(depth.handle()))?
            .build()?;
        frame.trace_rays_with_desc_and_writes(
            format!("path_tracer_{cache_key}_hardware_trace"),
            self.pipeline.trace_desc(width.max(1), height.max(1)),
            &[&bind_group],
            &[
                &output,
                &guide,
                &material_guide,
                &motion_vectors,
                &normals,
                &depth,
            ],
        )?;
        Ok(CornellRtFrame {
            color: output,
            guide,
            material_guide,
            motion_vectors,
            normals,
            depth,
        })
    }

    fn subscene_gpu(&self, subscene: PathTracerSubscene) -> &PathTracerSubsceneGpu {
        match subscene {
            PathTracerSubscene::CornellBox => &self.cornell,
            PathTracerSubscene::Outdoor => &self.outdoor,
        }
    }
}

impl PathTracerSubsceneGpu {
    fn new(engine: &Engine, subscene: PathTracerSubscene) -> Result<Self> {
        let geometry = geometry_for_subscene(subscene);
        let scene_id = subscene.scene_id();
        debug_assert_eq!(geometry.scene_id, scene_id);
        tracing::debug!(
            subscene = subscene.label(),
            vertices = geometry.vertices.len(),
            indices = geometry.indices.len(),
            triangles = geometry.triangle_infos.len(),
            "building path tracer subscene geometry"
        );

        let vertex_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(geometry.vertices.as_slice()) as u64,
            usage: BufferUsage::COPY_DST
                | BufferUsage::VERTEX
                | BufferUsage::ACCELERATION_STRUCTURE
                | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        vertex_buffer.write(0, bytemuck::cast_slice(&geometry.vertices))?;
        let index_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(geometry.indices.as_slice()) as u64,
            usage: BufferUsage::COPY_DST
                | BufferUsage::INDEX
                | BufferUsage::ACCELERATION_STRUCTURE
                | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        index_buffer.write(0, bytemuck::cast_slice(&geometry.indices))?;
        let triangle_info_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(geometry.triangle_infos.as_slice()) as u64,
            usage: BufferUsage::COPY_DST | BufferUsage::STORAGE,
        })?;
        triangle_info_buffer.write(0, bytemuck::cast_slice(&geometry.triangle_infos))?;

        tracing::debug!(subscene = subscene.label(), "building BLAS");
        let blas = RealtimeBlas::new(
            engine,
            vec![BlasGeometryDesc {
                vertex_buffer: vertex_buffer.handle(),
                vertex_offset: 0,
                vertex_count: geometry.vertices.len() as u32,
                vertex_stride: std::mem::size_of::<PathTracerVertex>() as u32,
                vertex_format: VertexFormat::Float32x3,
                index_buffer: Some(index_buffer.handle()),
                index_offset: 0,
                index_count: geometry.indices.len() as u32,
                index_format: Some(IndexFormat::Uint32),
                transform_buffer: None,
                transform_offset: 0,
            }],
        )?;
        let blas_addr = blas.device_address()?;
        tracing::debug!(subscene = subscene.label(), blas_addr, "building TLAS");
        let tlas = RealtimeTlas::new(
            engine,
            &[RealtimeTlasInstance::new(
                identity_instance_transform(),
                blas_addr,
            )],
        )?;

        Ok(Self {
            subscene,
            scene_id,
            _vertex_buffer: vertex_buffer,
            _index_buffer: index_buffer,
            triangle_info_buffer,
            blas,
            tlas,
        })
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CornellRtConstants {
    frame_index: u32,
    width: u32,
    height: u32,
    scene_id: u32,
    aspect: f32,
    tan_half_fov_y: f32,
    _pad0: u32,
    _pad1: u32,
    camera_origin: [f32; 4],
    camera_forward: [f32; 4],
    camera_right: [f32; 4],
    camera_up: [f32; 4],
    previous_camera_origin: [f32; 4],
    previous_camera_forward: [f32; 4],
    previous_camera_right: [f32; 4],
    previous_camera_up: [f32; 4],
}

fn path_tracer_image_desc(
    width: u32,
    height: u32,
    debug_name: &'static str,
    format: Format,
) -> ImageDesc {
    ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format,
        usage: ImageUsage::SAMPLED | ImageUsage::STORAGE | ImageUsage::COPY_SRC,
        transient: false,
        clear_value: None,
        debug_name: Some(debug_name),
        ..ImageDesc::default()
    }
}

fn identity_instance_transform() -> [f32; 12] {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
}
