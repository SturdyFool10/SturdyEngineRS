use crate::*;

fn sampled_image_sampler_layout() -> CanonicalPipelineLayout {
    PipelineLayoutBuilder::new()
        .sampled_image(
            "material",
            "base_color",
            StageMask::FRAGMENT,
            UpdateRate::Material,
        )
        .sampler(
            "material",
            "base_sampler",
            StageMask::FRAGMENT,
            UpdateRate::Material,
        )
        .into_raw_layout()
}

fn create_sampled_image_sampler_bind_group(engine: &Engine) -> Result<BindGroup> {
    let image = engine.create_image(ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width: 2,
            height: 2,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::SAMPLED,
        transient: false,
        clear_value: None,
        debug_name: None,
    })?;
    let sampler = engine.create_sampler(SamplerDesc::default())?;
    let layout = engine
        .pipeline_layout()
        .sampled_image(
            "material",
            "base_color",
            StageMask::FRAGMENT,
            UpdateRate::Material,
        )
        .sampler(
            "material",
            "base_sampler",
            StageMask::FRAGMENT,
            UpdateRate::Material,
        )
        .build(engine)?;

    engine
        .bind_group(&layout)
        .image("base_color", &image)
        .sampler("base_sampler", &sampler)
        .build()
}

#[test]
fn creates_sampled_image_and_sampler_bind_group() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let bind_group = create_sampled_image_sampler_bind_group(&engine).unwrap();

    assert_eq!(bind_group.desc().entries.len(), 2);
}

#[test]
fn anti_aliasing_pass_constructs_builtin_shader() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    AntiAliasingPass::new(&engine).unwrap();
}

#[test]
fn engine_exposes_native_handle_capabilities() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let capabilities = engine.native_handle_capabilities();

    assert_eq!(capabilities.backend, BackendKind::Null);
    assert!(capabilities.handles.is_empty());
    assert!(!capabilities.supports_export(NativeHandleKind::VulkanDevice));
}

#[test]
fn engine_exposes_backend_raw_capabilities() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();

    assert_eq!(engine.raw_capabilities(), BackendRawCapabilities::None);
}

#[test]
fn null_backend_reports_conservative_format_capabilities() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();

    assert_eq!(
        engine.format_capabilities(Format::Rgba8Unorm),
        FormatCapabilities::default()
    );
}

#[test]
fn null_backend_rejects_external_resource_imports() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();

    let image_result = unsafe {
        engine.import_external_image(ExternalImageDesc {
            desc: small_image_desc(),
            handle: ExternalImageHandle::Vulkan(VulkanExternalImage {
                image: 1,
                image_view: 1,
            }),
        })
    };
    assert!(matches!(image_result, Err(Error::Unsupported(_))));

    let buffer_result = unsafe {
        engine.import_external_buffer(ExternalBufferDesc {
            desc: BufferDesc {
                size: 64,
                usage: BufferUsage::STORAGE,
            },
            handle: ExternalBufferHandle::Vulkan(VulkanExternalBuffer { buffer: 1 }),
        })
    };
    assert!(matches!(buffer_result, Err(Error::Unsupported(_))));
}

#[test]
fn debug_names_and_markers_are_accepted_on_null_backend() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let image = engine.create_image(small_image_desc()).unwrap();
    let buffer = engine
        .create_buffer(BufferDesc {
            size: 64,
            usage: BufferUsage::STORAGE,
        })
        .unwrap();

    image.set_debug_name("debug-image").unwrap();
    buffer.set_debug_name("debug-buffer").unwrap();

    let mut frame = engine.begin_frame().unwrap();
    frame.debug_marker("debug-marker").unwrap();
    frame.flush().unwrap();
}

