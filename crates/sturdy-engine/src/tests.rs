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
fn consecutive_flushes_succeed() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    for _ in 0..3 {
        let mut frame = engine.begin_frame().unwrap();
        frame.flush().unwrap();
    }
}

#[test]
fn render_image_convenience_flushes_and_waits() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let image = engine.create_image(small_image_desc()).unwrap();

    engine
        .render_image(&image, |_context| {
            // The convenience contract is that this returns only after the
            // internally-created frame has flushed and waited.
            Ok(())
        })
        .unwrap();
}

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
