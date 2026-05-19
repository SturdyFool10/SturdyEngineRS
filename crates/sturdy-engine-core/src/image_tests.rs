// Tests extracted from crates/sturdy-engine-core/src/image.rs
// See scripts/extract_tests.py for the extraction logic.

use super::*;

fn desc() -> ImageDesc {
    ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width: 16,
            height: 16,
            depth: 1,
        },
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::SAMPLED | ImageUsage::COPY_DST,
        transient: false,
        clear_value: Some(ImageClearValue::color_f32([0.0, 0.0, 0.0, 1.0])),
        debug_name: Some("image-desc-test"),
        ..ImageDesc::new()
    }
}

#[test]
fn image_desc_accepts_expanded_fields() {
    desc().validate().unwrap();
}

#[test]
fn image_desc_rejects_invalid_dimension_extent() {
    let invalid = ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d {
            width: 16,
            height: 16,
            depth: 4,
        },
        ..desc()
    };

    assert!(matches!(invalid.validate(), Err(Error::InvalidInput(_))));
}

#[test]
fn image_builder_produces_valid_desc() {
    let desc = ImageBuilder::new_2d(Format::Rgba16Float, 1920, 1080)
        .role(ImageRole::ColorAttachment)
        .mip_levels(1)
        .debug_name("hdr-color-buffer")
        .build()
        .unwrap();

    assert_eq!(desc.format, Format::Rgba16Float);
    assert_eq!(desc.extent.width, 1920);
    assert_eq!(desc.extent.height, 1080);
    assert!(desc.usage.contains(ImageUsage::RENDER_TARGET));
    assert_eq!(desc.debug_name, Some("hdr-color-buffer"));
}

#[test]
fn image_builder_rejects_zero_extent() {
    let result = ImageBuilder::new_2d(Format::Rgba8Unorm, 0, 1080).build();
    assert!(matches!(result, Err(Error::InvalidInput(_))));
}

#[test]
fn image_role_default_usage_covers_expected_flags() {
    assert!(
        ImageRole::ColorAttachment
            .default_usage()
            .contains(ImageUsage::RENDER_TARGET)
    );
    assert!(
        ImageRole::DepthAttachment
            .default_usage()
            .contains(ImageUsage::DEPTH_STENCIL)
    );
    assert!(
        ImageRole::Storage
            .default_usage()
            .contains(ImageUsage::STORAGE)
    );
    assert!(
        ImageRole::Presentable
            .default_usage()
            .contains(ImageUsage::PRESENT)
    );
}
