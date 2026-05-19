// Tests extracted from crates/sturdy-engine-core/src/backend/vulkan/resources.rs
// See scripts/extract_tests.py for the extraction logic.

use super::*;

#[test]
fn image_compression_fixed_rate_maps_bits_to_vk_flags() {
    assert_eq!(
        vk_image_compression_fixed_rate_flags(ImageCompression::Fixed {
            bits_per_component: 1,
        })
        .unwrap()[0]
            .as_raw(),
        vk::ImageCompressionFixedRateFlagsEXT::TYPE_1BPC.as_raw()
    );
    assert_eq!(
        vk_image_compression_fixed_rate_flags(ImageCompression::Fixed {
            bits_per_component: 8,
        })
        .unwrap()[0]
            .as_raw(),
        vk::ImageCompressionFixedRateFlagsEXT::TYPE_8BPC.as_raw()
    );
    assert_eq!(
        vk_image_compression_fixed_rate_flags(ImageCompression::Fixed {
            bits_per_component: 24,
        })
        .unwrap()[0]
            .as_raw(),
        vk::ImageCompressionFixedRateFlagsEXT::TYPE_24BPC.as_raw()
    );
}

#[test]
fn image_compression_fixed_rate_rejects_invalid_bit_counts() {
    assert!(matches!(
        vk_image_compression_fixed_rate_flags(ImageCompression::Fixed {
            bits_per_component: 0,
        }),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        vk_image_compression_fixed_rate_flags(ImageCompression::Fixed {
            bits_per_component: 25,
        }),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn image_compression_control_is_only_chained_when_enabled_and_explicit() {
    let mut fixed_rate = [vk::ImageCompressionFixedRateFlagsEXT::TYPE_8BPC];
    assert!(
        vk_image_compression_control(
            ImageCompression::Fixed {
                bits_per_component: 8,
            },
            false,
            &mut fixed_rate,
        )
        .is_none()
    );
    assert!(
        vk_image_compression_control(ImageCompression::Default, true, &mut fixed_rate).is_none()
    );

    let fixed = vk_image_compression_control(
        ImageCompression::Fixed {
            bits_per_component: 8,
        },
        true,
        &mut fixed_rate,
    )
    .unwrap();
    assert_eq!(
        fixed.flags.as_raw(),
        vk::ImageCompressionFlagsEXT::FIXED_RATE_EXPLICIT.as_raw()
    );
    assert_eq!(fixed.compression_control_plane_count, 1);

    let mut unused = [vk::ImageCompressionFixedRateFlagsEXT::NONE];
    let disabled =
        vk_image_compression_control(ImageCompression::Disabled, true, &mut unused).unwrap();
    assert_eq!(
        disabled.flags.as_raw(),
        vk::ImageCompressionFlagsEXT::DISABLED.as_raw()
    );
    assert_eq!(disabled.compression_control_plane_count, 0);
}

#[test]
fn optical_flow_image_info_requires_feature_when_usage_requested() {
    let usage = ImageUsage::OPTICAL_FLOW_INPUT | ImageUsage::SAMPLED;
    let flags = vk_optical_flow_image_usage(usage).unwrap();

    assert!(matches!(
        vk_optical_flow_image_info(flags, false, usage),
        Err(Error::Unsupported(_))
    ));

    let info = vk_optical_flow_image_info(flags, true, usage)
        .unwrap()
        .unwrap();
    assert_eq!(
        info.usage.as_raw(),
        vk::OpticalFlowUsageFlagsNV::INPUT.as_raw()
    );
}

#[test]
fn optical_flow_image_info_rejects_input_output_overlap() {
    let usage = ImageUsage::OPTICAL_FLOW_INPUT | ImageUsage::OPTICAL_FLOW_OUTPUT;
    let flags = vk_optical_flow_image_usage(usage).unwrap();

    assert!(matches!(
        vk_optical_flow_image_info(flags, true, usage),
        Err(Error::InvalidInput(_))
    ));
}
