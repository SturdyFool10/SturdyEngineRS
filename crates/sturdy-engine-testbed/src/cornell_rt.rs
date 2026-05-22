use std::{env, path::PathBuf};

use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Vec3};
use sturdy_engine::{
    BindingKind, BlasGeometryDesc, Buffer, BufferDesc, BufferUsage, Engine, Extent3d, Format,
    GraphImage, ImageDesc, ImageDimension, ImageUsage, IndexFormat, RealtimeBlas,
    RealtimeRayTracingPipeline, RealtimeRayTracingShaderDesc, RealtimeTlas, RealtimeTlasInstance,
    RenderFrame, ResourceBinding, Result, ShaderSource, StageMask, UpdateRate, VertexFormat,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CornellVertex {
    position: [f32; 3],
}

pub struct CornellRtScene {
    _vertex_buffer: Buffer,
    _index_buffer: Buffer,
    constants_buffer: Buffer,
    blas: RealtimeBlas,
    tlas: RealtimeTlas,
    pipeline: RealtimeRayTracingPipeline,
}

impl CornellRtScene {
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

        let (vertices, indices) = cornell_geometry();
        let vertex_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(vertices.as_slice()) as u64,
            usage: BufferUsage::COPY_DST
                | BufferUsage::VERTEX
                | BufferUsage::ACCELERATION_STRUCTURE
                | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        vertex_buffer.write(0, bytemuck::cast_slice(&vertices))?;
        let index_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of_val(indices.as_slice()) as u64,
            usage: BufferUsage::COPY_DST
                | BufferUsage::INDEX
                | BufferUsage::ACCELERATION_STRUCTURE
                | BufferUsage::SHADER_DEVICE_ADDRESS,
        })?;
        index_buffer.write(0, bytemuck::cast_slice(&indices))?;

        let constants_buffer = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<CornellRtConstants>() as u64,
            usage: BufferUsage::COPY_DST | BufferUsage::UNIFORM,
        })?;

        tracing::debug!(
            "building BLAS ({} verts, {} indices)",
            vertices.len(),
            indices.len()
        );
        let blas = RealtimeBlas::new(
            engine,
            vec![BlasGeometryDesc {
                vertex_buffer: vertex_buffer.handle(),
                vertex_offset: 0,
                vertex_count: vertices.len() as u32,
                vertex_stride: std::mem::size_of::<CornellVertex>() as u32,
                vertex_format: VertexFormat::Float32x3,
                index_buffer: Some(index_buffer.handle()),
                index_offset: 0,
                index_count: indices.len() as u32,
                index_format: Some(IndexFormat::Uint32),
                transform_buffer: None,
                transform_offset: 0,
            }],
        )?;
        tracing::debug!("BLAS OK, querying device address");
        let blas_addr = blas.device_address()?;
        tracing::debug!("BLAS address={blas_addr:#x}, building TLAS");
        let tlas = RealtimeTlas::new(
            engine,
            &[RealtimeTlasInstance::new(
                identity_instance_transform(),
                blas_addr,
            )],
        )?;
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
            _vertex_buffer: vertex_buffer,
            _index_buffer: index_buffer,
            constants_buffer,
            blas,
            tlas,
            pipeline,
        }))
    }

    pub fn draw(
        &self,
        frame: &RenderFrame,
        engine: &Engine,
        width: u32,
        height: u32,
        aspect: f32,
        frame_index: u32,
    ) -> Result<GraphImage> {
        frame.build_blas("cornell_static_blas", self.blas.build_desc())?;
        frame.build_tlas("cornell_static_tlas", self.tlas.build_desc())?;

        let output = frame.image(
            "cornell_rt_sample",
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
                format: Format::Rgba16Float,
                usage: ImageUsage::SAMPLED | ImageUsage::STORAGE | ImageUsage::COPY_SRC,
                transient: false,
                clear_value: None,
                debug_name: Some("cornell_rt_sample"),
                ..ImageDesc::default()
            },
        )?;

        self.constants_buffer.write(
            0,
            bytemuck::bytes_of(&CornellRtConstants {
                frame_index,
                width: width.max(1),
                height: height.max(1),
                aspect,
            }),
        )?;
        let bind_group = engine
            .bind_group(&self.pipeline.layout)
            .entry_binding(0, 0, ResourceBinding::Image(output.handle()))?
            .acceleration_structure_binding(0, 1, &self.tlas.acceleration_structure)?
            .buffer_binding(0, 2, &self.constants_buffer)?
            .build()?;
        frame.trace_rays_with_desc_and_writes(
            "cornell_hardware_path_trace",
            self.pipeline.trace_desc(width.max(1), height.max(1)),
            &[&bind_group],
            &[&output],
        )?;
        Ok(output)
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct CornellRtConstants {
    frame_index: u32,
    width: u32,
    height: u32,
    aspect: f32,
}

