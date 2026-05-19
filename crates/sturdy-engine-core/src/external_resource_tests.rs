// Tests extracted from crates/sturdy-engine-core/src/external_resource.rs
// See scripts/extract_tests.py for the extraction logic.

use super::*;
use crate::{BufferUsage, Extent3d, Format, ImageUsage};

#[test]
fn external_vulkan_image_requires_nonzero_image_and_view() {
    let desc = ExternalImageDesc {
        desc: image_desc(),
        handle: ExternalImageHandle::Vulkan(VulkanExternalImage {
            image: 0,
            image_view: 1,
        }),
    };

    assert!(matches!(desc.validate(), Err(Error::InvalidInput(_))));
}

#[test]
fn external_vulkan_buffer_requires_nonzero_buffer() {
    let desc = ExternalBufferDesc {
        desc: BufferDesc {
            size: 64,
            usage: BufferUsage::STORAGE,
        },
        handle: ExternalBufferHandle::Vulkan(VulkanExternalBuffer { buffer: 0 }),
    };

    assert!(matches!(desc.validate(), Err(Error::InvalidInput(_))));
}

fn image_desc() -> ImageDesc {
    ImageDesc {
        dimension: crate::ImageDimension::D2,
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
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
        drm_format_modifier: None,
    }
}
