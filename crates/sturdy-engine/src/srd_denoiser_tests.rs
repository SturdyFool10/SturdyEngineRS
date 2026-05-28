use super::*;
use crate::{Error, Format};
use glam::UVec2;
use std::mem;

#[test]
fn denoiser_settings_normalize_zero_frame_count() {
    let denoiser = SrdDenoiser::new(0);
    assert_eq!(denoiser.settings().max_frames, 1);

    let mut denoiser = SrdDenoiser::new(8);
    denoiser.set_settings(SrdDenoiserSettings {
        max_frames: 0,
        ..SrdDenoiserSettings::default()
    });
    assert_eq!(denoiser.settings().max_frames, 1);
}

#[test]
fn common_settings_default_is_valid() {
    SrdCommonSettings::default().validate().unwrap();
}

#[test]
fn common_settings_reject_zero_sizes() {
    let settings = SrdCommonSettings {
        resource_size: UVec2::new(0, 720),
        ..SrdCommonSettings::default()
    };
    let err = settings.validate().unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("resource_size"));
}

#[test]
fn common_settings_reject_invalid_split_screen() {
    let settings = SrdCommonSettings {
        split_screen: 1.25,
        ..SrdCommonSettings::default()
    };
    let err = settings.validate().unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("split_screen"));
}

#[test]
fn instance_desc_rejects_duplicate_denoiser_ids() {
    let desc = SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(7),
        SrdDenoiserDesc {
            id: SrdDenoiserId::new(7),
            mode: SrdDenoiserMode::RadianceStabilizer,
        },
    ]);
    let err = desc.validate().unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("not unique"));
}

#[test]
fn srd_temporal_bindings_use_srd_public_names() {
    let bindings = SrdTemporalBindings::default();
    assert_eq!(bindings.current_signal, "srd_current_signal");
    assert_eq!(bindings.history_signal, "srd_history_signal");
    assert_eq!(bindings.current_sampler, "srd_current_sampler");
    assert_eq!(bindings.history_sampler, "srd_history_sampler");
}

#[test]
fn temporal_constants_layout_stays_shader_compatible() {
    assert_eq!(SRD_TEMPORAL_CONSTANTS_SIZE, 16);
    assert_eq!(mem::align_of::<SrdTemporalConstants>(), 4);
    assert_eq!(mem::size_of::<SrdSignalMomentsConstants>(), 16);
    assert_eq!(mem::align_of::<SrdSignalMomentsConstants>(), 4);
}

#[test]
fn shader_contract_validates_spectral_layout_bounds() {
    SrdShaderContract {
        spectral_layout: SrdSpectralLayout::FixedBins { bins: 8 },
        ..SrdShaderContract::default()
    }
    .validate()
    .unwrap();

    let err = SrdShaderContract {
        spectral_layout: SrdSpectralLayout::FixedBins { bins: 1 },
        ..SrdShaderContract::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("spectral bin"));
}

#[test]
fn radiance_settings_validate_reconstruction_policy_ranges() {
    let err = SrdRadianceSettings {
        outlier_clamp: SrdOutlierClampSettings {
            luminance_sigma: 0.0,
            ..SrdOutlierClampSettings::default()
        },
        ..SrdRadianceSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("outlier clamp"));
}

#[test]
fn srd_instance_initializes_default_family_settings() {
    let instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(0),
        SrdDenoiserDesc {
            id: SrdDenoiserId::new(1),
            mode: SrdDenoiserMode::RadianceStabilizer,
        },
    ]))
    .unwrap();

    assert!(matches!(
        instance.denoiser_settings(SrdDenoiserId::new(0)),
        Some(SrdFamilySettings::Reference(_))
    ));
    assert!(matches!(
        instance.denoiser_settings(SrdDenoiserId::new(1)),
        Some(SrdFamilySettings::Radiance(_))
    ));
}

#[test]
fn srd_instance_rejects_wrong_family_settings_for_id() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(0),
    ]))
    .unwrap();
    let err = instance
        .set_denoiser_settings(
            SrdDenoiserId::new(0),
            SrdFamilySettings::Radiance(SrdRadianceSettings::default()),
        )
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("expects"));
}

#[test]
fn per_family_settings_validate_ranges() {
    let err = SrdFamilySettings::Occlusion(SrdOcclusionSettings {
        normal_weight_power: 0.0,
        ..SrdOcclusionSettings::default()
    })
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("normal_weight_power"));
}

#[test]
fn pipeline_and_dispatch_descriptors_validate_and_register() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(9),
    ]))
    .unwrap();
    let pipeline_index = instance
        .register_pipeline(SrdPipelineDesc {
            name: "SRD Reference Temporal".into(),
            debug_label: "SRD Reference Temporal".into(),
            shader_label: "srd_temporal_accumulate".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
        .unwrap();

    let dispatch = SrdPassBuilder::new(
        "SRD Reference Temporal",
        SrdDenoiserId::new(9),
        pipeline_index,
    )
    .read(SrdResourceSlot::CombinedRadianceInput)
    .read_pool(SrdPoolClass::History, 0)
    .write(SrdResourceSlot::CombinedRadianceOutput)
    .write_pool(SrdPoolClass::History, 1)
    .constants_size(SRD_TEMPORAL_CONSTANTS_SIZE)
    .grid_size([16, 9, 1])
    .build()
    .unwrap();

    instance.push_dispatch(dispatch).unwrap();
    assert_eq!(instance.dispatches().len(), 1);
    assert_eq!(instance.dispatches()[0].resources.len(), 4);
}

#[test]
fn dispatch_rejects_missing_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(3),
    ]))
    .unwrap();
    let dispatch = SrdPassBuilder::new("Missing Pipeline", SrdDenoiserId::new(3), 99)
        .grid_size([1, 1, 1])
        .build()
        .unwrap();
    let err = instance.push_dispatch(dispatch).unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("missing pipeline"));
}

#[test]
fn capabilities_validate_temporal_history_requirement() {
    let err = SrdCapabilities {
        temporal_history: false,
        ..SrdCapabilities::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("temporal history"));
}

#[test]
fn texture_pools_track_history_and_alias_scratch_textures() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(0),
    ]))
    .unwrap();
    let history = instance
        .add_history_texture(SrdTextureDesc {
            name: "reference_history_current".into(),
            debug_label: "SRD Reference History Current".into(),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })
        .unwrap();
    let scratch_a = instance
        .add_scratch_texture(SrdTextureDesc {
            name: "radiance_scratch_a".into(),
            debug_label: "SRD Radiance Scratch A".into(),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })
        .unwrap();
    let scratch_b = instance
        .add_scratch_texture(SrdTextureDesc {
            name: "radiance_scratch_b".into(),
            debug_label: "SRD Radiance Scratch B".into(),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })
        .unwrap();
    let unique_scratch = instance
        .add_unique_scratch_texture(SrdTextureDesc {
            name: "radiance_scratch_unique".into(),
            debug_label: "SRD Radiance Scratch Unique".into(),
            slot: SrdResourceSlot::ScratchPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::Scratch),
            downsample_factor: 1,
        })
        .unwrap();

    assert_eq!(history, 0);
    assert_eq!(scratch_a, scratch_b);
    assert_ne!(scratch_a, unique_scratch);
    assert_eq!(instance.history_pool().len(), 1);
    assert_eq!(instance.scratch_pool().len(), 2);
}

#[test]
fn history_ring_metadata_validates_and_rotates() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(4),
    ]))
    .unwrap();
    let write = instance
        .add_history_texture(SrdTextureDesc {
            name: "history_current".into(),
            debug_label: "SRD History Current".into(),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })
        .unwrap();
    let read = instance
        .add_history_texture(SrdTextureDesc {
            name: "history_previous".into(),
            debug_label: "SRD History Previous".into(),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })
        .unwrap();
    instance
        .add_history_ring(SrdHistoryRing {
            denoiser_id: SrdDenoiserId::new(4),
            write_index: write,
            read_index: read,
            label: "reference_history".into(),
        })
        .unwrap();

    assert_eq!(instance.history_rings()[0].write_index, write);
    instance.rotate_history_ring(SrdDenoiserId::new(4));
    assert_eq!(instance.history_rings()[0].write_index, read);
}

#[test]
fn clear_dispatches_are_generated_for_history_rings() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(5),
    ]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(65, 33),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    let write = instance
        .add_history_texture(SrdTextureDesc {
            name: "clear_current".into(),
            debug_label: "SRD Clear Current".into(),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })
        .unwrap();
    let read = instance
        .add_history_texture(SrdTextureDesc {
            name: "clear_previous".into(),
            debug_label: "SRD Clear Previous".into(),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })
        .unwrap();
    instance
        .add_history_ring(SrdHistoryRing {
            denoiser_id: SrdDenoiserId::new(5),
            write_index: write,
            read_index: read,
            label: "clear_history".into(),
        })
        .unwrap();
    let clear_pipeline = instance.register_clear_pipeline().unwrap();
    assert!(instance.pipelines()[clear_pipeline].has_constants);
    assert_eq!(
        instance.pipelines()[clear_pipeline].workgroup_size,
        SRD_CLEAR_HISTORY_WORKGROUP_SIZE
    );
    let pushed = instance.push_clear_dispatches(clear_pipeline).unwrap();

    assert_eq!(pushed, 1);
    assert_eq!(instance.dispatches().len(), 1);
    let dispatch = &instance.dispatches()[0];
    assert_eq!(dispatch.resources[0].pool_index, Some(write));
    assert!(dispatch.name.contains("clear_history"));
    assert_eq!(dispatch.grid_size, [9, 5, 1]);
    assert_eq!(
        dispatch.constants_size,
        std::mem::size_of::<SrdClearConstants>()
    );
    let constants = bytemuck::from_bytes::<SrdClearConstants>(
        instance.constant_bytes(dispatch.constants_range.unwrap()),
    );
    assert_eq!(constants.extent, [65, 33]);
}

#[test]
fn resource_format_validation_accepts_guides_and_rejects_bad_formats() {
    SrdResourceFormatDesc {
        slot: SrdResourceSlot::MotionVectorsInput,
        format: Format::Rg8Unorm,
    }
    .validate()
    .unwrap();

    let err = SrdResourceFormatDesc {
        slot: SrdResourceSlot::MotionVectorsInput,
        format: Format::Depth32Float,
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("MotionVectorsInput"));
}