#[test]
fn gpu_capture_integration_points_report_unsupported_on_null_backend() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let desc = GpuCaptureDesc::new(GpuCaptureTool::RenderDoc, "capture-test");

    assert!(engine.supported_gpu_capture_tools().is_empty());
    assert!(matches!(
        engine.begin_gpu_capture(&desc),
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        engine.end_gpu_capture(GpuCaptureTool::RenderDoc),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn bind_group_rejects_resource_kind_mismatch() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let sampler = engine.create_sampler(SamplerDesc::default()).unwrap();
    let layout = engine
        .create_pipeline_layout(sampled_image_sampler_layout())
        .unwrap();

    let err = match engine
        .bind_group(&layout)
        .sampler("base_color", &sampler)
        .build()
    {
        Ok(_) => panic!("bind group should reject sampler bound as sampled image"),
        Err(error) => error,
    };

    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn sampled_images_and_samplers_are_separate_bindings() {
    let layout = sampled_image_sampler_layout();
    let material = layout
        .groups
        .iter()
        .find(|group| group.name == "material")
        .expect("material group exists");

    assert_eq!(material.bindings.len(), 2);
    assert_eq!(material.bindings[0].kind, BindingKind::SampledImage);
    assert_eq!(material.bindings[1].kind, BindingKind::Sampler);
}

#[test]
fn bind_group_rejects_unknown_path() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let sampler = engine.create_sampler(SamplerDesc::default()).unwrap();
    let layout = engine
        .create_pipeline_layout(sampled_image_sampler_layout())
        .unwrap();

    let err = match engine
        .bind_group(&layout)
        .sampler("missing", &sampler)
        .build()
    {
        Ok(_) => panic!("bind group should reject unknown binding path"),
        Err(error) => error,
    };

    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn bind_group_rejects_duplicate_path() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let sampler = engine.create_sampler(SamplerDesc::default()).unwrap();
    let layout = engine
        .create_pipeline_layout(sampled_image_sampler_layout())
        .unwrap();

    let err = match engine
        .bind_group(&layout)
        .sampler("base_sampler", &sampler)
        .sampler("base_sampler", &sampler)
        .build()
    {
        Ok(_) => panic!("bind group should reject duplicate binding paths"),
        Err(error) => error,
    };

    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn runtime_controller_placeholder_transaction_is_noop() {
    let mut controller = RuntimeController::default();
    let report = controller
        .transact()
        .note_change(RuntimeSettingKey::OverlayVisibility)
        .apply()
        .unwrap();

    assert_eq!(
        report.changes,
        vec![RuntimeChangeResult::Exact {
            setting: RuntimeSettingId::from(RuntimeSettingKey::OverlayVisibility),
            path: RuntimeApplyPath::Immediate,
        }]
    );
    assert_eq!(controller.settings(), RuntimeSettingsSnapshot::default());
    assert_eq!(controller.diagnostics(), RuntimeDiagnostics::default());
    assert!(controller.overlay_lines().is_empty());
}

#[test]
fn runtime_setting_keys_report_expected_apply_paths() {
    assert_eq!(
        RuntimeSettingKey::HdrMode.apply_path(),
        RuntimeApplyPath::SurfaceRecreate
    );
    assert_eq!(
        RuntimeSettingKey::AntiAliasingMode.apply_path(),
        RuntimeApplyPath::GraphRebuild
    );
    assert_eq!(
        RuntimeSettingKey::OverlayVisibility.apply_path(),
        RuntimeApplyPath::Immediate
    );
    assert_eq!(
        RuntimeApplyPath::SurfaceRecreate.as_str(),
        "surface_recreate"
    );
    assert_eq!(
        RuntimeApplyPath::WindowReconfigure.to_string(),
        "window_reconfigure"
    );
}

#[test]
fn runtime_controller_registers_app_settings_and_records_changes() {
    let mut controller = RuntimeController::default();
    controller
        .register_app_setting(
            RuntimeSettingDescriptor::new(
                RuntimeSettingId::app("textures.resolution"),
                "Texture Resolution",
                RuntimeApplyPath::Immediate,
                "high",
            )
            .with_description("Select the active texture asset resolution tier.")
            .with_options(vec![
                RuntimeSettingOption {
                    value: "low".into(),
                    label: "Low".to_string(),
                },
                RuntimeSettingOption {
                    value: "medium".into(),
                    label: "Medium".to_string(),
                },
                RuntimeSettingOption {
                    value: "high".into(),
                    label: "High".to_string(),
                },
            ]),
        )
        .unwrap();

    let starting_revision = controller.settings_revision();
    let report = controller
        .transact()
        .set_app_value("textures.resolution", "medium")
        .apply()
        .unwrap();

    assert_eq!(
        report.changes,
        vec![RuntimeChangeResult::Exact {
            setting: RuntimeSettingId::app("textures.resolution"),
            path: RuntimeApplyPath::Immediate,
        }]
    );
    assert_eq!(
        controller.setting_value(RuntimeSettingId::app("textures.resolution")),
        Some(RuntimeSettingValue::Text("medium".to_string()))
    );

    let changes = controller.setting_changes_since(starting_revision);
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].setting,
        RuntimeSettingId::app("textures.resolution")
    );
    assert_eq!(
        changes[0].value,
        RuntimeSettingValue::Text("medium".to_string())
    );
    assert_eq!(changes[0].path, RuntimeApplyPath::Immediate);
}

