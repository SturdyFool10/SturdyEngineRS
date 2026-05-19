// Tests extracted from crates/sturdy-engine-core/src/device.rs
// Runtime code should stay separate from test code.

use super::*;
use crate::SurfaceColorSpace;

#[test]
fn surface_events_capture_resize_format_color_space_and_recreation() {
    let old = SurfaceInfo {
        size: SurfaceSize {
            width: 640,
            height: 360,
        },
        format: crate::Format::Bgra8Unorm,
        color_space: SurfaceColorSpace::SrgbNonlinear,
    };
    let new = SurfaceInfo {
        size: SurfaceSize {
            width: 1280,
            height: 720,
        },
        format: crate::Format::Rgba16Float,
        color_space: SurfaceColorSpace::Hdr10St2084,
    };
    let mut events = Vec::new();

    queue_surface_events(&mut events, old, new);

    assert_eq!(
        events,
        vec![
            SurfaceEvent::Resized {
                old: old.size,
                new: new.size,
            },
            SurfaceEvent::FormatChanged {
                old: old.format,
                new: new.format,
            },
            SurfaceEvent::ColorSpaceChanged {
                old: old.color_space,
                new: new.color_space,
            },
            SurfaceEvent::Recreated { old, new },
        ]
    );
}

#[test]
fn surface_events_always_capture_recreation() {
    let info = SurfaceInfo {
        size: SurfaceSize {
            width: 640,
            height: 360,
        },
        format: crate::Format::Bgra8Unorm,
        color_space: SurfaceColorSpace::SrgbNonlinear,
    };
    let mut events = Vec::new();

    queue_surface_events(&mut events, info, info);

    assert_eq!(
        events,
        vec![SurfaceEvent::Recreated {
            old: info,
            new: info
        }]
    );
}

#[test]
fn sample_count_validation_rejects_non_msaa_counts_and_device_overflow() {
    assert!(validate_sample_count(4, 8, "test image").is_ok());
    assert!(validate_sample_count(16, 16, "test image").is_ok());

    let invalid = validate_sample_count(3, 16, "test image").unwrap_err();
    assert!(format!("{invalid}").contains("1, 2, 4, 8, or 16"));

    let unsupported = validate_sample_count(8, 4, "test image").unwrap_err();
    assert!(format!("{unsupported}").contains("exceeds device max color sample count 4"));
}

#[test]
fn device_desc_accepts_portable_feature_policy() {
    let desc = DeviceDesc::default()
        .require_feature(DeviceFeature::RayTracing)
        .require_feature(DeviceFeature::RayQuery)
        .prefer_feature(DeviceFeature::MeshShading)
        .disable_feature(DeviceFeature::VrsPipeline);

    assert_eq!(desc.required_features, vec!["ray_tracing", "ray_query"]);
    assert_eq!(desc.optional_features, vec!["mesh_shading"]);
    assert_eq!(
        desc.disabled_features,
        vec!["pipeline_fragment_shading_rate"]
    );
}

#[test]
fn device_desc_deduplicates_portable_feature_policy_names() {
    let desc = DeviceDesc::default()
        .prefer_feature(DeviceFeature::BindlessResources)
        .prefer_feature(DeviceFeature::BindlessResources);

    assert_eq!(desc.optional_features, vec!["bindless_resources"]);
}

#[test]
fn required_portable_feature_rejects_null_backend() {
    let err = match Device::create(
        DeviceDesc {
            backend: BackendKind::Null,
            ..DeviceDesc::default()
        }
        .require_feature(DeviceFeature::RayTracing),
    ) {
        Ok(_) => panic!("required ray tracing should reject the null backend"),
        Err(err) => err,
    };

    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("ray_tracing"));
}

#[test]
fn preferred_portable_feature_allows_null_backend_fallback() {
    Device::create(
        DeviceDesc {
            backend: BackendKind::Null,
            ..DeviceDesc::default()
        }
        .prefer_feature(DeviceFeature::RayTracing),
    )
    .unwrap();
}

#[test]
fn buffer_device_address_usage_requires_backend_feature() {
    let device = Device::create(DeviceDesc {
        backend: BackendKind::Null,
        ..DeviceDesc::default()
    })
    .unwrap();

    let err = device
        .create_buffer(BufferDesc {
            size: 64,
            usage: BufferUsage::STORAGE | BufferUsage::SHADER_DEVICE_ADDRESS,
        })
        .unwrap_err();

    assert!(matches!(err, Error::Unsupported(_)));
    assert!(format!("{err}").contains("buffer device address"));
}