#[test]
fn constant_arena_stores_ranges_and_detects_adjacent_reuse() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(6),
    ]))
    .unwrap();
    let pipeline = instance
        .register_pipeline(SrdPipelineDesc {
            name: "SRD Reference Temporal".into(),
            debug_label: "SRD Reference Temporal".into(),
            shader_label: "srd_temporal_accumulate".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
        .unwrap();
    let constants = SrdTemporalConstants {
        frame_index: 1,
        has_history: 1,
        max_frames: 64,
        mode: SrdDenoiserMode::ReferenceTemporal as u32,
    };
    let first_range = instance.push_typed_constants(&constants);
    let first = SrdPassBuilder::new("SRD Reference A", SrdDenoiserId::new(6), pipeline)
        .constants_range(first_range)
        .grid_size([1, 1, 1])
        .build()
        .unwrap();
    instance.push_dispatch(first).unwrap();

    let second_range = instance.push_typed_constants(&constants);
    let second = SrdPassBuilder::new("SRD Reference B", SrdDenoiserId::new(6), pipeline)
        .constants_range(second_range)
        .grid_size([1, 1, 1])
        .build()
        .unwrap();
    instance.push_dispatch(second).unwrap();

    assert_eq!(
        instance.constant_bytes(first_range),
        instance.constant_bytes(second_range)
    );
    assert!(instance.dispatches()[1].reuses_previous_constants);
}

#[test]
fn reference_temporal_plan_orders_clear_then_accumulate() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(8),
    ]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(128, 72),
            history_mode: SrdHistoryMode::ZeroHistory,
            ..SrdCommonSettings::default()
        })
        .unwrap();
    let write = instance
        .add_history_texture(SrdTextureDesc {
            name: "generated_current".into(),
            debug_label: "SRD Generated Current".into(),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })
        .unwrap();
    let read = instance
        .add_history_texture(SrdTextureDesc {
            name: "generated_previous".into(),
            debug_label: "SRD Generated Previous".into(),
            slot: SrdResourceSlot::HistoryPool,
            format: Format::Rgba16Float,
            pool: Some(SrdPoolClass::History),
            downsample_factor: 1,
        })
        .unwrap();
    instance
        .add_history_ring(SrdHistoryRing {
            denoiser_id: SrdDenoiserId::new(8),
            write_index: write,
            read_index: read,
            label: "generated_history".into(),
        })
        .unwrap();
    let pipeline = instance
        .register_pipeline(SrdPipelineDesc {
            name: "SRD Reference Temporal".into(),
            debug_label: "SRD Reference Temporal".into(),
            shader_label: "srd_temporal_accumulate".into(),
            has_constants: true,
            workgroup_size: [8, 8, 1],
        })
        .unwrap();
    let dispatches = instance
        .plan_reference_temporal_passes(
            SrdDenoiserId::new(8),
            pipeline,
            SrdTemporalConstants {
                frame_index: 2,
                has_history: 1,
                max_frames: 64,
                mode: SrdDenoiserMode::ReferenceTemporal as u32,
            },
        )
        .unwrap();

    assert_eq!(dispatches.len(), 2);
    assert!(dispatches[0].name.contains("SRD Clear"));
    assert_eq!(dispatches[1].name, "SRD Reference Temporal");
    assert_eq!(dispatches[1].grid_size, [16, 9, 1]);
    assert_eq!(dispatches[1].constants_size, SRD_TEMPORAL_CONSTANTS_SIZE);
}

#[test]
fn prepare_reference_temporal_registers_resources_and_pipelines() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(11),
    ]))
    .unwrap();

    let pipelines = instance
        .prepare_reference_temporal(SrdDenoiserId::new(11), Format::Rgba16Float)
        .unwrap();

    assert_eq!(instance.history_pool().len(), 2);
    assert_eq!(instance.history_rings().len(), 1);
    assert_eq!(instance.history_rings()[0].label, "reference_history_11");
    assert_eq!(
        instance.pipelines()[pipelines.temporal].shader_label,
        "srd_temporal_accumulate"
    );
    assert_eq!(
        instance.pipelines()[pipelines.clear].shader_label,
        "srd_clear_history"
    );
}

#[test]
fn reference_temporal_plan_uses_dedicated_clear_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(12),
    ]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::ZeroHistory,
            rect_size: UVec2::new(8, 8),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    let pipelines = instance
        .prepare_reference_temporal(SrdDenoiserId::new(12), Format::Rgba16Float)
        .unwrap();

    let dispatches = instance
        .plan_reference_temporal_passes_with_pipelines(
            SrdDenoiserId::new(12),
            pipelines,
            SrdTemporalConstants {
                frame_index: 0,
                has_history: 0,
                max_frames: 64,
                mode: SrdDenoiserMode::ReferenceTemporal as u32,
            },
        )
        .unwrap();

    assert_eq!(dispatches.len(), 2);
    assert_eq!(dispatches[0].pipeline_index, pipelines.clear);
    assert_eq!(dispatches[1].pipeline_index, pipelines.temporal);
}

#[test]
fn radiance_surface_mask_prepare_allocates_tile_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(20),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(20))
        .unwrap();

    assert_eq!(instance.scratch_pool().len(), 1);
    let texture = &instance.scratch_pool()[resources.scratch_index as usize];
    assert_eq!(texture.slot, SrdResourceSlot::SurfaceMaskOutput);
    assert_eq!(texture.format, Format::R8Unorm);
    assert_eq!(
        texture.downsample_factor,
        SRD_RADIANCE_SURFACE_MASK_TILE_SIZE
    );
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_surface_mask");
    assert_eq!(
        pipeline.workgroup_size,
        [
            SRD_RADIANCE_SURFACE_MASK_TILE_SIZE,
            SRD_RADIANCE_SURFACE_MASK_TILE_SIZE,
            1
        ]
    );
}

#[test]
fn radiance_surface_mask_plan_emits_one_tile_dispatch() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(21),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(128, 72),
            effective_range: 250.0,
            ..SrdCommonSettings::default()
        })
        .unwrap();
    let resources = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(21))
        .unwrap();

    let dispatches = instance
        .plan_radiance_surface_mask_passes(SrdDenoiserId::new(21), resources, UVec2::new(128, 72))
        .unwrap();

    assert_eq!(dispatches.len(), 1);
    let d = &dispatches[0];
    assert_eq!(d.pipeline_index, resources.pipeline);
    assert_eq!(d.grid_size, [16, 9, 1]);
    assert_eq!(d.resources.len(), 2);
    assert_eq!(d.resources[0].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(d.resources[0].slot, SrdResourceSlot::LinearDepthInput);
    assert_eq!(
        d.resources[1].descriptor_type,
        SrdDescriptorType::StorageTexture
    );
    assert_eq!(d.resources[1].slot, SrdResourceSlot::ScratchPool);
    assert_eq!(d.resources[1].pool_index, Some(resources.scratch_index));
    assert_eq!(
        d.constants_size,
        std::mem::size_of::<SrdRadianceSurfaceMaskConstants>()
    );
}

#[test]
fn radiance_surface_mask_plan_rejects_wrong_family() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(22),
    ]))
    .unwrap();
    let err = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(22))
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("RadianceStabilizer"));
}

#[test]
fn radiance_reproject_prepare_allocates_fullres_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(30),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(30))
        .unwrap();

    let texture = &instance.scratch_pool()[resources.scratch_index as usize];
    assert_eq!(texture.format, Format::Rgba16Float);
    assert_eq!(texture.downsample_factor, 1);
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_reproject");
    assert_eq!(pipeline.workgroup_size, [8, 8, 1]);
}

#[test]
fn radiance_reproject_plan_chains_after_surface_mask() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(31),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(128, 72),
            motion_vector_scale: glam::Vec2::new(1.0, -1.0),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    let mask = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(31))
        .unwrap();
    let reproject = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(31))
        .unwrap();

    instance
        .plan_radiance_surface_mask_passes(SrdDenoiserId::new(31), mask, UVec2::new(128, 72))
        .unwrap();
    let dispatches = instance
        .plan_radiance_reproject_passes(
            SrdDenoiserId::new(31),
            reproject,
            Some(mask),
            UVec2::new(128, 72),
        )
        .unwrap();

    assert_eq!(dispatches.len(), 2);
    let reproject_dispatch = &dispatches[1];
    assert_eq!(reproject_dispatch.pipeline_index, reproject.pipeline);
    assert_eq!(reproject_dispatch.grid_size, [16, 9, 1]);
    // Five guide reads + one scratch surface-mask read + one scratch write
    assert_eq!(reproject_dispatch.resources.len(), 7);
    assert_eq!(
        reproject_dispatch.resources[0].slot,
        SrdResourceSlot::MotionVectorsInput
    );
    assert_eq!(
        reproject_dispatch.resources[3].slot,
        SrdResourceSlot::PrevLinearDepthInput
    );
    assert_eq!(
        reproject_dispatch.resources[5].descriptor_type,
        SrdDescriptorType::Texture
    );
    assert_eq!(
        reproject_dispatch.resources[5].pool_index,
        Some(mask.scratch_index)
    );
    assert_eq!(
        reproject_dispatch.resources[6].descriptor_type,
        SrdDescriptorType::StorageTexture
    );
    assert_eq!(
        reproject_dispatch.resources[6].pool_index,
        Some(reproject.scratch_index)
    );
    assert_eq!(
        reproject_dispatch.constants_size,
        std::mem::size_of::<SrdRadianceReprojectConstants>()
    );
}

#[test]
fn radiance_accumulate_prepare_allocates_history_ring_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(40),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(40))
        .unwrap();

    assert_eq!(instance.history_pool().len(), 2);
    assert_eq!(instance.history_rings().len(), 1);
    assert_eq!(resources.history_ring_index, 0);
    let ring = &instance.history_rings()[resources.history_ring_index];
    assert_eq!(ring.label, "radiance_history_40");
    assert_ne!(ring.write_index, ring.read_index);
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_accumulate");
}

#[test]
fn rotate_history_ring_at_swaps_indices_for_one_ring() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(41),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(41))
        .unwrap();
    let before_write = instance.history_rings()[resources.history_ring_index].write_index;
    let before_read = instance.history_rings()[resources.history_ring_index].read_index;
    instance
        .rotate_history_ring_at(resources.history_ring_index)
        .unwrap();
    let after_write = instance.history_rings()[resources.history_ring_index].write_index;
    let after_read = instance.history_rings()[resources.history_ring_index].read_index;
    assert_eq!(after_write, before_read);
    assert_eq!(after_read, before_write);
}