#[test]
fn runtime_controller_rejects_invalid_setting_value_kind() {
    let mut controller = RuntimeController::default();
    let report = controller
        .transact()
        .set_engine_value(RuntimeSettingKey::OverlayVisibility, "visible")
        .apply()
        .unwrap();

    assert!(matches!(
        &report.changes[0],
        RuntimeChangeResult::Rejected { setting, .. }
            if setting == &RuntimeSettingId::from(RuntimeSettingKey::OverlayVisibility)
    ));
    assert_eq!(
        controller.setting_value(RuntimeSettingKey::OverlayVisibility),
        Some(RuntimeSettingValue::Bool(true))
    );
}

#[test]
fn runtime_controller_reports_setting_support_and_menu_metadata() {
    let controller = RuntimeController::default();
    let overlay_entry = controller
        .setting_entry(RuntimeSettingKey::OverlayVisibility)
        .unwrap();

    assert_eq!(overlay_entry.descriptor.options.len(), 2);
    assert_eq!(
        controller.setting_support(RuntimeSettingKey::OverlayVisibility),
        Some(RuntimeSettingSupport::supported())
    );
}

#[test]
fn runtime_controller_rejects_unsupported_engine_setting_changes() {
    let mut controller = RuntimeController::default();

    let report = controller
        .transact()
        .set_engine_value(RuntimeSettingKey::BackendSelection, "Vulkan")
        .apply()
        .unwrap();

    assert!(matches!(
        &report.changes[0],
        RuntimeChangeResult::Unavailable {
            setting, reason, ..
        }
            if setting == &RuntimeSettingId::from(RuntimeSettingKey::BackendSelection)
                && reason.contains("not implemented")
    ));
}

#[test]
fn runtime_controller_records_precise_apply_notifications_and_user_diagnostics() {
    let mut controller = RuntimeController::default();
    let starting_revision = controller.apply_notifications_revision();

    let report = controller
        .transact()
        .set_engine_value(RuntimeSettingKey::OverlayVisibility, false)
        .set_engine_value(RuntimeSettingKey::BackendSelection, "Vulkan")
        .apply()
        .unwrap();

    assert_eq!(controller.last_apply_report(), Some(report.clone()));

    let notifications = controller.apply_notifications_since(starting_revision);
    assert_eq!(
        notifications,
        vec![
            RuntimeApplyNotification {
                revision: starting_revision + 1,
                result: RuntimeChangeResult::Exact {
                    setting: RuntimeSettingId::from(RuntimeSettingKey::OverlayVisibility),
                    path: RuntimeApplyPath::Immediate,
                },
            },
            RuntimeApplyNotification {
                revision: starting_revision + 2,
                result: RuntimeChangeResult::Unavailable {
                    setting: RuntimeSettingId::from(RuntimeSettingKey::BackendSelection),
                    path: Some(RuntimeApplyPath::DeviceMigration),
                    reason: "live backend migration is not implemented yet".to_string(),
                },
            },
        ]
    );
    let diagnostics = controller.diagnostics();
    assert_eq!(diagnostics.user_diagnostics.len(), 1);
    assert_eq!(
        diagnostics.user_diagnostics[0].message,
        "backend selection is unavailable in this runtime."
    );
}

#[test]
fn keybind_serializes_and_parses_round_trip() {
    let binding = Keybind::new(
        [KeyModifier::Ctrl, KeyModifier::Shift],
        Some("KeyK".to_string()),
    );
    let serialized = binding.to_string();

    assert_eq!(serialized, "Ctrl+Shift+KeyK");
    assert_eq!(serialized.parse::<Keybind>().unwrap(), binding);
    assert_eq!(binding.display_label(), "Ctrl+Shift+K");
}

