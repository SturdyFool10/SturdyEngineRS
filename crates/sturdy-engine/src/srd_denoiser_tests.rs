use super::*;

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

    assert_eq!(history, 0);
    assert_eq!(scratch_a, scratch_b);
    assert_eq!(instance.history_pool().len(), 1);
    assert_eq!(instance.scratch_pool().len(), 1);
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
    let pushed = instance.push_clear_dispatches(clear_pipeline).unwrap();

    assert_eq!(pushed, 1);
    assert_eq!(instance.dispatches().len(), 1);
    assert_eq!(
        instance.dispatches()[0].resources[0].pool_index,
        Some(write)
    );
    assert!(instance.dispatches()[0].name.contains("clear_history"));
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