#[test]
fn rotate_history_ring_at_rejects_oob_index() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc::reference_temporal(42),
    ]))
    .unwrap();
    let err = instance.rotate_history_ring_at(99).unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn radiance_accumulate_plan_chains_after_reproject_with_history_pool() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(43),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(64, 32),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    let mask = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(43))
        .unwrap();
    let reproject = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(43))
        .unwrap();
    let accumulate = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(43))
        .unwrap();

    let dispatches = instance
        .plan_radiance_accumulate_passes(
            SrdDenoiserId::new(43),
            accumulate,
            reproject,
            Some(mask),
            UVec2::new(64, 32),
        )
        .unwrap();

    assert_eq!(dispatches.len(), 1);
    let d = &dispatches[0];
    assert_eq!(d.pipeline_index, accumulate.pipeline);
    assert_eq!(d.grid_size, [8, 4, 1]);
    // CombinedRadianceInput + reproject scratch + mask scratch + history read + history write = 5
    assert_eq!(d.resources.len(), 5);
    assert_eq!(d.resources[0].slot, SrdResourceSlot::CombinedRadianceInput);
    assert_eq!(d.resources[1].pool_index, Some(reproject.scratch_index));
    assert_eq!(d.resources[2].pool_index, Some(mask.scratch_index));
    assert_eq!(d.resources[3].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(d.resources[3].slot, SrdResourceSlot::HistoryPool);
    assert_eq!(
        d.resources[4].descriptor_type,
        SrdDescriptorType::StorageTexture
    );
    assert_eq!(d.resources[4].slot, SrdResourceSlot::HistoryPool);
    assert_ne!(d.resources[3].pool_index, d.resources[4].pool_index);
}

#[test]
fn radiance_accumulate_plan_after_rotation_swaps_read_and_write() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(44),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let reproject = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(44))
        .unwrap();
    let accumulate = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(44))
        .unwrap();

    let dispatches_a = instance
        .plan_radiance_accumulate_passes(
            SrdDenoiserId::new(44),
            accumulate,
            reproject,
            None,
            UVec2::new(8, 8),
        )
        .unwrap();
    let write_a = dispatches_a
        .last()
        .unwrap()
        .resources
        .last()
        .unwrap()
        .pool_index;
    let read_a = dispatches_a
        .last()
        .unwrap()
        .resources
        .iter()
        .rev()
        .nth(1)
        .unwrap()
        .pool_index;

    instance.clear_dispatches();
    instance
        .rotate_history_ring_at(accumulate.history_ring_index)
        .unwrap();
    let dispatches_b = instance
        .plan_radiance_accumulate_passes(
            SrdDenoiserId::new(44),
            accumulate,
            reproject,
            None,
            UVec2::new(8, 8),
        )
        .unwrap();
    let write_b = dispatches_b
        .last()
        .unwrap()
        .resources
        .last()
        .unwrap()
        .pool_index;
    let read_b = dispatches_b
        .last()
        .unwrap()
        .resources
        .iter()
        .rev()
        .nth(1)
        .unwrap()
        .pool_index;

    assert_eq!(write_b, read_a);
    assert_eq!(read_b, write_a);
}

#[test]
fn radiance_reconstruct_prepare_allocates_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(50),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(50))
        .unwrap();

    let texture = &instance.scratch_pool()[resources.scratch_index as usize];
    assert_eq!(texture.format, Format::Rgba16Float);
    assert_eq!(texture.downsample_factor, 1);
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_reconstruct");
    assert_eq!(pipeline.workgroup_size, [8, 8, 1]);
}

#[test]
fn radiance_reconstruct_plan_reads_history_write_slot_with_guides() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(51),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(64, 32),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    let mask = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(51))
        .unwrap();
    let accumulate = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(51))
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(51))
        .unwrap();
    let write_index = instance.history_rings()[accumulate.history_ring_index].write_index;

    let dispatches = instance
        .plan_radiance_reconstruct_passes(
            SrdDenoiserId::new(51),
            reconstruct,
            accumulate,
            None,
            Some(mask),
            UVec2::new(64, 32),
        )
        .unwrap();

    assert_eq!(dispatches.len(), 1);
    let d = &dispatches[0];
    assert_eq!(d.pipeline_index, reconstruct.pipeline);
    assert_eq!(d.grid_size, [8, 4, 1]);
    // history_read + depth + normal/roughness + mask_scratch + reconstruct_scratch = 5
    assert_eq!(d.resources.len(), 5);
    assert_eq!(d.resources[0].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(d.resources[0].slot, SrdResourceSlot::HistoryPool);
    assert_eq!(d.resources[0].pool_index, Some(write_index));
    assert_eq!(d.resources[1].slot, SrdResourceSlot::LinearDepthInput);
    assert_eq!(d.resources[2].slot, SrdResourceSlot::NormalRoughnessInput);
    assert_eq!(d.resources[3].pool_index, Some(mask.scratch_index));
    assert_eq!(
        d.resources[4].descriptor_type,
        SrdDescriptorType::StorageTexture
    );
    assert_eq!(d.resources[4].pool_index, Some(reconstruct.scratch_index));
    assert_eq!(
        d.constants_size,
        std::mem::size_of::<SrdRadianceReconstructConstants>()
    );
}

#[test]
fn radiance_reconstruct_plan_rejects_ring_owned_by_other_denoiser() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![
        SrdDenoiserDesc {
            id: SrdDenoiserId::new(52),
            mode: SrdDenoiserMode::RadianceStabilizer,
        },
        SrdDenoiserDesc {
            id: SrdDenoiserId::new(53),
            mode: SrdDenoiserMode::RadianceStabilizer,
        },
    ]))
    .unwrap();
    let foreign_accumulate = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(52))
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(53))
        .unwrap();

    let err = instance
        .plan_radiance_reconstruct_passes(
            SrdDenoiserId::new(53),
            reconstruct,
            foreign_accumulate,
            None,
            None,
            UVec2::new(8, 8),
        )
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("ring belongs to"));
}

#[test]
fn radiance_reproject_plan_without_surface_mask_omits_mask_read() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(32),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let reproject = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(32))
        .unwrap();
    let dispatches = instance
        .plan_radiance_reproject_passes(SrdDenoiserId::new(32), reproject, None, UVec2::new(16, 16))
        .unwrap();

    let d = &dispatches[0];
    // Five guide reads + one scratch write, no surface mask
    assert_eq!(d.resources.len(), 6);
    assert!(d.resources.iter().all(|r| !matches!(
        (r.descriptor_type, r.slot, r.pool_index),
        (SrdDescriptorType::Texture, SrdResourceSlot::ScratchPool, _)
    )));
}

#[test]
fn radiance_outlier_suppress_prepare_allocates_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(60),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_outlier_suppress(SrdDenoiserId::new(60))
        .unwrap();

    let texture = &instance.scratch_pool()[resources.scratch_index as usize];
    assert_eq!(texture.format, Format::Rgba16Float);
    assert_eq!(texture.downsample_factor, 1);
    assert_eq!(texture.debug_label, "SRD Radiance Outlier Suppress 60");
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_outlier_suppress");
    assert!(pipeline.has_constants);
    assert_eq!(pipeline.workgroup_size, [8, 8, 1]);
}

#[test]
fn radiance_outlier_suppress_plan_skips_when_disabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(61),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(61),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                outlier_clamp: SrdOutlierClampSettings {
                    enabled: false,
                    ..SrdOutlierClampSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(61))
        .unwrap();
    let outlier = instance
        .prepare_radiance_outlier_suppress(SrdDenoiserId::new(61))
        .unwrap();

    let dispatches = instance
        .plan_radiance_outlier_suppress_passes(
            SrdDenoiserId::new(61),
            outlier,
            reconstruct,
            None,
            UVec2::new(32, 16),
        )
        .unwrap();

    assert!(dispatches.is_empty());
}

#[test]
fn radiance_outlier_suppress_plan_appends_when_enabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(62),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(62),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                outlier_clamp: SrdOutlierClampSettings {
                    enabled: true,
                    luminance_sigma: 1.75,
                    max_relative_luminance: 4.5,
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let mask = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(62))
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(62))
        .unwrap();
    let outlier = instance
        .prepare_radiance_outlier_suppress(SrdDenoiserId::new(62))
        .unwrap();

    let d = {
        let dispatches = instance
            .plan_radiance_outlier_suppress_passes(
                SrdDenoiserId::new(62),
                outlier,
                reconstruct,
                Some(mask),
                UVec2::new(63, 17),
            )
            .unwrap();

        assert_eq!(dispatches.len(), 1);
        dispatches[0].clone()
    };
    assert_eq!(d.name, "SRD Radiance Outlier Suppress");
    assert_eq!(d.pipeline_index, outlier.pipeline);
    assert_eq!(d.grid_size, [8, 3, 1]);
    // reconstruct scratch + surface mask scratch + outlier scratch
    assert_eq!(d.resources.len(), 3);
    assert_eq!(d.resources[0].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(d.resources[0].pool_index, Some(reconstruct.scratch_index));
    assert_eq!(d.resources[1].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(d.resources[1].pool_index, Some(mask.scratch_index));
    assert_eq!(
        d.resources[2].descriptor_type,
        SrdDescriptorType::StorageTexture
    );
    assert_eq!(d.resources[2].pool_index, Some(outlier.scratch_index));
    assert_eq!(
        d.constants_size,
        std::mem::size_of::<SrdRadianceOutlierSuppressConstants>()
    );
    let constants = bytemuck::from_bytes::<SrdRadianceOutlierSuppressConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.rect_extent, [63, 17]);
    assert_eq!(constants.mask_extent, [8, 3]);
    assert_eq!(constants.luminance_sigma, 1.75);
    assert_eq!(constants.max_relative_luminance, 4.5);
    assert_eq!(constants.use_surface_mask, 1);
    assert_eq!(constants.tile_size, SRD_RADIANCE_SURFACE_MASK_TILE_SIZE);
}

