// Tests extracted from crates/sturdy-engine-core/src/render_graph.rs
// See scripts/extract_tests.py for the extraction logic.

use super::*;
use crate::{
    render_graph::{ConditionalRenderingDesc, copy_byte_count},
    *,
};

fn image_desc_defaults() -> ImageDesc {
    ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d::default(),
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::SAMPLED,
        transient: false,
        clear_value: None,
        debug_name: None,
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
        drm_format_modifier: None,
    }
}

#[test]
fn copy_byte_count_uses_bc_blocks() {
    let mut desc = image_desc();
    desc.format = Format::Bc4Unorm;

    assert_eq!(copy_byte_count(desc, 4, 4, 1, 1).unwrap(), 8);
    assert_eq!(copy_byte_count(desc, 5, 4, 1, 1).unwrap(), 16);
    assert_eq!(copy_byte_count(desc, 7, 9, 1, 2).unwrap(), 96);
}

fn image_desc() -> ImageDesc {
    ImageDesc {
        extent: Extent3d {
            width: 4,
            height: 4,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::SAMPLED | ImageUsage::COPY_DST | ImageUsage::COPY_SRC,
        ..image_desc_defaults()
    }
}

fn buffer_desc(size: u64) -> BufferDesc {
    BufferDesc {
        size,
        usage: BufferUsage::COPY_SRC | BufferUsage::COPY_DST,
    }
}

fn transient_image_desc() -> ImageDesc {
    ImageDesc {
        extent: Extent3d {
            width: 64,
            height: 64,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba16Float,
        usage: ImageUsage::SAMPLED
            | ImageUsage::STORAGE
            | ImageUsage::RENDER_TARGET
            | ImageUsage::COPY_DST,
        transient: true,
        debug_name: Some("transient test image"),
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
        ..image_desc_defaults()
    }
}

fn register_transient_image(graph: &mut RenderGraph, handle: ImageHandle, desc: ImageDesc) {
    graph.image_set.insert(handle);
    graph.images.push(VirtualImage {
        handle,
        desc,
        imported: false,
        first_use: u32::MAX,
        last_use: 0,
    });
}

fn register_transient_buffer(graph: &mut RenderGraph, handle: BufferHandle, desc: BufferDesc) {
    graph.buffer_set.insert(handle);
    graph.buffers.push(VirtualBuffer {
        handle,
        desc,
        imported: false,
        first_use: u32::MAX,
        last_use: 0,
    });
}

fn pass_with_work(work: PassWork) -> PassDesc {
    PassDesc {
        work,
        ..PassDesc::default_graphics("test-pass")
    }
}

#[test]
fn video_decode_pass_is_explicitly_non_executable() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(pass_with_work(PassWork::DecodeVideoFrame(
            DecodeFrameDesc {
                session: VideoSessionHandle(1),
                bitstream_buffer: BufferHandle(2),
                bitstream_offset: 0,
                bitstream_size: 128,
                output_image: ImageHandle(3),
                output_layer: 0,
            },
        )))
        .unwrap_err();

    assert!(matches!(err, Error::Unsupported(message) if message.contains("video passes")));
}

#[test]
fn video_encode_pass_is_explicitly_non_executable() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(pass_with_work(PassWork::EncodeVideoFrame(
            EncodeFrameDesc {
                session: VideoSessionHandle(1),
                input_image: ImageHandle(2),
                output_buffer: BufferHandle(3),
                output_offset: 0,
                quantization_map: None,
            },
        )))
        .unwrap_err();

    assert!(matches!(err, Error::Unsupported(message) if message.contains("video passes")));
}

#[test]
fn generated_command_execute_pass_is_explicitly_non_executable() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(pass_with_work(PassWork::ExecuteGeneratedCommands(
            DgcExecuteDesc {
                layout: IndirectCommandLayoutHandle(1),
                commands_buffer: BufferHandle(2),
                commands_offset: 0,
                max_command_count: 1,
                state_pipeline: None,
            },
        )))
        .unwrap_err();

    assert!(
        matches!(err, Error::Unsupported(message) if message.contains("device-generated command passes"))
    );
}