#[test]
fn keybind_capture_finishes_modifier_only_binding_on_last_release() {
    let mut capture = KeybindCapture::new();
    let press_ctrl = KeyInput {
        key: KeyToken::Modifier(KeyModifier::Ctrl),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: None,
    };
    let press_shift = KeyInput {
        key: KeyToken::Modifier(KeyModifier::Shift),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers {
            ctrl: true,
            shift: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: None,
    };
    let release_shift = KeyInput {
        key: KeyToken::Modifier(KeyModifier::Shift),
        state: KeyInputState::Released,
        modifiers: KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: None,
    };
    let release_ctrl = KeyInput {
        key: KeyToken::Modifier(KeyModifier::Ctrl),
        state: KeyInputState::Released,
        modifiers: KeyModifiers::default(),
        repeat: false,
        text: None,
    };

    assert!(capture.handle_input(&press_ctrl).is_none());
    assert!(capture.handle_input(&press_shift).is_none());
    assert!(capture.handle_input(&release_shift).is_none());

    let binding = capture.handle_input(&release_ctrl).unwrap();
    assert_eq!(binding.to_string(), "Ctrl+Shift");
}

#[test]
fn keybind_capture_uses_first_non_modifier_with_current_modifiers() {
    let mut capture = KeybindCapture::new();
    let press_ctrl = KeyInput {
        key: KeyToken::Modifier(KeyModifier::Ctrl),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: None,
    };
    let press_k = KeyInput {
        key: KeyToken::key("KeyK"),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: Some("k".to_string()),
    };
    let press_j = KeyInput {
        key: KeyToken::key("KeyJ"),
        state: KeyInputState::Pressed,
        modifiers: KeyModifiers {
            ctrl: true,
            ..KeyModifiers::default()
        },
        repeat: false,
        text: Some("j".to_string()),
    };

    assert!(capture.handle_input(&press_ctrl).is_none());
    let binding = capture.handle_input(&press_k).unwrap();
    assert_eq!(binding.to_string(), "Ctrl+KeyK");
    assert_eq!(capture.handle_input(&press_j).unwrap(), binding);
}

#[test]
fn action_binding_registry_rebinds_action_from_capture() {
    let mut registry = ActionBindingRegistry::new();
    registry.set_binding("toggle_overlay", "Ctrl+KeyO".parse().unwrap());
    registry.request_rebind("toggle_overlay");

    let change = registry
        .handle_input(&KeyInput {
            key: KeyToken::Modifier(KeyModifier::Alt),
            state: KeyInputState::Pressed,
            modifiers: KeyModifiers {
                alt: true,
                ..KeyModifiers::default()
            },
            repeat: false,
            text: None,
        })
        .is_none();
    assert!(change);

    let change = registry
        .handle_input(&KeyInput {
            key: KeyToken::key("KeyP"),
            state: KeyInputState::Pressed,
            modifiers: KeyModifiers {
                alt: true,
                ..KeyModifiers::default()
            },
            repeat: false,
            text: Some("p".to_string()),
        })
        .unwrap();

    assert_eq!(change.action, "toggle_overlay");
    assert_eq!(change.binding.to_string(), "Alt+KeyP");
    assert_eq!(
        registry.binding("toggle_overlay").unwrap().to_string(),
        "Alt+KeyP"
    );
}

#[test]
fn vulkan_writes_sampled_image_and_sampler_descriptors_when_available() {
    let engine = match Engine::with_backend(BackendKind::Vulkan) {
        Ok(engine) => engine,
        Err(Error::Unsupported(_)) => return,
        Err(Error::Backend(message))
            if message.contains("failed to load Vulkan loader")
                || message.contains("no Vulkan physical device") =>
        {
            return;
        }
        Err(error) => panic!("unexpected Vulkan backend creation error: {error}"),
    };

    let bind_group = create_sampled_image_sampler_bind_group(&engine).unwrap();
    assert_eq!(bind_group.desc().entries.len(), 2);
}

#[test]
fn upload_texture_2d_rejects_wrong_byte_count() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let mut frame = engine.begin_frame().unwrap();
    let err = match frame.upload_texture_2d(
        "bad-upload",
        TextureUploadDesc::sampled_rgba8(2, 2),
        &[0; 15],
    ) {
        Ok(_) => panic!("upload should reject incorrect data length"),
        Err(error) => error,
    };
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn texture_uploads_share_frame_upload_arena() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let mut frame = engine.begin_frame().unwrap();
    let pixels = [255u8; 16];

    frame
        .upload_texture_2d("first", TextureUploadDesc::sampled_rgba8(2, 2), &pixels)
        .unwrap();
    frame
        .upload_texture_2d("second", TextureUploadDesc::sampled_rgba8(2, 2), &pixels)
        .unwrap();

    assert_eq!(frame.upload_arena.block_count(), 1);
}