#[test]
fn radiance_constants_layout_stays_shader_compatible() {
    assert_eq!(mem::size_of::<SrdRadianceSurfaceMaskConstants>(), 32);
    assert_eq!(mem::align_of::<SrdRadianceSurfaceMaskConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceReprojectConstants>(), 52);
    assert_eq!(mem::align_of::<SrdRadianceReprojectConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceAccumulateConstants>(), 32);
    assert_eq!(mem::align_of::<SrdRadianceAccumulateConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceClampConstants>(), 32);
    assert_eq!(mem::align_of::<SrdRadianceClampConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceReconstructConstants>(), 48);
    assert_eq!(mem::align_of::<SrdRadianceReconstructConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceOutlierSuppressConstants>(), 32);
    assert_eq!(mem::align_of::<SrdRadianceOutlierSuppressConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceSpatialFilterConstants>(), 56);
    assert_eq!(mem::align_of::<SrdRadianceSpatialFilterConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceAtrousConstants>(), 56);
    assert_eq!(mem::align_of::<SrdRadianceAtrousConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadiancePostBlurConstants>(), 48);
    assert_eq!(mem::align_of::<SrdRadiancePostBlurConstants>(), 4);
}

#[test]
fn radiance_outlier_suppress_does_not_alias_reconstruct_output() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(63),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();

    let reproject = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(63))
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(63))
        .unwrap();
    let outlier = instance
        .prepare_radiance_outlier_suppress(SrdDenoiserId::new(63))
        .unwrap();

    assert_eq!(reconstruct.scratch_index, reproject.scratch_index);
    assert_ne!(outlier.scratch_index, reconstruct.scratch_index);
}

#[test]
fn radiance_stabilizer_prepare_registers_v0_resources_and_pipelines() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(70),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();

    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(70))
        .unwrap();

    assert_eq!(instance.history_rings().len(), 1);
    assert_eq!(instance.history_pool().len(), 2);
    assert_ne!(
        resources.reconstruct.scratch_index,
        resources.outlier_suppress.scratch_index
    );
    assert_eq!(
        instance.pipelines()[resources.clear_pipeline].shader_label,
        "srd_clear_history"
    );
    assert_eq!(
        instance.pipelines()[resources.surface_mask.pipeline].shader_label,
        "srd_radiance_surface_mask"
    );
    assert_eq!(
        instance.pipelines()[resources.reproject.pipeline].shader_label,
        "srd_radiance_reproject"
    );
    assert_eq!(
        instance.pipelines()[resources.accumulate.pipeline].shader_label,
        "srd_radiance_accumulate"
    );
    assert_eq!(
        instance.pipelines()[resources.clamp.pipeline].shader_label,
        "srd_radiance_clamp"
    );
    assert_eq!(
        instance.pipelines()[resources.reconstruct.pipeline].shader_label,
        "srd_radiance_reconstruct"
    );
    assert_eq!(
        instance.pipelines()[resources.outlier_suppress.pipeline].shader_label,
        "srd_radiance_outlier_suppress"
    );
    assert_ne!(
        resources.clamp.scratch_index,
        resources.reconstruct.scratch_index
    );
    assert_ne!(
        resources.clamp.scratch_index,
        resources.outlier_suppress.scratch_index
    );
}

#[test]
fn radiance_stabilizer_plan_orders_v0_dispatches_with_clear_and_outlier() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(71),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(64, 32),
            history_mode: SrdHistoryMode::ZeroHistory,
            ..SrdCommonSettings::default()
        })
        .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(71),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                outlier_clamp: SrdOutlierClampSettings {
                    enabled: true,
                    ..SrdOutlierClampSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(71))
        .unwrap();
    let before_write =
        instance.history_rings()[resources.accumulate.history_ring_index].write_index;
    let before_read = instance.history_rings()[resources.accumulate.history_ring_index].read_index;

    let plan = instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(71), resources, UVec2::new(64, 32))
        .unwrap();

    assert_eq!(plan.dispatch_count, 7);
    assert_eq!(
        plan.final_output,
        SrdRadianceOutputResource::OutlierSuppress {
            scratch_index: resources.outlier_suppress.scratch_index
        }
    );
    let names: Vec<&str> = instance
        .dispatches()
        .iter()
        .map(|dispatch| dispatch.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "SRD Clear radiance_history_71",
            "SRD Radiance Surface Mask",
            "SRD Radiance Reproject",
            "SRD Radiance Accumulate",
            "SRD Radiance Clamp",
            "SRD Radiance Reconstruct",
            "SRD Radiance Outlier Suppress",
        ]
    );
    let after_ring = &instance.history_rings()[resources.accumulate.history_ring_index];
    assert_eq!(after_ring.write_index, before_read);
    assert_eq!(after_ring.read_index, before_write);
}

#[test]
fn radiance_stabilizer_plan_skips_outlier_when_disabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(72),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            ..SrdCommonSettings::default()
        })
        .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(72),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                outlier_clamp: SrdOutlierClampSettings {
                    enabled: false,
                    ..SrdOutlierClampSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(72))
        .unwrap();

    let plan = instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(72), resources, UVec2::new(16, 16))
        .unwrap();

    assert_eq!(plan.dispatch_count, 5);
    assert_eq!(
        plan.final_output,
        SrdRadianceOutputResource::Reconstruct {
            scratch_index: resources.reconstruct.scratch_index
        }
    );
    assert!(
        instance
            .dispatches()
            .iter()
            .all(|dispatch| dispatch.name != "SRD Radiance Outlier Suppress")
    );
}

#[test]
fn radiance_stabilizer_plan_rotates_history_once_per_frame() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(73),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(73))
        .unwrap();
    let initial_write =
        instance.history_rings()[resources.accumulate.history_ring_index].write_index;
    let initial_read = instance.history_rings()[resources.accumulate.history_ring_index].read_index;

    instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(73), resources, UVec2::new(8, 8))
        .unwrap();
    let after_first = instance.history_rings()[resources.accumulate.history_ring_index].clone();
    assert_eq!(after_first.write_index, initial_read);
    assert_eq!(after_first.read_index, initial_write);

    instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(73), resources, UVec2::new(8, 8))
        .unwrap();
    let after_second = &instance.history_rings()[resources.accumulate.history_ring_index];
    assert_eq!(after_second.write_index, initial_write);
    assert_eq!(after_second.read_index, initial_read);
}

#[test]
fn radiance_stabilizer_dispatches_do_not_read_and_write_same_pool_resource() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(74),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(74))
        .unwrap();
    instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(74), resources, UVec2::new(16, 16))
        .unwrap();

    for dispatch in instance.dispatches() {
        for read in dispatch
            .resources
            .iter()
            .filter(|resource| resource.descriptor_type == SrdDescriptorType::Texture)
        {
            for write in dispatch
                .resources
                .iter()
                .filter(|resource| resource.descriptor_type == SrdDescriptorType::StorageTexture)
            {
                assert_ne!(
                    (read.slot, read.pool_index),
                    (write.slot, write.pool_index),
                    "dispatch '{}' reads and writes the same SRD pool resource",
                    dispatch.name
                );
            }
        }
    }
}

#[test]
fn radiance_stabilizer_executor_binding_map_matches_v0_shaders() {
    let scratch_read = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::ScratchPool,
        pool_index: Some(0),
    };
    let scratch_write = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::StorageTexture,
        slot: SrdResourceSlot::ScratchPool,
        pool_index: Some(0),
    };
    let history_read = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::HistoryPool,
        pool_index: Some(0),
    };
    let history_write = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::StorageTexture,
        slot: SrdResourceSlot::HistoryPool,
        pool_index: Some(1),
    };
    let motion = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::MotionVectorsInput,
        pool_index: None,
    };

    assert_eq!(
        super::radiance_stabilizer_executor::srd_storage_binding_for_resource(
            "srd_clear_history",
            &history_write,
        )
        .unwrap(),
        "srd_clear_target"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_storage_binding_for_resource(
            "srd_radiance_reproject",
            &scratch_write,
        )
        .unwrap(),
        "srd_radiance_reproject_out"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_reproject",
            &motion,
            0,
        )
        .unwrap(),
        "srd_guide_motion"
    );
    let hit_distance = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::HitDistanceInput,
        pool_index: None,
    };
    let prev_hit_distance = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::PrevHitDistanceInput,
        pool_index: None,
    };
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_reproject",
            &hit_distance,
            0,
        )
        .unwrap(),
        "srd_guide_hit_distance"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_reproject",
            &prev_hit_distance,
            0,
        )
        .unwrap(),
        "srd_prev_guide_hit_distance"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_accumulate",
            &scratch_read,
            1,
        )
        .unwrap(),
        "srd_radiance_reproject"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_accumulate",
            &scratch_read,
            2,
        )
        .unwrap(),
        "srd_radiance_surface_mask"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_accumulate",
            &history_read,
            0,
        )
        .unwrap(),
        "srd_radiance_history_read"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_outlier_suppress",
            &scratch_read,
            1,
        )
        .unwrap(),
        "srd_radiance_input"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_outlier_suppress",
            &scratch_read,
            2,
        )
        .unwrap(),
        "srd_radiance_surface_mask"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_storage_binding_for_resource(
            "srd_radiance_clamp",
            &scratch_write,
        )
        .unwrap(),
        "srd_radiance_clamped_out"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_clamp",
            &history_read,
            0,
        )
        .unwrap(),
        "srd_radiance_accumulated"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_clamp",
            &scratch_read,
            1,
        )
        .unwrap(),
        "srd_radiance_surface_mask"
    );
    let clamped_read = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::ClampedRadianceInput,
        pool_index: Some(0),
    };
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_reconstruct",
            &clamped_read,
            0,
        )
        .unwrap(),
        "srd_radiance_accumulated"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_storage_binding_for_resource(
            "srd_radiance_spatial_filter",
            &scratch_write,
        )
        .unwrap(),
        "srd_radiance_filtered_out"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_spatial_filter",
            &scratch_read,
            1,
        )
        .unwrap(),
        "srd_radiance_input"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_spatial_filter",
            &scratch_read,
            2,
        )
        .unwrap(),
        "srd_radiance_surface_mask"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_storage_binding_for_resource(
            "srd_radiance_atrous",
            &scratch_write,
        )
        .unwrap(),
        "srd_radiance_atrous_out"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_atrous",
            &scratch_read,
            1,
        )
        .unwrap(),
        "srd_radiance_input"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_atrous",
            &scratch_read,
            2,
        )
        .unwrap(),
        "srd_radiance_surface_mask"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_storage_binding_for_resource(
            "srd_radiance_post_blur",
            &scratch_write,
        )
        .unwrap(),
        "srd_radiance_post_blur_out"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_post_blur",
            &scratch_read,
            1,
        )
        .unwrap(),
        "srd_radiance_input"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_post_blur",
            &scratch_read,
            2,
        )
        .unwrap(),
        "srd_radiance_surface_mask"
    );
}