#[test]
fn generated_command_preprocess_pass_is_explicitly_non_executable() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(pass_with_work(PassWork::PreprocessGeneratedCommands(
            DgcPreprocessDesc {
                layout: IndirectCommandLayoutHandle(1),
                input_buffer: BufferHandle(2),
                input_offset: 0,
                output_buffer: BufferHandle(3),
                output_offset: 0,
                max_command_count: 1,
            },
        )))
        .unwrap_err();

    assert!(
        matches!(err, Error::Unsupported(message) if message.contains("device-generated command passes"))
    );
}

#[test]
fn optical_flow_estimate_pass_is_executable_when_images_are_distinct() {
    let mut graph = RenderGraph::new();
    graph
        .add_pass(pass_with_work(PassWork::EstimateOpticalFlow(
            OpticalFlowEstimateDesc {
                session: OpticalFlowSessionHandle(1),
                input_current: ImageHandle(2),
                input_previous: ImageHandle(3),
                output_motion_vectors: ImageHandle(4),
                input_hint: None,
            },
        )))
        .unwrap();
}

#[test]
fn optical_flow_estimate_rejects_overlapping_output() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(pass_with_work(PassWork::EstimateOpticalFlow(
            OpticalFlowEstimateDesc {
                session: OpticalFlowSessionHandle(1),
                input_current: ImageHandle(2),
                input_previous: ImageHandle(3),
                output_motion_vectors: ImageHandle(2),
                input_hint: None,
            },
        )))
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(message) if message.contains("output image")));
}

