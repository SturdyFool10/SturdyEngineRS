use crate::*;

#[test]
fn creates_sampled_image_and_sampler_bind_group() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let image = engine
        .create_image(ImageDesc {
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
        })
        .unwrap();
    let sampler = engine.create_sampler(SamplerDesc::default()).unwrap();
    let layout = engine
        .create_pipeline_layout(CanonicalPipelineLayout {
            groups: vec![CanonicalGroupLayout {
                name: "material".into(),
                bindings: vec![
                    CanonicalBinding {
                        path: "base_color".into(),
                        kind: BindingKind::SampledImage,
                        count: 1,
                        stage_mask: StageMask::FRAGMENT,
                        update_rate: UpdateRate::Material,
                    },
                    CanonicalBinding {
                        path: "base_sampler".into(),
                        kind: BindingKind::Sampler,
                        count: 1,
                        stage_mask: StageMask::FRAGMENT,
                        update_rate: UpdateRate::Material,
                    },
                ],
            }],
            push_constants_bytes: 0,
        })
        .unwrap();

    let bind_group = engine
        .create_bind_group(BindGroupDesc {
            layout: layout.handle(),
            entries: vec![
                BindGroupEntry {
                    path: "base_color".into(),
                    resource: ResourceBinding::Image(image.handle()),
                },
                BindGroupEntry {
                    path: "base_sampler".into(),
                    resource: ResourceBinding::Sampler(sampler.handle()),
                },
            ],
        })
        .unwrap();

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