#[test]
fn radiance_post_blur_prepare_allocates_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(110),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_post_blur(SrdDenoiserId::new(110))
        .unwrap();

    let texture = &instance.scratch_pool()[resources.scratch_index as usize];
    assert_eq!(texture.format, Format::Rgba16Float);
    assert_eq!(texture.downsample_factor, 1);
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_post_blur");
    assert_eq!(pipeline.workgroup_size, [8, 8, 1]);
    assert!(pipeline.has_constants);
}

#[test]
fn radiance_post_blur_plan_skips_when_disabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(111),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(111))
        .unwrap();
    let post_blur = instance
        .prepare_radiance_post_blur(SrdDenoiserId::new(111))
        .unwrap();

    let dispatches = instance
        .plan_radiance_post_blur_passes(
            SrdDenoiserId::new(111),
            post_blur,
            reconstruct.scratch_index,
            None,
            UVec2::new(16, 16),
        )
        .unwrap();
    assert!(dispatches.is_empty());
}

#[test]
fn radiance_post_blur_plan_emits_dispatch_with_adaptive_sigma_constants() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(112),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(112),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                post_blur: SrdPostBlurSettings {
                    enabled: true,
                    blur_radius: 2,
                    max_blur_sigma: 1.5,
                    normal_sharpness: 20.0,
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let mask = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(112))
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(112))
        .unwrap();
    let post_blur = instance
        .prepare_radiance_post_blur(SrdDenoiserId::new(112))
        .unwrap();

    let d = {
        let dispatches = instance
            .plan_radiance_post_blur_passes(
                SrdDenoiserId::new(112),
                post_blur,
                reconstruct.scratch_index,
                Some(mask),
                UVec2::new(64, 32),
            )
            .unwrap();
        assert_eq!(dispatches.len(), 1);
        dispatches[0].clone()
    };
    assert_eq!(d.name, "SRD Radiance Post Blur");
    assert_eq!(d.pipeline_index, post_blur.pipeline);
    assert_eq!(d.grid_size, [8, 4, 1]);
    // input + depth + normal/roughness + mask + write
    assert_eq!(d.resources.len(), 5);
    assert_eq!(d.resources[0].pool_index, Some(reconstruct.scratch_index));
    assert_eq!(d.resources[1].slot, SrdResourceSlot::LinearDepthInput);
    assert_eq!(d.resources[2].slot, SrdResourceSlot::NormalRoughnessInput);
    assert_eq!(d.resources[3].pool_index, Some(mask.scratch_index));
    assert_eq!(d.resources[4].descriptor_type, SrdDescriptorType::StorageTexture);
    assert_eq!(d.resources[4].pool_index, Some(post_blur.scratch_index));
    assert_eq!(d.constants_size, mem::size_of::<SrdRadiancePostBlurConstants>());
    let constants = bytemuck::from_bytes::<SrdRadiancePostBlurConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.rect_extent, [64, 32]);
    assert_eq!(constants.blur_radius, 2);
    assert_eq!(constants.max_blur_sigma, 1.5);
    assert_eq!(constants.normal_sharpness, 20.0);
    assert_eq!(constants.use_surface_mask, 1);
    assert_eq!(constants.tile_size, SRD_RADIANCE_SURFACE_MASK_TILE_SIZE);
}

#[test]
fn radiance_stabilizer_plan_appends_post_blur_as_final_when_enabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(113),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(16, 16),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(113),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                outlier_clamp: SrdOutlierClampSettings {
                    enabled: false,
                    ..SrdOutlierClampSettings::default()
                },
                post_blur: SrdPostBlurSettings {
                    enabled: true,
                    ..SrdPostBlurSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(113))
        .unwrap();
    let plan = instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(113), resources, UVec2::new(16, 16))
        .unwrap();

    // Surface Mask, Reproject, Accumulate, Clamp, Reconstruct, Post Blur = 6
    assert_eq!(plan.dispatch_count, 6);
    assert_eq!(
        plan.final_output,
        SrdRadianceOutputResource::PostBlur {
            scratch_index: resources.post_blur.scratch_index
        }
    );
    let last = instance.dispatches().last().unwrap();
    assert_eq!(last.name, "SRD Radiance Post Blur");
}

#[test]
fn post_blur_settings_validate_ranges() {
    SrdPostBlurSettings::default().validate().unwrap();

    let err = SrdPostBlurSettings {
        blur_radius: 0,
        ..SrdPostBlurSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("blur_radius"));

    let err = SrdPostBlurSettings {
        max_blur_sigma: 0.0,
        ..SrdPostBlurSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("max_blur_sigma"));
}

#[test]
fn radiance_clamp_prepare_allocates_unique_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(80),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_clamp(SrdDenoiserId::new(80))
        .unwrap();

    let texture = &instance.scratch_pool()[resources.scratch_index as usize];
    assert_eq!(texture.format, Format::Rgba16Float);
    assert_eq!(texture.downsample_factor, 1);
    assert_eq!(texture.slot, SrdResourceSlot::ClampedRadianceInput);
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_clamp");
    assert_eq!(pipeline.workgroup_size, [8, 8, 1]);
    assert!(pipeline.has_constants);
}

#[test]
fn radiance_clamp_plan_skips_when_disabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(81),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(81),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                history_clamp: SrdHistoryClampSettings {
                    enabled: false,
                    ..SrdHistoryClampSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let accumulate = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(81))
        .unwrap();
    let clamp = instance
        .prepare_radiance_clamp(SrdDenoiserId::new(81))
        .unwrap();

    let dispatches = instance
        .plan_radiance_clamp_passes(
            SrdDenoiserId::new(81),
            clamp,
            accumulate,
            None,
            UVec2::new(32, 16),
        )
        .unwrap();

    assert!(dispatches.is_empty());
}

#[test]
fn radiance_clamp_plan_emits_dispatch_with_correct_resources() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(82),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(82),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                history_clamp: SrdHistoryClampSettings {
                    enabled: true,
                    sigma_scale: 1.25,
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let mask = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(82))
        .unwrap();
    let accumulate = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(82))
        .unwrap();
    let clamp = instance
        .prepare_radiance_clamp(SrdDenoiserId::new(82))
        .unwrap();
    let write_index = instance.history_rings()[accumulate.history_ring_index].write_index;

    let d = {
        let dispatches = instance
            .plan_radiance_clamp_passes(
                SrdDenoiserId::new(82),
                clamp,
                accumulate,
                Some(mask),
                UVec2::new(64, 32),
            )
            .unwrap();
        assert_eq!(dispatches.len(), 1);
        dispatches[0].clone()
    };
    assert_eq!(d.name, "SRD Radiance Clamp");
    assert_eq!(d.pipeline_index, clamp.pipeline);
    assert_eq!(d.grid_size, [8, 4, 1]);
    // history_read + CombinedRadianceInput + surface_mask + clamp_write = 4
    assert_eq!(d.resources.len(), 4);
    assert_eq!(d.resources[0].slot, SrdResourceSlot::HistoryPool);
    assert_eq!(d.resources[0].pool_index, Some(write_index));
    assert_eq!(d.resources[1].slot, SrdResourceSlot::CombinedRadianceInput);
    assert_eq!(d.resources[2].slot, SrdResourceSlot::ScratchPool);
    assert_eq!(d.resources[2].pool_index, Some(mask.scratch_index));
    assert_eq!(d.resources[3].descriptor_type, SrdDescriptorType::StorageTexture);
    assert_eq!(d.resources[3].pool_index, Some(clamp.scratch_index));
    assert_eq!(d.constants_size, mem::size_of::<SrdRadianceClampConstants>());
    let constants = bytemuck::from_bytes::<SrdRadianceClampConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.rect_extent, [64, 32]);
    assert_eq!(constants.mask_extent, [8, 4]);
    assert_eq!(constants.sigma_scale, 1.25);
    assert_eq!(constants.use_surface_mask, 1);
    assert_eq!(constants.tile_size, SRD_RADIANCE_SURFACE_MASK_TILE_SIZE);
}

#[test]
fn radiance_reconstruct_with_clamp_reads_clamped_scratch() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(83),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let accumulate = instance
        .prepare_radiance_accumulate(SrdDenoiserId::new(83))
        .unwrap();
    let clamp = instance
        .prepare_radiance_clamp(SrdDenoiserId::new(83))
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(83))
        .unwrap();

    let dispatches = instance
        .plan_radiance_reconstruct_passes(
            SrdDenoiserId::new(83),
            reconstruct,
            accumulate,
            Some(clamp),
            None,
            UVec2::new(16, 16),
        )
        .unwrap();

    assert_eq!(dispatches.len(), 1);
    let d = &dispatches[0];
    // First read must be the clamped scratch, not raw history
    assert_eq!(d.resources[0].slot, SrdResourceSlot::ClampedRadianceInput);
    assert_eq!(d.resources[0].pool_index, Some(clamp.scratch_index));
    assert_ne!(d.resources[0].slot, SrdResourceSlot::HistoryPool);
}

#[test]
fn radiance_spatial_filter_prepare_allocates_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(90),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_spatial_filter(SrdDenoiserId::new(90))
        .unwrap();

    let texture = &instance.scratch_pool()[resources.scratch_index as usize];
    assert_eq!(texture.format, Format::Rgba16Float);
    assert_eq!(texture.downsample_factor, 1);
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_spatial_filter");
    assert_eq!(pipeline.workgroup_size, [8, 8, 1]);
    assert!(pipeline.has_constants);
}

#[test]
fn radiance_spatial_filter_plan_skips_when_disabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(91),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(91))
        .unwrap();
    let filter = instance
        .prepare_radiance_spatial_filter(SrdDenoiserId::new(91))
        .unwrap();

    let dispatches = instance
        .plan_radiance_spatial_filter_passes(
            SrdDenoiserId::new(91),
            filter,
            reconstruct.scratch_index,
            None,
            UVec2::new(16, 16),
        )
        .unwrap();
    // Disabled by default
    assert!(dispatches.is_empty());
}