#[test]
fn resolve_image_pass_validates_msaa_source_and_single_sample_destination() {
    let src = ImageHandle(1);
    let dst = ImageHandle(2);
    let mut graph = RenderGraph::new();
    graph
        .import_image(
            src,
            ImageDesc {
                samples: 4,
                usage: ImageUsage::COPY_SRC | ImageUsage::RENDER_TARGET,
                ..image_desc()
            },
        )
        .unwrap();
    graph
        .import_image(
            dst,
            ImageDesc {
                samples: 1,
                usage: ImageUsage::COPY_DST | ImageUsage::SAMPLED,
                ..image_desc()
            },
        )
        .unwrap();

    graph
        .add_pass(PassDesc {
            name: "resolve-msaa".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::ResolveImage(ResolveImageDesc {
                src,
                dst,
                src_mip_level: 0,
                dst_mip_level: 0,
                src_base_layer: 0,
                dst_base_layer: 0,
                layer_count: 1,
                width: 4,
                height: 4,
            }),
            reads: vec![ImageUse {
                image: src,
                access: Access::Read,
                state: RgState::CopySrc,
                subresource: SubresourceRange::WHOLE,
            }],
            writes: vec![ImageUse {
                image: dst,
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: SubresourceRange::WHOLE,
            }],
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();
    assert_eq!(
        compiled.passes[0].work,
        PassWork::ResolveImage(ResolveImageDesc {
            src,
            dst,
            src_mip_level: 0,
            dst_mip_level: 0,
            src_base_layer: 0,
            dst_base_layer: 0,
            layer_count: 1,
            width: 4,
            height: 4,
        })
    );
}

#[test]
fn resolve_image_pass_rejects_single_sample_source() {
    let src = ImageHandle(1);
    let dst = ImageHandle(2);
    let mut graph = RenderGraph::new();
    graph.import_image(src, image_desc()).unwrap();
    graph.import_image(dst, image_desc()).unwrap();

    let err = graph
        .add_pass(PassDesc {
            name: "bad-resolve".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::ResolveImage(ResolveImageDesc {
                src,
                dst,
                src_mip_level: 0,
                dst_mip_level: 0,
                src_base_layer: 0,
                dst_base_layer: 0,
                layer_count: 1,
                width: 4,
                height: 4,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();

    assert!(format!("{err}").contains("source image must have more than one sample"));
}

#[test]
fn copy_buffer_to_image_pass_compiles_barriers() {
    let image = ImageHandle(1);
    let buffer = BufferHandle(2);
    let mut graph = RenderGraph::new();
    graph.import_image(image, image_desc()).unwrap();
    graph.import_buffer(buffer, buffer_desc(64)).unwrap();

    graph
        .add_pass(PassDesc {
            name: "upload-texture".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                buffer,
                image,
                buffer_offset: 0,
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: 4,
                height: 4,
                depth: 1,
            }),
            reads: Vec::new(),
            writes: vec![ImageUse {
                image,
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            buffer_reads: vec![BufferUse {
                buffer,
                access: Access::Read,
                state: RgState::CopySrc,
                offset: 0,
                size: 64,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.passes.len(), 1);
    assert_eq!(compiled.barriers_per_pass[0].len(), 1);
    assert_eq!(compiled.barriers_per_pass[0][0].after, RgState::CopyDst);
    assert_eq!(compiled.buffer_barriers_per_pass[0].len(), 1);
    assert_eq!(
        compiled.buffer_barriers_per_pass[0][0].after,
        RgState::CopySrc
    );
}

#[test]
fn copy_buffer_to_image_rejects_short_buffer() {
    let image = ImageHandle(1);
    let buffer = BufferHandle(2);
    let mut graph = RenderGraph::new();
    graph.import_image(image, image_desc()).unwrap();
    graph.import_buffer(buffer, buffer_desc(63)).unwrap();

    let err = graph
        .add_pass(PassDesc {
            name: "upload-texture".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                buffer,
                image,
                buffer_offset: 0,
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: 4,
                height: 4,
                depth: 1,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn push_constants_require_aligned_byte_ranges() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "draw-with-bad-push".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: Some(PipelineHandle(1)),
            bind_groups: Vec::new(),
            push_constants: Some(PushConstants {
                offset: 2,
                stages: crate::StageMask::VERTEX,
                bytes: vec![0, 1, 2, 3],
            }),
            pipeline_shading_rate: None,
            work: PassWork::Draw(DrawDesc {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer: None,
                index_buffer: None,
                viewport: None,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn push_constants_require_pipeline() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "push-without-pipeline".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: Some(PushConstants {
                offset: 0,
                stages: crate::StageMask::VERTEX,
                bytes: vec![0, 1, 2, 3],
            }),
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn dispatch_accepts_shader_object_binding_without_pipeline() {
    let mut graph = RenderGraph::new();
    graph
        .add_pass(PassDesc {
            name: "dispatch-shader-object".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::Dispatch(DispatchDesc { x: 1, y: 1, z: 1 }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: Some(ShaderBinding::ShaderObjects(vec![
                crate::shader_object::ShaderObjectHandle(1),
            ])),
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();
}

#[test]
fn shader_object_binding_rejects_empty_list() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "empty-shader-objects".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::Dispatch(DispatchDesc { x: 1, y: 1, z: 1 }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: Some(ShaderBinding::ShaderObjects(Vec::new())),
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn graphics_shader_objects_require_pipeline_state_anchor() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "draw-shader-object-without-anchor".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::Draw(DrawDesc {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer: None,
                index_buffer: None,
                viewport: None,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: Some(ShaderBinding::ShaderObjects(vec![
                crate::shader_object::ShaderObjectHandle(1),
            ])),
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn pipeline_shading_rate_requires_graphics_work() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "bad-vrs-compute".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: Some(PipelineHandle(1)),
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: Some(ShadingRate::Rate2x2),
            work: PassWork::Dispatch(DispatchDesc { x: 1, y: 1, z: 1 }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("pipeline shading rate"));
}

#[test]
fn conditional_rendering_predicate_requires_aligned_offset() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "bad-predicate-offset".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: Some(ConditionalRenderingDesc {
                buffer: BufferHandle(1),
                offset: 2,
                inverted: false,
            }),
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("4-byte aligned"));
}

#[test]
fn push_descriptors_require_pipeline_and_nonempty_bindings() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "bad-push-descriptor".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: Some(PushDescriptorSetDesc {
                set: 0,
                bindings: Vec::new(),
            }),
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("push descriptors"));
}

#[test]
fn draw_indirect_count_requires_valid_count_contract() {
    let mut graph = RenderGraph::new();
    let indirect = BufferHandle(1);
    let count = BufferHandle(2);
    graph
        .import_buffer(
            indirect,
            BufferDesc {
                size: 64,
                usage: BufferUsage::INDIRECT,
            },
        )
        .unwrap();
    graph
        .import_buffer(
            count,
            BufferDesc {
                size: 4,
                usage: BufferUsage::INDIRECT,
            },
        )
        .unwrap();

    let err = graph
        .add_pass(PassDesc {
            name: "bad-indirect-count".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: Some(PipelineHandle(1)),
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::DrawIndirectCount(DrawIndirectCountDesc {
                indirect_buffer: indirect,
                indirect_offset: 0,
                count_buffer: count,
                count_offset: 0,
                max_draw_count: 0,
                stride: 20,
                indexed: true,
                vertex_buffer: None,
                index_buffer: None,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn blas_build_allows_backend_allocated_scratch() {
    let vertex_buffer = BufferHandle(1);
    let dst_as = AccelerationStructureHandle(1);
    let mut graph = RenderGraph::new();

    graph
        .add_pass(PassDesc {
            name: "build-blas".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::BuildBlas(BlasBuildDesc {
                opacity_micromap: None,
                dst: dst_as,
                src: None,
                scratch_buffer: None,
                geometries: vec![BlasGeometryDesc {
                    vertex_buffer,
                    vertex_offset: 0,
                    vertex_count: 3,
                    vertex_stride: 12,
                    vertex_format: VertexFormat::Float32x3,
                    index_buffer: None,
                    index_offset: 0,
                    index_count: 0,
                    index_format: None,
                    transform_buffer: None,
                    transform_offset: 0,
                }],
                mode: AccelerationStructureBuildMode::Build,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();
}

#[test]
fn blas_compaction_requires_source_as() {
    let mut graph = RenderGraph::new();
    let err = graph
        .add_pass(PassDesc {
            name: "compact-blas".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::BuildBlas(BlasBuildDesc {
                opacity_micromap: None,
                dst: AccelerationStructureHandle(2),
                src: None,
                scratch_buffer: None,
                geometries: Vec::new(),
                mode: AccelerationStructureBuildMode::Compact,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap_err();

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("source acceleration structure"));
}

#[test]
fn draw_indirect_count_tracks_indirect_and_count_buffer_reads() {
    let mut graph = RenderGraph::new();
    let indirect = BufferHandle(1);
    let count = BufferHandle(2);
    let buffer_desc = BufferDesc {
        size: 64,
        usage: BufferUsage::INDIRECT | BufferUsage::STORAGE,
    };
    graph.import_buffer(indirect, buffer_desc).unwrap();
    graph.import_buffer(count, buffer_desc).unwrap();

    graph
        .add_pass(PassDesc {
            name: "write-count".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: vec![BufferUse {
                buffer: count,
                access: Access::Write,
                state: RgState::ShaderWrite,
                offset: 0,
                size: 4,
            }],
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    graph
        .add_pass(PassDesc {
            name: "consume-count".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: Some(PipelineHandle(2)),
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::DrawIndirectCount(DrawIndirectCountDesc {
                indirect_buffer: indirect,
                indirect_offset: 0,
                count_buffer: count,
                count_offset: 0,
                max_draw_count: 3,
                stride: 20,
                indexed: false,
                vertex_buffer: None,
                index_buffer: None,
            }),
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: vec![
                BufferUse {
                    buffer: indirect,
                    access: Access::Read,
                    state: RgState::IndirectRead,
                    offset: 0,
                    size: u64::MAX,
                },
                BufferUse {
                    buffer: count,
                    access: Access::Read,
                    state: RgState::IndirectRead,
                    offset: 0,
                    size: 4,
                },
            ],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();
    assert_eq!(compiled.passes[0].name, "write-count");
    assert_eq!(compiled.passes[1].name, "consume-count");
    assert!(
        compiled.buffer_barriers_per_pass[1]
            .iter()
            .any(|barrier| barrier.buffer == count
                && barrier.before == RgState::ShaderWrite
                && barrier.after == RgState::IndirectRead)
    );
}

#[test]
fn imported_image_uses_initial_state_for_first_barrier() {
    let image = ImageHandle(1);
    let mut graph = RenderGraph::new();
    graph.import_image(image, image_desc()).unwrap();
    graph.set_initial_image_state(image, RgState::ShaderRead);
    graph
        .add_pass(PassDesc {
            name: "read-texture".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: vec![ImageUse {
                image,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();
    assert!(compiled.barriers_per_pass[0].is_empty());
    assert!(compiled.final_image_states.contains(&(
        ImageStateKey {
            image,
            subresource: SubresourceRange {
                base_mip: 0,
                mip_count: 1,
                base_layer: 0,
                layer_count: 1,
            },
        },
        RgState::ShaderRead
    )));
}

#[test]
fn imported_image_tracks_distinct_subresource_states() {
    let image = ImageHandle(1);
    let mut graph = RenderGraph::new();
    let desc = ImageDesc {
        mip_levels: 2,
        ..image_desc()
    };
    graph.import_image(image, desc).unwrap();
    graph.set_initial_image_subresource_state(
        image,
        SubresourceRange {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        },
        RgState::ShaderRead,
    );
    graph.set_initial_image_subresource_state(
        image,
        SubresourceRange {
            base_mip: 1,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        },
        RgState::CopyDst,
    );
    graph
        .add_pass(PassDesc {
            name: "use-mips".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: vec![ImageUse {
                image,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: SubresourceRange {
                    base_mip: 0,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            writes: vec![ImageUse {
                image,
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: SubresourceRange {
                    base_mip: 1,
                    mip_count: 1,
                    base_layer: 0,
                    layer_count: 1,
                },
            }],
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();
    assert!(compiled.barriers_per_pass[0].is_empty());
    assert_eq!(compiled.final_image_states.len(), 2);
}

#[test]
fn queue_changes_emit_ownership_barriers() {
    let image = ImageHandle(1);
    let buffer = BufferHandle(2);
    let mut graph = RenderGraph::new();
    graph.import_image(image, image_desc()).unwrap();
    graph.import_buffer(buffer, buffer_desc(64)).unwrap();

    graph
        .add_pass(PassDesc {
            name: "upload".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: vec![ImageUse {
                image,
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: SubresourceRange::WHOLE,
            }],
            buffer_reads: Vec::new(),
            buffer_writes: vec![BufferUse {
                buffer,
                access: Access::Write,
                state: RgState::CopyDst,
                offset: 0,
                size: 64,
            }],
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();
    graph
        .add_pass(PassDesc {
            name: "sample".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: vec![ImageUse {
                image,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource: SubresourceRange::WHOLE,
            }],
            writes: Vec::new(),
            buffer_reads: vec![BufferUse {
                buffer,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: 64,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();
    let image_barrier = compiled.barriers_per_pass[1][0];
    assert_eq!(image_barrier.before_queue, QueueType::Transfer);
    assert_eq!(image_barrier.after_queue, QueueType::Graphics);
    assert_eq!(image_barrier.before, RgState::CopyDst);
    assert_eq!(image_barrier.after, RgState::ShaderRead);

    let buffer_barrier = compiled.buffer_barriers_per_pass[1][0];
    assert_eq!(buffer_barrier.before_queue, QueueType::Transfer);
    assert_eq!(buffer_barrier.after_queue, QueueType::Graphics);
    assert_eq!(buffer_barrier.before, RgState::CopyDst);
    assert_eq!(buffer_barrier.after, RgState::ShaderRead);
}

#[test]
fn independent_queue_batches_compile_without_cross_batch_barriers() {
    let compute_buffer = BufferHandle(1);
    let transfer_image = ImageHandle(2);
    let transfer_buffer = BufferHandle(3);
    let graphics_image = ImageHandle(4);
    let mut graph = RenderGraph::new();
    graph
        .import_buffer(compute_buffer, buffer_desc(64))
        .unwrap();
    graph.import_image(transfer_image, image_desc()).unwrap();
    graph
        .import_buffer(transfer_buffer, buffer_desc(64))
        .unwrap();
    graph.import_image(graphics_image, image_desc()).unwrap();

    graph
        .add_pass(PassDesc {
            name: "compute-independent".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: Vec::new(),
            buffer_reads: Vec::new(),
            buffer_writes: vec![BufferUse {
                buffer: compute_buffer,
                access: Access::Write,
                state: RgState::ShaderWrite,
                offset: 0,
                size: 64,
            }],
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();
    graph
        .add_pass(PassDesc {
            name: "transfer-independent".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: transfer_image,
                access: Access::Write,
                state: RgState::CopyDst,
                subresource: SubresourceRange::WHOLE,
            }],
            buffer_reads: vec![BufferUse {
                buffer: transfer_buffer,
                access: Access::Read,
                state: RgState::CopySrc,
                offset: 0,
                size: 64,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();
    graph
        .add_pass(PassDesc {
            name: "graphics-independent".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::None,
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: graphics_image,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource: SubresourceRange::WHOLE,
            }],
            buffer_reads: Vec::new(),
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();
    assert_eq!(
        compiled.batches,
        vec![
            RecordBatch {
                queue: QueueType::Graphics,
                pass_indices: vec![0],
            },
            RecordBatch {
                queue: QueueType::Transfer,
                pass_indices: vec![1],
            },
            RecordBatch {
                queue: QueueType::Compute,
                pass_indices: vec![2],
            },
        ]
    );
    for (pass, barriers) in compiled.passes.iter().zip(&compiled.barriers_per_pass) {
        for barrier in barriers {
            assert_eq!(barrier.before_queue, pass.queue);
            assert_eq!(barrier.after_queue, pass.queue);
        }
    }
    for (pass, barriers) in compiled
        .passes
        .iter()
        .zip(&compiled.buffer_barriers_per_pass)
    {
        for barrier in barriers {
            assert_eq!(barrier.before_queue, pass.queue);
            assert_eq!(barrier.after_queue, pass.queue);
        }
    }
}

#[test]
fn showcase_upload_push_constants_multi_queue_and_aliasing_plan() {
    let staging = BufferHandle(1);
    let uploaded = ImageHandle(10);
    let gbuffer = ImageHandle(11);
    let lighting = ImageHandle(12);
    let postprocess = ImageHandle(13);
    let scratch_a = BufferHandle(20);
    let scratch_b = BufferHandle(21);

    let image_desc = transient_image_desc();
    let scratch_desc = BufferDesc {
        size: 4096,
        usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
    };
    let subresource = SubresourceRange {
        base_mip: 0,
        mip_count: 1,
        base_layer: 0,
        layer_count: 1,
    };
    let mut graph = RenderGraph::new();
    graph
        .import_buffer(staging, buffer_desc(64 * 64 * 8))
        .unwrap();
    for image in [uploaded, gbuffer, lighting, postprocess] {
        register_transient_image(&mut graph, image, image_desc);
    }
    for buffer in [scratch_a, scratch_b] {
        register_transient_buffer(&mut graph, buffer, scratch_desc);
    }

    graph
        .add_pass(PassDesc {
            name: "upload-material-texture".into(),
            queue: QueueType::Transfer,
            shader: None,
            pipeline: None,
            bind_groups: Vec::new(),
            push_constants: None,
            pipeline_shading_rate: None,
            work: PassWork::CopyBufferToImage(CopyBufferToImageDesc {
                buffer: staging,
                image: uploaded,
                buffer_offset: 0,
                mip_level: 0,
                base_layer: 0,
                layer_count: 1,
                width: 64,
                height: 64,
                depth: 1,
            }),
            reads: Vec::new(),
            writes: vec![ImageUse {
                image: uploaded,
                access: Access::Write,
                state: RgState::CopyDst,
                subresource,
            }],
            buffer_reads: vec![BufferUse {
                buffer: staging,
                access: Access::Read,
                state: RgState::CopySrc,
                offset: 0,
                size: 64 * 64 * 8,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    graph
        .add_pass(PassDesc {
            name: "gbuffer-draw-with-push-constants".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: Some(PipelineHandle(1)),
            bind_groups: Vec::new(),
            push_constants: Some(PushConstants {
                offset: 0,
                stages: crate::StageMask::VERTEX | crate::StageMask::FRAGMENT,
                bytes: vec![0x11; 16],
            }),
            pipeline_shading_rate: None,
            work: PassWork::Draw(DrawDesc {
                vertex_count: 3,
                instance_count: 2,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer: None,
                index_buffer: None,
                viewport: None,
            }),
            reads: vec![ImageUse {
                image: uploaded,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            }],
            writes: vec![ImageUse {
                image: gbuffer,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource,
            }],
            buffer_reads: Vec::new(),
            buffer_writes: vec![BufferUse {
                buffer: scratch_a,
                access: Access::Write,
                state: RgState::ShaderWrite,
                offset: 0,
                size: scratch_desc.size,
            }],
            clear_colors: vec![(gbuffer, [0, 0, 0, f32::to_bits(1.0)])],
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    graph
        .add_pass(PassDesc {
            name: "compute-lighting".into(),
            queue: QueueType::Compute,
            shader: None,
            pipeline: Some(PipelineHandle(2)),
            bind_groups: Vec::new(),
            push_constants: Some(PushConstants {
                offset: 16,
                stages: crate::StageMask::COMPUTE,
                bytes: vec![0x22; 16],
            }),
            pipeline_shading_rate: None,
            work: PassWork::Dispatch(DispatchDesc { x: 8, y: 8, z: 1 }),
            reads: vec![ImageUse {
                image: gbuffer,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            }],
            writes: vec![ImageUse {
                image: lighting,
                access: Access::Write,
                state: RgState::ShaderWrite,
                subresource,
            }],
            buffer_reads: vec![BufferUse {
                buffer: scratch_a,
                access: Access::Read,
                state: RgState::ShaderRead,
                offset: 0,
                size: scratch_desc.size,
            }],
            buffer_writes: Vec::new(),
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    graph
        .add_pass(PassDesc {
            name: "postprocess-and-presentable-output".into(),
            queue: QueueType::Graphics,
            shader: None,
            pipeline: Some(PipelineHandle(3)),
            bind_groups: Vec::new(),
            push_constants: Some(PushConstants {
                offset: 0,
                stages: crate::StageMask::FRAGMENT,
                bytes: vec![0x33; 16],
            }),
            pipeline_shading_rate: None,
            work: PassWork::Draw(DrawDesc {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
                vertex_buffer: None,
                index_buffer: None,
                viewport: None,
            }),
            reads: vec![ImageUse {
                image: lighting,
                access: Access::Read,
                state: RgState::ShaderRead,
                subresource,
            }],
            writes: vec![ImageUse {
                image: postprocess,
                access: Access::Write,
                state: RgState::RenderTarget,
                subresource,
            }],
            buffer_reads: Vec::new(),
            buffer_writes: vec![BufferUse {
                buffer: scratch_b,
                access: Access::Write,
                state: RgState::ShaderWrite,
                offset: 0,
                size: scratch_desc.size,
            }],
            clear_colors: Vec::new(),
            clear_depth: None,
            push_descriptor_set: None,
            predicate: None,
            shader_binding: None,
            shading_rate_image: None,
            perf_counters: None,
        })
        .unwrap();

    let compiled = graph.compile().unwrap();

    assert_eq!(compiled.passes.len(), 4);
    assert_eq!(
        compiled.batches,
        vec![
            RecordBatch {
                queue: QueueType::Transfer,
                pass_indices: vec![0],
            },
            RecordBatch {
                queue: QueueType::Graphics,
                pass_indices: vec![1],
            },
            RecordBatch {
                queue: QueueType::Compute,
                pass_indices: vec![2],
            },
            RecordBatch {
                queue: QueueType::Graphics,
                pass_indices: vec![3],
            },
        ]
    );

    assert_eq!(
        compiled.passes[1].push_constants.as_ref().unwrap().stages,
        crate::StageMask::VERTEX | crate::StageMask::FRAGMENT
    );
    assert_eq!(
        compiled.passes[2].push_constants.as_ref().unwrap().offset,
        16
    );

    let upload_to_draw = compiled.barriers_per_pass[1]
        .iter()
        .find(|barrier| barrier.image == uploaded)
        .expect("uploaded image transitions from transfer upload to graphics sampling");
    assert_eq!(upload_to_draw.before_queue, QueueType::Transfer);
    assert_eq!(upload_to_draw.after_queue, QueueType::Graphics);
    assert_eq!(upload_to_draw.before, RgState::CopyDst);
    assert_eq!(upload_to_draw.after, RgState::ShaderRead);

    let draw_to_compute = compiled.barriers_per_pass[2]
        .iter()
        .find(|barrier| barrier.image == gbuffer)
        .expect("gbuffer transitions from graphics render target to compute input");
    assert_eq!(draw_to_compute.before_queue, QueueType::Graphics);
    assert_eq!(draw_to_compute.after_queue, QueueType::Compute);
    assert_eq!(draw_to_compute.before, RgState::RenderTarget);
    assert_eq!(draw_to_compute.after, RgState::ShaderRead);

    let compute_to_post = compiled.barriers_per_pass[3]
        .iter()
        .find(|barrier| barrier.image == lighting)
        .expect("lighting image transitions from compute output to graphics sampling");
    assert_eq!(compute_to_post.before_queue, QueueType::Compute);
    assert_eq!(compute_to_post.after_queue, QueueType::Graphics);
    assert_eq!(compute_to_post.before, RgState::ShaderWrite);
    assert_eq!(compute_to_post.after, RgState::ShaderRead);

    assert_eq!(compiled.alias_plan.transient_image_count, 4);
    assert_eq!(compiled.alias_plan.transient_buffer_count, 2);
    assert!(compiled.alias_plan.image_slot_count < 4);
    assert_eq!(compiled.alias_plan.buffer_slot_count, 1);
    assert!(compiled.alias_plan.image_savings_bytes > 0);
    assert_eq!(compiled.alias_plan.buffer_savings_bytes, scratch_desc.size);
    assert!(compiled.alias_plan.total_savings_bytes() > scratch_desc.size);

    // Cross-queue release barriers: for every acquire barrier (before_queue != after_queue),
    // a matching release barrier must exist in the source batch.
    for (pass_idx, barriers) in compiled.barriers_per_pass.iter().enumerate() {
        for barrier in barriers {
            if barrier.before_queue == barrier.after_queue {
                continue;
            }
            // Find the batch that contains this pass (the acquire side).
            let acquire_batch = compiled.batches.iter().enumerate()
                .find(|(_, batch)| batch.pass_indices.contains(&(pass_idx as u32)))
                .map(|(i, _)| i)
                .expect("every pass belongs to a batch");
            // Find the source batch (before_queue).
            let source_batch = compiled.batches.iter().enumerate()
                .rev()
                .find(|(_, batch)| {
                    batch.queue == barrier.before_queue
                        && batch.pass_indices.iter().any(|&pi| pi < pass_idx as u32)
                })
                .map(|(i, _)| i);
            if let Some(src) = source_batch {
                // Release barrier must exist in source batch.
                let release_bufs = &compiled.release_buffer_barriers_per_batch[src];
                let release_imgs = &compiled.release_image_barriers_per_batch[src];
                // The acquire barrier is for an image — check image releases.
                let has_release = release_imgs.iter().any(|r| {
                    r.image == barrier.image
                        && r.before_queue == barrier.before_queue
                        && r.after_queue == barrier.after_queue
                });
                assert!(
                    has_release,
                    "acquire barrier for image {:?} from {:?} to {:?} at pass {pass_idx} \
                     must have a matching release barrier in source batch {src}",
                    barrier.image, barrier.before_queue, barrier.after_queue
                );
                let _ = (acquire_batch, release_bufs);
            }
        }
    }
}