fn small_image_desc() -> ImageDesc {
    ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width: 1,
            height: 1,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::SAMPLED,
        transient: false,
        clear_value: None,
        debug_name: None,
    }
}

#[test]
fn flush_returns_submission_handle() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let mut frame = engine.begin_frame().unwrap();
    let token = frame.flush().unwrap();
    // Token is a monotonically-increasing counter; first submission >= 0.
    let _ = token;
    frame.wait().unwrap();
}

#[test]
fn explicit_sync_reports_submission_and_wait_reason() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let mut frame = engine.begin_frame().unwrap();

    let flush_report = frame
        .flush_with_reason(FrameSyncReason::ExplicitUserRequest)
        .unwrap();
    assert_eq!(flush_report.reason, FrameSyncReason::ExplicitUserRequest);
    assert!(flush_report.submitted);
    assert!(!flush_report.waited);
    assert!(flush_report.submission.is_some());

    let wait_report = frame
        .wait_with_reason(FrameSyncReason::ExplicitUserRequest)
        .unwrap();
    assert_eq!(wait_report.reason, FrameSyncReason::ExplicitUserRequest);
    assert!(!wait_report.submitted);
    assert!(wait_report.waited);
    assert_eq!(wait_report.submission, flush_report.submission);
}

#[test]
fn screenshot_readback_reports_explicit_blocking_reason() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let mut frame = engine.begin_frame().unwrap();
    let capture = ScreenshotCapture::new(&engine, 1, 1, Format::Rgba8Unorm).unwrap();

    let (flush_report, wait_report) = capture.finish_readback(&mut frame).unwrap();

    assert_eq!(flush_report.reason, FrameSyncReason::ReadbackCompletion);
    assert_eq!(wait_report.reason, FrameSyncReason::ReadbackCompletion);
    assert!(flush_report.submitted);
    assert!(wait_report.waited);
}

#[test]
fn consecutive_flushes_succeed() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    for _ in 0..3 {
        let mut frame = engine.begin_frame().unwrap();
        frame.flush().unwrap();
    }
}

// TODO: Uncomment when Engine::render_image is implemented
// #[test]
// fn render_image_convenience_flushes_and_waits() {
//     let engine = Engine::with_backend(BackendKind::Null).unwrap();
//     let image = engine.create_image(small_image_desc()).unwrap();
//
//     engine
//         .render_image(&image, |_context| {
//             // The convenience contract is that this returns only after the
//             // internally-created frame has flushed and waited.
//             Ok(())
//         })
//         .unwrap();
// }

#[test]
fn deferred_destroy_image_is_invalid_immediately_after_destroy() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    // RAII Image — dropping it calls destroy_image on the underlying handle.
    let image = engine.create_image(small_image_desc()).unwrap();
    let handle = image.handle();
    // Use the raw device to verify the handle is tracked before drop.
    assert!(engine.device.image_desc(handle).is_ok());
    drop(image);
    // After drop (which calls destroy_image), the device no longer tracks the handle.
    assert!(matches!(
        engine.device.image_desc(handle),
        Err(Error::InvalidHandle)
    ));
}

#[test]
fn deferred_destroy_processed_at_next_flush() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    // Create and immediately destroy a resource (via RAII drop) without flushing first.
    drop(engine.create_image(small_image_desc()).unwrap());
    drop(engine.create_image(small_image_desc()).unwrap());
    // A flush after deferred destroys have been queued must not error.
    let mut frame = engine.begin_frame().unwrap();
    frame.flush().unwrap();
    frame.wait().unwrap();
}

#[test]
fn deferred_destroy_after_flush_processed_at_subsequent_flush() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let image = engine.create_image(small_image_desc()).unwrap();
    // Flush first (image in use by GPU conceptually).
    let mut frame = engine.begin_frame().unwrap();
    frame.import_image(&image).unwrap();
    frame.flush().unwrap();
    // Now destroy — the RAII drop queues it for deferred destruction.
    drop(image);
    // Next flush must drain the deferred destroy without error.
    let mut frame2 = engine.begin_frame().unwrap();
    frame2.flush().unwrap();
    frame2.wait().unwrap();
}