#[test]
fn radiance_spatial_filter_plan_emits_dispatch_with_bilateral_inputs() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(92),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(92),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                spatial_filter: SrdSpatialFilterSettings {
                    enabled: true,
                    stride: 2,
                    luminance_sigma: 0.75,
                    normal_sharpness: 24.0,
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let mask = instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(92))
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(92))
        .unwrap();
    let filter = instance
        .prepare_radiance_spatial_filter(SrdDenoiserId::new(92))
        .unwrap();

    let d = {
        let dispatches = instance
            .plan_radiance_spatial_filter_passes(
                SrdDenoiserId::new(92),
                filter,
                reconstruct.scratch_index,
                Some(mask),
                UVec2::new(64, 32),
            )
            .unwrap();
        assert_eq!(dispatches.len(), 1);
        dispatches[0].clone()
    };
    assert_eq!(d.name, "SRD Radiance Spatial Filter");
    assert_eq!(d.pipeline_index, filter.pipeline);
    assert_eq!(d.grid_size, [8, 4, 1]);
    // input + depth + normal/roughness + mask + write
    assert_eq!(d.resources.len(), 5);
    assert_eq!(d.resources[0].pool_index, Some(reconstruct.scratch_index));
    assert_eq!(d.resources[1].slot, SrdResourceSlot::LinearDepthInput);
    assert_eq!(d.resources[2].slot, SrdResourceSlot::NormalRoughnessInput);
    assert_eq!(d.resources[3].pool_index, Some(mask.scratch_index));
    assert_eq!(d.resources[4].descriptor_type, SrdDescriptorType::StorageTexture);
    assert_eq!(d.resources[4].pool_index, Some(filter.scratch_index));
    let constants = bytemuck::from_bytes::<SrdRadianceSpatialFilterConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.rect_extent, [64, 32]);
    assert_eq!(constants.stride, 2);
    assert_eq!(constants.luminance_sigma, 0.75);
    assert_eq!(constants.normal_sharpness, 24.0);
    assert_eq!(constants.use_surface_mask, 1);
}

#[test]
fn radiance_stabilizer_plan_appends_spatial_filter_when_enabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(93),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(16, 16),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(93),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                outlier_clamp: SrdOutlierClampSettings {
                    enabled: false,
                    ..SrdOutlierClampSettings::default()
                },
                spatial_filter: SrdSpatialFilterSettings {
                    enabled: true,
                    ..SrdSpatialFilterSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(93))
        .unwrap();
    let plan = instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(93), resources, UVec2::new(16, 16))
        .unwrap();

    // Surface Mask, Reproject, Accumulate, Clamp, Reconstruct, Spatial Filter = 6
    assert_eq!(plan.dispatch_count, 6);
    assert_eq!(
        plan.final_output,
        SrdRadianceOutputResource::SpatialFilter {
            scratch_index: resources.spatial_filter.scratch_index
        }
    );
    let last = instance.dispatches().last().unwrap();
    assert_eq!(last.name, "SRD Radiance Spatial Filter");
}

#[test]
fn spatial_filter_settings_validate_ranges() {
    SrdSpatialFilterSettings::default().validate().unwrap();

    let err = SrdSpatialFilterSettings {
        stride: 0,
        ..SrdSpatialFilterSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("stride"));

    let err = SrdSpatialFilterSettings {
        luminance_sigma: 0.0,
        ..SrdSpatialFilterSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("luminance_sigma"));
}

#[test]
fn radiance_atrous_prepare_allocates_ping_pong_scratch_and_pipeline() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(100),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_atrous(SrdDenoiserId::new(100))
        .unwrap();

    assert_ne!(resources.ping_index, resources.pong_index);
    let ping = &instance.scratch_pool()[resources.ping_index as usize];
    let pong = &instance.scratch_pool()[resources.pong_index as usize];
    assert_eq!(ping.format, Format::Rgba16Float);
    assert_eq!(pong.format, Format::Rgba16Float);
    let pipeline = &instance.pipelines()[resources.pipeline];
    assert_eq!(pipeline.shader_label, "srd_radiance_atrous");
}

#[test]
fn radiance_atrous_plan_skips_when_disabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(101),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(101))
        .unwrap();
    let atrous = instance
        .prepare_radiance_atrous(SrdDenoiserId::new(101))
        .unwrap();
    let before = instance.dispatches().len();
    let returned = instance
        .plan_radiance_atrous_passes(
            SrdDenoiserId::new(101),
            atrous,
            reconstruct.scratch_index,
            None,
            UVec2::new(16, 16),
        )
        .unwrap();
    assert_eq!(instance.dispatches().len(), before);
    // Disabled → returns input unchanged
    assert_eq!(returned, reconstruct.scratch_index);
}

#[test]
fn radiance_atrous_plan_emits_one_dispatch_per_iteration_with_doubling_stride() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(102),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(102),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                atrous: SrdAtrousSettings {
                    enabled: true,
                    iterations: 4,
                    luminance_sigma: 2.0,
                    normal_sharpness: 12.0,
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(102))
        .unwrap();
    let atrous = instance
        .prepare_radiance_atrous(SrdDenoiserId::new(102))
        .unwrap();

    let final_idx = instance
        .plan_radiance_atrous_passes(
            SrdDenoiserId::new(102),
            atrous,
            reconstruct.scratch_index,
            None,
            UVec2::new(32, 16),
        )
        .unwrap();

    let dispatches = instance.dispatches();
    assert_eq!(dispatches.len(), 4);
    // Iteration 0: reads input, writes ping. iter1: reads ping, writes pong.
    // iter2: reads pong, writes ping. iter3: reads ping, writes pong.
    // With 4 iterations the final output is pong.
    assert_eq!(final_idx, atrous.pong_index);

    let expected_strides = [1u32, 2, 4, 8];
    for (i, dispatch) in dispatches.iter().enumerate() {
        assert!(dispatch.name.starts_with("SRD Radiance Atrous Iter"));
        let constants = bytemuck::from_bytes::<SrdRadianceAtrousConstants>(
            instance.constant_bytes(dispatch.constants_range.unwrap()),
        );
        assert_eq!(constants.stride, expected_strides[i]);
        assert_eq!(constants.luminance_sigma, 2.0);
        assert_eq!(constants.normal_sharpness, 12.0);
    }
}

#[test]
fn radiance_atrous_plan_ping_pongs_inputs_across_iterations() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(103),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(103),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                atrous: SrdAtrousSettings {
                    enabled: true,
                    iterations: 3,
                    ..SrdAtrousSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let reconstruct = instance
        .prepare_radiance_reconstruct(SrdDenoiserId::new(103))
        .unwrap();
    let atrous = instance
        .prepare_radiance_atrous(SrdDenoiserId::new(103))
        .unwrap();

    let final_idx = instance
        .plan_radiance_atrous_passes(
            SrdDenoiserId::new(103),
            atrous,
            reconstruct.scratch_index,
            None,
            UVec2::new(8, 8),
        )
        .unwrap();

    // 3 iterations: final lands in ping (idx 0=ping, 1=pong, 2=ping).
    assert_eq!(final_idx, atrous.ping_index);
    let dispatches = instance.dispatches();
    assert_eq!(dispatches.len(), 3);
    // No dispatch may read and write the same texture.
    for dispatch in dispatches {
        for r in &dispatch.resources {
            for w in &dispatch.resources {
                if r.descriptor_type == SrdDescriptorType::Texture
                    && w.descriptor_type == SrdDescriptorType::StorageTexture
                {
                    assert_ne!(r.pool_index, w.pool_index);
                }
            }
        }
    }
}

#[test]
fn radiance_stabilizer_plan_uses_atrous_as_final_when_enabled() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(104),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(16, 16),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(104),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                outlier_clamp: SrdOutlierClampSettings {
                    enabled: false,
                    ..SrdOutlierClampSettings::default()
                },
                atrous: SrdAtrousSettings {
                    enabled: true,
                    iterations: 2,
                    ..SrdAtrousSettings::default()
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let resources = instance
        .prepare_radiance_stabilizer(SrdDenoiserId::new(104))
        .unwrap();
    let plan = instance
        .plan_radiance_stabilizer_passes(SrdDenoiserId::new(104), resources, UVec2::new(16, 16))
        .unwrap();

    // Surface Mask, Reproject, Accumulate, Clamp, Reconstruct, Atrous*2 = 7
    assert_eq!(plan.dispatch_count, 7);
    // 2 iterations → final lands in pong (iter 0 writes ping, iter 1 writes pong).
    assert_eq!(
        plan.final_output,
        SrdRadianceOutputResource::Atrous {
            scratch_index: resources.atrous.pong_index
        }
    );
}

#[test]
fn atrous_settings_validate_ranges() {
    SrdAtrousSettings::default().validate().unwrap();

    let err = SrdAtrousSettings {
        enabled: true,
        iterations: 0,
        ..SrdAtrousSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("iterations"));

    let err = SrdAtrousSettings {
        iterations: SrdAtrousSettings::MAX_ITERATIONS + 1,
        ..SrdAtrousSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));

    let err = SrdAtrousSettings {
        luminance_sigma: 0.0,
        ..SrdAtrousSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("luminance_sigma"));
}

#[test]
fn history_clamp_settings_validate_sigma_scale() {
    SrdHistoryClampSettings::default().validate().unwrap();

    let err = SrdHistoryClampSettings {
        sigma_scale: 0.0,
        ..SrdHistoryClampSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("sigma_scale"));
}

#[test]
fn hit_distance_settings_validate_threshold() {
    SrdHitDistanceSettings::default().validate().unwrap();

    let err = SrdHitDistanceSettings {
        relative_threshold: 0.0,
        ..SrdHitDistanceSettings::default()
    }
    .validate()
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("relative_threshold"));
}

#[test]
fn radiance_reproject_plan_with_hit_distance_adds_two_extra_reads() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(120),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(120),
            SrdFamilySettings::Radiance(SrdRadianceSettings {
                hit_distance: SrdHitDistanceSettings {
                    enabled: true,
                    relative_threshold: 0.15,
                },
                ..SrdRadianceSettings::default()
            }),
        )
        .unwrap();
    let reproject = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(120))
        .unwrap();
    let d = {
        let dispatches = instance
            .plan_radiance_reproject_passes(
                SrdDenoiserId::new(120),
                reproject,
                None,
                UVec2::new(16, 16),
            )
            .unwrap();
        dispatches[0].clone()
    };
    // Five base guide reads + HitDistance + PrevHitDistance + scratch write = 8
    assert_eq!(d.resources.len(), 8);
    assert!(d.resources.iter().any(|r| r.slot == SrdResourceSlot::HitDistanceInput));
    assert!(d.resources.iter().any(|r| r.slot == SrdResourceSlot::PrevHitDistanceInput));

    let constants = bytemuck::from_bytes::<SrdRadianceReprojectConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.use_hit_distance, 1);
    assert_eq!(constants.hit_distance_relative_threshold, 0.15);
}

