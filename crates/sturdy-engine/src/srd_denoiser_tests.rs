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
    assert_eq!(mem::size_of::<SrdRadianceReprojectConstants>(), 48);
    assert_eq!(mem::align_of::<SrdRadianceReprojectConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceAccumulateConstants>(), 32);
    assert_eq!(mem::align_of::<SrdRadianceAccumulateConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceReconstructConstants>(), 48);
    assert_eq!(mem::align_of::<SrdRadianceReconstructConstants>(), 4);
    assert_eq!(mem::size_of::<SrdRadianceOutlierSuppressConstants>(), 32);
    assert_eq!(mem::align_of::<SrdRadianceOutlierSuppressConstants>(), 4);
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
        instance.pipelines()[resources.reconstruct.pipeline].shader_label,
        "srd_radiance_reconstruct"
    );
    assert_eq!(
        instance.pipelines()[resources.outlier_suppress.pipeline].shader_label,
        "srd_radiance_outlier_suppress"
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

    assert_eq!(plan.dispatch_count, 6);
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

    assert_eq!(plan.dispatch_count, 4);
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
}