#[test]
fn buffer_device_address_query_returns_none_for_fallback_path() {
    let device = Device::create(DeviceDesc {
        backend: BackendKind::Null,
        ..DeviceDesc::default()
    })
    .unwrap();
    let buffer = device
        .create_buffer(BufferDesc {
            size: 64,
            usage: BufferUsage::STORAGE,
        })
        .unwrap();

    assert_eq!(device.buffer_device_address(buffer).unwrap(), None);
}

#[test]
fn acceleration_structure_creation_requires_backend_support() {
    let device = Device::create(DeviceDesc {
        backend: BackendKind::Null,
        ..DeviceDesc::default()
    })
    .unwrap();

    let err = device
        .create_acceleration_structure(AccelerationStructureDesc {
            kind: crate::AccelerationStructureKind::BottomLevel,
            size: 1024,
        })
        .unwrap_err();

    assert!(matches!(err, Error::Unsupported(_)));
    assert!(format!("{err}").contains("acceleration structures"));
}

#[test]
fn blas_compaction_validation_requires_source_but_not_geometry() {
    let missing_src = validate_blas_build_desc(&BlasBuildDesc {
        opacity_micromap: None,
        dst: AccelerationStructureHandle(1),
        src: None,
        scratch_buffer: None,
        geometries: Vec::new(),
        mode: AccelerationStructureBuildMode::Compact,
    })
    .unwrap_err();
    assert!(format!("{missing_src}").contains("requires a source"));

    validate_blas_build_desc(&BlasBuildDesc {
        opacity_micromap: None,
        dst: AccelerationStructureHandle(1),
        src: Some(AccelerationStructureHandle(2)),
        scratch_buffer: None,
        geometries: Vec::new(),
        mode: AccelerationStructureBuildMode::Compact,
    })
    .unwrap();
}

#[test]
fn tlas_compaction_validation_requires_source_but_not_instances() {
    let missing_src = validate_tlas_build_desc(&TlasBuildDesc {
        dst: AccelerationStructureHandle(1),
        src: None,
        scratch_buffer: None,
        instance_buffer: BufferHandle(2),
        instance_offset: 0,
        instance_count: 0,
        mode: AccelerationStructureBuildMode::Compact,
    })
    .unwrap_err();
    assert!(format!("{missing_src}").contains("requires a source"));

    validate_tlas_build_desc(&TlasBuildDesc {
        dst: AccelerationStructureHandle(1),
        src: Some(AccelerationStructureHandle(2)),
        scratch_buffer: None,
        instance_buffer: BufferHandle(3),
        instance_offset: 0,
        instance_count: 0,
        mode: AccelerationStructureBuildMode::Compact,
    })
    .unwrap();
}

#[test]
fn compaction_sizes_use_source_allocation_as_upper_bound() {
    let sizes = compact_acceleration_structure_build_sizes(
        AccelerationStructureDesc {
            kind: crate::AccelerationStructureKind::BottomLevel,
            size: 4096,
        },
        crate::AccelerationStructureKind::BottomLevel,
    )
    .unwrap();

    assert_eq!(
        sizes,
        AccelerationStructureBuildSizes {
            acceleration_structure_size: 4096,
            build_scratch_size: 0,
            update_scratch_size: 0,
        }
    );
}

#[test]
fn compaction_sizes_reject_wrong_source_kind() {
    let err = compact_acceleration_structure_build_sizes(
        AccelerationStructureDesc {
            kind: crate::AccelerationStructureKind::TopLevel,
            size: 4096,
        },
        crate::AccelerationStructureKind::BottomLevel,
    )
    .unwrap_err();

    assert!(format!("{err}").contains("source kind"));
}

#[test]
fn shader_binding_table_layout_aligns_regions() {
    let desc = ShaderBindingTableDesc {
        pipeline: PipelineHandle(1),
        raygen_group: 0,
        miss_groups: vec![1, 2],
        hit_groups: vec![3],
        callable_groups: vec![4],
    };
    let layout = SbtLayout::new(
        &desc,
        crate::ShaderBindingTableProperties {
            shader_group_handle_size: 24,
            shader_group_handle_alignment: 16,
            shader_group_base_alignment: 64,
            max_shader_group_stride: 64,
        },
    )
    .unwrap();

    assert_eq!(layout.stride, 32);
    assert_eq!(layout.raygen_offset, 0);
    assert_eq!(layout.raygen_size, 32);
    assert_eq!(layout.miss_offset, 64);
    assert_eq!(layout.miss_size, 64);
    assert_eq!(layout.hit_offset, 128);
    assert_eq!(layout.hit_size, 32);
    assert_eq!(layout.callable_offset, 192);
    assert_eq!(layout.callable_size, 32);
    assert_eq!(layout.total_size, 224);
}