#[test]
fn radiance_combined_prepare_allocates_core_five_resources() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(140),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();

    let resources = instance
        .prepare_radiance_combined(SrdDenoiserId::new(140))
        .unwrap();

    assert_eq!(instance.history_rings().len(), 1);
    assert_eq!(instance.history_pool().len(), 2);
    assert_eq!(instance.scratch_pool().len(), 3); // surface_mask + reproject/reconstruct (aliased) + clamp
    assert_eq!(
        instance.pipelines()[resources.reconstruct.pipeline].shader_label,
        "srd_radiance_reconstruct"
    );
    assert_eq!(
        instance.pipelines()[resources.accumulate.pipeline].shader_label,
        "srd_radiance_accumulate"
    );
}

#[test]
fn radiance_combined_plan_emits_five_core_passes() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(141),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(32, 16),
            history_mode: SrdHistoryMode::KeepAccumulating,
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_combined(SrdDenoiserId::new(141))
        .unwrap();
    let plan = instance
        .plan_radiance_combined_passes(SrdDenoiserId::new(141), resources, UVec2::new(32, 16))
        .unwrap();

    assert_eq!(plan.dispatch_count, 5);
    let names: Vec<&str> = instance
        .dispatches()
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "SRD Radiance Surface Mask",
            "SRD Radiance Reproject",
            "SRD Radiance Accumulate",
            "SRD Radiance Clamp",
            "SRD Radiance Reconstruct",
        ]
    );
    assert_eq!(
        plan.final_output,
        SrdRadianceOutputResource::Reconstruct {
            scratch_index: resources.reconstruct.scratch_index
        }
    );
}

#[test]
fn radiance_combined_plan_clear_uses_zero_history_mode() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(142),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(8, 8),
            history_mode: SrdHistoryMode::ZeroHistory,
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_combined(SrdDenoiserId::new(142))
        .unwrap();
    let plan = instance
        .plan_radiance_combined_passes(SrdDenoiserId::new(142), resources, UVec2::new(8, 8))
        .unwrap();

    // 1 clear + 5 passes = 6
    assert_eq!(plan.dispatch_count, 6);
    assert!(instance.dispatches()[0].name.starts_with("SRD Clear"));
}

#[test]
fn radiance_diffuse_specular_prepare_allocates_independent_history_rings() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(130),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();

    let resources = instance
        .prepare_radiance_diffuse_specular(SrdDenoiserId::new(130))
        .unwrap();

    // Two independent history rings (diffuse + specular)
    assert_eq!(instance.history_rings().len(), 2);
    assert_ne!(
        resources.diffuse_accumulate.history_ring_index,
        resources.specular_accumulate.history_ring_index
    );
    assert_eq!(instance.history_pool().len(), 4); // 2 per ring
    // Clamp and reconstruct are unique scratch slots
    assert_ne!(
        resources.diffuse_clamp.scratch_index,
        resources.specular_clamp.scratch_index
    );
    assert_ne!(
        resources.diffuse_reconstruct.scratch_index,
        resources.specular_reconstruct.scratch_index
    );
    // Pipelines are shared
    assert_eq!(
        resources.diffuse_accumulate.pipeline,
        resources.specular_accumulate.pipeline
    );
    assert_eq!(
        resources.diffuse_clamp.pipeline,
        resources.specular_clamp.pipeline
    );
    assert_eq!(
        resources.diffuse_reconstruct.pipeline,
        resources.specular_reconstruct.pipeline
    );
}

#[test]
fn radiance_diffuse_specular_plan_orders_shared_then_per_channel_passes() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(131),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(32, 16),
            history_mode: SrdHistoryMode::KeepAccumulating,
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_diffuse_specular(SrdDenoiserId::new(131))
        .unwrap();
    let plan = instance
        .plan_radiance_diffuse_specular_passes(
            SrdDenoiserId::new(131),
            resources,
            UVec2::new(32, 16),
        )
        .unwrap();

    // Surface Mask + Reproject + Diffuse(Acc+Clamp+Reconstruct) + Specular(Acc+Clamp+Reconstruct) = 8
    assert_eq!(plan.dispatch_count, 8);
    let names: Vec<&str> = instance
        .dispatches()
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "SRD Radiance Surface Mask",
            "SRD Radiance Reproject",
            "SRD Radiance Diffuse Accumulate",
            "SRD Radiance Diffuse Clamp",
            "SRD Radiance Reconstruct",
            "SRD Radiance Specular Accumulate",
            "SRD Radiance Specular Clamp",
            "SRD Radiance Reconstruct",
        ]
    );
    // Outputs are independent scratch indices
    assert_ne!(
        plan.diffuse_output.scratch_index(),
        plan.specular_output.scratch_index()
    );
}

#[test]
fn radiance_diffuse_specular_plan_clear_emits_four_clears_for_zero_history() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(132),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            rect_size: UVec2::new(8, 8),
            history_mode: SrdHistoryMode::ZeroHistory,
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_diffuse_specular(SrdDenoiserId::new(132))
        .unwrap();
    let plan = instance
        .plan_radiance_diffuse_specular_passes(
            SrdDenoiserId::new(132),
            resources,
            UVec2::new(8, 8),
        )
        .unwrap();

    // 4 clears (2 rings × 2 write slots) + 8 passes = 12? No: clear dispatches use write slots
    // only. Each ring has one write slot that needs clearing. So 2 clears + 8 = 10... wait.
    // Actually push_clear_dispatches_for clears all rings for this denoiser (2 rings, each with
    // one history write index). So 2 clears + 8 passes = 10 total.
    assert_eq!(plan.dispatch_count, 10);
    let clear_count = instance
        .dispatches()
        .iter()
        .filter(|d| d.name.starts_with("SRD Clear"))
        .count();
    assert_eq!(clear_count, 2);
}

#[test]
fn radiance_accumulate_passes_with_diffuse_slot_uses_diffuse_input_binding() {
    let diffuse_input = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::DiffuseRadianceInput,
        pool_index: None,
    };
    let specular_input = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::SpecularRadianceInput,
        pool_index: None,
    };
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_accumulate",
            &diffuse_input,
            0,
        )
        .unwrap(),
        "srd_radiance_input"
    );
    assert_eq!(
        super::radiance_stabilizer_executor::srd_sampled_binding_for_resource(
            "srd_radiance_clamp",
            &specular_input,
            0,
        )
        .unwrap(),
        "srd_radiance_input"
    );
}

#[test]
fn radiance_reproject_plan_without_hit_distance_leaves_fields_zero() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(121),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let reproject = instance
        .prepare_radiance_reproject(SrdDenoiserId::new(121))
        .unwrap();
    let d = {
        let dispatches = instance
            .plan_radiance_reproject_passes(
                SrdDenoiserId::new(121),
                reproject,
                None,
                UVec2::new(16, 16),
            )
            .unwrap();
        dispatches[0].clone()
    };
    // No hit-distance slots in resources
    assert!(d.resources.iter().all(|r| r.slot != SrdResourceSlot::HitDistanceInput));
    assert!(d.resources.iter().all(|r| r.slot != SrdResourceSlot::PrevHitDistanceInput));

    let constants = bytemuck::from_bytes::<SrdRadianceReprojectConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.use_hit_distance, 0);
}

// ─── Occlusion Stabilizer ────────────────────────────────────────────────────

#[test]
fn occlusion_constants_layout_stays_shader_compatible() {
    assert_eq!(mem::size_of::<SrdOcclusionAccumulateConstants>(), 32);
    assert_eq!(mem::align_of::<SrdOcclusionAccumulateConstants>(), 4);
    assert_eq!(mem::size_of::<SrdOcclusionFilterConstants>(), 32);
    assert_eq!(mem::align_of::<SrdOcclusionFilterConstants>(), 4);
}

#[test]
fn occlusion_prepare_requires_occlusion_stabilizer_mode() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(200),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let err = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(200))
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("OcclusionStabilizer"));
}

#[test]
fn occlusion_prepare_allocates_correct_resources() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(201),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();

    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(201))
        .unwrap();

    // surface_mask scratch + reproject scratch + filter scratch = 3 scratch textures
    assert_eq!(instance.scratch_pool().len(), 3);
    // 2 history textures (ring write + read)
    assert_eq!(instance.history_pool().len(), 2);
    assert_eq!(instance.history_rings().len(), 1);

    let acc_ring = &instance.history_rings()[resources.accumulate.history_ring_index];
    assert_ne!(acc_ring.write_index, acc_ring.read_index);
    assert!(acc_ring.label.contains("occlusion_history"));

    let filter_tex = &instance.scratch_pool()[resources.filter.scratch_index as usize];
    assert_eq!(filter_tex.format, Format::Rgba16Float);
    assert_eq!(filter_tex.downsample_factor, 1);
    assert_eq!(filter_tex.slot, SrdResourceSlot::OcclusionOutput);

    let acc_pipeline = &instance.pipelines()[resources.accumulate.pipeline];
    assert_eq!(acc_pipeline.shader_label, "srd_occlusion_accumulate");
    assert_eq!(acc_pipeline.workgroup_size, [8, 8, 1]);
    assert!(acc_pipeline.has_constants);

    let flt_pipeline = &instance.pipelines()[resources.filter.pipeline];
    assert_eq!(flt_pipeline.shader_label, "srd_occlusion_filter");
    assert_eq!(flt_pipeline.workgroup_size, [8, 8, 1]);
    assert!(flt_pipeline.has_constants);
}

#[test]
fn occlusion_plan_emits_four_core_passes_with_keep_accumulating() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(202),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(64, 32),
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(202))
        .unwrap();
    let plan = instance
        .plan_radiance_occlusion_passes(SrdDenoiserId::new(202), resources, UVec2::new(64, 32))
        .unwrap();

    // Surface Mask, Reproject, Accumulate, Filter = 4
    assert_eq!(plan.dispatch_count, 4);
    assert_eq!(plan.final_output_scratch_index, resources.filter.scratch_index);

    let dispatches = instance.dispatches();
    assert_eq!(dispatches[0].name, "SRD Radiance Surface Mask");
    assert_eq!(dispatches[1].name, "SRD Radiance Reproject");
    assert_eq!(dispatches[2].name, "SRD Occlusion Accumulate");
    assert_eq!(dispatches[3].name, "SRD Occlusion Filter");
}