fn identity_instance_transform() -> [f32; 12] {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
}

fn cornell_geometry() -> (Vec<CornellVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    push_quad(
        &mut vertices,
        &mut indices,
        v(-1.0, 0.0, 0.0),
        v(1.0, 0.0, 0.0),
        v(1.0, 0.0, 2.0),
        v(-1.0, 0.0, 2.0),
    );
    push_quad(
        &mut vertices,
        &mut indices,
        v(-1.0, 2.0, 2.0),
        v(1.0, 2.0, 2.0),
        v(1.0, 2.0, 0.0),
        v(-1.0, 2.0, 0.0),
    );
    push_quad(
        &mut vertices,
        &mut indices,
        v(-1.0, 0.0, 2.0),
        v(1.0, 0.0, 2.0),
        v(1.0, 2.0, 2.0),
        v(-1.0, 2.0, 2.0),
    );
    push_quad(
        &mut vertices,
        &mut indices,
        v(-1.0, 0.0, 0.0),
        v(-1.0, 0.0, 2.0),
        v(-1.0, 2.0, 2.0),
        v(-1.0, 2.0, 0.0),
    );
    push_quad(
        &mut vertices,
        &mut indices,
        v(1.0, 0.0, 2.0),
        v(1.0, 0.0, 0.0),
        v(1.0, 2.0, 0.0),
        v(1.0, 2.0, 2.0),
    );
    push_quad(
        &mut vertices,
        &mut indices,
        v(-0.32, 1.998, 0.78),
        v(0.32, 1.998, 0.78),
        v(0.32, 1.998, 1.36),
        v(-0.32, 1.998, 1.36),
    );

    push_rotated_box(
        &mut vertices,
        &mut indices,
        Vec3::new(-0.26, 0.36, 0.77),
        Vec3::new(0.21, 0.36, 0.21),
        45.0_f32.to_radians(),
    );
    push_rotated_box(
        &mut vertices,
        &mut indices,
        Vec3::new(0.445, 0.59, 1.06),
        Vec3::new(0.225, 0.59, 0.24),
        45.0_f32.to_radians(),
    );

    (vertices, indices)
}

fn push_rotated_box(
    vertices: &mut Vec<CornellVertex>,
    indices: &mut Vec<u32>,
    original_center: Vec3,
    half_extent: Vec3,
    y_rotation: f32,
) {
    let rot = Mat3::from_rotation_y(y_rotation);
    let lifted_center = Vec3::new(
        original_center.x,
        rotated_floor_lift(half_extent, rot),
        original_center.z,
    );
    let corner = |sx: f32, sy: f32, sz: f32| {
        lifted_center + rot * Vec3::new(sx * half_extent.x, sy * half_extent.y, sz * half_extent.z)
    };

    let nnn = corner(-1.0, -1.0, -1.0);
    let nnp = corner(-1.0, -1.0, 1.0);
    let npn = corner(-1.0, 1.0, -1.0);
    let npp = corner(-1.0, 1.0, 1.0);
    let pnn = corner(1.0, -1.0, -1.0);
    let pnp = corner(1.0, -1.0, 1.0);
    let ppn = corner(1.0, 1.0, -1.0);
    let ppp = corner(1.0, 1.0, 1.0);

    push_quad(vertices, indices, nnp, nnn, npn, npp);
    push_quad(vertices, indices, pnn, pnp, ppp, ppn);
    push_quad(vertices, indices, nnn, pnn, ppn, npn);
    push_quad(vertices, indices, pnp, nnp, npp, ppp);
    push_quad(vertices, indices, nnn, nnp, pnp, pnn);
    push_quad(vertices, indices, npn, ppn, ppp, npp);
}

fn rotated_floor_lift(half_extent: Vec3, rot: Mat3) -> f32 {
    let mut min_y = f32::MAX;
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                min_y = min_y.min(
                    (rot * Vec3::new(sx * half_extent.x, sy * half_extent.y, sz * half_extent.z)).y,
                );
            }
        }
    }
    -min_y
}

fn push_quad(
    vertices: &mut Vec<CornellVertex>,
    indices: &mut Vec<u32>,
    a: Vec3,
    b: Vec3,
    c: Vec3,
    d: Vec3,
) {
    let base = vertices.len() as u32;
    vertices.extend([
        CornellVertex {
            position: a.to_array(),
        },
        CornellVertex {
            position: b.to_array(),
        },
        CornellVertex {
            position: c.to_array(),
        },
        CornellVertex {
            position: d.to_array(),
        },
    ]);
    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}