#[test]
fn occlusion_plan_with_zero_history_emits_two_clears_plus_four_passes() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(203),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::ZeroHistory,
            rect_size: UVec2::new(16, 16),
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(203))
        .unwrap();
    let plan = instance
        .plan_radiance_occlusion_passes(SrdDenoiserId::new(203), resources, UVec2::new(16, 16))
        .unwrap();

    // 1 clear (1 ring) + 4 core = 5
    assert_eq!(plan.dispatch_count, 5);
    assert!(instance.dispatches()[0].name.starts_with("SRD Clear"));
    assert_eq!(instance.dispatches()[1].name, "SRD Radiance Surface Mask");
    assert_eq!(instance.dispatches()[4].name, "SRD Occlusion Filter");
}

#[test]
fn occlusion_accumulate_dispatch_reads_occlusion_input_and_history() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(204),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(32, 16),
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(204))
        .unwrap();
    instance
        .plan_radiance_occlusion_passes(SrdDenoiserId::new(204), resources, UVec2::new(32, 16))
        .unwrap();

    let acc_dispatch = &instance.dispatches()[2];
    assert_eq!(acc_dispatch.name, "SRD Occlusion Accumulate");
    // OcclusionInput + reproject scratch + surface_mask scratch + history_read + history_write = 5
    assert_eq!(acc_dispatch.resources.len(), 5);
    assert_eq!(acc_dispatch.resources[0].slot, SrdResourceSlot::OcclusionInput);
    assert_eq!(acc_dispatch.resources[0].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(acc_dispatch.resources[1].slot, SrdResourceSlot::ScratchPool);
    assert_eq!(acc_dispatch.resources[1].pool_index, Some(resources.reproject.scratch_index));
    assert_eq!(acc_dispatch.resources[2].slot, SrdResourceSlot::ScratchPool);
    assert_eq!(acc_dispatch.resources[2].pool_index, Some(resources.surface_mask.scratch_index));
    assert_eq!(acc_dispatch.resources[3].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(acc_dispatch.resources[3].slot, SrdResourceSlot::HistoryPool);
    assert_eq!(acc_dispatch.resources[4].descriptor_type, SrdDescriptorType::StorageTexture);
    assert_eq!(acc_dispatch.resources[4].slot, SrdResourceSlot::HistoryPool);
    assert_ne!(acc_dispatch.resources[3].pool_index, acc_dispatch.resources[4].pool_index);
    assert_eq!(acc_dispatch.grid_size, [4, 2, 1]);
    assert_eq!(acc_dispatch.constants_size, mem::size_of::<SrdOcclusionAccumulateConstants>());
}

#[test]
fn occlusion_filter_dispatch_reads_accumulated_history_and_guide_buffers() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(205),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(32, 16),
            ..SrdCommonSettings::default()
        })
        .unwrap();

    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(205))
        .unwrap();
    instance
        .plan_radiance_occlusion_passes(SrdDenoiserId::new(205), resources, UVec2::new(32, 16))
        .unwrap();

    let flt_dispatch = &instance.dispatches()[3];
    assert_eq!(flt_dispatch.name, "SRD Occlusion Filter");
    // history_write (accumulated) + depth + normal + surface_mask + filter_out = 5
    assert_eq!(flt_dispatch.resources.len(), 5);
    assert_eq!(flt_dispatch.resources[0].descriptor_type, SrdDescriptorType::Texture);
    assert_eq!(flt_dispatch.resources[0].slot, SrdResourceSlot::HistoryPool);
    assert_eq!(flt_dispatch.resources[1].slot, SrdResourceSlot::LinearDepthInput);
    assert_eq!(flt_dispatch.resources[2].slot, SrdResourceSlot::NormalRoughnessInput);
    assert_eq!(flt_dispatch.resources[3].slot, SrdResourceSlot::ScratchPool);
    assert_eq!(flt_dispatch.resources[3].pool_index, Some(resources.surface_mask.scratch_index));
    assert_eq!(flt_dispatch.resources[4].descriptor_type, SrdDescriptorType::StorageTexture);
    assert_eq!(flt_dispatch.resources[4].pool_index, Some(resources.filter.scratch_index));
    assert_eq!(flt_dispatch.constants_size, mem::size_of::<SrdOcclusionFilterConstants>());
}

#[test]
fn occlusion_accumulate_constants_reflect_settings() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(206),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(64, 32),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(206),
            SrdFamilySettings::Occlusion(SrdOcclusionSettings {
                history_frame_budget: 48,
                ..SrdOcclusionSettings::default()
            }),
        )
        .unwrap();

    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(206))
        .unwrap();
    instance
        .plan_radiance_occlusion_passes(SrdDenoiserId::new(206), resources, UVec2::new(64, 32))
        .unwrap();

    let d = &instance.dispatches()[2];
    let constants = bytemuck::from_bytes::<SrdOcclusionAccumulateConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.rect_extent, [64, 32]);
    assert_eq!(constants.mask_extent, [8, 4]);
    assert_eq!(constants.history_frame_budget, 48.0);
    assert_eq!(constants.use_surface_mask, 1);
    assert_eq!(constants.tile_size, SRD_RADIANCE_SURFACE_MASK_TILE_SIZE);
}

#[test]
fn occlusion_filter_constants_reflect_settings() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(207),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    instance
        .set_common_settings(SrdCommonSettings {
            history_mode: SrdHistoryMode::KeepAccumulating,
            rect_size: UVec2::new(64, 32),
            ..SrdCommonSettings::default()
        })
        .unwrap();
    instance
        .set_denoiser_settings(
            SrdDenoiserId::new(207),
            SrdFamilySettings::Occlusion(SrdOcclusionSettings {
                spatial_radius: 3.0,
                normal_weight_power: 16.0,
                ..SrdOcclusionSettings::default()
            }),
        )
        .unwrap();

    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(207))
        .unwrap();
    instance
        .plan_radiance_occlusion_passes(SrdDenoiserId::new(207), resources, UVec2::new(64, 32))
        .unwrap();

    let d = &instance.dispatches()[3];
    let constants = bytemuck::from_bytes::<SrdOcclusionFilterConstants>(
        instance.constant_bytes(d.constants_range.unwrap()),
    );
    assert_eq!(constants.rect_extent, [64, 32]);
    assert_eq!(constants.mask_extent, [8, 4]);
    assert_eq!(constants.spatial_radius, 3.0);
    assert_eq!(constants.normal_weight_power, 16.0);
    assert_eq!(constants.use_surface_mask, 1);
    assert_eq!(constants.tile_size, SRD_RADIANCE_SURFACE_MASK_TILE_SIZE);
}

#[test]
fn occlusion_executor_binding_map_resolves_occlusion_slots() {
    use super::radiance_stabilizer_executor::{
        srd_sampled_binding_for_resource, srd_storage_binding_for_resource,
    };
    use super::resources::{SrdDescriptorType, SrdResourceDesc, SrdResourceSlot};

    let occlusion_input = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::OcclusionInput,
        pool_index: None,
    };
    assert_eq!(
        srd_sampled_binding_for_resource("srd_occlusion_accumulate", &occlusion_input, 0).unwrap(),
        "srd_occlusion_input"
    );

    let history_storage = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::StorageTexture,
        slot: SrdResourceSlot::HistoryPool,
        pool_index: Some(0),
    };
    assert_eq!(
        srd_storage_binding_for_resource("srd_occlusion_accumulate", &history_storage).unwrap(),
        "srd_occlusion_history_write"
    );

    let scratch_storage = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::StorageTexture,
        slot: SrdResourceSlot::ScratchPool,
        pool_index: Some(0),
    };
    assert_eq!(
        srd_storage_binding_for_resource("srd_occlusion_filter", &scratch_storage).unwrap(),
        "srd_occlusion_out"
    );

    let history_read = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::HistoryPool,
        pool_index: Some(0),
    };
    assert_eq!(
        srd_sampled_binding_for_resource("srd_occlusion_accumulate", &history_read, 0).unwrap(),
        "srd_occlusion_history_read"
    );
    assert_eq!(
        srd_sampled_binding_for_resource("srd_occlusion_filter", &history_read, 0).unwrap(),
        "srd_occlusion_accumulated"
    );

    let scratch_read = SrdResourceDesc {
        descriptor_type: SrdDescriptorType::Texture,
        slot: SrdResourceSlot::ScratchPool,
        pool_index: Some(0),
    };
    assert_eq!(
        srd_sampled_binding_for_resource("srd_occlusion_accumulate", &scratch_read, 1).unwrap(),
        "srd_occlusion_reproject"
    );
    assert_eq!(
        srd_sampled_binding_for_resource("srd_occlusion_accumulate", &scratch_read, 2).unwrap(),
        "srd_radiance_surface_mask"
    );
    assert_eq!(
        srd_sampled_binding_for_resource("srd_occlusion_filter", &scratch_read, 1).unwrap(),
        "srd_radiance_surface_mask"
    );
}

#[test]
fn occlusion_surface_mask_and_reproject_accept_occlusion_mode() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(208),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    // Both prepare functions must succeed for OcclusionStabilizer
    instance
        .prepare_radiance_surface_mask(SrdDenoiserId::new(208))
        .unwrap();
    instance
        .prepare_radiance_reproject(SrdDenoiserId::new(208))
        .unwrap();
}

#[test]
fn occlusion_plan_rejects_wrong_mode() {
    let mut instance = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(209),
        mode: SrdDenoiserMode::OcclusionStabilizer,
    }]))
    .unwrap();
    let resources = instance
        .prepare_radiance_occlusion(SrdDenoiserId::new(209))
        .unwrap();

    // Create a second instance with a radiance denoiser — the resources from
    // denoiser 209 must not be usable there.
    let mut other = SrdInstance::new(SrdInstanceDesc::new(vec![SrdDenoiserDesc {
        id: SrdDenoiserId::new(210),
        mode: SrdDenoiserMode::RadianceStabilizer,
    }]))
    .unwrap();
    let err = other
        .plan_radiance_occlusion_passes(SrdDenoiserId::new(210), resources, UVec2::new(8, 8))
        .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
    assert!(format!("{err}").contains("OcclusionStabilizer"));
}
